//! Typing on the slide: clicking its text, typing, selecting, undoing, and what the talk's
//! file holds while it's done.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Entity, InputEvent as _, Modifiers, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, TestAppContext, VisualTestContext,
    Window, WindowBounds, WindowOptions, point, px, size,
};
use slides_engine::{Fonts, Hit, TextLayout};
use slides_format::{FlowPlace, ParaKind, Pos, Selection, TextFlow, text_flows};
use stagepack_studio::document::{Document, TalkPlace};
use stagepack_studio::recovery::Recovery;
use stagepack_studio::slide_editor::{CARET_ID, SLIDE_EDITOR_ID};
use stagepack_studio::text_editing::{COULD_NOT_TYPE, EditingText};
use stagepack_studio::workspace_view::{self, CLOSE_SAVE_ID, STATUS_ID, WorkspaceView};

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
