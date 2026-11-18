//! What the tests of the slide editor share: a window on a talk, and clicks, keys and looks
//! at what it shows.

#![allow(dead_code)]

use std::cell::Cell;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, ElementId, Entity, InputEvent as _, Modifiers, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, TestAppContext, Window,
    WindowBounds, WindowOptions, point, px, size,
};
use slides_engine::{ElementBox, Fonts, TextLayout};
use slides_format::{Pos, Selection, TextFlow, text_flows};
use stagepack_studio::document::{Document, UntitledId};
use stagepack_studio::permissions::BuildPermissions;
use stagepack_studio::recovery::Recovery;
use stagepack_studio::slide_editor::{CARET_ID, SELECTION_OUTLINE_ID};
use stagepack_studio::text_editing::EditingText;
use stagepack_studio::workspace_view::{self, STATUS_ID, WorkspaceView};

pub struct Typing {
    pub _folder: tempfile::TempDir,
    pub file: PathBuf,
    pub handle: AnyWindowHandle,
    pub view: Entity<WorkspaceView>,
}

pub fn open(cx: &mut TestAppContext, source: &str) -> Typing {
    open_with(cx, source, None)
}

pub fn open_with(cx: &mut TestAppContext, source: &str, recovery: Option<Recovery>) -> Typing {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("slides.md"), source).unwrap();
    open_folder(cx, folder, recovery)
}

/// Opens the talk already in `folder`, with its working copies kept in `recovery`, if given.
pub fn open_folder(cx: &mut TestAppContext, folder: tempfile::TempDir, recovery: Option<Recovery>) -> Typing {
    let file = folder.path().join("slides.md");
    let talk = Document::open(folder.path(), Fonts::bundled_only()).unwrap();
    open_document(cx, talk, folder, file, move |view, cx| {
        if let Some(recovery) = recovery {
            view.set_recovery_for_test(recovery, cx);
        }
    })
}

/// Where the window of an untitled talk keeps what a test looks at, all in a scratch folder:
/// the folder standing in for Documents, the working copies, and the build permissions.
pub struct UntitledPlaces {
    pub documents: PathBuf,
    pub recovery: PathBuf,
    pub permissions: PathBuf,
}

impl UntitledPlaces {
    pub fn under(root: &std::path::Path) -> UntitledPlaces {
        let documents = root.join("Documents");
        std::fs::create_dir_all(&documents).unwrap();
        UntitledPlaces {
            documents,
            recovery: root.join("recovery"),
            permissions: root.join("build-permissions.json"),
        }
    }
}

/// Opens a new, untitled talk made from the template `template` in a window, with everything
/// it keeps in a scratch folder of its own: see [`open_untitled_under`].
pub fn open_untitled(cx: &mut TestAppContext, template: &str) -> (Typing, UntitledPlaces) {
    let root = tempfile::tempdir().unwrap();
    let places = UntitledPlaces::under(root.path());
    let typing = open_untitled_under(cx, template, 1, root.path());
    let typing = Typing { _folder: root, ..typing };
    (typing, places)
}

/// Opens a new, untitled talk made from the template `template`, numbered `number` among those
/// open, in a window whose Documents folder, working copies and build permissions are those of
/// [`UntitledPlaces::under`] `root`. Its saves are followed without watching for real.
pub fn open_untitled_under(cx: &mut TestAppContext, template: &str, number: usize, root: &std::path::Path) -> Typing {
    let template = stagepack_studio::templates::template(template).expect("a template");
    let mut talk = Document::new_untitled(template, UntitledId::random(), Fonts::bundled_only()).unwrap();
    talk.set_untitled_number(number);
    open_untitled_talk(cx, talk, root)
}

/// Shows `talk`, an untitled one, in a window set up as [`open_untitled_under`] sets it up.
pub fn open_untitled_talk(cx: &mut TestAppContext, talk: Document, root: &std::path::Path) -> Typing {
    let places = UntitledPlaces::under(root);
    let scratch = tempfile::tempdir().unwrap();
    let file = scratch.path().join("no file yet");
    open_document(cx, talk, scratch, file, move |view, cx| {
        view.set_recovery_for_test(Recovery::new(places.recovery), cx);
        view.set_permissions_for_test(BuildPermissions::load(places.permissions));
        view.set_documents_folder_for_test(places.documents);
        view.set_no_watching_for_test();
    })
}

/// Shows `talk` in a window, set up by `set_up` before anything is drawn.
fn open_document(
    cx: &mut TestAppContext,
    talk: Document,
    folder: tempfile::TempDir,
    file: PathBuf,
    set_up: impl FnOnce(&mut WorkspaceView, &mut gpui_kit::Context<WorkspaceView>) + 'static,
) -> Typing {
    let (handle, view) = cx.update(|cx: &mut App| {
        gpui_kit::init(cx);
        workspace_view::init(cx);
        let bounds = Bounds {
            origin: Point::default(),
            size: size(px(1280.), px(800.)),
        };
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            cx,
            |window, cx| {
                cx.new(|cx| {
                    let mut view = WorkspaceView::open_with(talk, window, cx);
                    set_up(&mut view, cx);
                    view
                })
            },
        )
        .expect("open test window")
    });
    let typing = Typing {
        _folder: folder,
        file,
        handle,
        view,
    };
    typing.settle(cx);
    typing
}

/// Lets `millis` of the test clock pass, a quarter of a second at a time.
pub fn wait(cx: &mut TestAppContext, millis: u64) {
    for _ in 0..millis / 250 {
        cx.executor().advance_clock(Duration::from_millis(250));
        cx.run_until_parked();
    }
    cx.executor()
        .advance_clock(Duration::from_millis(millis % 250));
    cx.run_until_parked();
}

impl Typing {
    /// Draws until what one frame asked for has reached the screen.
    pub fn settle(&self, cx: &mut TestAppContext) {
        for _ in 0..3 {
            cx.update_window(self.handle, |_, window, cx| window.render_frame(cx))
                .unwrap();
            cx.run_until_parked();
        }
    }

    pub fn with_window<T>(
        &self,
        cx: &mut TestAppContext,
        f: impl FnOnce(&mut Window, &mut App) -> T,
    ) -> T {
        cx.update_window(self.handle, |_, window, cx| f(window, cx))
            .unwrap()
    }

    pub fn document(&self, cx: &TestAppContext) -> Entity<Document> {
        cx.read(|cx| self.view.read(cx).document().unwrap().clone())
    }

    pub fn source(&self, cx: &TestAppContext) -> String {
        self.document(cx)
            .read_with(cx, |document, _| document.source().to_string())
    }

    pub fn current(&self, cx: &TestAppContext) -> usize {
        self.document(cx)
            .read_with(cx, |document, _| document.current())
    }

    pub fn slide_count(&self, cx: &TestAppContext) -> usize {
        self.document(cx)
            .read_with(cx, |document, _| document.slide_count())
    }

    pub fn editing(&self, cx: &TestAppContext) -> Option<EditingText> {
        self.document(cx)
            .read_with(cx, |document, _| document.text().cloned())
    }

    pub fn selection(&self, cx: &TestAppContext) -> Selection {
        self.editing(cx)
            .expect("text is being typed")
            .edit
            .selection
    }

    pub fn flows(&self, cx: &TestAppContext, slide: usize) -> Vec<TextFlow> {
        self.document(cx)
            .read_with(cx, |document, _| text_flows(document.talk(), slide))
    }

    pub fn layout(&self, cx: &mut TestAppContext) -> Arc<TextLayout> {
        let document = self.document(cx);
        cx.update(|cx| document.update(cx, |document, _| document.text_layout(document.current())))
    }

    /// Where the slide is in the window.
    pub fn slide_bounds(&self, cx: &TestAppContext) -> Bounds<Pixels> {
        cx.read(|cx| {
            let canvas = self.view.read(cx).slide_canvas().clone();
            canvas
                .read(cx)
                .slide_bounds()
                .expect("the slide has been drawn")
        })
    }

    /// The point of the window at (`x`, `y`) of the slide, in slide units.
    pub fn at(&self, cx: &TestAppContext, x: f32, y: f32) -> Point<Pixels> {
        let slide = self.slide_bounds(cx);
        let unit = f32::from(slide.size.width) / 1920.;
        slide.origin + point(px(x * unit), px(y * unit))
    }

    /// The middle of where the caret goes at `pos` of flow `flow` of the current slide.
    pub fn point_of(&self, cx: &mut TestAppContext, flow: usize, pos: Pos) -> Point<Pixels> {
        let caret = self
            .layout(cx)
            .caret(flow, pos)
            .expect("the place has a caret");
        // A hair inside the text, so that rounding at the slide's scale never puts the point
        // just before where the text starts.
        let (x, y) = caret.on_slide(caret.x + CARET_NUDGE, caret.top + caret.height / 2.);
        self.at(cx, x, y)
    }

    /// The middle of the empty region `region` of the current slide.
    pub fn placeholder_point(&self, cx: &mut TestAppContext, region: &str) -> Point<Pixels> {
        let (rect, _) = self
            .layout(cx)
            .placeholder(region)
            .expect("the region is empty");
        self.at(cx, rect.x + rect.w / 2., rect.y + rect.h / 2.)
    }

    pub fn mouse_down(
        &self,
        cx: &mut TestAppContext,
        at: Point<Pixels>,
        click_count: usize,
        modifiers: Modifiers,
    ) {
        self.with_window(cx, |window, cx| {
            window.render_frame(cx);
            window.dispatch_event(
                MouseMoveEvent {
                    position: at,
                    pressed_button: None,
                    modifiers,
                }
                .to_platform_input(),
                cx,
            );
            window.dispatch_event(
                MouseDownEvent {
                    button: MouseButton::Left,
                    position: at,
                    modifiers,
                    click_count,
                    first_mouse: false,
                }
                .to_platform_input(),
                cx,
            );
            window.render_frame(cx);
        });
    }

    pub fn mouse_up(
        &self,
        cx: &mut TestAppContext,
        at: Point<Pixels>,
        click_count: usize,
        modifiers: Modifiers,
    ) {
        self.with_window(cx, |window, cx| {
            window.dispatch_event(
                MouseUpEvent {
                    button: MouseButton::Left,
                    position: at,
                    modifiers,
                    click_count,
                }
                .to_platform_input(),
                cx,
            );
            window.render_frame(cx);
        });
        self.settle(cx);
    }

    /// Moves the pointer to `at` with the button held, as it goes between a press and its
    /// release.
    pub fn move_held(&self, cx: &mut TestAppContext, at: Point<Pixels>, modifiers: Modifiers) {
        self.with_window(cx, |window, cx| {
            window.dispatch_event(
                MouseMoveEvent {
                    position: at,
                    pressed_button: Some(MouseButton::Left),
                    modifiers,
                }
                .to_platform_input(),
                cx,
            );
            window.render_frame(cx);
        });
        cx.run_until_parked();
    }

    /// Clicks `count` times in a row at `at`, as a double or triple click does.
    pub fn clicks(
        &self,
        cx: &mut TestAppContext,
        at: Point<Pixels>,
        count: usize,
        modifiers: Modifiers,
    ) {
        for click_count in 1..=count {
            self.mouse_down(cx, at, click_count, modifiers);
            self.mouse_up(cx, at, click_count, modifiers);
        }
    }

    pub fn click(&self, cx: &mut TestAppContext, at: Point<Pixels>) {
        self.clicks(cx, at, 1, Modifiers::default());
    }

    pub fn click_text(&self, cx: &mut TestAppContext, flow: usize, pos: Pos) {
        let at = self.point_of(cx, flow, pos);
        self.click(cx, at);
    }

    /// Double-clicks the text at `pos` of flow `flow`, which types in a box's text at once,
    /// as a click on it only selects the box.
    pub fn double_click_text(&self, cx: &mut TestAppContext, flow: usize, pos: Pos) {
        let at = self.point_of(cx, flow, pos);
        self.clicks(cx, at, 2, Modifiers::default());
    }

    pub fn drag(&self, cx: &mut TestAppContext, from: Point<Pixels>, to: Point<Pixels>) {
        self.with_window(cx, |window, cx| window.drag(from, to, cx));
        self.settle(cx);
    }

    pub fn type_text(&self, cx: &mut TestAppContext, text: &str) {
        self.with_window(cx, |window, cx| window.input(text, cx));
        self.settle(cx);
    }

    pub fn press(&self, cx: &mut TestAppContext, key: &str) {
        self.with_window(cx, |window, cx| window.press(key, cx));
        self.settle(cx);
    }

    /// Where the caret is in the window, if one is showing.
    pub fn caret(&self, cx: &mut TestAppContext) -> Option<Bounds<Pixels>> {
        self.with_window(cx, |window, cx| {
            window.render_frame(cx);
            window.try_find(CARET_ID).map(|caret| caret.bounds())
        })
    }

    pub fn status(&self, cx: &mut TestAppContext) -> Option<String> {
        self.with_window(cx, |window, cx| {
            window.render_frame(cx);
            window
                .try_find(STATUS_ID)
                .and_then(|status| status.label().map(str::to_string))
        })
    }

    /// The text of flow `flow` of slide `slide`, a paragraph to a line.
    pub fn text_of(&self, cx: &TestAppContext, slide: usize, flow: usize) -> Vec<String> {
        self.flows(cx, slide)[flow]
            .paras
            .iter()
            .map(|para| para.spans.iter().map(|span| span.text.as_str()).collect())
            .collect()
    }
}

pub fn pos(para: usize, offset: usize) -> Pos {
    Pos { para, offset }
}

pub fn caret_at(at: Pos) -> Selection {
    Selection::caret(at)
}

/// Whether the caret drawn in `caret` is where `at` is, give or take a pixel.
pub fn caret_is_at(caret: Bounds<Pixels>, at: Point<Pixels>) -> bool {
    let middle_x = f32::from(caret.origin.x) + f32::from(caret.size.width) / 2.;
    (middle_x - f32::from(at.x)).abs() <= 2.
        && caret.origin.y - px(1.) <= at.y
        && at.y <= caret.origin.y + caret.size.height + px(1.)
}

/// Goes on to slide `index` with the keys, from the first.
pub fn go_to(typing: &Typing, cx: &mut TestAppContext, index: usize) {
    for _ in 0..index {
        typing.press(cx, "pagedown");
    }
    assert_eq!(typing.current(cx), index);
}

/// The thing on the current slide whose source starts with `start`.
pub fn thing(typing: &Typing, cx: &mut TestAppContext, start: &str) -> ElementBox {
    let source = typing.source(cx);
    typing
        .layout(cx)
        .elements()
        .iter()
        .find(|element| source[element.source.clone()].starts_with(start))
        .cloned()
        .unwrap_or_else(|| panic!("no thing starts with {start:?}"))
}

/// The middle of `element` in the window.
pub fn middle(typing: &Typing, cx: &mut TestAppContext, element: &ElementBox) -> Point<Pixels> {
    let rect = element.rect;
    typing.at(cx, rect.x + rect.w / 2., rect.y + rect.h / 2.)
}

/// The slide and the source of the thing selected, if anything is.
pub fn selected(typing: &Typing, cx: &TestAppContext) -> Option<(usize, Range<usize>)> {
    typing.document(cx).read_with(cx, |document, _| {
        document
            .selected()
            .map(|selected| (selected.slide, selected.source.clone()))
    })
}

/// Where the selection's outline is drawn, if it is.
pub fn outline(typing: &Typing, cx: &mut TestAppContext) -> Option<Bounds<Pixels>> {
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window
            .try_find(SELECTION_OUTLINE_ID)
            .map(|outline| outline.bounds())
    })
}

/// Where the element `id` is drawn in the window, if it is.
pub fn bounds_of(typing: &Typing, cx: &mut TestAppContext, id: impl Into<ElementId>) -> Option<Bounds<Pixels>> {
    let id = id.into();
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find(id).map(|found| found.bounds())
    })
}

/// Clicks the element `id`, and lets what it does settle.
pub fn click_on(typing: &Typing, cx: &mut TestAppContext, id: impl Into<ElementId>) {
    let id = id.into();
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    });
    typing.settle(cx);
}

/// Counts how often `entity` asks to be drawn again.
pub fn redraws<T: 'static>(
    cx: &mut TestAppContext,
    entity: &Entity<T>,
) -> (Rc<Cell<usize>>, gpui_kit::Subscription) {
    let count = Rc::new(Cell::new(0));
    let counter = count.clone();
    let watching = cx.update(|cx| cx.observe(entity, move |_, _| counter.set(counter.get() + 1)));
    (count, watching)
}

/// How far past the caret, in slide units, a click aimed at it lands: far less than a letter,
/// but more than rounding to the window's pixels can take away.
const CARET_NUDGE: f32 = 0.05;
