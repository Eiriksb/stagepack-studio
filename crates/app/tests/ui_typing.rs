//! Typing on the slide: clicking its text, typing, selecting, undoing, and what the talk's
//! file holds while it's done.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Context, Entity, EntityInputHandler as _,
    InputEvent as _, KeyDownEvent, Keystroke, Modifiers, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, TestAppContext, VisualTestContext, Window,
    WindowBounds, WindowOptions, point, px, size,
};
use slides_engine::{Fonts, Hit, TextLayout};
use slides_format::{FlowPlace, ParaKind, Pos, Selection, TextFlow, text_flows};
use stagepack_studio::document::{Document, LAYOUT_FAILED, TalkPlace, panic_in_next_layout};
use stagepack_studio::ribbon::{DELETE_ID, DUPLICATE_ID, HIDE_ID, NEW_SLIDE_ID};
use stagepack_studio::recovery::Recovery;
use stagepack_studio::slide_editor::{CARET_ID, COMPOSITION_ID, SLIDE_EDITOR_ID, SlideEditor};
use stagepack_studio::text_editing::{COULD_NOT_TYPE, EditingText};
use stagepack_studio::workspace_view::{self, CLOSE_SAVE_ID, ExportPdf, STATUS_ID, WorkspaceView};

/// Two slides, the first with a title and a line of words.
const TWO: &str = "---\ntitle: T\n---\n\n# Hello\n\nSome words here\n\n---\n\n# Second\n";
const WORDS: &str = "Some words here";

struct Typing {
    _folder: tempfile::TempDir,
    file: PathBuf,
    handle: AnyWindowHandle,
    view: Entity<WorkspaceView>,
}

fn open(cx: &mut TestAppContext, source: &str) -> Typing {
    open_with(cx, source, None)
}

fn open_with(cx: &mut TestAppContext, source: &str, recovery: Option<Recovery>) -> Typing {
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("slides.md");
    std::fs::write(&file, source).unwrap();
    let talk = Document::open(folder.path(), Fonts::bundled_only()).unwrap();
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
                    if let Some(recovery) = recovery {
                        view.set_recovery_for_test(recovery, cx);
                    }
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
fn wait(cx: &mut TestAppContext, millis: u64) {
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
    fn settle(&self, cx: &mut TestAppContext) {
        for _ in 0..3 {
            cx.update_window(self.handle, |_, window, cx| window.render_frame(cx))
                .unwrap();
            cx.run_until_parked();
        }
    }

    fn with_window<T>(
        &self,
        cx: &mut TestAppContext,
        f: impl FnOnce(&mut Window, &mut App) -> T,
    ) -> T {
        cx.update_window(self.handle, |_, window, cx| f(window, cx))
            .unwrap()
    }

    fn document(&self, cx: &TestAppContext) -> Entity<Document> {
        cx.read(|cx| self.view.read(cx).document().unwrap().clone())
    }

    fn source(&self, cx: &TestAppContext) -> String {
        self.document(cx)
            .read_with(cx, |document, _| document.source().to_string())
    }

    fn current(&self, cx: &TestAppContext) -> usize {
        self.document(cx)
            .read_with(cx, |document, _| document.current())
    }

    fn slide_count(&self, cx: &TestAppContext) -> usize {
        self.document(cx)
            .read_with(cx, |document, _| document.slide_count())
    }

    fn editing(&self, cx: &TestAppContext) -> Option<EditingText> {
        self.document(cx)
            .read_with(cx, |document, _| document.text().cloned())
    }

    fn selection(&self, cx: &TestAppContext) -> Selection {
        self.editing(cx)
            .expect("text is being typed")
            .edit
            .selection
    }

    fn flows(&self, cx: &TestAppContext, slide: usize) -> Vec<TextFlow> {
        self.document(cx)
            .read_with(cx, |document, _| text_flows(document.talk(), slide))
    }

    fn layout(&self, cx: &mut TestAppContext) -> Arc<TextLayout> {
        let document = self.document(cx);
        cx.update(|cx| document.update(cx, |document, _| document.text_layout(document.current())))
    }

    /// Where the slide is in the window.
    fn slide_bounds(&self, cx: &TestAppContext) -> Bounds<Pixels> {
        cx.read(|cx| {
            let canvas = self.view.read(cx).slide_canvas().clone();
            canvas
                .read(cx)
                .slide_bounds()
                .expect("the slide has been drawn")
        })
    }

    /// The point of the window at (`x`, `y`) of the slide, in slide units.
    fn at(&self, cx: &TestAppContext, x: f32, y: f32) -> Point<Pixels> {
        let slide = self.slide_bounds(cx);
        let unit = f32::from(slide.size.width) / 1920.;
        slide.origin + point(px(x * unit), px(y * unit))
    }

    /// The middle of where the caret goes at `pos` of flow `flow` of the current slide.
    fn point_of(&self, cx: &mut TestAppContext, flow: usize, pos: Pos) -> Point<Pixels> {
        let caret = self
            .layout(cx)
            .caret(flow, pos)
            .expect("the place has a caret");
        let (x, y) = caret.on_slide(caret.x, caret.top + caret.height / 2.);
        self.at(cx, x, y)
    }

    /// The middle of the empty region `region` of the current slide.
    fn placeholder_point(&self, cx: &mut TestAppContext, region: &str) -> Point<Pixels> {
        let (rect, _) = self
            .layout(cx)
            .placeholder(region)
            .expect("the region is empty");
        self.at(cx, rect.x + rect.w / 2., rect.y + rect.h / 2.)
    }

    fn mouse_down(
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

    fn mouse_up(
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

    /// Clicks `count` times in a row at `at`, as a double or triple click does.
    fn clicks(
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

    fn click(&self, cx: &mut TestAppContext, at: Point<Pixels>) {
        self.clicks(cx, at, 1, Modifiers::default());
    }

    fn click_text(&self, cx: &mut TestAppContext, flow: usize, pos: Pos) {
        let at = self.point_of(cx, flow, pos);
        self.click(cx, at);
    }

    fn drag(&self, cx: &mut TestAppContext, from: Point<Pixels>, to: Point<Pixels>) {
        self.with_window(cx, |window, cx| window.drag(from, to, cx));
        self.settle(cx);
    }

    fn type_text(&self, cx: &mut TestAppContext, text: &str) {
        self.with_window(cx, |window, cx| window.input(text, cx));
        self.settle(cx);
    }

    fn press(&self, cx: &mut TestAppContext, key: &str) {
        self.with_window(cx, |window, cx| window.press(key, cx));
        self.settle(cx);
    }

    /// Where the caret is in the window, if one is showing.
    fn caret(&self, cx: &mut TestAppContext) -> Option<Bounds<Pixels>> {
        self.with_window(cx, |window, cx| {
            window.render_frame(cx);
            window.try_find(CARET_ID).map(|caret| caret.bounds())
        })
    }

    fn status(&self, cx: &mut TestAppContext) -> Option<String> {
        self.with_window(cx, |window, cx| {
            window.render_frame(cx);
            window
                .try_find(STATUS_ID)
                .and_then(|status| status.label().map(str::to_string))
        })
    }

    /// The text of flow `flow` of slide `slide`, a paragraph to a line.
    fn text_of(&self, cx: &TestAppContext, slide: usize, flow: usize) -> Vec<String> {
        self.flows(cx, slide)[flow]
            .paras
            .iter()
            .map(|para| para.spans.iter().map(|span| span.text.as_str()).collect())
            .collect()
    }
}

fn pos(para: usize, offset: usize) -> Pos {
    Pos { para, offset }
}

fn caret_at(at: Pos) -> Selection {
    Selection::caret(at)
}

/// Whether the caret drawn in `caret` is where `at` is, give or take a pixel.
fn caret_is_at(caret: Bounds<Pixels>, at: Point<Pixels>) -> bool {
    let middle_x = f32::from(caret.origin.x) + f32::from(caret.size.width) / 2.;
    (middle_x - f32::from(at.x)).abs() <= 2.
        && caret.origin.y - px(1.) <= at.y
        && at.y <= caret.origin.y + caret.size.height + px(1.)
}

#[gpui_kit::test]
fn clicking_on_text_shows_a_caret_there(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    assert!(typing.caret(cx).is_none(), "no caret before a click");
    let at = typing.point_of(cx, 1, pos(0, 4));
    typing.click(cx, at);
    let editing = typing.editing(cx).expect("the click started typing");
    assert_eq!(
        (editing.slide, editing.edit.selection),
        (0, caret_at(pos(0, 4)))
    );
    let caret = typing.caret(cx).expect("a caret shows");
    assert!(caret_is_at(caret, at), "{caret:?} at {at:?}");
    typing.with_window(cx, |window, _| {
        assert!(window.find(SLIDE_EDITOR_ID).visible())
    });
}

#[gpui_kit::test]
fn typing_adds_letters_at_the_caret_and_the_slide_shows_them(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 4));
    let before = cx.read(|cx| typing.view.read(cx).slide_canvas().read(cx).images());
    let caret_before = typing.caret(cx).unwrap();
    typing.type_text(cx, "XY");
    assert!(
        typing.source(cx).contains("SomeXY words here"),
        "{}",
        typing.source(cx)
    );
    assert_eq!(typing.selection(cx), caret_at(pos(0, 6)));
    let after = cx.read(|cx| typing.view.read(cx).slide_canvas().read(cx).images());
    assert_eq!(before.len(), after.len());
    assert!(
        before
            .iter()
            .zip(&after)
            .any(|(old, new)| !Arc::ptr_eq(old, new)),
        "the paragraph is drawn again"
    );
    let caret = typing.caret(cx).unwrap();
    assert!(
        caret.origin.x > caret_before.origin.x,
        "the caret moved on with the letters"
    );
    let expected = typing.point_of(cx, 1, pos(0, 6));
    assert!(caret_is_at(caret, expected));
}

#[gpui_kit::test]
fn the_markdown_changes_only_in_that_paragraph(cx: &mut TestAppContext) {
    let source = "---\r\ntitle: T  \r\n---\r\n\r\n# Hello   \r\n\r\nFirst *line*\r\n\r\n- one\r\n- two  \r\n\r\n---\r\n\r\n# Next\r\n";
    let typing = open(cx, source);
    typing.click_text(cx, 1, pos(0, 5));
    typing.type_text(cx, "!");
    assert_eq!(
        typing.source(cx),
        source.replacen("First *line*", "First! *line*", 1)
    );
}

#[gpui_kit::test]
fn one_undo_takes_back_a_run_of_typing(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 4));
    typing.type_text(cx, "abc");
    typing.press(cx, "backspace");
    typing.type_text(cx, "d");
    assert!(typing.source(cx).contains("Someabd words"));
    let label = typing
        .document(cx)
        .read_with(cx, |document, _| document.undo_label());
    assert_eq!(label, Some("Typing"));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TWO);
    let can_undo = typing
        .document(cx)
        .read_with(cx, |document, _| document.can_undo());
    assert!(!can_undo, "it was one step");
    typing.press(cx, "ctrl-shift-z");
    assert!(typing.source(cx).contains("Someabd words"));
}

#[gpui_kit::test]
fn moving_the_caret_starts_a_new_step(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 4));
    typing.type_text(cx, "ab");
    typing.press(cx, "left");
    typing.type_text(cx, "c");
    assert!(typing.source(cx).contains("Someacb words"));
    typing.press(cx, "ctrl-z");
    assert!(
        typing.source(cx).contains("Someab words"),
        "{}",
        typing.source(cx)
    );
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TWO);
}

#[gpui_kit::test]
fn a_pause_starts_a_new_step(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 4));
    typing.type_text(cx, "ab");
    wait(cx, 2_500);
    typing.type_text(cx, "c");
    typing.press(cx, "ctrl-z");
    assert!(
        typing.source(cx).contains("Someab words"),
        "{}",
        typing.source(cx)
    );
}

#[gpui_kit::test]
fn undo_puts_the_caret_back(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 4));
    typing.type_text(cx, "abc");
    // Somewhere else, and on another slide.
    typing.press(cx, "escape");
    typing.press(cx, "right");
    assert_eq!(typing.current(cx), 1);

    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TWO);
    assert_eq!(typing.current(cx), 0);
    let editing = typing
        .editing(cx)
        .expect("typing goes on where the change was");
    assert_eq!(editing.edit.selection, caret_at(pos(0, 4)));
    let caret = typing.caret(cx).expect("with its caret");
    let at = typing.point_of(cx, 1, pos(0, 4));
    assert!(caret_is_at(caret, at));

    typing.press(cx, "ctrl-shift-z");
    assert!(typing.source(cx).contains("Someabc words"));
    assert_eq!(
        typing.selection(cx),
        caret_at(pos(0, 7)),
        "a redo puts it after the typing"
    );
    // The keys are the text's again.
    typing.type_text(cx, "d");
    assert!(typing.source(cx).contains("Someabcd words"));
}

#[gpui_kit::test]
fn enter_makes_a_new_bullet(cx: &mut TestAppContext) {
    let typing = open(cx, "# T\n\n- one\n- two\n");
    typing.click_text(cx, 1, pos(0, 3));
    typing.press(cx, "enter");
    typing.type_text(cx, "new");
    assert_eq!(typing.source(cx), "# T\n\n- one\n- new\n- two\n");
}

#[gpui_kit::test]
fn enter_in_a_title_breaks_the_line(cx: &mut TestAppContext) {
    let typing = open(cx, "# Hello\n\nWords\n");
    typing.click_text(cx, 0, pos(0, 5));
    typing.press(cx, "enter");
    typing.type_text(cx, "there");
    assert_eq!(typing.text_of(cx, 0, 0), ["Hello\nthere"]);
    assert_eq!(typing.flows(cx, 0)[0].paras[0].kind, ParaKind::Heading(1));
    assert_eq!(typing.text_of(cx, 0, 1), ["Words"]);
}

#[gpui_kit::test]
fn backspace_at_the_start_joins_paragraphs(cx: &mut TestAppContext) {
    let typing = open(cx, "# T\n\nFirst\n\nSecond\n");
    typing.click_text(cx, 1, pos(1, 0));
    typing.press(cx, "backspace");
    assert_eq!(typing.source(cx), "# T\n\nFirstSecond\n");
    assert_eq!(typing.selection(cx), caret_at(pos(0, 5)));
}

#[gpui_kit::test]
fn tab_indents_a_bullet(cx: &mut TestAppContext) {
    let source = "# T\n\n- one\n- two\n\nWords\n";
    let typing = open(cx, source);
    typing.click_text(cx, 1, pos(1, 1));
    typing.press(cx, "tab");
    let kinds: Vec<ParaKind> = typing.flows(cx, 0)[1]
        .paras
        .iter()
        .map(|para| para.kind)
        .collect();
    assert_eq!(
        kinds,
        [
            ParaKind::Bullet { level: 0 },
            ParaKind::Bullet { level: 1 },
            ParaKind::Body
        ]
    );
    assert!(
        typing.source(cx).starts_with("# T\n\n- one\n"),
        "the item before is left alone"
    );
    assert_eq!(
        typing.text_of(cx, 0, 1),
        ["one", "two", "Words"],
        "and no tab is typed"
    );
    typing.press(cx, "shift-tab");
    assert_eq!(typing.source(cx), source);
    // On a paragraph that isn't a list item, Tab does nothing.
    typing.click_text(cx, 1, pos(2, 2));
    typing.press(cx, "tab");
    assert_eq!(typing.source(cx), source);
}

#[gpui_kit::test]
fn a_drag_selects_and_typing_replaces_it(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    let from = typing.point_of(cx, 1, pos(0, 5));
    let to = typing.point_of(cx, 1, pos(0, 10));
    typing.drag(cx, from, to);
    assert_eq!(
        typing.selection(cx),
        Selection {
            anchor: pos(0, 5),
            head: pos(0, 10)
        }
    );
    typing.type_text(cx, "W");
    assert!(
        typing.source(cx).contains("Some W here"),
        "{}",
        typing.source(cx)
    );
}

#[gpui_kit::test]
fn a_double_click_selects_a_word(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    let at = typing.point_of(cx, 1, pos(0, 7));
    typing.clicks(cx, at, 2, Modifiers::default());
    assert_eq!(
        typing.selection(cx),
        Selection {
            anchor: pos(0, 5),
            head: pos(0, 10)
        }
    );
    typing.clicks(cx, at, 3, Modifiers::default());
    assert_eq!(
        typing.selection(cx),
        Selection {
            anchor: pos(0, 0),
            head: pos(0, WORDS.len())
        }
    );
}

#[gpui_kit::test]
fn shift_arrows_extend(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 4));
    for _ in 0..3 {
        typing.press(cx, "shift-right");
    }
    assert_eq!(
        typing.selection(cx),
        Selection {
            anchor: pos(0, 4),
            head: pos(0, 7)
        }
    );
    typing.press(cx, "shift-left");
    assert_eq!(
        typing.selection(cx),
        Selection {
            anchor: pos(0, 4),
            head: pos(0, 6)
        }
    );
    typing.press(cx, "shift-end");
    assert_eq!(
        typing.selection(cx),
        Selection {
            anchor: pos(0, 4),
            head: pos(0, WORDS.len())
        }
    );
    typing.press(cx, "ctrl-shift-left");
    assert_eq!(
        typing.selection(cx),
        Selection {
            anchor: pos(0, 4),
            head: pos(0, 11)
        }
    );
    typing.press(cx, "shift-home");
    assert_eq!(
        typing.selection(cx),
        Selection {
            anchor: pos(0, 4),
            head: pos(0, 0)
        }
    );
    // Shift+click takes it to where the click is.
    let at = typing.point_of(cx, 1, pos(0, 10));
    typing.clicks(cx, at, 1, Modifiers::shift());
    assert_eq!(
        typing.selection(cx),
        Selection {
            anchor: pos(0, 4),
            head: pos(0, 10)
        }
    );
    typing.press(cx, "ctrl-a");
    assert_eq!(
        typing.selection(cx),
        Selection {
            anchor: pos(0, 0),
            head: pos(0, WORDS.len())
        }
    );
}

#[gpui_kit::test]
fn up_and_down_keep_to_their_column(cx: &mut TestAppContext) {
    let typing = open(
        cx,
        "# T\n\nA long first line\n\nShort\n\nAnother long line\n",
    );
    typing.click_text(cx, 1, pos(0, 12));
    typing.press(cx, "down");
    assert_eq!(typing.selection(cx).head.para, 1);
    assert_eq!(typing.selection(cx).head.offset, 5, "the short line's end");
    typing.press(cx, "down");
    let head = typing.selection(cx).head;
    assert_eq!(head.para, 2);
    assert!(
        head.offset > 5,
        "back near where it started across: {head:?}"
    );
    typing.press(cx, "ctrl-home");
    assert_eq!(typing.selection(cx), caret_at(pos(0, 0)));
    typing.press(cx, "ctrl-end");
    assert_eq!(typing.selection(cx), caret_at(pos(2, 17)));
}

#[gpui_kit::test]
fn clicking_click_to_add_title_and_typing_makes_a_heading(cx: &mut TestAppContext) {
    let source = "---\ntitle: T\n---\n\nJust a line\n";
    let typing = open(cx, source);
    let at = typing.placeholder_point(cx, "title");
    typing.click(cx, at);
    assert!(typing.editing(cx).is_some());
    assert!(
        typing.caret(cx).is_some(),
        "a caret shows in the empty title"
    );
    assert_eq!(
        typing.source(cx),
        source,
        "nothing is written before anything is typed"
    );
    typing.type_text(cx, "Hi there");
    let flows = typing.flows(cx, 0);
    assert_eq!(flows[0].place, FlowPlace::Region("title".into()));
    assert_eq!(flows[0].paras[0].kind, ParaKind::Heading(1));
    assert_eq!(typing.text_of(cx, 0, 0), ["Hi there"]);
    assert_eq!(typing.text_of(cx, 0, 1), ["Just a line"]);
    let caret = typing.caret(cx).unwrap();
    assert!(caret_is_at(caret, typing.point_of(cx, 0, pos(0, 8))));
}

#[gpui_kit::test]
fn typing_in_the_right_column_adds_its_marker(cx: &mut TestAppContext) {
    let typing = open(cx, "---\nlayout: two-columns\n---\n\n# T\n\nleft text\n");
    let at = typing.placeholder_point(cx, "right");
    typing.click(cx, at);
    typing.type_text(cx, "Right side");
    let source = typing.source(cx);
    assert!(source.contains("<Region name=\"right\" />"), "{source}");
    assert!(
        source.starts_with("---\nlayout: two-columns\n---\n\n# T\n\nleft text\n"),
        "{source}"
    );
    let flows = typing.flows(cx, 0);
    assert_eq!(flows[2].place, FlowPlace::Region("right".into()));
    assert_eq!(typing.text_of(cx, 0, 2), ["Right side"]);
}

#[gpui_kit::test]
fn a_new_slide_can_be_typed_into_straight_away(cx: &mut TestAppContext) {
    let typing = open(cx, "# One\n");
    typing.press(cx, "ctrl-m");
    assert_eq!((typing.slide_count(cx), typing.current(cx)), (2, 1));
    let title = typing.layout(cx).paras()[0].rect;
    let at = typing.at(cx, title.x + title.w / 2., title.y + title.h / 2.);
    typing.click(cx, at);
    typing.type_text(cx, "Two");
    let at = typing.placeholder_point(cx, "body");
    typing.click(cx, at);
    typing.type_text(cx, "Its words");
    assert_eq!(typing.text_of(cx, 1, 0), ["Two"]);
    assert_eq!(typing.text_of(cx, 1, 1), ["Its words"]);
    assert_eq!(typing.text_of(cx, 0, 0), ["One"]);
    assert_eq!(typing.slide_count(cx), 2);
}

#[gpui_kit::test]
fn clicking_a_table_says_it_cannot_be_edited_yet(cx: &mut TestAppContext) {
    let source = "# T\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";
    let typing = open(cx, source);
    let layout = typing.layout(cx);
    let (x, y) = (0..40)
        .flat_map(|row| (0..40).map(move |column| (column as f32 * 48., row as f32 * 27.)))
        .find(|&(x, y)| matches!(layout.hit(x, y), Some(Hit::Uneditable(_))))
        .expect("the table is somewhere");
    let at = typing.at(cx, x, y);
    typing.click(cx, at);
    assert_eq!(
        typing.status(cx).as_deref(),
        Some("Tables can't be edited on the slide yet.")
    );
    assert!(typing.editing(cx).is_none());
    assert!(typing.caret(cx).is_none());
    assert_eq!(typing.source(cx), source);
}

#[gpui_kit::test]
fn what_cannot_be_typed_is_refused_and_said(cx: &mut TestAppContext) {
    // A small heading of more than one line can't be written.
    let source = "# T\n\n### Small\n";
    let typing = open(cx, source);
    typing.click_text(cx, 1, pos(0, 5));
    typing.press(cx, "shift-enter");
    assert_eq!(typing.source(cx), source);
    assert_eq!(typing.status(cx).as_deref(), Some(COULD_NOT_TYPE));
    assert_eq!(
        typing.selection(cx),
        caret_at(pos(0, 5)),
        "and the text is as it was"
    );
    typing.type_text(cx, "er");
    assert_eq!(typing.text_of(cx, 0, 1), ["Smaller"]);
}

#[gpui_kit::test]
fn escape_ends_editing(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 4));
    typing.press(cx, "escape");
    assert!(typing.editing(cx).is_none());
    assert!(typing.caret(cx).is_none());
    typing.press(cx, "right");
    assert_eq!(typing.current(cx), 1, "the keys are the slides' again");
}

#[gpui_kit::test]
fn a_click_on_the_desk_ends_editing(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 4));
    let slide = typing.slide_bounds(cx);
    typing.click(cx, slide.origin - point(px(4.), px(4.)));
    assert!(typing.editing(cx).is_none());
    assert!(typing.caret(cx).is_none());
}

#[gpui_kit::test]
fn arrow_keys_move_the_caret_not_the_slide(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 4));
    typing.press(cx, "right");
    assert_eq!(typing.current(cx), 0);
    assert_eq!(typing.selection(cx), caret_at(pos(0, 5)));
    typing.press(cx, "left");
    typing.press(cx, "left");
    assert_eq!(typing.selection(cx), caret_at(pos(0, 3)));
    for key in [
        "pagedown",
        "pageup",
        "down",
        "up",
        "ctrl-m",
        "ctrl-d",
        "delete",
        "backspace",
    ] {
        typing.press(cx, key);
        assert_eq!(
            (typing.current(cx), typing.slide_count(cx)),
            (0, 2),
            "{key}"
        );
    }
    assert!(typing.editing(cx).is_some());
}

#[gpui_kit::test]
fn text_being_typed_is_in_the_talk_before_a_save(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 4));
    typing.type_text(cx, "abc");
    cx.simulate_keystrokes(typing.handle, "ctrl-s");
    cx.run_until_parked();
    assert_eq!(
        std::fs::read_to_string(&typing.file).unwrap(),
        typing.source(cx)
    );
    assert!(typing.source(cx).contains("Someabc words"));
    assert!(typing.editing(cx).is_some(), "typing goes on after a save");
}

#[gpui_kit::test]
fn text_being_typed_is_in_the_talk_before_a_close_answered_with_save(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 4));
    typing.type_text(cx, "abc");
    assert!(!VisualTestContext::from_window(typing.handle, cx).simulate_close());
    cx.run_until_parked();
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.click(CLOSE_SAVE_ID, cx);
    });
    cx.run_until_parked();
    let written = std::fs::read_to_string(&typing.file).unwrap();
    assert!(written.contains("Someabc words"), "{written}");
    assert!(cx.read(|cx| cx.windows().is_empty()));
}

#[gpui_kit::test]
fn text_being_typed_is_in_the_talk_before_a_show(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 4));
    typing.type_text(cx, "abc");
    cx.simulate_keystrokes(typing.handle, "f5");
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| cx.windows().len()), 2, "the show started");
    assert!(
        typing.source(cx).contains("Someabc words"),
        "and shows what was typed"
    );
}

#[gpui_kit::test]
fn the_working_copy_follows_typing(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let recovery = Recovery::new(data.path().join("recovery"));
    let typing = open_with(cx, TWO, Some(recovery.clone()));
    let place = TalkPlace::Folder(typing.file.parent().unwrap().to_path_buf());
    typing.click_text(cx, 1, pos(0, 4));
    typing.type_text(cx, "abc");
    wait(cx, 1_000);
    assert_eq!(
        recovery.find(&place).map(|found| found.text),
        Some(typing.source(cx))
    );
    typing.type_text(cx, "d");
    wait(cx, 1_000);
    let kept = recovery.find(&place).unwrap().text;
    assert!(kept.contains("Someabcd words"), "{kept}");
}

/// Counts how often `entity` asks to be drawn again.
fn redraws<T: 'static>(
    cx: &mut TestAppContext,
    entity: &Entity<T>,
) -> (Rc<Cell<usize>>, gpui_kit::Subscription) {
    let count = Rc::new(Cell::new(0));
    let counter = count.clone();
    let watching = cx.update(|cx| cx.observe(entity, move |_, _| counter.set(counter.get() + 1)));
    (count, watching)
}

#[gpui_kit::test]
fn the_caret_stops_blinking_when_left_alone(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 4));
    let editor = cx.read(|cx| typing.view.read(cx).slide_editor().clone());
    let (count, _watching) = redraws(cx, &editor);
    wait(cx, 2_000);
    assert!(count.get() > 0, "it blinks at first");
    wait(cx, 10_000);
    count.set(0);
    wait(cx, 10_000);
    assert_eq!(count.get(), 0, "then it stays still");
    assert!(typing.caret(cx).is_some(), "and showing");
    typing.type_text(cx, "a");
    count.set(0);
    wait(cx, 2_000);
    assert!(count.get() > 0, "and typing starts it again");
}

#[gpui_kit::test]
fn an_idle_window_with_no_caret_does_nothing(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    let editor = cx.read(|cx| typing.view.read(cx).slide_editor().clone());
    let canvas = cx.read(|cx| typing.view.read(cx).slide_canvas().clone());
    let (editor_count, _a) = redraws(cx, &editor);
    let (view_count, _b) = redraws(cx, &typing.view);
    let (canvas_count, _c) = redraws(cx, &canvas);
    let quiet = |cx: &mut TestAppContext, what: &str| {
        editor_count.set(0);
        view_count.set(0);
        canvas_count.set(0);
        wait(cx, 20_000);
        assert_eq!(
            (editor_count.get(), view_count.get(), canvas_count.get()),
            (0, 0, 0),
            "{what}"
        );
    };
    quiet(cx, "before any typing");
    typing.click_text(cx, 1, pos(0, 4));
    typing.type_text(cx, "abc");
    typing.press(cx, "escape");
    // The working copy is kept a moment after the typing.
    wait(cx, 5_000);
    quiet(cx, "once typing has ended");
}

#[gpui_kit::test]
fn typing_in_a_turned_box_works(cx: &mut TestAppContext) {
    let source =
        "# T\n\n<Text x=\"600\" y=\"300\" w=\"600\" h=\"200\" rotate=\"30\">\n\nHello\n\n</Text>\n";
    let typing = open(cx, source);
    let at = typing.point_of(cx, 1, pos(0, 5));
    typing.click(cx, at);
    let editing = typing.editing(cx).expect("the box's text is typed into");
    assert_eq!(editing.edit.selection, caret_at(pos(0, 5)));
    typing.type_text(cx, "!");
    assert_eq!(typing.source(cx), source.replacen("Hello", "Hello!", 1));
    // The caret is drawn turned with the box, where the turned text ends.
    let caret = typing.caret(cx).unwrap();
    let end = typing.point_of(cx, 1, pos(0, 6));
    let middle = caret.center();
    assert!(
        (middle.x - end.x).abs() <= px(2.) && (middle.y - end.y).abs() <= px(2.),
        "{caret:?} at {end:?}"
    );
}

#[gpui_kit::test]
fn a_reload_while_typing_ends_editing_without_losing_what_was_typed(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 4));
    typing.type_text(cx, "abc");
    cx.simulate_keystrokes(typing.handle, "ctrl-s");
    cx.run_until_parked();

    // Changed elsewhere after the save: the window takes it.
    let theirs = format!("{}\n---\n\n# Added elsewhere\n", typing.source(cx));
    std::fs::write(&typing.file, &theirs).unwrap();
    cx.update(|cx| {
        typing.view.update(cx, |view, cx| {
            view.reload_for_test(Some(theirs.clone()), cx)
        })
    });
    typing.settle(cx);
    assert!(typing.editing(cx).is_none(), "typing ends");
    assert!(typing.caret(cx).is_none());
    assert_eq!(typing.source(cx), theirs);
    assert!(typing.source(cx).contains("Someabc words"));

    // Changed elsewhere with typing unsaved: nothing is taken until asked.
    typing.click_text(cx, 1, pos(0, 7));
    typing.type_text(cx, "d");
    let mine = typing.source(cx);
    cx.update(|cx| {
        typing.view.update(cx, |view, cx| {
            view.reload_for_test(Some(TWO.to_string()), cx)
        })
    });
    typing.settle(cx);
    assert_eq!(typing.source(cx), mine);
    assert!(mine.contains("Someabcd words"));
}

impl Typing {
    fn can_undo(&self, cx: &TestAppContext) -> bool {
        self.document(cx)
            .read_with(cx, |document, _| document.can_undo())
    }

    fn undo(&self, cx: &mut TestAppContext) {
        let document = self.document(cx);
        cx.update(|cx| document.update(cx, |document, _| document.undo().unwrap()));
        self.settle(cx);
    }

    fn click_id(&self, cx: &mut TestAppContext, id: &'static str) {
        self.with_window(cx, |window, cx| {
            window.render_frame(cx);
            window.click(id, cx);
        });
        self.settle(cx);
    }
}

/// A region whose text a table splits in two.
const SPLIT: &str = "# T\n\nfirst\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nsecond\n";
/// And in three.
const SPLIT_THREE: &str =
    "# T\n\nfirst\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nsecond\n\n| c | d |\n|---|---|\n| 3 | 4 |\n\nthird\n";

fn redo_of_a_tidy_up(cx: &mut TestAppContext, source: &str) {
    let typing = open(cx, source);
    assert_eq!(typing.text_of(cx, 0, 2), ["second"]);
    typing.click_text(cx, 2, pos(0, 3));
    typing.press(cx, "ctrl-a");
    typing.press(cx, "backspace");
    typing.press(cx, "escape");
    assert!(!typing.source(cx).contains("second"));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), source);
    assert_eq!(typing.editing(cx).map(|editing| editing.flow), {
        let flows = typing.flows(cx, 0);
        stagepack_studio::text_editing::FlowKey::of(&flows, 2)
    });
    typing.press(cx, "ctrl-shift-z");
    let tidied = typing.source(cx);
    assert!(!tidied.contains("second"), "{tidied}");
    assert!(typing.editing(cx).is_none(), "the text typed in has gone, so typing ends");
    assert!(typing.caret(cx).is_none());
    typing.type_text(cx, "x");
    assert_eq!(typing.source(cx), tidied, "nothing else is typed into");
}

#[gpui_kit::test]
fn a_redo_of_a_tidy_up_types_nowhere_else(cx: &mut TestAppContext) {
    redo_of_a_tidy_up(cx, SPLIT);
}

#[gpui_kit::test]
fn a_redo_of_a_tidy_up_types_nowhere_else_with_more_text_after(cx: &mut TestAppContext) {
    redo_of_a_tidy_up(cx, SPLIT_THREE);
}

/// Two slides, the first with nothing in its body.
const EMPTY_BODY: &str = "# T\n\n---\n\n# Two\n";

/// `EMPTY_BODY`, with two empty lines typed into the first slide's body, and typing going on.
fn typed_empty_lines(cx: &mut TestAppContext) -> Typing {
    let typing = open(cx, EMPTY_BODY);
    let at = typing.placeholder_point(cx, "body");
    typing.click(cx, at);
    typing.press(cx, "enter");
    typing.press(cx, "enter");
    assert!(typing.source(cx).contains("<Text></Text>"), "{}", typing.source(cx));
    assert!(typing.editing(cx).is_some());
    typing
}

/// A ribbon button pressed while typing leaves what typing left empty tidy: no empty lines in
/// the talk, and one undo takes the button's edit back to the talk as it was opened.
fn ribbon_while_typing(cx: &mut TestAppContext, id: &'static str) -> Typing {
    let typing = typed_empty_lines(cx);
    typing.click_id(cx, id);
    assert!(typing.editing(cx).is_none(), "typing ended");
    assert!(!typing.source(cx).contains("<Text"), "{}", typing.source(cx));
    typing.undo(cx);
    assert_eq!(typing.source(cx), EMPTY_BODY);
    assert!(!typing.can_undo(cx), "the typing left no step");
    typing
}

#[gpui_kit::test]
fn new_slide_while_typing_tidies_up_first(cx: &mut TestAppContext) {
    ribbon_while_typing(cx, NEW_SLIDE_ID);
}

#[gpui_kit::test]
fn duplicate_while_typing_tidies_up_first(cx: &mut TestAppContext) {
    ribbon_while_typing(cx, DUPLICATE_ID);
}

#[gpui_kit::test]
fn hide_while_typing_tidies_up_first(cx: &mut TestAppContext) {
    ribbon_while_typing(cx, HIDE_ID);
}

#[gpui_kit::test]
fn delete_while_typing_tidies_up_first(cx: &mut TestAppContext) {
    ribbon_while_typing(cx, DELETE_ID);
}

#[gpui_kit::test]
fn a_save_while_typing_tidies_up_and_typing_goes_on(cx: &mut TestAppContext) {
    let typing = typed_empty_lines(cx);
    cx.simulate_keystrokes(typing.handle, "ctrl-s");
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&typing.file).unwrap(), EMPTY_BODY);
    assert_eq!(typing.source(cx), EMPTY_BODY);
    let saved = typing
        .document(cx)
        .read_with(cx, |document, _| !document.has_unsaved_changes());
    assert!(saved);
    let editing = typing.editing(cx).expect("typing goes on in the empty body");
    assert_eq!(editing.flow.region(), Some("body"));
    assert!(typing.caret(cx).is_some());
    typing.type_text(cx, "Words");
    assert_eq!(typing.text_of(cx, 0, 1), ["Words"]);
}

#[gpui_kit::test]
fn a_show_while_typing_tidies_up_first(cx: &mut TestAppContext) {
    let typing = typed_empty_lines(cx);
    cx.simulate_keystrokes(typing.handle, "f5");
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| cx.windows().len()), 2, "the show started");
    assert_eq!(typing.source(cx), EMPTY_BODY);
}

#[gpui_kit::test]
fn an_export_while_typing_tidies_up_first(cx: &mut TestAppContext) {
    let typing = typed_empty_lines(cx);
    cx.dispatch_action(typing.handle, ExportPdf);
    cx.run_until_parked();
    assert!(cx.did_prompt_for_new_path(), "a Save dialog is showing");
    assert_eq!(typing.source(cx), EMPTY_BODY);
    cx.simulate_new_path_selection(|_| None);
    cx.run_until_parked();
}

#[gpui_kit::test]
fn an_emptied_title_gives_back_its_placeholder(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 0, pos(0, 2));
    typing.press(cx, "ctrl-a");
    typing.press(cx, "backspace");
    typing.press(cx, "escape");
    let source = typing.source(cx);
    assert!(!source.lines().any(|line| line.trim() == "#"), "{source:?}");
    assert!(source.contains(WORDS));
    assert!(typing.layout(cx).placeholder("title").is_some(), "Click to add title is back");
}

#[gpui_kit::test]
fn an_emptied_list_gives_back_its_placeholder(cx: &mut TestAppContext) {
    let typing = open(cx, "# T\n\n- one\n- two\n");
    typing.click_text(cx, 1, pos(0, 2));
    typing.press(cx, "ctrl-a");
    typing.press(cx, "backspace");
    typing.press(cx, "escape");
    let source = typing.source(cx);
    assert!(!source.lines().any(|line| line.trim() == "-"), "{source:?}");
    assert!(typing.flows(cx, 0).len() == 1, "{source:?}");
    assert!(typing.layout(cx).placeholder("body").is_some(), "Click to add text is back");
}

#[gpui_kit::test]
fn text_that_keeps_some_words_keeps_its_empty_lines(cx: &mut TestAppContext) {
    let typing = open(cx, "# T\n\nWords\n");
    typing.click_text(cx, 1, pos(0, 5));
    typing.press(cx, "enter");
    typing.press(cx, "escape");
    assert_eq!(typing.text_of(cx, 0, 1), ["Words", ""]);
}

#[gpui_kit::test]
fn a_tidy_up_that_undoes_the_typing_leaves_no_step(cx: &mut TestAppContext) {
    let typing = open(cx, "# T\n");
    let at = typing.placeholder_point(cx, "body");
    typing.click(cx, at);
    typing.press(cx, "enter");
    wait(cx, 3_000);
    typing.press(cx, "escape");
    assert_eq!(typing.source(cx), "# T\n");
    assert!(!typing.can_undo(cx), "the history is as it was");
}

#[gpui_kit::test]
fn a_tidy_up_joins_the_step_before_it(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 2));
    typing.press(cx, "ctrl-a");
    typing.press(cx, "backspace");
    wait(cx, 3_000);
    typing.press(cx, "escape");
    assert!(!typing.source(cx).contains("<Text"));
    typing.undo(cx);
    assert_eq!(typing.source(cx), TWO, "one undo brings the words back");
    assert!(!typing.can_undo(cx));
}

#[gpui_kit::test]
fn a_fault_laying_out_the_slide_is_said_and_not_fatal(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    let at = typing.point_of(cx, 1, pos(0, 4));
    panic_in_next_layout();
    typing.click(cx, at);
    assert_eq!(typing.status(cx).as_deref(), Some(LAYOUT_FAILED));
    assert!(typing.editing(cx).is_none());
    // Laid out again, it works. The status line took some room, so the slide moved.
    let at = typing.point_of(cx, 1, pos(0, 4));
    typing.click(cx, at);
    assert_eq!(typing.selection(cx), caret_at(pos(0, 4)));
    typing.press(cx, "escape");

    panic_in_next_layout();
    typing.press(cx, "right");
    assert_eq!(typing.current(cx), 1);
    assert_eq!(typing.status(cx).as_deref(), Some(LAYOUT_FAILED));
    let images = cx.read(|cx| typing.view.read(cx).slide_canvas().read(cx).images());
    assert!(images.is_empty(), "nothing is drawn of a slide that couldn't be laid out");
    typing.press(cx, "left");
    let images = cx.read(|cx| typing.view.read(cx).slide_canvas().read(cx).images());
    assert!(!images.is_empty());
}

#[gpui_kit::test]
fn a_drag_off_the_slide_keeps_selecting(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    let from = typing.point_of(cx, 1, pos(0, 5));
    let slide = typing.slide_bounds(cx);
    let to = point(slide.origin.x + slide.size.width + px(40.), from.y);
    typing.drag(cx, from, to);
    assert_eq!(
        typing.selection(cx),
        Selection {
            anchor: pos(0, 5),
            head: pos(0, WORDS.len())
        }
    );
    // Below the slide, under the end of "words": the nearest place is there.
    let across = typing.point_of(cx, 1, pos(0, 10)).x;
    let below = point(across, slide.origin.y + slide.size.height + px(40.));
    typing.drag(cx, from, below);
    let dragged = Selection {
        anchor: pos(0, 5),
        head: pos(0, 10),
    };
    assert_eq!(typing.selection(cx), dragged);
    // Let go of off the editor, the drag is over: a later press elsewhere selects nothing.
    let elsewhere = typing.point_of(cx, 1, pos(0, 2));
    typing.with_window(cx, |window, cx| {
        window.dispatch_event(
            MouseMoveEvent {
                position: elsewhere,
                pressed_button: Some(MouseButton::Left),
                modifiers: Modifiers::default(),
            }
            .to_platform_input(),
            cx,
        );
    });
    typing.settle(cx);
    assert_eq!(typing.selection(cx), dragged);
}

// The system's input method: dead keys, compose sequences, the emoji picker and the keys
// that type, each typing once.

impl Typing {
    fn editor(&self, cx: &TestAppContext) -> Entity<SlideEditor> {
        cx.read(|cx| self.view.read(cx).slide_editor().clone())
    }

    /// Does `ask` of the editor as the system's input method would, and draws.
    fn system<T>(
        &self,
        cx: &mut TestAppContext,
        ask: impl FnOnce(&mut SlideEditor, &mut Window, &mut Context<SlideEditor>) -> T,
    ) -> T {
        let editor = self.editor(cx);
        let answer = self.with_window(cx, |window, cx| {
            editor.update(cx, |editor, cx| ask(editor, window, cx))
        });
        self.settle(cx);
        answer
    }

    /// The system composes `text` at the caret, as a dead key does.
    fn compose(&self, cx: &mut TestAppContext, text: &str) {
        self.system(cx, |editor, window, cx| {
            editor.replace_and_mark_text_in_range(None, text, None, window, cx)
        });
    }

    /// The system types `text`, as it does a letter, a finished composition or an emoji.
    fn commit(&self, cx: &mut TestAppContext, text: &str) {
        self.system(cx, |editor, window, cx| {
            editor.replace_text_in_range(None, text, window, cx)
        });
    }

    fn composing(&self, cx: &TestAppContext) -> Option<String> {
        self.editing(cx)?.composing.map(|composing| composing.text)
    }

    fn system_selection(&self, cx: &mut TestAppContext) -> std::ops::Range<usize> {
        self.system(cx, |editor, window, cx| {
            editor.selected_text_range(false, window, cx)
        })
        .expect("text is being typed")
        .range
    }

    fn marked(&self, cx: &mut TestAppContext) -> Option<std::ops::Range<usize>> {
        self.system(cx, |editor, window, cx| {
            editor.marked_text_range(window, cx)
        })
    }

    /// The composition as it's shown, if it is.
    fn composition(&self, cx: &mut TestAppContext) -> Option<(Bounds<Pixels>, String)> {
        self.with_window(cx, |window, cx| {
            window.render_frame(cx);
            window.try_find(COMPOSITION_ID).map(|shown| {
                assert!(shown.visible());
                (
                    shown.bounds(),
                    shown.label().unwrap_or_default().to_string(),
                )
            })
        })
    }

    /// A key pressed that gives `text`, as the platform reports it, and the text it types, if
    /// it isn't taken as a key.
    fn key(&self, cx: &mut TestAppContext, key: &str, modifiers: Modifiers, text: &str) {
        let keystroke = Keystroke {
            modifiers,
            key: key.into(),
            key_char: Some(text.into()),
        };
        self.with_window(cx, |window, cx| {
            window.render_frame(cx);
            window.dispatch_keystroke(keystroke, cx);
        });
        self.settle(cx);
    }
}

fn ctrl_alt() -> Modifiers {
    Modifiers {
        control: true,
        alt: true,
        ..Modifiers::default()
    }
}

#[gpui_kit::test]
fn a_dead_key_then_a_letter_gives_one_character(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 4));
    typing.compose(cx, "´");
    typing.commit(cx, "é");
    assert_eq!(
        typing.source(cx),
        TWO.replacen("Some words", "Someé words", 1)
    );
    assert_eq!(typing.selection(cx), caret_at(pos(0, 4 + "é".len())));
    assert_eq!(typing.composing(cx), None);
    assert_eq!(typing.system_selection(cx), 5..5);
    assert!(
        typing.composition(cx).is_none(),
        "nothing is left composing"
    );
}

#[gpui_kit::test]
fn a_dead_key_from_the_keyboard_types_once(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 4));
    // Where the system composes it, as on Linux: the letter's key gives the finished one.
    typing.compose(cx, "¨");
    typing.key(cx, "u", Modifiers::default(), "ü");
    // Where it doesn't, as on Windows: the dead key's own press types nothing, and the
    // character comes after the letter.
    typing.with_window(cx, |window, cx| {
        window.dispatch_event(
            KeyDownEvent {
                keystroke: Keystroke {
                    modifiers: Modifiers::default(),
                    key: "´".into(),
                    key_char: Some("´".into()),
                },
                is_held: false,
                prefer_character_input: true,
            }
            .to_platform_input(),
            cx,
        );
    });
    typing.settle(cx);
    typing.commit(cx, "é");
    // A dead key then a space gives the accent itself.
    typing.compose(cx, "^");
    typing.key(cx, "space", Modifiers::default(), "^");
    assert_eq!(
        typing.source(cx),
        TWO.replacen("Some words", "Someüé^ words", 1)
    );
    assert_eq!(typing.composing(cx), None);
}

#[gpui_kit::test]
fn the_keys_that_type_type_once(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 4));
    typing.type_text(cx, "ab");
    typing.press(cx, "shift-c");
    typing.press(cx, "space");
    // AltGr, which the system reports as Ctrl and Alt together.
    typing.key(cx, "2", ctrl_alt(), "@");
    typing.key(cx, "7", ctrl_alt(), "{");
    typing.key(cx, "e", ctrl_alt(), "€");
    typing.type_text(cx, "øæå");
    assert_eq!(
        typing.source(cx),
        TWO.replacen("Some words", "SomeabC @{€øæå words", 1)
    );
    // A key that's the editor's own does what it does, and types nothing as well.
    typing.key(cx, "enter", Modifiers::default(), "\n");
    assert_eq!(typing.text_of(cx, 0, 1), ["SomeabC @{€øæå", " words here"]);
}

#[gpui_kit::test]
fn composing_text_shows_underlined_and_is_not_in_the_talk(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 4));
    let caret = typing.caret(cx).unwrap();
    typing.compose(cx, "´");
    assert_eq!(typing.source(cx), TWO, "the talk doesn't have it");
    assert!(!typing.can_undo(cx));
    assert_eq!(typing.composing(cx).as_deref(), Some("´"));
    let (shown, text) = typing.composition(cx).expect("it's shown");
    assert_eq!(text, "´");
    assert!(
        (shown.origin.x - (caret.origin.x + caret.size.width)).abs() <= px(1.)
            && (shown.origin.y - caret.origin.y).abs() <= px(1.),
        "at the caret: {shown:?} by {caret:?}"
    );
    // The system sees it where the caret was, and its caret after it.
    assert_eq!(typing.marked(cx), Some(4..5));
    assert_eq!(typing.system_selection(cx), 5..5);
    let text = typing.system(cx, |editor, window, cx| {
        let mut adjusted = None;
        editor.text_for_range(0..100, &mut adjusted, window, cx)
    });
    assert_eq!(text.as_deref(), Some("Some´ words here"));
    // The system goes on composing.
    typing.compose(cx, "´´");
    assert_eq!(typing.composition(cx).unwrap().1, "´´");
    assert_eq!(typing.source(cx), TWO);
}

#[gpui_kit::test]
fn a_cancelled_composition_leaves_no_text(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 4));
    typing.compose(cx, "´");
    typing.commit(cx, "");
    assert_eq!(typing.composing(cx), None);
    assert!(typing.composition(cx).is_none());
    // Cancelled by its range, as some systems do.
    typing.compose(cx, "~");
    let marked = typing.marked(cx).unwrap();
    typing.system(cx, |editor, window, cx| {
        editor.replace_text_in_range(Some(marked), "", window, cx)
    });
    assert_eq!(typing.composing(cx), None);
    // Or by composing nothing.
    typing.compose(cx, "`");
    typing.compose(cx, "");
    assert_eq!(typing.composing(cx), None);
    // Over a selection, which stays as it was.
    let word = typing.point_of(cx, 1, pos(0, 6));
    typing.clicks(cx, word, 2, Modifiers::default());
    let words = typing.selection(cx);
    typing.compose(cx, "´");
    typing.commit(cx, "");
    assert_eq!(typing.selection(cx), words);

    assert_eq!(typing.source(cx), TWO);
    assert!(!typing.can_undo(cx), "and no step to undo");
}

#[gpui_kit::test]
fn a_committed_composition_is_one_undo_step(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 4));
    typing.compose(cx, "´");
    typing.compose(cx, "´");
    typing.commit(cx, "é");
    let label = typing
        .document(cx)
        .read_with(cx, |document, _| document.undo_label());
    assert_eq!(label, Some("Typing"));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TWO);
    assert!(!typing.can_undo(cx), "it was one step");

    // Among typed letters, it's one of them: the run is one step.
    typing.click_text(cx, 1, pos(0, 4));
    typing.type_text(cx, "a");
    typing.compose(cx, "¨");
    typing.commit(cx, "ö");
    typing.type_text(cx, "b");
    // Committed as it is, when the system ends it so.
    typing.compose(cx, "~");
    typing.system(cx, |editor, window, cx| editor.unmark_text(window, cx));
    assert_eq!(
        typing.source(cx),
        TWO.replacen("Some words", "Someaöb~ words", 1)
    );
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TWO);
    assert!(!typing.can_undo(cx));
}

#[gpui_kit::test]
fn an_emoji_from_the_picker_lands_at_the_caret(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 4));
    typing.commit(cx, "😀");
    assert_eq!(
        typing.source(cx),
        TWO.replacen("Some words", "Some😀 words", 1)
    );
    assert_eq!(typing.selection(cx), caret_at(pos(0, 4 + "😀".len())));
    assert_eq!(typing.system_selection(cx), 6..6, "two UTF-16 units");
    // One with a skin tone is one character, which the caret goes past whole.
    typing.commit(cx, "👍🏽");
    typing.press(cx, "left");
    assert_eq!(typing.selection(cx), caret_at(pos(0, 4 + "😀".len())));
}

#[gpui_kit::test]
fn utf16_offsets_round_trip_across_emoji_and_accents(cx: &mut TestAppContext) {
    let source = "# T\n\n- Hé😀\n- e\u{301}👍🏽 x\n";
    let typing = open(cx, source);
    typing.click_text(cx, 1, pos(0, 0));
    let set = |cx: &mut TestAppContext, range: std::ops::Range<usize>| {
        typing.system(cx, |editor, window, cx| {
            editor.set_selected_text_range(range, window, cx)
        });
    };
    // Each edge of a character, as the system counts and as the text holds it.
    for (at, place) in [
        (0, pos(0, 0)),
        (1, pos(0, 1)),
        (2, pos(0, 3)),
        (4, pos(0, 7)),
        (5, pos(1, 0)),
        (7, pos(1, 3)),
        (11, pos(1, 11)),
        (12, pos(1, 12)),
        (13, pos(1, 13)),
    ] {
        set(cx, at..at);
        assert_eq!(typing.selection(cx), caret_at(place), "{at}");
        assert_eq!(typing.system_selection(cx), at..at, "{place:?}");
    }
    // Inside a character, or between the parts of one, a caret goes before it.
    for (at, place) in [
        (3, pos(0, 3)),
        (6, pos(1, 0)),
        (9, pos(1, 3)),
        (10, pos(1, 3)),
    ] {
        set(cx, at..at);
        assert_eq!(typing.selection(cx), caret_at(place), "{at}");
    }
    // And a range takes in all of the characters it cuts.
    set(cx, 3..9);
    assert_eq!(
        typing.selection(cx),
        Selection {
            anchor: pos(0, 3),
            head: pos(1, 11)
        }
    );
    assert_eq!(typing.system_selection(cx), 2..11);
    let (text, adjusted) = typing.system(cx, |editor, window, cx| {
        let mut adjusted = None;
        let text = editor.text_for_range(3..6, &mut adjusted, window, cx);
        (text, adjusted)
    });
    assert_eq!(text.as_deref(), Some("😀\ne"));
    assert_eq!(adjusted, Some(2..6));
    // Typed there, it replaces exactly that.
    typing.commit(cx, "!");
    assert_eq!(typing.source(cx), "# T\n\n- Hé! x\n");
}

#[gpui_kit::test]
fn the_candidate_window_sits_at_the_caret(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 4));
    let caret = typing.caret(cx).unwrap();
    let near = |a: Bounds<Pixels>, b: Bounds<Pixels>| {
        (a.origin.x - b.origin.x).abs() <= px(1.)
            && (a.origin.y - b.origin.y).abs() <= px(1.)
            && (a.size.height - b.size.height).abs() <= px(1.)
    };
    let bounds_for = |cx: &mut TestAppContext, range: std::ops::Range<usize>| {
        typing
            .system(cx, |editor, window, cx| {
                editor.bounds_for_range(range, Bounds::default(), window, cx)
            })
            .expect("the place has bounds")
    };
    let bounds = bounds_for(cx, 4..4);
    assert!(near(bounds, caret), "{bounds:?} at {caret:?}");
    // While composing, it stays where the composition starts.
    typing.compose(cx, "´");
    let marked = typing.marked(cx).unwrap();
    let bounds = bounds_for(cx, marked);
    assert!(near(bounds, caret), "{bounds:?} at {caret:?}");
    // And the system's place for a point on the slide is the text's there.
    let at = typing.point_of(cx, 1, pos(0, 10));
    let index = typing.system(cx, |editor, window, cx| {
        editor.character_index_for_point(at, window, cx)
    });
    assert_eq!(index, Some(11), "after the composition");
}

#[gpui_kit::test]
fn saving_during_a_composition_ends_it_first(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 4));
    typing.type_text(cx, "ab");
    typing.compose(cx, "´");
    cx.simulate_keystrokes(typing.handle, "ctrl-s");
    cx.run_until_parked();
    typing.settle(cx);
    // What was composed isn't text yet: it's dropped, and what was typed is saved.
    let saved = std::fs::read_to_string(&typing.file).unwrap();
    assert_eq!(saved, TWO.replacen("Some words", "Someab words", 1));
    assert_eq!(typing.source(cx), saved);
    assert_eq!(typing.composing(cx), None);
    assert!(typing.composition(cx).is_none());
    // Typing goes on, and the letter the system then gives is typed once.
    typing.commit(cx, "é");
    assert!(typing.source(cx).contains("Someabé words"));
}

#[gpui_kit::test]
fn ending_or_undoing_typing_during_a_composition_drops_it(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 4));
    typing.compose(cx, "´");
    typing.press(cx, "escape");
    assert!(typing.editing(cx).is_none());
    assert!(typing.composition(cx).is_none());
    assert_eq!(typing.source(cx), TWO);

    typing.click_text(cx, 1, pos(0, 4));
    typing.type_text(cx, "a");
    typing.compose(cx, "´");
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TWO);
    assert_eq!(typing.composing(cx), None);
    assert!(typing.composition(cx).is_none());

    // Another slide.
    typing.compose(cx, "´");
    typing.click_id(cx, NEW_SLIDE_ID);
    assert_eq!(typing.composing(cx), None);
    assert!(typing.composition(cx).is_none());
}

#[gpui_kit::test]
fn replacing_a_range_the_system_names_replaces_exactly_that(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 0));
    let replace = |cx: &mut TestAppContext, range: std::ops::Range<usize>, text: &str| {
        typing.system(cx, |editor, window, cx| {
            editor.replace_text_in_range(Some(range), text, window, cx)
        });
    };
    replace(cx, 5..10, "WORDS");
    assert_eq!(
        typing.source(cx),
        TWO.replacen("Some words", "Some WORDS", 1)
    );
    assert_eq!(typing.selection(cx), caret_at(pos(0, 10)));
    replace(cx, 0..4, "😀");
    replace(cx, 0..2, "é");
    assert_eq!(typing.source(cx), TWO.replacen("Some words", "é WORDS", 1));
    // Composed over a range of its own, the system's text goes in place of it once committed.
    typing.system(cx, |editor, window, cx| {
        editor.replace_and_mark_text_in_range(Some(2..7), "w", None, window, cx)
    });
    assert_eq!(typing.source(cx), TWO.replacen("Some words", "é WORDS", 1));
    typing.commit(cx, "words");
    assert_eq!(typing.source(cx), TWO.replacen("Some words", "é words", 1));
}

/// The text tools on the Home tab.
mod tools {
    use super::*;
    use gpui_kit::component::color_picker::ColorPickerState;
    use gpui_kit::{ElementId, Hsla, rgb};
    use slides_format::{Span, Style};
    use stagepack_studio::text_editing::ALIGN_TEXT;
    use stagepack_studio::text_tools::{
        ALIGN_CENTER_ID, ALIGN_LEFT_ID, ALIGN_RIGHT_ID, AUTOMATIC_COLOR_ID, BOLD_ID, BULLETS_ID,
        COLOR_ID, INDENT_LESS_ID, INDENT_MORE_ID, ITALIC_ID, NUMBERING_ID, SIZE_ID, SIZE_LIST_ID,
        LISTS_STAY_LEFT, SetTextSize, TextTools, UNDERLINE_ID, button_id,
    };
    use gpui_kit::test::TestQueryExt as _;
    use gpui_kit::{ScrollDelta, ScrollEvent, TouchPhase};
    use stagepack_studio::clipboard_tools::{COPY_ID, CUT_ID, PASTE_ID};
    use stagepack_studio::ribbon::{
        MORE_LEFT_ID, MORE_RIGHT_ID, REDO_ID, RIBBON_ID, RIBBON_TOOLS_ID,
        SAVE_ID, UNDO_ID,
    };

    /// How a tool shows the selection.
    #[derive(Debug, PartialEq, Eq)]
    enum Shows {
        Off,
        On,
        Half,
    }

    /// The talk `source` in a window that's the active one, as a window being used is: the
    /// keyboard moving from one place to another is heard.
    fn open(cx: &mut TestAppContext, source: &str) -> Typing {
        let typing = super::open(cx, source);
        typing.with_window(cx, |window, _| window.activate_window());
        typing.settle(cx);
        typing
    }

    impl Typing {
        fn click_on(&self, cx: &mut TestAppContext, id: impl Into<ElementId>) {
            let id = id.into();
            self.with_window(cx, |window, cx| {
                window.render_frame(cx);
                window.click(id, cx);
            });
            self.settle(cx);
        }

        fn shows(&self, cx: &mut TestAppContext, tool: &'static str) -> Shows {
            self.with_window(cx, |window, cx| {
                window.render_frame(cx);
                let tool = window.find(tool);
                match (tool.checked(), tool.indeterminate()) {
                    (_, Some(true)) => Shows::Half,
                    (Some(true), _) => Shows::On,
                    _ => Shows::Off,
                }
            })
        }

        fn disabled(&self, cx: &mut TestAppContext, id: impl Into<ElementId>) -> bool {
            let id = id.into();
            self.with_window(cx, |window, cx| {
                window.render_frame(cx);
                window.find(id).disabled() == Some(true)
            })
        }

        /// What the size field shows.
        fn size_shown(&self, cx: &mut TestAppContext) -> String {
            self.with_window(cx, |window, cx| {
                window.render_frame(cx);
                window.find(SIZE_ID).value().unwrap_or_default().to_string()
            })
        }

        fn tools(&self, cx: &TestAppContext) -> Entity<TextTools> {
            cx.read(|cx| self.view.read(cx).ribbon().read(cx).text_tools().clone())
        }

        fn picker(&self, cx: &TestAppContext) -> Entity<ColorPickerState> {
            self.tools(cx).read_with(cx, |tools, _| tools.color_picker().clone())
        }

        /// The colour the picker shows, and whether it's open.
        fn picker_shows(&self, cx: &TestAppContext) -> (Option<Hsla>, bool) {
            self.picker(cx)
                .read_with(cx, |picker, _| (picker.value(), picker.is_open()))
        }

        /// Whether the keyboard is with the text being typed.
        fn text_has_keyboard(&self, cx: &mut TestAppContext) -> bool {
            let editor = self.editor(cx);
            self.with_window(cx, |window, cx| editor.read(cx).focus_handle().is_focused(window))
        }

        /// Selects the word at `offset` of the body's first paragraph on the first slide.
        fn select_word(&self, cx: &mut TestAppContext, offset: usize) {
            let at = self.point_of(cx, 1, pos(0, offset));
            self.clicks(cx, at, 2, Modifiers::default());
        }

        /// The spans of flow `flow`'s paragraph `para` on the first slide.
        fn spans(&self, cx: &TestAppContext, flow: usize, para: usize) -> Vec<Span> {
            self.flows(cx, 0)[flow].paras[para].spans.clone()
        }

        fn undo_label(&self, cx: &TestAppContext) -> Option<&'static str> {
            self.document(cx).read_with(cx, |document, _| document.undo_label())
        }

        /// Picks `color` in the colour picker, as a click on one of its swatches does.
        fn pick(&self, cx: &mut TestAppContext, color: Hsla) {
            let picker = self.picker(cx);
            self.with_window(cx, |window, cx| {
                picker.update(cx, |picker, cx| picker.select_color(color, window, cx))
            });
            self.settle(cx);
        }
    }

    fn span(text: &str, style: Style) -> Span {
        Span {
            text: text.to_string(),
            style,
        }
    }

    fn bold() -> Style {
        Style {
            bold: true,
            ..Style::default()
        }
    }

    /// Selects "words", then gives it a style twice: by `tool` and by `key`, each once on and
    /// once off again, checking the source and the button each time.
    fn styles_by_button_and_by_key(
        cx: &mut TestAppContext,
        tool: &'static str,
        key: &str,
        on: &str,
        style: Style,
    ) {
        let typing = open(cx, TWO);
        typing.select_word(cx, 6);
        assert_eq!(typing.shows(cx, tool), Shows::Off);
        let styled = TWO.replacen("words", on, 1);
        for by_key in [false, true] {
            if by_key {
                typing.press(cx, key);
            } else {
                typing.click_on(cx, tool);
            }
            assert_eq!(typing.source(cx), styled, "by key: {by_key}");
            assert_eq!(
                typing.spans(cx, 1, 0),
                [
                    span("Some ", Style::default()),
                    span("words", style.clone()),
                    span(" here", Style::default())
                ]
            );
            assert_eq!(typing.shows(cx, tool), Shows::On);
            assert_eq!(typing.selection(cx).start(), pos(0, 5), "the words stay selected");
            assert_eq!(typing.selection(cx).end(), pos(0, 10));
            if by_key {
                typing.click_on(cx, tool);
            } else {
                typing.press(cx, key);
            }
            assert_eq!(typing.source(cx), TWO);
            assert_eq!(typing.shows(cx, tool), Shows::Off);
        }
    }

    #[gpui_kit::test]
    fn bold_makes_the_selected_words_bold_and_the_button_shows_it(cx: &mut TestAppContext) {
        styles_by_button_and_by_key(cx, BOLD_ID, "ctrl-b", "**words**", bold());
    }

    #[gpui_kit::test]
    fn italic_makes_the_selected_words_italic_and_the_button_shows_it(cx: &mut TestAppContext) {
        let italic = Style {
            italic: true,
            ..Style::default()
        };
        styles_by_button_and_by_key(cx, ITALIC_ID, "ctrl-i", "*words*", italic);
    }

    #[gpui_kit::test]
    fn underline_underlines_the_selected_words_and_the_button_shows_it(cx: &mut TestAppContext) {
        let underline = Style {
            underline: true,
            ..Style::default()
        };
        styles_by_button_and_by_key(
            cx,
            UNDERLINE_ID,
            "ctrl-u",
            "<Text underline=\"true\">words</Text>",
            underline,
        );
    }

    #[gpui_kit::test]
    fn a_style_change_is_a_step_of_its_own_and_typing_after_it_another(cx: &mut TestAppContext) {
        let typing = open(cx, TWO);
        typing.click_text(cx, 1, pos(0, 4));
        typing.type_text(cx, "ab");
        typing.select_word(cx, 8);
        typing.press(cx, "ctrl-b");
        assert_eq!(typing.undo_label(cx), Some("Format text"));
        typing.click_text(cx, 1, pos(0, 0));
        typing.type_text(cx, "X");
        typing.press(cx, "ctrl-z");
        assert_eq!(typing.source(cx), TWO.replacen("Some words", "Someab **words**", 1));
        typing.press(cx, "ctrl-z");
        assert_eq!(typing.source(cx), TWO.replacen("Some words", "Someab words", 1));
        typing.press(cx, "ctrl-z");
        assert_eq!(typing.source(cx), TWO);
    }

    #[gpui_kit::test]
    fn a_mixed_selection_shows_half_pressed(cx: &mut TestAppContext) {
        let typing = open(cx, TWO);
        typing.select_word(cx, 6);
        typing.press(cx, "ctrl-b");
        // From the start of "Some" to the end of "words": some of it bold, some not.
        let start = typing.point_of(cx, 1, pos(0, 0));
        let end = typing.point_of(cx, 1, pos(0, 10));
        typing.click(cx, start);
        typing.clicks(cx, end, 1, Modifiers::shift());
        assert_eq!(typing.selection(cx).end(), pos(0, 10));
        assert_eq!(typing.shows(cx, BOLD_ID), Shows::Half);
        assert_eq!(typing.shows(cx, ITALIC_ID), Shows::Off);
        // Bold over a mixed selection makes all of it bold.
        typing.press(cx, "ctrl-b");
        assert_eq!(typing.shows(cx, BOLD_ID), Shows::On);
        assert_eq!(typing.source(cx), TWO.replacen("Some words", "**Some words**", 1));
    }

    #[gpui_kit::test]
    fn bold_with_no_selection_applies_to_what_is_typed_next(cx: &mut TestAppContext) {
        let typing = open(cx, TWO);
        typing.click_text(cx, 1, pos(0, 15));
        typing.click_on(cx, BOLD_ID);
        assert_eq!(typing.source(cx), TWO, "nothing to make bold yet");
        assert_eq!(typing.shows(cx, BOLD_ID), Shows::On, "what's typed next is bold");
        typing.type_text(cx, " too");
        assert_eq!(
            typing.spans(cx, 1, 0),
            [span(WORDS, Style::default()), span(" too", bold())]
        );
        // Moving the caret forgets it.
        typing.click_text(cx, 1, pos(0, 4));
        typing.press(cx, "ctrl-b");
        typing.press(cx, "right");
        assert_eq!(typing.shows(cx, BOLD_ID), Shows::Off);
    }

    #[gpui_kit::test]
    fn choosing_a_size_changes_only_the_selected_words(cx: &mut TestAppContext) {
        let typing = open(cx, TWO);
        typing.select_word(cx, 6);
        assert_eq!(typing.size_shown(cx), "40", "the body's own size");
        // The list opens, and typing goes on while it's open.
        typing.click_on(cx, SIZE_LIST_ID);
        assert!(typing.editing(cx).is_some());
        typing.press(cx, "escape");
        assert!(typing.editing(cx).is_some(), "Escape closes the list, not the typing");
        assert!(typing.text_has_keyboard(cx));
        // A size from the list is given as the list gives it.
        typing.with_window(cx, |window, cx| {
            window.dispatch_action(Box::new(SetTextSize { size: 64 }), cx)
        });
        typing.settle(cx);
        assert_eq!(
            typing.source(cx),
            TWO.replacen("words", "<Text size=\"64\">words</Text>", 1)
        );
        assert_eq!(typing.size_shown(cx), "64");
        assert_eq!(typing.selection(cx).start(), pos(0, 5));
        typing.click_text(cx, 1, pos(0, 2));
        assert_eq!(typing.size_shown(cx), "40", "the words around it keep theirs");
    }

    #[gpui_kit::test]
    fn the_size_shown_is_what_is_drawn(cx: &mut TestAppContext) {
        let typing = open(cx, TWO);
        typing.click_text(cx, 0, pos(0, 2));
        assert_eq!(typing.size_shown(cx), "64", "the title's size");
        typing.click_text(cx, 1, pos(0, 2));
        assert_eq!(typing.size_shown(cx), "40");
        // Over words of two sizes, it's empty.
        typing.select_word(cx, 6);
        typing.with_window(cx, |window, cx| {
            window.dispatch_action(Box::new(SetTextSize { size: 64 }), cx)
        });
        typing.settle(cx);
        let start = typing.point_of(cx, 1, pos(0, 0));
        typing.click(cx, start);
        let end = typing.point_of(cx, 1, pos(0, 10));
        typing.clicks(cx, end, 1, Modifiers::shift());
        assert_eq!(typing.size_shown(cx), "");
        // Words given the size their region draws them at already are all one size.
        typing.select_word(cx, 0);
        typing.with_window(cx, |window, cx| {
            window.dispatch_action(Box::new(SetTextSize { size: 40 }), cx)
        });
        typing.settle(cx);
        let start = typing.point_of(cx, 1, pos(0, 0));
        typing.click(cx, start);
        let space = typing.point_of(cx, 1, pos(0, 5));
        typing.clicks(cx, space, 1, Modifiers::shift());
        assert_eq!(typing.size_shown(cx), "40");
    }

    /// Types `size` into the size field, and presses Enter.
    fn type_size(typing: &Typing, cx: &mut TestAppContext, size: &str) {
        typing.click_on(cx, SIZE_ID);
        assert!(!typing.text_has_keyboard(cx), "the field has the keyboard");
        assert!(typing.editing(cx).is_some(), "typing goes on");
        typing.press(cx, "ctrl-a");
        typing.type_text(cx, size);
        typing.press(cx, "enter");
    }

    #[gpui_kit::test]
    fn typing_a_size_and_pressing_enter_applies_it(cx: &mut TestAppContext) {
        let typing = open(cx, TWO);
        typing.select_word(cx, 6);
        let selected = typing.selection(cx);
        type_size(&typing, cx, "57");
        assert_eq!(
            typing.source(cx),
            TWO.replacen("words", "<Text size=\"57\">words</Text>", 1)
        );
        assert!(typing.text_has_keyboard(cx), "the keyboard is back with the text");
        assert_eq!(typing.selection(cx), selected, "and so is the selection");
        assert_eq!(typing.size_shown(cx), "57");
        // Typing goes on, over the selection.
        typing.type_text(cx, "terms");
        assert!(typing.source(cx).contains("<Text size=\"57\">terms</Text>"));
    }

    #[gpui_kit::test]
    fn escape_in_the_size_field_goes_back_to_the_text_as_it_was(cx: &mut TestAppContext) {
        let typing = open(cx, TWO);
        typing.select_word(cx, 6);
        let selected = typing.selection(cx);
        typing.click_on(cx, SIZE_ID);
        typing.press(cx, "ctrl-a");
        typing.type_text(cx, "90");
        typing.press(cx, "escape");
        assert_eq!(typing.source(cx), TWO);
        assert!(typing.text_has_keyboard(cx));
        assert_eq!(typing.selection(cx), selected);
        assert_eq!(typing.size_shown(cx), "40", "the field shows the size again");
        // Something that isn't a number changes nothing either.
        type_size(&typing, cx, "big");
        assert_eq!(typing.source(cx), TWO);
        assert!(typing.text_has_keyboard(cx));
    }

    #[gpui_kit::test]
    fn leaving_the_size_field_for_elsewhere_ends_the_typing(cx: &mut TestAppContext) {
        let typing = open(cx, TWO);
        typing.select_word(cx, 6);
        typing.click_on(cx, SIZE_ID);
        assert!(typing.editing(cx).is_some());
        let strip = cx.read(|cx| typing.view.read(cx).slide_strip().clone());
        typing.with_window(cx, |window, cx| {
            let focus = strip.read(cx).focus_handle().clone();
            window.focus(&focus, cx);
        });
        typing.settle(cx);
        assert!(typing.editing(cx).is_none());
    }

    #[gpui_kit::test]
    fn a_size_out_of_range_is_held_to_the_limits(cx: &mut TestAppContext) {
        let typing = open(cx, TWO);
        typing.select_word(cx, 6);
        type_size(&typing, cx, "9000");
        assert_eq!(
            typing.source(cx),
            TWO.replacen("words", "<Text size=\"512\">words</Text>", 1)
        );
        type_size(&typing, cx, "0");
        assert_eq!(
            typing.source(cx),
            TWO.replacen("words", "<Text size=\"1\">words</Text>", 1)
        );
        type_size(&typing, cx, "-20");
        assert!(typing.source(cx).contains("<Text size=\"1\">words</Text>"));
        type_size(&typing, cx, "31.6");
        assert!(typing.source(cx).contains("<Text size=\"32\">words</Text>"));
    }

    fn red() -> Hsla {
        rgb(0xff0000).into()
    }

    #[gpui_kit::test]
    fn choosing_a_colour_changes_the_selected_words(cx: &mut TestAppContext) {
        let typing = open(cx, TWO);
        typing.select_word(cx, 6);
        let selected = typing.selection(cx);
        typing.click_on(cx, COLOR_ID);
        assert!(typing.picker_shows(cx).1, "the picker opens");
        assert!(typing.editing(cx).is_some(), "typing goes on while it's open");
        typing.pick(cx, red());
        assert_eq!(
            typing.source(cx),
            TWO.replacen("words", "<Text color=\"#ff0000\">words</Text>", 1)
        );
        assert!(typing.text_has_keyboard(cx), "the keyboard is back with the text");
        assert_eq!(typing.selection(cx), selected);
        assert_eq!(typing.picker_shows(cx).0, Some(red()));
        // Elsewhere, the picker shows no colour of the text's own.
        typing.click_text(cx, 1, pos(0, 2));
        assert_eq!(typing.picker_shows(cx).0, None);
    }

    #[gpui_kit::test]
    fn the_picker_offers_the_talks_own_colours_first(cx: &mut TestAppContext) {
        let source = "---\ncolor.text: #112233\ncolor.accent: #445566\n---\n\n# T\n\nWords\n";
        let typing = open(cx, source);
        let tools = typing.tools(cx);
        let offered = cx.read(|cx| tools.read(cx).featured_colors(cx));
        let theirs: Vec<Hsla> = [0x112233, 0x445566, 0xffffff].map(|color| rgb(color).into()).into();
        assert_eq!(offered, theirs, "its text, accent and background");
        typing.select_word(cx, 2);
        typing.click_on(cx, COLOR_ID);
        assert!(typing.picker_shows(cx).1);
        typing.pick(cx, offered[1]);
        assert!(typing.source(cx).contains("<Text color=\"#445566\">Words</Text>"), "{}", typing.source(cx));
    }

    #[gpui_kit::test]
    fn automatic_clears_the_colour(cx: &mut TestAppContext) {
        let coloured = TWO.replacen("words", "<Text color=\"#ff0000\">words</Text>", 1);
        let typing = open(cx, &coloured);
        typing.select_word(cx, 6);
        assert_eq!(typing.picker_shows(cx).0, Some(red()));
        typing.click_on(cx, AUTOMATIC_COLOR_ID);
        assert_eq!(typing.source(cx), TWO);
        assert!(typing.text_has_keyboard(cx));
        assert_eq!(typing.picker_shows(cx).0, None);
    }

    #[gpui_kit::test]
    fn the_markdown_for_a_red_bold_word_is_what_the_design_shows(cx: &mut TestAppContext) {
        let typing = open(cx, TWO);
        typing.select_word(cx, 6);
        typing.click_on(cx, BOLD_ID);
        typing.click_on(cx, COLOR_ID);
        typing.pick(cx, red());
        let source = typing.source(cx);
        let either = [
            TWO.replacen("words", "<Text color=\"#ff0000\">**words**</Text>", 1),
            TWO.replacen("words", "**<Text color=\"#ff0000\">words</Text>**", 1),
        ];
        assert!(either.contains(&source), "{source}");
        let red_bold = Style {
            bold: true,
            color: Some("#ff0000".to_string()),
            ..Style::default()
        };
        assert_eq!(typing.spans(cx, 1, 0)[1], span("words", red_bold));
    }

    fn kinds(typing: &Typing, cx: &TestAppContext, flow: usize) -> Vec<ParaKind> {
        typing.flows(cx, 0)[flow].paras.iter().map(|para| para.kind).collect()
    }

    const LINES: &str = "# T\n\nOne\n\nTwo\n";

    /// Selects from the start of the body's first paragraph to the end of its second.
    fn select_both(typing: &Typing, cx: &mut TestAppContext) {
        let start = typing.point_of(cx, 1, pos(0, 0));
        typing.click(cx, start);
        let end = typing.point_of(cx, 1, pos(1, 3));
        typing.clicks(cx, end, 1, Modifiers::shift());
    }

    #[gpui_kit::test]
    fn bullets_turn_the_paragraphs_into_a_list_and_back(cx: &mut TestAppContext) {
        let typing = open(cx, LINES);
        select_both(&typing, cx);
        typing.click_on(cx, BULLETS_ID);
        assert_eq!(typing.source(cx), "# T\n\n- One\n\n- Two\n");
        let bullet = ParaKind::Bullet { level: 0 };
        assert_eq!(kinds(&typing, cx, 1), [bullet, bullet]);
        assert_eq!(typing.shows(cx, BULLETS_ID), Shows::On);
        assert_eq!(typing.shows(cx, NUMBERING_ID), Shows::Off);
        typing.click_on(cx, BULLETS_ID);
        assert_eq!(kinds(&typing, cx, 1), [ParaKind::Body, ParaKind::Body]);
        assert_eq!(typing.source(cx), LINES);
        assert_eq!(typing.shows(cx, BULLETS_ID), Shows::Off);
        assert!(typing.editing(cx).is_some());
    }

    #[gpui_kit::test]
    fn numbering_numbers_them(cx: &mut TestAppContext) {
        let typing = open(cx, "# T\n\n- One\n\nTwo\n");
        select_both(&typing, cx);
        assert_eq!(typing.shows(cx, BULLETS_ID), Shows::Half);
        typing.click_on(cx, NUMBERING_ID);
        let numbered = ParaKind::Numbered { level: 0 };
        assert_eq!(kinds(&typing, cx, 1), [numbered, numbered]);
        assert!(typing.source(cx).starts_with("# T\n\n1. One\n"), "{}", typing.source(cx));
        assert_eq!(typing.shows(cx, NUMBERING_ID), Shows::On);
        assert_eq!(typing.shows(cx, BULLETS_ID), Shows::Off);
    }

    #[gpui_kit::test]
    fn indent_more_and_less(cx: &mut TestAppContext) {
        let typing = open(cx, "# T\n\n- One\n- Two\n");
        typing.click_text(cx, 1, pos(1, 1));
        typing.click_on(cx, INDENT_MORE_ID);
        let bullet = |level| ParaKind::Bullet { level };
        assert_eq!(kinds(&typing, cx, 1), [bullet(0), bullet(1)]);
        assert_eq!(typing.source(cx), "# T\n\n- One\n  - Two\n");
        typing.click_on(cx, INDENT_LESS_ID);
        assert_eq!(kinds(&typing, cx, 1), [bullet(0), bullet(0)]);
        assert_eq!(typing.source(cx), "# T\n\n- One\n- Two\n");
        assert_eq!(typing.selection(cx), caret_at(pos(1, 1)));
    }

    #[gpui_kit::test]
    fn a_title_cannot_be_made_a_list(cx: &mut TestAppContext) {
        let typing = open(cx, TWO);
        typing.click_text(cx, 0, pos(0, 2));
        for tool in [BULLETS_ID, NUMBERING_ID] {
            assert!(typing.disabled(cx, button_id(tool)), "{tool}");
        }
        assert!(!typing.disabled(cx, button_id(BOLD_ID)));
        assert!(!typing.disabled(cx, ALIGN_CENTER_ID));
        // Nor by a key that's sent all the same.
        typing.with_window(cx, |window, cx| {
            window.dispatch_action(Box::new(stagepack_studio::text_tools::ToggleBullets), cx)
        });
        typing.settle(cx);
        assert_eq!(typing.source(cx), TWO);
        typing.click_text(cx, 1, pos(0, 2));
        assert!(!typing.disabled(cx, button_id(BULLETS_ID)));
    }

    #[gpui_kit::test]
    fn centre_centres_the_whole_region_and_one_undo_puts_it_back(cx: &mut TestAppContext) {
        let typing = open(cx, "---\ntitle: T\n---\n\n# Hello\n\nOne\n\nTwo\n");
        typing.click_text(cx, 1, pos(1, 2));
        assert_eq!(typing.shows(cx, ALIGN_LEFT_ID), Shows::On);
        let before = typing.layout(cx).caret(1, pos(0, 0)).unwrap().x;
        typing.click_on(cx, ALIGN_CENTER_ID);
        let centred = "---\ntitle: T\nregion.body: align=center\n---\n\n# Hello\n\nOne\n\nTwo\n";
        assert_eq!(typing.source(cx), centred);
        assert_eq!(typing.shows(cx, ALIGN_CENTER_ID), Shows::On);
        assert_eq!(typing.shows(cx, ALIGN_LEFT_ID), Shows::Off);
        assert_eq!(typing.undo_label(cx), Some(ALIGN_TEXT));
        // Both paragraphs of the region moved, the title didn't.
        let layout = typing.layout(cx);
        assert!(layout.caret(1, pos(0, 0)).unwrap().x > before + 100.);
        assert!(layout.caret(1, pos(1, 0)).unwrap().x > before + 100.);
        assert!((layout.caret(0, pos(0, 0)).unwrap().x - 100.).abs() < 1.);
        // Typing goes on where it was.
        assert_eq!(typing.selection(cx), caret_at(pos(1, 2)));
        assert!(typing.text_has_keyboard(cx));
        typing.type_text(cx, "!");
        assert!(typing.source(cx).contains("\nTw!o\n"));
        typing.press(cx, "ctrl-z");
        assert_eq!(typing.source(cx), centred);
        typing.press(cx, "ctrl-z");
        assert_eq!(typing.source(cx), "---\ntitle: T\n---\n\n# Hello\n\nOne\n\nTwo\n");
        assert_eq!(typing.selection(cx), caret_at(pos(1, 2)), "and typing goes on there");
        // Right, then left again, on the same line.
        typing.click_on(cx, ALIGN_RIGHT_ID);
        typing.click_on(cx, ALIGN_LEFT_ID);
        assert_eq!(
            typing.source(cx),
            "---\ntitle: T\nregion.body: align=left\n---\n\n# Hello\n\nOne\n\nTwo\n"
        );
    }

    #[gpui_kit::test]
    fn a_box_is_aligned_by_its_own_attribute(cx: &mut TestAppContext) {
        let source = "# T\n\n<Text x=\"100\" y=\"400\" w=\"800\" h=\"200\">\n\nHi\n\n</Text>\n";
        let typing = open(cx, source);
        typing.click_text(cx, 1, pos(0, 1));
        // What the system was composing is dropped first, as before any other change.
        typing.compose(cx, "´");
        typing.click_on(cx, ALIGN_RIGHT_ID);
        assert_eq!(typing.composing(cx), None);
        assert_eq!(
            typing.source(cx),
            source.replace("h=\"200\">", "h=\"200\" align=\"right\">")
        );
        assert_eq!(typing.shows(cx, ALIGN_RIGHT_ID), Shows::On);
        assert!(typing.editing(cx).is_some());
    }

    #[gpui_kit::test]
    fn an_empty_region_is_aligned_before_anything_is_typed(cx: &mut TestAppContext) {
        let typing = open(cx, TWO);
        typing.with_window(cx, |window, cx| window.press("right", cx));
        typing.settle(cx);
        assert_eq!(typing.current(cx), 1);
        let at = typing.placeholder_point(cx, "body");
        typing.click(cx, at);
        let left = typing.caret(cx).expect("a caret in the empty region");
        // As tall as a line of the region's text, which is 40 high.
        let unit = f32::from(typing.slide_bounds(cx).size.width) / 1920.;
        let height = f32::from(left.size.height) / unit;
        assert!((height - 50.).abs() < 1., "the caret is {height} high");
        typing.click_on(cx, ALIGN_CENTER_ID);
        assert!(typing.source(cx).ends_with("---\nregion.body: align=center\n---\n\n# Second\n"));
        let centred = typing.caret(cx).expect("the caret is still there");
        assert!(centred.origin.x > left.origin.x + px(100.), "{left:?} {centred:?}");
        typing.type_text(cx, "Middle");
        assert!(typing.source(cx).contains("Middle"));
    }

    #[gpui_kit::test]
    fn the_tools_are_disabled_with_no_text_being_edited(cx: &mut TestAppContext) {
        let typing = open(cx, TWO);
        for tool in [BOLD_ID, ITALIC_ID, UNDERLINE_ID, BULLETS_ID, NUMBERING_ID] {
            assert!(typing.disabled(cx, button_id(tool)), "{tool}");
        }
        for tool in [
            SIZE_LIST_ID,
            AUTOMATIC_COLOR_ID,
            INDENT_LESS_ID,
            INDENT_MORE_ID,
            ALIGN_LEFT_ID,
            ALIGN_CENTER_ID,
            ALIGN_RIGHT_ID,
        ] {
            assert!(typing.disabled(cx, tool), "{tool}");
        }
        assert_eq!(typing.size_shown(cx), "");
        // Their keys do nothing.
        for key in ["ctrl-b", "ctrl-i", "ctrl-u"] {
            typing.press(cx, key);
        }
        assert_eq!(typing.source(cx), TWO);
        assert!(!typing.can_undo(cx));
        // And they come on with typing.
        typing.click_text(cx, 1, pos(0, 2));
        assert!(!typing.disabled(cx, button_id(BOLD_ID)));
        assert!(!typing.disabled(cx, ALIGN_LEFT_ID));
        typing.press(cx, "escape");
        assert!(typing.disabled(cx, button_id(BOLD_ID)));
    }

    #[gpui_kit::test]
    fn using_a_tool_keeps_the_caret_in_the_text(cx: &mut TestAppContext) {
        let typing = open(cx, TWO);
        typing.click_text(cx, 1, pos(0, 4));
        for tool in [BOLD_ID, ITALIC_ID] {
            typing.click_on(cx, tool);
            assert!(typing.text_has_keyboard(cx), "{tool}");
            assert_eq!(typing.selection(cx), caret_at(pos(0, 4)));
        }
        typing.click_on(cx, button_id(BOLD_ID));
        typing.type_text(cx, "x");
        assert_eq!(
            typing.source(cx),
            TWO.replacen("Some words", "Some*x* words", 1)
        );
        assert!(typing.caret(cx).is_some());
    }

    impl Typing {
        fn bounds_of(&self, cx: &mut TestAppContext, id: impl Into<ElementId>) -> Option<Bounds<Pixels>> {
            let id = id.into();
            self.with_window(cx, |window, cx| {
                window.render_frame(cx);
                window.try_find(id).map(|found| found.bounds())
            })
        }

        /// Turns the mouse wheel over the row of tools by `notches`, down for more than 0.
        fn wheel(&self, cx: &mut TestAppContext, notches: f32, shift: bool) {
            let row = self.bounds_of(cx, RIBBON_TOOLS_ID).expect("the row of tools");
            let modifiers = Modifiers { shift, ..Modifiers::default() };
            self.with_window(cx, |window, cx| {
                window.dispatch_event(
                    MouseMoveEvent { position: row.center(), pressed_button: None, modifiers }.to_platform_input(),
                    cx,
                );
                window.dispatch_event(
                    ScrollEvent {
                        position: row.center(),
                        delta: ScrollDelta::Pixels(point(px(0.), px(-40. * notches))),
                        modifiers,
                        touch_phase: TouchPhase::Moved,
                        source: Default::default(),
                    }
                    .to_platform_input(),
                    cx,
                );
            });
            self.settle(cx);
        }

        /// The part of the ribbon a tool can be clicked in: not under an arrow at an end.
        fn clear_part(&self, cx: &mut TestAppContext) -> (Pixels, Pixels) {
            let ribbon = self.bounds_of(cx, RIBBON_ID).unwrap();
            let left = self.bounds_of(cx, MORE_LEFT_ID).map_or(ribbon.left(), |arrow| arrow.right());
            let right = self.bounds_of(cx, MORE_RIGHT_ID).map_or(ribbon.right(), |arrow| arrow.left());
            (left, right)
        }

        /// Scrolls the row until `tool` is in the clear, and says whether it got there.
        fn bring_into_view(&self, cx: &mut TestAppContext, tool: impl Into<ElementId>) -> bool {
            let tool = tool.into();
            for turn in 0..60 {
                let bounds = self
                    .bounds_of(cx, tool.clone())
                    .unwrap_or_else(|| panic!("{tool:?} is there"));
                let (left, right) = self.clear_part(cx);
                if bounds.left() >= left && bounds.right() <= right {
                    return true;
                }
                // Both ways of turning the wheel scroll the row.
                let notches = if bounds.right() > right { 1. } else { -1. };
                self.wheel(cx, notches, turn % 2 == 1);
            }
            false
        }

        fn resize(&self, cx: &mut TestAppContext, width: f32, height: f32) {
            cx.simulate_window_resize(self.handle, size(px(width), px(height)));
            self.settle(cx);
        }
    }

    const HOME_TOOLS: [&str; 24] = [
        PASTE_ID, CUT_ID, COPY_ID, SAVE_ID, DUPLICATE_ID, DELETE_ID, HIDE_ID,
        BOLD_ID, ITALIC_ID, UNDERLINE_ID, SIZE_ID, SIZE_LIST_ID, COLOR_ID, AUTOMATIC_COLOR_ID,
        BULLETS_ID, NUMBERING_ID, INDENT_LESS_ID, INDENT_MORE_ID, ALIGN_LEFT_ID, ALIGN_CENTER_ID,
        ALIGN_RIGHT_ID, UNDO_ID, REDO_ID, NEW_SLIDE_ID,
    ];

    #[gpui_kit::test]
    fn in_a_narrow_window_the_tools_scroll_and_every_one_can_be_reached(cx: &mut TestAppContext) {
        let typing = open(cx, TWO);
        typing.resize(cx, 800., 500.);
        typing.select_word(cx, 6);
        assert!(typing.bounds_of(cx, MORE_RIGHT_ID).is_some(), "an arrow says there's more");
        assert!(typing.bounds_of(cx, MORE_LEFT_ID).is_none(), "but not to the left yet");
        for tool in HOME_TOOLS {
            assert!(typing.bring_into_view(cx, tool), "{tool} can't be reached");
        }
        // At the far end, the arrow is on the other side.
        typing.bring_into_view(cx, REDO_ID);
        assert!(typing.bounds_of(cx, MORE_LEFT_ID).is_some());
        // Tools reached by scrolling work, and typing goes on.
        assert!(typing.bring_into_view(cx, ALIGN_RIGHT_ID));
        typing.click_on(cx, ALIGN_RIGHT_ID);
        assert!(typing.source(cx).contains("region.body: align=right"));
        assert!(typing.bring_into_view(cx, BOLD_ID));
        typing.click_on(cx, BOLD_ID);
        assert!(typing.source(cx).contains("Some **words** here"));
        assert!(typing.text_has_keyboard(cx));
        // The arrow scrolls too.
        assert!(typing.bring_into_view(cx, PASTE_ID));
        let before = typing.bounds_of(cx, PASTE_ID).unwrap();
        typing.click_on(cx, MORE_RIGHT_ID);
        assert!(typing.bounds_of(cx, PASTE_ID).unwrap().left() < before.left());
    }

    #[gpui_kit::test]
    fn in_a_wide_window_nothing_scrolls(cx: &mut TestAppContext) {
        let typing = open(cx, TWO);
        typing.resize(cx, 1400., 800.);
        typing.select_word(cx, 6);
        assert!(typing.bounds_of(cx, MORE_RIGHT_ID).is_none());
        assert!(typing.bounds_of(cx, MORE_LEFT_ID).is_none());
        let before = typing.bounds_of(cx, BOLD_ID).unwrap();
        typing.wheel(cx, 3., false);
        typing.wheel(cx, 3., true);
        assert_eq!(typing.bounds_of(cx, BOLD_ID).unwrap(), before);
        let ribbon = typing.bounds_of(cx, RIBBON_ID).unwrap();
        for tool in HOME_TOOLS {
            let bounds = typing.bounds_of(cx, tool).unwrap();
            assert!(bounds.right() <= ribbon.right(), "{tool} is cut off");
        }
        // Narrowed, it does; widened again, it doesn't.
        typing.resize(cx, 800., 500.);
        assert!(typing.bounds_of(cx, MORE_RIGHT_ID).is_some());
        typing.resize(cx, 1400., 800.);
        assert!(typing.bounds_of(cx, MORE_RIGHT_ID).is_none());
    }

    #[gpui_kit::test]
    fn aligning_as_it_is_already_writes_nothing(cx: &mut TestAppContext) {
        // A region with no setting of its own, on a slide with no settings block.
        let typing = open(cx, LINES);
        typing.click_text(cx, 1, pos(0, 1));
        assert_eq!(typing.shows(cx, ALIGN_LEFT_ID), Shows::On);
        typing.click_on(cx, ALIGN_LEFT_ID);
        assert_eq!(typing.source(cx), LINES);
        assert!(!typing.can_undo(cx), "and makes no step");
        // One set already.
        let centred = "---\nregion.body: align=center\n---\n\n# T\n\nOne\n";
        let typing = open(cx, centred);
        typing.click_text(cx, 1, pos(0, 1));
        typing.click_on(cx, ALIGN_CENTER_ID);
        assert_eq!(typing.source(cx), centred);
        assert!(!typing.can_undo(cx));
        // A box, left as boxes are.
        let boxed = "# T\n\n<Text x=\"100\" y=\"400\" w=\"800\" h=\"200\">\n\nHi\n\n</Text>\n";
        let typing = open(cx, boxed);
        typing.click_text(cx, 1, pos(0, 1));
        assert_eq!(typing.shows(cx, ALIGN_LEFT_ID), Shows::On);
        typing.click_on(cx, ALIGN_LEFT_ID);
        assert_eq!(typing.source(cx), boxed);
        assert!(!typing.can_undo(cx));
    }

    #[gpui_kit::test]
    fn lists_alone_are_not_aligned(cx: &mut TestAppContext) {
        let list = "# T\n\n- One\n- Two\n";
        let typing = open(cx, list);
        typing.click_text(cx, 1, pos(0, 1));
        for tool in [ALIGN_LEFT_ID, ALIGN_CENTER_ID, ALIGN_RIGHT_ID] {
            assert!(typing.disabled(cx, tool), "{tool}");
        }
        let tip = typing.with_window(cx, |window, cx| {
            window.render_frame(cx);
            window.find(ALIGN_CENTER_ID).description().map(str::to_string)
        });
        assert_eq!(tip.as_deref(), Some(LISTS_STAY_LEFT));
        typing.click_on(cx, ALIGN_CENTER_ID);
        assert_eq!(typing.source(cx), list);
        // With a body paragraph too, they work, and say how the region is set.
        let mixed = "# T\n\nIntro\n\n- One\n";
        let typing = open(cx, mixed);
        typing.click_text(cx, 1, pos(1, 1));
        assert!(!typing.disabled(cx, ALIGN_CENTER_ID));
        let before = typing.layout(cx).caret(1, pos(0, 0)).unwrap().x;
        typing.click_on(cx, ALIGN_CENTER_ID);
        assert!(typing.source(cx).contains("region.body: align=center"));
        assert_eq!(typing.shows(cx, ALIGN_CENTER_ID), Shows::On, "with the caret in the list");
        assert!(typing.layout(cx).caret(1, pos(0, 0)).unwrap().x > before + 100., "Intro moved");
    }

    #[gpui_kit::test]
    fn indent_is_only_for_list_items(cx: &mut TestAppContext) {
        let typing = open(cx, "# T\n\nIntro\n\n- One\n");
        typing.click_text(cx, 1, pos(0, 1));
        assert!(typing.disabled(cx, INDENT_LESS_ID));
        assert!(typing.disabled(cx, INDENT_MORE_ID));
        typing.click_text(cx, 1, pos(1, 1));
        assert!(!typing.disabled(cx, INDENT_LESS_ID));
        assert!(!typing.disabled(cx, INDENT_MORE_ID));
    }

    #[gpui_kit::test]
    fn the_size_text_is_drawn_at_anyway_gives_it_no_size_of_its_own(cx: &mut TestAppContext) {
        let typing = open(cx, TWO);
        typing.select_word(cx, 6);
        type_size(&typing, cx, "40");
        assert_eq!(typing.source(cx), TWO, "nothing to write");
        assert!(!typing.can_undo(cx), "and no step");
        typing.with_window(cx, |window, cx| window.dispatch_action(Box::new(SetTextSize { size: 40 }), cx));
        typing.settle(cx);
        assert_eq!(typing.source(cx), TWO);
        assert!(!typing.can_undo(cx));
        let sized = TWO.replacen("words", "<Text size=\"64\">words</Text>", 1);
        let typing = open(cx, &sized);
        typing.select_word(cx, 6);
        type_size(&typing, cx, "40");
        assert_eq!(typing.source(cx), TWO, "the size of its own is taken away");
    }

    #[gpui_kit::test]
    fn a_size_is_chosen_from_the_list(cx: &mut TestAppContext) {
        let typing = open(cx, TWO);
        typing.select_word(cx, 6);
        typing.click_on(cx, SIZE_LIST_ID);
        let item = typing.with_window(cx, |window, cx| {
            window.render_frame(cx);
            window.find_by_label("80").bounds().center()
        });
        typing.click(cx, item);
        assert_eq!(typing.source(cx), TWO.replacen("words", "<Text size=\"80\">words</Text>", 1));
        assert!(typing.text_has_keyboard(cx));
        assert_eq!(typing.size_shown(cx), "80");
    }

    /// Opens the picker over "words" and moves its sliders to red, as a drag does.
    fn drag_to_red(typing: &Typing, cx: &mut TestAppContext) {
        typing.select_word(cx, 6);
        typing.click_on(cx, COLOR_ID);
        assert!(typing.picker_shows(cx).1);
        let picker = typing.picker(cx);
        typing.with_window(cx, |window, cx| {
            picker.update(cx, |picker, cx| picker.update_color(red(), window, cx))
        });
        typing.settle(cx);
        assert_eq!(typing.source(cx), TWO, "nothing yet, while it's open");
    }

    #[gpui_kit::test]
    fn a_colour_from_the_sliders_is_given_however_the_picker_closes(cx: &mut TestAppContext) {
        let red_words = TWO.replacen("words", "<Text color=\"#ff0000\">words</Text>", 1);
        let typing = open(cx, TWO);
        drag_to_red(&typing, cx);
        typing.press(cx, "escape");
        assert!(!typing.picker_shows(cx).1);
        assert_eq!(typing.source(cx), red_words, "closed with Escape");
        assert!(typing.text_has_keyboard(cx));

        let typing = open(cx, TWO);
        drag_to_red(&typing, cx);
        // The tab bar, beside its last tab.
        let tab = typing.bounds_of(cx, stagepack_studio::ribbon::TAB_VIEW_ID).unwrap();
        typing.click(cx, point(tab.right() + px(120.), tab.center().y));
        assert!(!typing.picker_shows(cx).1);
        assert_eq!(typing.source(cx), red_words, "closed by a click elsewhere");
    }
}

/// `start..end`, the wrong way round, as the system may give it.
fn backwards(start: usize, end: usize) -> std::ops::Range<usize> {
    start..end
}

#[gpui_kit::test]
fn reversed_or_out_of_range_ranges_from_the_system_are_taken_in_order(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 4));
    // A reversed selection inside the composition, and then across it.
    typing.compose(cx, "abc");
    assert_eq!(typing.marked(cx), Some(4..7));
    typing.system(cx, |editor, window, cx| {
        editor.replace_and_mark_text_in_range(None, "abc", Some(backwards(3, 1)), window, cx)
    });
    assert_eq!(typing.system_selection(cx), 5..7);
    typing.system(cx, |editor, window, cx| {
        editor.set_selected_text_range(backwards(6, 5), window, cx)
    });
    assert_eq!(typing.system_selection(cx), 5..6);
    typing.system(cx, |editor, window, cx| {
        editor.set_selected_text_range(backwards(6, 2), window, cx)
    });
    assert_eq!(typing.composing(cx), None, "a selection outside it ends it");
    assert_eq!(
        typing.selection(cx),
        Selection {
            anchor: pos(0, 2),
            head: pos(0, 4)
        }
    );
    assert_eq!(typing.source(cx), TWO);
    // Each of the others, reversed and past the end.
    let text_for = |cx: &mut TestAppContext, range: std::ops::Range<usize>| {
        typing.system(cx, |editor, window, cx| {
            let mut adjusted = None;
            let text = editor.text_for_range(range, &mut adjusted, window, cx);
            (text, adjusted)
        })
    };
    assert_eq!(text_for(cx, backwards(10, 5)), (Some("words".into()), Some(5..10)));
    assert_eq!(text_for(cx, 100..200), (Some(String::new()), Some(15..15)));
    for range in [backwards(10, 5), 100..200] {
        let bounds = typing.system(cx, |editor, window, cx| {
            editor.bounds_for_range(range, Bounds::default(), window, cx)
        });
        assert!(bounds.is_some());
    }
    typing.system(cx, |editor, window, cx| {
        editor.set_selected_text_range(backwards(300, 100), window, cx)
    });
    assert_eq!(typing.selection(cx), caret_at(pos(0, WORDS.len())));
    let replace = |cx: &mut TestAppContext, range: std::ops::Range<usize>, text: &str| {
        typing.system(cx, |editor, window, cx| {
            editor.replace_text_in_range(Some(range), text, window, cx)
        });
    };
    replace(cx, backwards(10, 5), "WORDS");
    replace(cx, backwards(200, 100), "!");
    assert_eq!(typing.text_of(cx, 0, 1), ["Some WORDS here!"]);
    typing.system(cx, |editor, window, cx| {
        let (range, selected) = (Some(backwards(4, 0)), Some(backwards(9, 4)));
        editor.replace_and_mark_text_in_range(range, "x", selected, window, cx)
    });
    assert_eq!(typing.marked(cx), Some(0..1));
    assert_eq!(typing.system_selection(cx), 1..1);
    assert_eq!(typing.text_of(cx, 0, 1), ["Some WORDS here!"]);
}

#[gpui_kit::test]
fn text_from_the_system_is_cleaned_as_typed_text_is(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 4));
    typing.commit(cx, "a\r\nb");
    assert_eq!(typing.text_of(cx, 0, 1), ["Somea", "b words here"]);
    typing.commit(cx, "\u{1b}x");
    assert_eq!(typing.text_of(cx, 0, 1), ["Somea", "bx words here"]);
    // What's shown while composing is what's committed.
    typing.compose(cx, "\u{7}´\r");
    assert_eq!(typing.composing(cx).as_deref(), Some("´\n"));
    typing.compose(cx, "\u{7}´");
    assert_eq!(typing.composition(cx).unwrap().1, "´");
    typing.system(cx, |editor, window, cx| editor.unmark_text(window, cx));
    assert_eq!(typing.text_of(cx, 0, 1), ["Somea", "bx´ words here"]);
}

#[gpui_kit::test]
fn a_shortcut_during_a_composition_leaves_no_stale_composition(cx: &mut TestAppContext) {
    let typing = open(cx, TWO);
    typing.click_text(cx, 1, pos(0, 4));
    // On Linux, a dead key then Ctrl+S: the system composes the letter with Ctrl held, and
    // drops its own composition without saying so.
    typing.compose(cx, "´");
    let ctrl = Modifiers {
        control: true,
        ..Modifiers::default()
    };
    // Only the key-down, as Linux types nothing with Ctrl held.
    typing.with_window(cx, |window, cx| {
        window.dispatch_event(
            KeyDownEvent {
                keystroke: Keystroke {
                    modifiers: ctrl,
                    key: "sacute".into(),
                    key_char: Some("ś".into()),
                },
                is_held: false,
                prefer_character_input: false,
            }
            .to_platform_input(),
            cx,
        );
    });
    typing.settle(cx);
    assert_eq!(typing.composing(cx), None);
    assert!(typing.composition(cx).is_none());
    assert_eq!(typing.source(cx), TWO, "and nothing is typed");
    // AltGr typing a character is text, and the composition stays until the system ends it.
    typing.compose(cx, "´");
    typing.with_window(cx, |window, cx| {
        window.dispatch_event(
            KeyDownEvent {
                keystroke: Keystroke {
                    modifiers: ctrl_alt(),
                    key: "e".into(),
                    key_char: None,
                },
                is_held: false,
                prefer_character_input: false,
            }
            .to_platform_input(),
            cx,
        );
    });
    typing.settle(cx);
    assert_eq!(typing.composing(cx), None, "a Ctrl and Alt key that types nothing is a shortcut");
    typing.compose(cx, "´");
    typing.key(cx, "e", ctrl_alt(), "€");
    assert_eq!(typing.source(cx), TWO.replacen("Some words", "Some€ words", 1));

}
