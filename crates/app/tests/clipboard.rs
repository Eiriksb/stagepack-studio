//! Copy, cut and paste: of text on the slide, and of slides in the list, in one talk and
//! between two; and what's taken from the clipboard, which may hold anything.

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, ClipboardItem, Entity, EntityInputHandler as _,
    InputEvent as _, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Modifiers, Point,
    TestAppContext, WindowBounds, WindowOptions, point, px, size,
};
use proptest::prelude::*;
use slides_engine::{Fonts, Pixmap};
use slides_format::{Para, ParaKind, Pos, Span, Style, TextFlow, is_hidden, parse_talk, storable, text_flows};
use stagepack_studio::clipboard::{
    self, APP_KEY, MAX_BYTES, MAX_PARAS, MAX_SPANS, MISSING_PICTURES, NO_SLIDE, TOO_LARGE,
    TextPaste, decode_paras, encode_paras, text_entry, text_to_paste,
};
use stagepack_studio::clipboard_tools::{COPY_ID, CUT_ID, PASTE_ID};
use stagepack_studio::document::Document;
use stagepack_studio::slide_editor::SlideEditor;
use stagepack_studio::slide_strip::strip_row_id;
use stagepack_studio::text_editing::EditingText;
use stagepack_studio::workspace_view::{self, STATUS_ID, WorkspaceView};

const STYLED: &str = "# Hello\n\nSome **bold** words\n\n---\n\n# Second\n\nend\n";
const TWO: &str = "---\ntitle: T\n---\n\n# Hello\n\nSome words here\n\n---\n\n# Second\n";
const THREE: &str = "# One\n\nfirst words\n\n---\n\n# Two\n\n---\n\n# Three\n";

struct Talk {
    folder: tempfile::TempDir,
    handle: AnyWindowHandle,
    view: Entity<WorkspaceView>,
}

fn open(cx: &mut TestAppContext, source: &str) -> Talk {
    open_in(cx, tempfile::tempdir().unwrap(), source)
}

fn open_in(cx: &mut TestAppContext, folder: tempfile::TempDir, source: &str) -> Talk {
    std::fs::write(folder.path().join("slides.md"), source).unwrap();
    let document = Document::open(folder.path(), Fonts::bundled_only()).unwrap();
    let (handle, view) = cx.update(|cx: &mut App| {
        gpui_kit::init(cx);
        workspace_view::init(cx);
        let bounds = Bounds {
            origin: Point::default(),
            size: size(px(1280.), px(800.)),
        };
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| WorkspaceView::open_with(document, window, cx))
        })
        .expect("open test window")
    });
    let talk = Talk { folder, handle, view };
    talk.settle(cx);
    talk
}

impl Talk {
    fn settle(&self, cx: &mut TestAppContext) {
        for _ in 0..3 {
            cx.update_window(self.handle, |_, window, cx| window.render_frame(cx))
                .unwrap();
            cx.run_until_parked();
        }
    }

    fn document(&self, cx: &TestAppContext) -> Entity<Document> {
        cx.read(|cx| self.view.read(cx).document().unwrap().clone())
    }

    fn editor(&self, cx: &TestAppContext) -> Entity<SlideEditor> {
        cx.read(|cx| self.view.read(cx).slide_editor().clone())
    }

    fn source(&self, cx: &TestAppContext) -> String {
        self.document(cx).read_with(cx, |document, _| document.source().to_string())
    }

    fn count(&self, cx: &TestAppContext) -> usize {
        self.document(cx).read_with(cx, |document, _| document.slide_count())
    }

    fn current(&self, cx: &TestAppContext) -> usize {
        self.document(cx).read_with(cx, |document, _| document.current())
    }

    fn can_undo(&self, cx: &TestAppContext) -> bool {
        self.document(cx).read_with(cx, |document, _| document.can_undo())
    }

    fn undo_label(&self, cx: &TestAppContext) -> Option<&'static str> {
        self.document(cx).read_with(cx, |document, _| document.undo_label())
    }

    fn editing(&self, cx: &TestAppContext) -> Option<EditingText> {
        self.document(cx).read_with(cx, |document, _| document.text().cloned())
    }

    fn flows(&self, cx: &TestAppContext, slide: usize) -> Vec<TextFlow> {
        self.document(cx).read_with(cx, |document, _| text_flows(document.talk(), slide))
    }

    /// The text of flow `flow` of slide `slide`, a paragraph to a line.
    fn text_of(&self, cx: &TestAppContext, slide: usize, flow: usize) -> Vec<String> {
        self.flows(cx, slide)[flow].paras.iter().map(para_text).collect()
    }

    /// Each slide's first line of text.
    fn titles(&self, cx: &TestAppContext) -> Vec<String> {
        (0..self.count(cx))
            .map(|slide| {
                let flows = self.flows(cx, slide);
                flows.first().map(|flow| para_text(&flow.paras[0])).unwrap_or_default()
            })
            .collect()
    }

    /// The middle of where the caret goes at `pos` of flow `flow` of the current slide.
    fn point_of(&self, cx: &mut TestAppContext, flow: usize, pos: Pos) -> Point<gpui_kit::Pixels> {
        let document = self.document(cx);
        let layout = cx.update(|cx| document.update(cx, |document, _| document.text_layout(document.current())));
        let caret = layout.caret(flow, pos).expect("the place has a caret");
        let (x, y) = caret.on_slide(caret.x, caret.top + caret.height / 2.);
        self.slide_point(cx, x, y)
    }

    fn slide_point(&self, cx: &TestAppContext, x: f32, y: f32) -> Point<gpui_kit::Pixels> {
        let slide = cx.read(|cx| {
            let canvas = self.view.read(cx).slide_canvas().clone();
            canvas.read(cx).slide_bounds().expect("the slide has been drawn")
        });
        let unit = f32::from(slide.size.width) / 1920.;
        slide.origin + point(px(x * unit), px(y * unit))
    }

    fn click_at(&self, cx: &mut TestAppContext, at: Point<gpui_kit::Pixels>) {
        cx.update_window(self.handle, |_, window, cx| {
            window.render_frame(cx);
            let modifiers = Modifiers::default();
            window.dispatch_event(MouseMoveEvent { position: at, pressed_button: None, modifiers }.to_platform_input(), cx);
            let down = MouseDownEvent { button: MouseButton::Left, position: at, modifiers, click_count: 1, first_mouse: false };
            window.dispatch_event(down.to_platform_input(), cx);
            let up = MouseUpEvent { button: MouseButton::Left, position: at, modifiers, click_count: 1 };
            window.dispatch_event(up.to_platform_input(), cx);
            window.render_frame(cx);
        })
        .unwrap();
        self.settle(cx);
    }

    fn click_text(&self, cx: &mut TestAppContext, flow: usize, pos: Pos) {
        let at = self.point_of(cx, flow, pos);
        self.click_at(cx, at);
        assert!(self.editing(cx).is_some(), "typing on the slide");
    }

    fn click(&self, cx: &mut TestAppContext, id: impl Into<gpui_kit::ElementId>) {
        let id = id.into();
        cx.update_window(self.handle, |_, window, cx| {
            window.render_frame(cx);
            window.click(id, cx);
        })
        .unwrap();
        self.settle(cx);
    }

    fn press(&self, cx: &mut TestAppContext, key: &str) {
        cx.update_window(self.handle, |_, window, cx| window.press(key, cx)).unwrap();
        self.settle(cx);
    }

    fn type_text(&self, cx: &mut TestAppContext, text: &str) {
        cx.update_window(self.handle, |_, window, cx| window.input(text, cx)).unwrap();
        self.settle(cx);
    }

    fn status(&self, cx: &mut TestAppContext) -> Option<String> {
        cx.update_window(self.handle, |_, window, cx| {
            window.render_frame(cx);
            window.try_find(STATUS_ID).and_then(|status| status.label().map(str::to_string))
        })
        .unwrap()
    }

    fn disabled(&self, cx: &mut TestAppContext, id: &'static str) -> bool {
        cx.update_window(self.handle, |_, window, cx| {
            window.render_frame(cx);
            window.find(id).disabled() == Some(true)
        })
        .unwrap()
    }

    /// Picks item `item` of row `row`'s menu.
    fn pick_from_rows_menu(&self, cx: &mut TestAppContext, row: usize, item: &str) {
        cx.update_window(self.handle, |_, window, cx| {
            window.render_frame(cx);
            window.right_click(strip_row_id(row), cx);
        })
        .unwrap();
        self.settle(cx);
        cx.update_window(self.handle, |_, window, cx| {
            window.render_frame(cx);
            let menu = window.within("popup-menu");
            let index = (0..20).find(|&index| menu.try_find(index).is_some_and(|found| found.label() == Some(item)));
            window.within("popup-menu").click(index.expect("the menu has the item"), cx);
        })
        .unwrap();
        self.settle(cx);
    }
}

fn pos(para: usize, offset: usize) -> Pos {
    Pos { para, offset }
}

fn para_text(para: &Para) -> String {
    para.spans.iter().map(|span| span.text.as_str()).collect()
}

fn bold() -> Style {
    Style { bold: true, ..Style::default() }
}

fn put(cx: &mut TestAppContext, text: &str, metadata: Option<&str>) {
    let item = match metadata {
        Some(metadata) => ClipboardItem::new_string_with_metadata(text.to_string(), metadata.to_string()),
        None => ClipboardItem::new_string(text.to_string()),
    };
    cx.write_to_clipboard(item);
}

fn on_clipboard(cx: &mut TestAppContext) -> (Option<String>, Option<String>) {
    cx.read_from_clipboard().map(|item| (item.text(), item.metadata().cloned())).unwrap_or_default()
}

// Text.

#[gpui_kit::test]
fn copied_words_paste_with_their_styles(cx: &mut TestAppContext) {
    let talk = open(cx, STYLED);
    talk.click_text(cx, 1, pos(0, 5));
    talk.press(cx, "shift-end");
    talk.press(cx, "ctrl-c");
    let (text, metadata) = on_clipboard(cx);
    assert_eq!(text.as_deref(), Some("bold words"), "other programs get the words");
    assert!(metadata.is_some_and(|metadata| metadata.contains(APP_KEY)), "the styles go under the app's name");
    assert_eq!(talk.source(cx), STYLED, "copying changes nothing");

    talk.press(cx, "end");
    talk.press(cx, "ctrl-v");
    let flows = talk.flows(cx, 0);
    let spans: Vec<(&str, bool)> = flows[1].paras[0].spans.iter().map(|span| (span.text.as_str(), span.style.bold)).collect();
    assert_eq!(spans, [("Some ", false), ("bold", true), (" words", false), ("bold", true), (" words", false)]);
    assert_eq!(talk.undo_label(cx), Some("Paste"));
    assert!(talk.source(cx).ends_with("---\n\n# Second\n\nend\n"), "the other slide is as it was");
}

#[gpui_kit::test]
fn text_from_another_program_pastes_plain(cx: &mut TestAppContext) {
    let talk = open(cx, STYLED);
    // Inside the bold word: what's pasted takes the style there, and its stars stay stars.
    talk.click_text(cx, 1, pos(0, 7));
    put(cx, "from **elsewhere**", None);
    talk.press(cx, "ctrl-v");
    let para = &talk.flows(cx, 0)[1].paras[0];
    assert_eq!(para_text(para), "Some bofrom **elsewhere**ld words");
    assert_eq!(para.spans[1], Span { text: "bofrom **elsewhere**ld".to_string(), style: bold() });
}

#[gpui_kit::test]
fn a_paste_with_newlines_makes_paragraphs(cx: &mut TestAppContext) {
    let talk = open(cx, TWO);
    talk.click_text(cx, 1, pos(0, 15));
    put(cx, "one\ntwo\r\nthree\u{7}", None);
    talk.press(cx, "ctrl-v");
    assert_eq!(talk.text_of(cx, 0, 1), ["Some words hereone", "two", "three"]);
    assert!(!talk.source(cx).contains('v'), "Ctrl+V types no letter");
    assert_eq!(talk.editing(cx).unwrap().edit.selection.head, pos(2, 5), "the caret is after it");
}

#[gpui_kit::test]
fn a_paste_into_a_title_joins_its_lines(cx: &mut TestAppContext) {
    let talk = open(cx, TWO);
    talk.click_text(cx, 0, pos(0, 5));
    put(cx, "one\ntwo", None);
    talk.press(cx, "ctrl-v");
    assert_eq!(talk.text_of(cx, 0, 0), ["Helloone\ntwo"], "a title is one paragraph");
    assert_eq!(talk.flows(cx, 0)[0].paras[0].kind, ParaKind::Heading(1));

    // Styled paragraphs too, and a list item stays out of the title.
    let paras = vec![
        Para { kind: ParaKind::Bullet { level: 0 }, spans: vec![Span { text: "a".into(), style: bold() }] },
        Para { kind: ParaKind::Body, spans: vec![Span { text: "b".into(), style: Style::default() }] },
    ];
    let entry = text_entry(&paras);
    put(cx, &entry.text, entry.metadata.as_deref());
    talk.press(cx, "ctrl-v");
    assert_eq!(talk.text_of(cx, 0, 0), ["Helloone\ntwoa\nb"]);
    assert_eq!(talk.flows(cx, 0)[0].paras[0].kind, ParaKind::Heading(1));
}

#[gpui_kit::test]
fn a_paste_into_an_empty_region_fills_it(cx: &mut TestAppContext) {
    let talk = open(cx, "# Only a title\n");
    let document = talk.document(cx);
    let (rect, _) = cx
        .update(|cx| document.update(cx, |document, _| document.text_layout(0)))
        .placeholder("body")
        .expect("the body is empty");
    let at = talk.slide_point(cx, rect.x + rect.w / 2., rect.y + rect.h / 2.);
    talk.click_at(cx, at);
    put(cx, "first\nsecond", None);
    talk.press(cx, "ctrl-v");
    assert_eq!(talk.text_of(cx, 0, 1), ["first", "second"]);
    talk.press(cx, "ctrl-z");
    assert_eq!(talk.source(cx), "# Only a title\n");
}

#[gpui_kit::test]
fn cut_removes_and_copies(cx: &mut TestAppContext) {
    let talk = open(cx, TWO);
    talk.click_text(cx, 1, pos(0, 5));
    // With nothing selected, Cut and Copy do nothing.
    put(cx, "before", None);
    assert!(talk.disabled(cx, CUT_ID) && talk.disabled(cx, COPY_ID));
    talk.press(cx, "ctrl-x");
    assert_eq!(talk.source(cx), TWO);
    assert_eq!(on_clipboard(cx).0.as_deref(), Some("before"));

    talk.press(cx, "shift-end");
    assert!(!talk.disabled(cx, CUT_ID) && !talk.disabled(cx, COPY_ID));
    talk.press(cx, "ctrl-x");
    assert_eq!(talk.text_of(cx, 0, 1), ["Some "]);
    assert_eq!(on_clipboard(cx).0.as_deref(), Some("words here"));
    assert_eq!(talk.undo_label(cx), Some("Cut"));
    talk.press(cx, "ctrl-z");
    assert_eq!(talk.source(cx), TWO, "one undo brings it back");

    // The Cut tool does the same.
    talk.click_text(cx, 1, pos(0, 0));
    talk.press(cx, "shift-end");
    talk.click(cx, CUT_ID);
    assert_eq!(talk.text_of(cx, 0, 1), [""], "the body is cut away");
    assert_eq!(on_clipboard(cx).0.as_deref(), Some("Some words here"));
    assert!(talk.editing(cx).is_some(), "typing goes on");
}

#[gpui_kit::test]
fn a_paste_is_one_undo_step(cx: &mut TestAppContext) {
    let talk = open(cx, TWO);
    talk.click_text(cx, 1, pos(0, 15));
    talk.type_text(cx, "ab");
    put(cx, "XY", None);
    talk.press(cx, "ctrl-v");
    talk.type_text(cx, "cd");
    assert_eq!(talk.text_of(cx, 0, 1), ["Some words hereabXYcd"]);
    talk.press(cx, "ctrl-z");
    assert_eq!(talk.text_of(cx, 0, 1), ["Some words hereabXY"], "typing after it is a step of its own");
    talk.press(cx, "ctrl-z");
    assert_eq!(talk.text_of(cx, 0, 1), ["Some words hereab"], "the paste is one step");
    talk.press(cx, "ctrl-z");
    assert_eq!(talk.source(cx), TWO);
}

#[gpui_kit::test]
fn a_paste_drops_what_was_being_composed(cx: &mut TestAppContext) {
    let talk = open(cx, TWO);
    talk.click_text(cx, 1, pos(0, 15));
    let editor = talk.editor(cx);
    cx.update_window(talk.handle, |_, window, cx| {
        editor.update(cx, |editor, cx| editor.replace_and_mark_text_in_range(None, "´", None, window, cx))
    })
    .unwrap();
    assert!(talk.editing(cx).unwrap().composing.is_some());
    put(cx, "X", None);
    talk.press(cx, "ctrl-v");
    assert!(talk.editing(cx).unwrap().composing.is_none());
    assert_eq!(talk.text_of(cx, 0, 1), ["Some words hereX"]);
}

#[gpui_kit::test]
fn a_paste_too_large_is_refused(cx: &mut TestAppContext) {
    let talk = open(cx, TWO);
    talk.click_text(cx, 1, pos(0, 15));
    put(cx, &"x".repeat(MAX_BYTES + 1), None);
    talk.press(cx, "ctrl-v");
    assert_eq!(talk.source(cx), TWO);
    assert!(!talk.can_undo(cx));
    assert_eq!(talk.status(cx).as_deref(), Some(TOO_LARGE));

    // So are more lines than paragraphs a paste makes.
    put(cx, &"\n".repeat(MAX_PARAS), None);
    talk.press(cx, "ctrl-v");
    assert_eq!(talk.source(cx), TWO);
    assert!(talk.editing(cx).is_some(), "typing goes on");
}

#[gpui_kit::test]
fn broken_clipboard_data_falls_back_to_plain_text(cx: &mut TestAppContext) {
    let talk = open(cx, TWO);
    let wrap = |paras: &str| format!(r#"{{"{APP_KEY}":{{"kind":"text","version":1,"paras":{paras}}}}}"#);
    let span = |extra: &str| wrap(&format!(r#"[{{"kind":"body","spans":[{{"text":"styled"{extra}}}]}}]"#));
    let huge = wrap(&format!(r#"[{{"kind":"body","spans":[{{"text":"{}"}}]}}]"#, "y".repeat(MAX_BYTES)));
    let many = wrap(&format!("[{}]", vec![r#"{"kind":"body"}"#; MAX_PARAS + 1].join(",")));
    let broken = [
        "not json".to_string(),
        r#"{"other-app":{"kind":"text","version":1,"paras":[]}}"#.to_string(),
        wrap(r#"[{"kind":"body","spans":[{"text":"styled"}]}]"#).replace("\"version\":1", "\"version\":2"),
        wrap(r#"[{"kind":"bullet","level":99,"spans":[{"text":"styled"}]}]"#),
        wrap(r#"[{"kind":"heading","level":0,"spans":[{"text":"styled"}]}]"#),
        wrap(r#"[{"kind":"title","spans":[{"text":"styled"}]}]"#),
        span(r#","size":0"#),
        span(r#","size":513"#),
        span(r#","color":"red\" onclick=\"x"#),
        span(r#","color":"<b>"#),
        span(r##","color":"#12345g"##),
        span(r#","link":"javascript:alert(1)"#),
        span(r#","link":"data:text/html,x"#),
        span(r#","link":"a\u0001b"#),
        span(r#","bold":"yes"#),
        huge,
        many,
    ];
    for metadata in broken {
        talk.click_text(cx, 1, pos(0, 0));
        put(cx, "plain\u{0}words ", Some(&metadata));
        talk.press(cx, "ctrl-v");
        let para = &talk.flows(cx, 0)[1].paras[0];
        assert_eq!(para_text(para), "plainwords Some words here", "{metadata:.80}");
        assert!(para.spans.iter().all(|span| span.style == Style::default()), "{metadata:.80}");
        talk.press(cx, "ctrl-z");
        assert_eq!(talk.source(cx), TWO);
    }
}

// Slides.

#[gpui_kit::test]
fn a_copied_slide_pastes_after_the_current_one(cx: &mut TestAppContext) {
    let talk = open(cx, THREE);
    talk.click(cx, strip_row_id(0));
    talk.press(cx, "ctrl-c");
    assert_eq!(talk.source(cx), THREE, "copying changes nothing");
    assert_eq!(on_clipboard(cx).0.as_deref(), Some("# One\n\nfirst words\n\n"), "other programs get its Markdown");
    talk.click(cx, strip_row_id(1));
    talk.press(cx, "ctrl-v");
    assert_eq!(talk.titles(cx), ["One", "Two", "One", "Three"]);
    assert_eq!(talk.current(cx), 2, "the pasted slide is shown");
    assert_eq!(talk.text_of(cx, 2, 1), ["first words"]);
    assert_eq!(talk.undo_label(cx), Some("Paste slide"));
    talk.press(cx, "ctrl-z");
    assert_eq!(talk.source(cx), THREE);

    // From the slide's menu too.
    talk.pick_from_rows_menu(cx, 2, "Copy");
    talk.pick_from_rows_menu(cx, 0, "Paste");
    assert_eq!(talk.titles(cx), ["One", "Three", "Two", "Three"]);
    talk.pick_from_rows_menu(cx, 1, "Cut");
    assert_eq!(talk.titles(cx), ["One", "Two", "Three"]);
}

#[gpui_kit::test]
fn a_slide_pastes_into_another_talk(cx: &mut TestAppContext) {
    let from = open(cx, "# One\n\n---\nhidden: true\n---\n\n# Secret\n\n- a\n- b\n\n<Notes>\nsay this\n</Notes>\n\n---\n\n# Three\n");
    let into = open(cx, "# A\r\n\r\n---\r\n\r\n# B\r\n");
    from.click(cx, strip_row_id(1));
    from.press(cx, "ctrl-c");
    into.click(cx, strip_row_id(0));
    into.press(cx, "ctrl-v");
    assert_eq!(into.titles(cx), ["A", "Secret", "B"]);
    let document = into.document(cx);
    document.read_with(cx, |document, _| {
        let talk = document.talk();
        assert!(is_hidden(&talk.slides[1]), "hidden as it was");
        assert_eq!(talk.slides[1].notes.as_ref().unwrap().text, "say this", "its notes come with it");
        let text = &talk.source[talk.slides[1].range.clone()];
        assert!(!text.replace("\r\n", "").contains('\n'), "the talk's own line endings: {text:?}");
    });
    assert_eq!(into.text_of(cx, 1, 1), ["a", "b"]);
    assert_eq!(from.count(cx), 3, "the talk it came from is as it was");
}

#[gpui_kit::test]
fn a_copied_first_slide_leaves_the_talks_settings_behind(cx: &mut TestAppContext) {
    let from = open(cx, "---\ntitle: First talk\ncolor.accent: #ff0000\nlayout: section\n---\n\n# One\n");
    let into = open(cx, "---\ntitle: Second talk\n---\n\n# A\n");
    from.click(cx, strip_row_id(0));
    from.press(cx, "ctrl-c");
    into.click(cx, strip_row_id(0));
    into.press(cx, "ctrl-v");
    assert_eq!(into.source(cx), "---\ntitle: Second talk\n---\n\n# A\n---\nlayout: section\n---\n\n# One\n");
}

#[gpui_kit::test]
fn a_cut_slide_is_gone_and_one_undo_brings_it_back(cx: &mut TestAppContext) {
    let talk = open(cx, THREE);
    talk.click(cx, strip_row_id(1));
    talk.press(cx, "ctrl-x");
    assert_eq!(talk.titles(cx), ["One", "Three"]);
    assert_eq!(talk.undo_label(cx), Some("Cut slide"));
    talk.press(cx, "ctrl-z");
    assert_eq!(talk.source(cx), THREE);
    talk.press(cx, "ctrl-v");
    assert_eq!(talk.titles(cx), ["One", "Two", "Two", "Three"], "it was copied");

    // The only slide can't be cut.
    let only = open(cx, "# Only\n");
    only.click(cx, strip_row_id(0));
    assert!(only.disabled(cx, CUT_ID));
    only.press(cx, "ctrl-x");
    assert_eq!(only.source(cx), "# Only\n");
    assert!(!only.can_undo(cx));
}

#[gpui_kit::test]
fn a_pasted_slide_missing_its_picture_says_so(cx: &mut TestAppContext) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join("assets")).unwrap();
    let png = Pixmap::new(4, 4).unwrap().encode_png().unwrap();
    std::fs::write(folder.path().join("assets/chart.png"), png).unwrap();
    let from = open_in(cx, folder, "# One\n\n![A chart](assets/chart.png)\n\n---\n\n# Two\n\n<Gauge />\n\n---\n\n# Three\n");
    let into = open(cx, "# A\n");
    from.click(cx, strip_row_id(0));
    assert_ne!(from.status(cx).as_deref(), Some(MISSING_PICTURES));
    from.press(cx, "ctrl-c");
    into.click(cx, strip_row_id(0));
    into.press(cx, "ctrl-v");
    assert_eq!(into.count(cx), 2);
    assert_eq!(into.status(cx).as_deref(), Some(MISSING_PICTURES));
    assert!(!into.folder.path().join("assets/chart.png").exists(), "nothing is carried across");

    // A component the talk lacks says the same, and a slide that has all it uses says nothing,
    // each pasted into a talk of its own, whose status line says nothing yet.
    for (row, says) in [(1, true), (2, false)] {
        let into = open(cx, "# A\n");
        from.click(cx, strip_row_id(row));
        from.press(cx, "ctrl-c");
        into.click(cx, strip_row_id(0));
        into.press(cx, "ctrl-v");
        assert_eq!(into.count(cx), 2);
        assert_eq!(into.status(cx).as_deref() == Some(MISSING_PICTURES), says, "row {row}");
    }
    // Pasting into the talk it came from, where the picture is, says nothing either.
    from.click(cx, strip_row_id(0));
    from.press(cx, "ctrl-c");
    from.press(cx, "ctrl-v");
    assert_eq!(from.count(cx), 4);
    assert_eq!(from.status(cx), None);
}

#[gpui_kit::test]
fn a_pasted_slide_that_would_split_or_join_slides_is_refused(cx: &mut TestAppContext) {
    let talk = open(cx, THREE);
    talk.click(cx, strip_row_id(1));
    let slide = |markdown: &str| {
        serde_json::json!({ APP_KEY: { "kind": "slide", "version": 1, "markdown": markdown } }).to_string()
    };
    for markdown in ["# A\n\n---\n\n# B\n", "# A\n\n---\n\n```\ncode\n"] {
        put(cx, markdown, Some(&slide(markdown)));
        talk.press(cx, "ctrl-v");
        assert_eq!(talk.source(cx), THREE, "{markdown:?}");
        assert!(!talk.can_undo(cx));
        assert!(talk.status(cx).is_some_and(|status| status.contains("wasn't made")), "{markdown:?}");
    }
    // Plain text isn't a slide.
    put(cx, "# Words\n", None);
    talk.press(cx, "ctrl-v");
    assert_eq!(talk.source(cx), THREE);
    assert_eq!(talk.status(cx).as_deref(), Some(NO_SLIDE));
}

#[gpui_kit::test]
fn paste_goes_to_the_text_when_text_has_the_focus_and_to_slides_when_the_list_has(cx: &mut TestAppContext) {
    let talk = open(cx, THREE);
    talk.click(cx, strip_row_id(0));
    talk.click(cx, COPY_ID);
    assert!(on_clipboard(cx).1.is_some_and(|metadata| metadata.contains("slide")));

    // In text, a copied slide pastes as its words, not its Markdown.
    talk.click_text(cx, 1, pos(0, 11));
    assert!(!talk.disabled(cx, PASTE_ID));
    talk.click(cx, PASTE_ID);
    assert_eq!(talk.count(cx), 3);
    assert_eq!(talk.text_of(cx, 0, 1), ["first wordsOne", "first words"]);
    assert!(talk.editing(cx).is_some(), "typing goes on");
    talk.type_text(cx, "!");
    assert_eq!(talk.text_of(cx, 0, 1), ["first wordsOne", "first words!"], "the keyboard is still in the text");

    // In the list, it's a slide.
    talk.click(cx, strip_row_id(2));
    assert!(talk.editing(cx).is_none());
    talk.click(cx, PASTE_ID);
    assert_eq!(talk.titles(cx), ["One", "Two", "Three", "One"]);
    talk.press(cx, "ctrl-v");
    assert_eq!(talk.count(cx), 5, "the list still has the keyboard");
}

// What's on the clipboard, whatever it is.

fn kind() -> impl Strategy<Value = ParaKind> {
    prop_oneof![
        Just(ParaKind::Body),
        (1u8..=6).prop_map(ParaKind::Heading),
        (0u8..=8).prop_map(|level| ParaKind::Bullet { level }),
        (0u8..=8).prop_map(|level| ParaKind::Numbered { level }),
    ]
}

fn style() -> impl Strategy<Value = Style> {
    let color = prop_oneof![Just(None), Just(Some("#abc".to_string())), Just(Some("#00ff7f".to_string())), Just(Some("red".to_string()))];
    let link = prop_oneof![Just(None), Just(Some("https://example.com/a?b=c".to_string())), Just(Some("#next".to_string())), Just(Some("mailto:a@b.c".to_string()))];
    (any::<[bool; 4]>(), color, prop::option::of(1u32..=512), link).prop_map(|([bold, italic, underline, code], color, size, link)| Style {
        bold,
        italic,
        underline,
        code,
        color,
        size,
        link,
    })
}

fn paras() -> impl Strategy<Value = Vec<Para>> {
    let span = ("[a-zé \t\n*#<>&\\\\`_\\[\\]😀]{0,8}", style()).prop_map(|(text, style)| Span { text, style });
    let para = (kind(), prop::collection::vec(span, 0..5)).prop_map(|(kind, spans)| Para { kind, spans });
    prop::collection::vec(para, 0..6)
}

/// Whether `paras` keep to the text model's rules, as a paste must.
fn keeps_the_rules(paras: &[Para]) -> bool {
    let kinds = paras.iter().all(|para| match para.kind {
        ParaKind::Body => true,
        ParaKind::Heading(level) => (1..=6).contains(&level),
        ParaKind::Bullet { level } | ParaKind::Numbered { level } => level <= 8,
    });
    let spans: Vec<&Span> = paras.iter().flat_map(|para| &para.spans).collect();
    let texts = spans.iter().all(|span| {
        !span.text.is_empty()
            && span.text.chars().all(|c| c == '\n' || c == '\t' || !(c.is_control() || matches!(c, '\u{feff}' | '\u{2028}' | '\u{2029}')))
    });
    let styles = spans.iter().all(|span| {
        let style = &span.style;
        style.size.is_none_or(|size| (1..=512).contains(&size))
            && style.color.as_deref().is_none_or(|color| !color.contains(|c: char| c.is_control() || "\"'<>&".contains(c)))
            && style.link.as_deref().is_none_or(|link| {
                !link.contains(|c: char| c.is_control() || c.is_whitespace() || "\"'<>".contains(c))
                    && !link.to_ascii_lowercase().starts_with("javascript:")
            })
    });
    let tidy = paras.iter().all(|para| para.spans.windows(2).all(|pair| pair[0].style != pair[1].style));
    kinds && texts && styles && tidy && paras.len() <= MAX_PARAS && spans.len() <= MAX_SPANS
}

fn json() -> impl Strategy<Value = serde_json::Value> {
    use serde_json::Value;
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::from),
        (0u32..600).prop_map(Value::from),
        prop_oneof![
            Just("body"), Just("heading"), Just("bullet"), Just("numbered"), Just("text"), Just("slide"),
            Just("#fff"), Just("javascript:x"), Just("a\u{0}b\u{2029}c"), Just("http://x"), Just("\u{feff}"),
        ]
        .prop_map(Value::from),
        ".{0,6}".prop_map(Value::from),
    ];
    let keys = prop_oneof![
        Just("kind"), Just("version"), Just("paras"), Just("spans"), Just("text"), Just("level"), Just("bold"),
        Just("size"), Just("color"), Just("link"), Just("code"), Just("markdown"),
    ];
    leaf.prop_recursive(4, 48, 6, move |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..6).prop_map(Value::Array),
            prop::collection::vec((keys.clone(), inner), 0..6)
                .prop_map(|pairs| Value::Object(pairs.into_iter().map(|(key, value)| (key.to_string(), value)).collect())),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    #[test]
    fn styled_paragraphs_come_back_as_they_went(paras in paras()) {
        let expected = storable(&paras);
        let decoded = decode_paras(&encode_paras(&paras));
        prop_assert!(decoded.as_deref().is_some_and(keeps_the_rules));
        prop_assert_eq!(decoded, Some(expected.clone()));
        let entry = text_entry(&paras);
        let pasted = text_to_paste(Some(&entry.text), entry.metadata.as_deref()).unwrap();
        prop_assert_eq!(pasted, if expected.is_empty() { TextPaste::Nothing } else { TextPaste::Styled(expected) });
    }

    #[test]
    fn any_bytes_on_the_clipboard_are_safe(bytes in prop::collection::vec(any::<u8>(), 0..400)) {
        let text = String::from_utf8_lossy(&bytes);
        if let Some(paras) = decode_paras(&text) {
            prop_assert!(keeps_the_rules(&paras));
        }
        match text_to_paste(Some(&text), Some(&text)) {
            Ok(TextPaste::Styled(paras)) => prop_assert!(keeps_the_rules(&paras)),
            Ok(TextPaste::Plain(plain)) => prop_assert!(!plain.contains(|c: char| c.is_control() && c != '\n' && c != '\t')),
            _ => {}
        }
        let _ = clipboard::slide_to_paste(Some(&text));
    }

    #[test]
    fn any_json_on_the_clipboard_is_safe(value in json(), kind in prop_oneof![Just("text"), Just("slide")]) {
        let mut payload = match value {
            serde_json::Value::Object(map) => map,
            other => [("paras".to_string(), other)].into_iter().collect(),
        };
        payload.insert("kind".into(), kind.into());
        payload.entry("version").or_insert(1.into());
        let json = serde_json::json!({ APP_KEY: payload }).to_string();
        if let Some(paras) = decode_paras(&json) {
            prop_assert!(keeps_the_rules(&paras));
        }
        if let Ok(TextPaste::Styled(paras)) = text_to_paste(Some("plain"), Some(&json)) {
            prop_assert!(keeps_the_rules(&paras));
        }
        if let Ok(Some(markdown)) = clipboard::slide_to_paste(Some(&json)) {
            prop_assert!(markdown.len() <= MAX_BYTES);
            prop_assert!(!markdown.contains(|c: char| c.is_control() && c != '\n' && c != '\t'));
            let _ = parse_talk(&markdown);
        }
    }
}

#[test]
fn a_slides_words_are_held_to_the_rules_too() {
    let markdown = "# Title\n\n[click](javascript:alert(1)) and <Text size=\"9000\">big</Text>\n";
    let metadata = serde_json::json!({ APP_KEY: { "kind": "slide", "version": 1, "markdown": markdown } }).to_string();
    match text_to_paste(Some(markdown), Some(&metadata)).unwrap() {
        TextPaste::Styled(paras) => assert!(keeps_the_rules(&paras), "{paras:?}"),
        other => assert_eq!(other, TextPaste::Nothing),
    }
}
