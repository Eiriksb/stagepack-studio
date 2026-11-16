//! The Insert tab: a picture from a file, dropped or pasted, a shape, and a text box, put on the
//! slide in the middle, selected, one undo step each.

use std::fs::File;
use std::ops::Range;
use std::path::{Path, PathBuf};

#[path = "support/typing.rs"]
mod typing;

use typing::*;

use gpui_kit::test::{TestQueryExt, TestWindowExt};
use gpui_kit::{
    AppContext as _, ClipboardEntry, ClipboardItem, ElementId, Entity, ExternalPaths, FileDropEvent, Image, ImageFormat,
    InputEvent as _, Pixels, Point, TestAppContext, px, size,
};
use slides_engine::{Deck, FolderFiles, Fonts, NativeContent};
use stagepack_studio::clipboard_tools::PASTE_ID;
use stagepack_studio::export::{ExportKind, run_export};
use stagepack_studio::insert::place_for;
use stagepack_studio::insert_tools::{INSERT_ICON_ID, INSERT_PICTURE_ID, INSERT_SHAPES_ID, INSERT_TEXT_BOX_ID, shape_id};
use stagepack_studio::recovery::Recovery;
use stagepack_studio::ribbon::{MORE_RIGHT_ID, RIBBON_ID, TAB_HOME_ID, TAB_INSERT_ID};
use stagepack_studio::text_tools::{ALIGN_CENTER_ID, BOLD_ID, BULLETS_ID, NUMBERING_ID};
use stagepack_studio::inspector::{INSPECTOR_HEADING_ID, INSPECTOR_MESSAGE_ID};
use stagepack_studio::slide_editor::{NEW_BOX_ID, SlideEditor};
use stagepack_studio::slide_strip::STRIP_ID;
use stagepack_studio::workspace_view::{DIALOG_FAILED, ExportPdf, ONLY_THE_FIRST, RECOVER_ID};
use toolkit::ShapeKind;

const TALK: &str = "---\ntitle: T\n---\n\n# One\n\nSome words\n\n---\n\n# Two\n";
/// Where the first slide's content ends, and where what's inserted on it goes.
const END_OF_ONE: &str = "Some words";
const NOT_A_PICTURE: &str = "That file isn't a picture this app can show.";
const ONLY_PICTURES: &str = "Only pictures can be dropped on a slide.";

/// A picture `width` by `height`, all `rgba`, in `format`.
fn encoded(width: u32, height: u32, rgba: [u8; 4], format: image::ImageFormat) -> Vec<u8> {
    let picture = image::RgbaImage::from_pixel(width, height, image::Rgba(rgba));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(picture).write_to(&mut bytes, format).unwrap();
    bytes.into_inner()
}

fn png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
    encoded(width, height, rgba, image::ImageFormat::Png)
}

/// A file named `name` holding `bytes`, outside any talk.
fn outside_file(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// What `typing`'s talk says once `inserted` is put after the first slide's content.
fn with_inserted(inserted: &str) -> String {
    TALK.replacen(END_OF_ONE, &format!("{END_OF_ONE}\n\n{inserted}"), 1)
}

fn selected(typing: &Typing, cx: &TestAppContext) -> Option<Range<usize>> {
    typing.document(cx).read_with(cx, |document, _| document.selected().map(|selected| selected.source.clone()))
}

/// The source of the thing selected.
fn selected_text(typing: &Typing, cx: &TestAppContext) -> Option<String> {
    let source = typing.source(cx);
    selected(typing, cx).map(|range| source[range].to_string())
}

fn can_undo(typing: &Typing, cx: &TestAppContext) -> bool {
    typing.document(cx).read_with(cx, |document, _| document.can_undo())
}

fn click_on(typing: &Typing, cx: &mut TestAppContext, id: impl Into<ElementId>) {
    let id = id.into();
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    });
    typing.settle(cx);
}

fn bounds_of(typing: &Typing, cx: &mut TestAppContext, id: impl Into<ElementId>) -> Option<gpui_kit::Bounds<Pixels>> {
    let id = id.into();
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find(id).map(|found| found.bounds())
    })
}

fn insert_tab(typing: &Typing, cx: &mut TestAppContext) {
    click_on(typing, cx, TAB_INSERT_ID);
}

/// Picks the shape `kind` from the Shapes list on the Insert tab.
fn insert_shape(typing: &Typing, cx: &mut TestAppContext, kind: &str) {
    insert_tab(typing, cx);
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.click(INSERT_SHAPES_ID, cx);
        window.render_frame(cx);
        window.within("popup-menu").click(shape_id(kind), cx);
    });
    typing.settle(cx);
}

fn insert_text_box(typing: &Typing, cx: &mut TestAppContext) {
    insert_tab(typing, cx);
    click_on(typing, cx, INSERT_TEXT_BOX_ID);
}

/// Hands `paths` to the window as the Picture tool's dialog would.
fn choose(typing: &Typing, cx: &mut TestAppContext, paths: Vec<PathBuf>) {
    let view = typing.view.clone();
    cx.update(|cx| view.update(cx, |view, cx| view.chosen_pictures(paths, cx)));
    typing.settle(cx);
}

/// Drops `paths` from outside the app at `at` in the window.
fn drop_at(typing: &Typing, cx: &mut TestAppContext, paths: &[PathBuf], at: Point<Pixels>) {
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        let paths = ExternalPaths(paths.iter().cloned().collect());
        window.dispatch_event(FileDropEvent::Entered { position: at, paths }.to_platform_input(), cx);
        window.dispatch_event(FileDropEvent::Pending { position: at }.to_platform_input(), cx);
        window.dispatch_event(FileDropEvent::Submit { position: at }.to_platform_input(), cx);
        window.render_frame(cx);
    });
    typing.settle(cx);
}

fn put_on_clipboard(cx: &mut TestAppContext, entries: Vec<ClipboardEntry>) {
    cx.write_to_clipboard(ClipboardItem { entries });
}

fn picture_entry(bytes: Vec<u8>) -> ClipboardEntry {
    ClipboardEntry::Image(Image::from_bytes(ImageFormat::Png, bytes))
}

fn editor(typing: &Typing, cx: &TestAppContext) -> Entity<SlideEditor> {
    cx.read(|cx| typing.view.read(cx).slide_editor().clone())
}

/// The system composes `text` at the caret, as a dead key does.
fn compose(typing: &Typing, cx: &mut TestAppContext, text: &str) {
    let editor = editor(typing, cx);
    typing.with_window(cx, |window, cx| {
        editor.update(cx, |editor, cx| {
            gpui_kit::EntityInputHandler::replace_and_mark_text_in_range(editor, None, text, None, window, cx)
        })
    });
    typing.settle(cx);
}

/// The system types `text`, as it does a finished composition.
fn commit(typing: &Typing, cx: &mut TestAppContext, text: &str) {
    let editor = editor(typing, cx);
    typing.with_window(cx, |window, cx| {
        editor.update(cx, |editor, cx| gpui_kit::EntityInputHandler::replace_text_in_range(editor, None, text, window, cx))
    });
    typing.settle(cx);
}

/// What the layers of slide `index` of `source`, in `folder`, are made of, for export.
fn native_of(folder: &Path, source: &str, index: usize) -> Vec<NativeContent> {
    let mut deck = Deck::open(Box::new(FolderFiles::new(folder.to_path_buf())), Fonts::bundled_only(), source.to_string());
    deck.specs(index).iter().flat_map(|spec| spec.native.iter().map(|native| native.content.clone())).collect()
}

fn folder_of(typing: &Typing) -> PathBuf {
    typing.file.parent().unwrap().to_path_buf()
}

// ---------------------------------------------------------------------------------------
// Where a new thing goes

#[test]
fn a_new_thing_goes_in_the_middle_and_steps_aside_from_one_there() {
    assert_eq!(place_for((480., 320.), &[]), (720., 380.));
    assert_eq!(place_for((480., 320.), &[(720., 380.)]), (760., 420.));
    assert_eq!(place_for((480., 320.), &[(100., 100.), (720., 380.), (760., 420.)]), (800., 460.));
    // Ten steps at most, and then where the last one put it, held on the slide.
    let every: Vec<(f32, f32)> = (0..=12).map(|step| (720. + 40. * step as f32, 380. + 40. * step as f32)).collect();
    assert_eq!(place_for((480., 320.), &every), (1120., 760.));
    // Bigger than the slide: its corner.
    assert_eq!(place_for((2400., 1300.), &[]), (0., 0.));
    assert_eq!(place_for((300., 150.), &[]), (810., 465.));
}

// ---------------------------------------------------------------------------------------
// Pictures

#[gpui_kit::test]
fn a_picture_from_a_file_lands_in_the_middle_selected(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let dir = tempfile::tempdir().unwrap();
    let path = outside_file(&dir, "Photo.png", &png(300, 150, [255, 0, 0, 255]));
    choose(&typing, cx, vec![path]);

    let line = "<Picture src=\"assets/photo.png\" x=\"810\" y=\"465\" w=\"300\" h=\"150\" />";
    assert_eq!(typing.source(cx), with_inserted(line));
    assert_eq!(selected_text(&typing, cx).as_deref(), Some(line));
    let document = typing.document(cx);
    let (notices, unsaved, drawn) = cx.update(|cx| {
        document.update(cx, |document, _| {
            let element = document.selected_box().expect("drawn");
            (document.notices(), document.has_unsaved_changes(), element.rect)
        })
    });
    assert!(notices.is_empty(), "{notices:?}");
    assert!(unsaved, "a save would write the picture");
    assert_eq!((drawn.x, drawn.y, drawn.w, drawn.h), (810., 465., 300., 150.));
    assert_eq!(typing.status(cx).as_deref(), Some("Picture selected"));
}

#[gpui_kit::test]
fn a_big_picture_keeps_its_shape(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let dir = tempfile::tempdir().unwrap();
    let path = outside_file(&dir, "wide.png", &png(2000, 500, [0, 0, 255, 255]));
    choose(&typing, cx, vec![path]);
    // Fitted into half the slide: 960 wide, a quarter as tall.
    let line = "<Picture src=\"assets/wide.png\" x=\"480\" y=\"420\" w=\"960\" h=\"240\" />";
    assert_eq!(typing.source(cx), with_inserted(line));
}

#[gpui_kit::test]
fn two_pictures_chosen_at_once_are_two_undo_steps(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let dir = tempfile::tempdir().unwrap();
    let red = outside_file(&dir, "red.png", &png(100, 100, [255, 0, 0, 255]));
    let blue = outside_file(&dir, "blue.png", &png(100, 100, [0, 0, 255, 255]));
    choose(&typing, cx, vec![red, blue]);

    let first = "<Picture src=\"assets/red.png\" x=\"910\" y=\"490\" w=\"100\" h=\"100\" />";
    let second = "<Picture src=\"assets/blue.png\" x=\"950\" y=\"530\" w=\"100\" h=\"100\" />";
    assert_eq!(typing.source(cx), with_inserted(&format!("{first}\n\n{second}")));
    assert_eq!(selected_text(&typing, cx).as_deref(), Some(second), "the last one is selected");
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), with_inserted(first));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TALK);
    assert!(!can_undo(&typing, cx));
}

#[gpui_kit::test]
fn a_file_that_is_not_a_picture_says_so_and_adds_nothing(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let dir = tempfile::tempdir().unwrap();
    let path = outside_file(&dir, "notes.png", b"these are words, not a picture");
    choose(&typing, cx, vec![path]);
    assert_eq!(typing.source(cx), TALK);
    assert_eq!(typing.status(cx).as_deref(), Some(NOT_A_PICTURE));
    let unsaved = typing.document(cx).read_with(cx, |document, _| document.unsaved_files().len());
    assert_eq!(unsaved, 0, "no file was added");
    assert!(!can_undo(&typing, cx));
}

#[gpui_kit::test]
fn the_picture_tool_is_on_the_insert_tab(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    insert_tab(&typing, cx);
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find(INSERT_PICTURE_ID).label(), Some("Picture"));
        assert_eq!(window.find(INSERT_SHAPES_ID).label(), Some("Shapes"));
        assert_eq!(window.find(INSERT_TEXT_BOX_ID).label(), Some("Text box"));
        assert!(window.find_by_label("Pictures").visible(), "the group's caption");
    });
}

#[gpui_kit::test]
fn a_dropped_picture_is_inserted(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let dir = tempfile::tempdir().unwrap();
    let red = outside_file(&dir, "red.png", &png(100, 100, [255, 0, 0, 255]));
    let blue = outside_file(&dir, "blue.png", &png(100, 100, [0, 0, 255, 255]));
    let middle = typing.at(cx, 960., 540.);
    drop_at(&typing, cx, &[red, blue], middle);

    let first = "<Picture src=\"assets/red.png\" x=\"910\" y=\"490\" w=\"100\" h=\"100\" />";
    let second = "<Picture src=\"assets/blue.png\" x=\"950\" y=\"530\" w=\"100\" h=\"100\" />";
    assert_eq!(typing.source(cx), with_inserted(&format!("{first}\n\n{second}")));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), with_inserted(first), "one undo step each");
}

#[gpui_kit::test]
fn a_dropped_text_file_says_only_pictures(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let dir = tempfile::tempdir().unwrap();
    let text = outside_file(&dir, "notes.txt", b"words");
    let middle = typing.at(cx, 960., 540.);
    drop_at(&typing, cx, &[text], middle);
    assert_eq!(typing.source(cx), TALK);
    assert_eq!(typing.status(cx).as_deref(), Some(ONLY_PICTURES));

    // On the slide list, a picture does nothing.
    let picture = outside_file(&dir, "red.png", &png(100, 100, [255, 0, 0, 255]));
    let list = bounds_of(&typing, cx, STRIP_ID).expect("the slide list").center();
    drop_at(&typing, cx, &[picture], list);
    assert_eq!(typing.source(cx), TALK);
    assert!(!can_undo(&typing, cx));
}

#[gpui_kit::test]
fn a_pasted_picture_is_inserted(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    put_on_clipboard(cx, vec![picture_entry(png(100, 100, [255, 0, 0, 255]))]);
    click_on(&typing, cx, PASTE_ID);
    let line = "<Picture src=\"assets/pasted-picture.png\" x=\"910\" y=\"490\" w=\"100\" h=\"100\" />";
    assert_eq!(typing.source(cx), with_inserted(line));
    assert_eq!(selected_text(&typing, cx).as_deref(), Some(line));
    assert_eq!(typing.slide_count(cx), 2, "no slide was pasted");
    // Again, with the keys, while it's selected: the same picture, a step aside.
    typing.press(cx, "ctrl-v");
    let again = "<Picture src=\"assets/pasted-picture.png\" x=\"950\" y=\"530\" w=\"100\" h=\"100\" />";
    assert_eq!(typing.source(cx), with_inserted(&format!("{line}\n\n{again}")));
}

#[gpui_kit::test]
fn pasted_file_paths_are_a_drop(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let dir = tempfile::tempdir().unwrap();
    let red = outside_file(&dir, "red.png", &png(100, 100, [255, 0, 0, 255]));
    put_on_clipboard(cx, vec![ClipboardEntry::ExternalPaths(ExternalPaths([red].into_iter().collect()))]);
    click_on(&typing, cx, PASTE_ID);
    let line = "<Picture src=\"assets/red.png\" x=\"910\" y=\"490\" w=\"100\" h=\"100\" />";
    assert_eq!(typing.source(cx), with_inserted(line));
}

#[gpui_kit::test]
fn a_pasted_picture_while_typing_ends_the_typing_first(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    // The first slide's body text.
    let words = typing.flows(cx, 0).len() - 1;
    typing.click_text(cx, words, pos(0, "Some words".len()));
    typing.type_text(cx, "!");
    assert!(typing.editing(cx).is_some());
    put_on_clipboard(cx, vec![picture_entry(png(100, 100, [255, 0, 0, 255]))]);
    typing.press(cx, "ctrl-v");

    assert!(typing.editing(cx).is_none(), "the typing ended");
    let line = "<Picture src=\"assets/pasted-picture.png\" x=\"910\" y=\"490\" w=\"100\" h=\"100\" />";
    let typed = TALK.replacen(END_OF_ONE, "Some words!", 1);
    assert_eq!(typing.source(cx), typed.replacen("Some words!", &format!("Some words!\n\n{line}"), 1));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), typed, "the picture is a step of its own");
}

#[gpui_kit::test]
fn pasting_text_still_pastes_text_when_both_are_on_the_clipboard(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let words = typing.flows(cx, 0).len() - 1;
    typing.click_text(cx, words, pos(0, "Some words".len()));
    put_on_clipboard(
        cx,
        vec![ClipboardEntry::String(" more".to_string().into()), picture_entry(png(10, 10, [255, 0, 0, 255]))],
    );
    typing.press(cx, "ctrl-v");
    assert_eq!(typing.source(cx), TALK.replacen(END_OF_ONE, "Some words more", 1));
    assert!(typing.editing(cx).is_some(), "typing goes on");
}

// ---------------------------------------------------------------------------------------
// Shapes

#[gpui_kit::test]
fn each_shape_is_written_and_drawn(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let accent = "fill=\"#2563eb\"";
    let line = "outline=\"#1e293b\" outline-width=\"6\"";
    let shapes = [
        ("rect", ShapeKind::Rect, format!("<Shape kind=\"rect\" {accent} x=\"720\" y=\"380\" w=\"480\" h=\"320\" />")),
        ("rounded", ShapeKind::Rounded, format!("<Shape kind=\"rounded\" {accent} x=\"720\" y=\"380\" w=\"480\" h=\"320\" />")),
        ("ellipse", ShapeKind::Ellipse, format!("<Shape kind=\"ellipse\" {accent} x=\"720\" y=\"380\" w=\"480\" h=\"320\" />")),
        ("line", ShapeKind::Line, format!("<Shape kind=\"line\" {line} x=\"720\" y=\"480\" w=\"480\" h=\"120\" />")),
        ("arrow", ShapeKind::Arrow, format!("<Shape kind=\"arrow\" {line} x=\"720\" y=\"480\" w=\"480\" h=\"120\" />")),
        ("callout", ShapeKind::Callout, format!("<Shape kind=\"callout\" {accent} x=\"720\" y=\"380\" w=\"480\" h=\"320\" />")),
    ];
    for (kind, native, written) in shapes {
        insert_shape(&typing, cx, kind);
        let source = typing.source(cx);
        assert_eq!(source, with_inserted(&written), "{kind}");
        assert_eq!(selected_text(&typing, cx).as_deref(), Some(written.as_str()), "{kind}");
        let made = native_of(&folder_of(&typing), &source, 0);
        assert!(
            made.iter().any(|content| matches!(content, NativeContent::Shape { kind, .. } if *kind == native)),
            "{kind} is drawn as one"
        );
        assert_eq!(typing.document(cx).read_with(cx, |document, _| document.undo_label()), Some("Insert shape"));
        typing.press(cx, "ctrl-z");
        assert_eq!(typing.source(cx), TALK);
    }
}

#[gpui_kit::test]
fn undo_of_an_insert_takes_it_away_and_deselects(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    insert_shape(&typing, cx, "ellipse");
    let written = selected_text(&typing, cx).expect("the new shape is selected");
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TALK);
    assert_eq!(selected(&typing, cx), None);
    typing.press(cx, "ctrl-shift-z");
    assert_eq!(selected_text(&typing, cx), Some(written), "made again, it's selected again");
}

#[gpui_kit::test]
fn a_second_shape_steps_aside_from_the_first(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    insert_shape(&typing, cx, "rect");
    insert_shape(&typing, cx, "rect");
    let source = typing.source(cx);
    assert!(source.contains("x=\"720\" y=\"380\""), "{source}");
    assert!(source.contains("x=\"760\" y=\"420\""), "{source}");
}

#[gpui_kit::test]
fn typing_on_a_selected_shape_puts_words_in_it(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    insert_shape(&typing, cx, "rect");
    typing.type_text(cx, "Hi");
    let shape = "<Shape kind=\"rect\" fill=\"#2563eb\" x=\"720\" y=\"380\" w=\"480\" h=\"320\">\n\nHi\n\n</Shape>";
    assert_eq!(typing.source(cx), with_inserted(shape));
    assert!(typing.editing(cx).is_some(), "the caret is in it");
    assert_eq!(selected_text(&typing, cx).as_deref(), Some(shape), "it stays selected");
    typing.press(cx, "escape");

    // The words go out with the shape.
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("talk.pptx");
    let input = typing.document(cx).read_with(cx, |document, _| document.export_input());
    run_export(input, ExportKind::PowerPoint, &out).unwrap();
    let mut archive = zip::ZipArchive::new(File::open(&out).unwrap()).unwrap();
    let mut slide = String::new();
    std::io::Read::read_to_string(&mut archive.by_name("ppt/slides/slide1.xml").unwrap(), &mut slide).unwrap();
    assert!(slide.contains(">Hi<"), "{slide}");

    // Undone, the typing goes, and then the shape.
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), with_inserted(&shape.replace(">\n\nHi\n\n</Shape>", " />")));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TALK);
}

#[gpui_kit::test]
fn a_dead_key_on_a_selected_shape_types_its_letter_in_it(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    insert_shape(&typing, cx, "ellipse");
    compose(&typing, cx, "´");
    assert_eq!(
        typing.editing(cx).and_then(|editing| editing.composing).map(|composing| composing.text).as_deref(),
        Some("´")
    );
    commit(&typing, cx, "é");
    let shape = "<Shape kind=\"ellipse\" fill=\"#2563eb\" x=\"720\" y=\"380\" w=\"480\" h=\"320\">\n\né\n\n</Shape>";
    assert_eq!(typing.source(cx), with_inserted(shape));
}

#[gpui_kit::test]
fn typing_on_a_selected_picture_does_nothing(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let dir = tempfile::tempdir().unwrap();
    let path = outside_file(&dir, "red.png", &png(100, 100, [255, 0, 0, 255]));
    choose(&typing, cx, vec![path]);
    let before = typing.source(cx);
    typing.type_text(cx, "x");
    assert_eq!(typing.source(cx), before);
    assert!(typing.editing(cx).is_none());
}

// ---------------------------------------------------------------------------------------
// Text boxes

/// The new text box waiting for its first character, if there's one: its slide and where it is.
fn new_box(typing: &Typing, cx: &TestAppContext) -> Option<(usize, (f32, f32, f32, f32))> {
    typing.document(cx).read_with(cx, |document, _| document.new_box())
}

fn unsaved(typing: &Typing, cx: &TestAppContext) -> bool {
    typing.document(cx).read_with(cx, |document, _| document.has_unsaved_changes())
}

fn undo_label(typing: &Typing, cx: &TestAppContext) -> Option<&'static str> {
    typing.document(cx).read_with(cx, |document, _| document.undo_label())
}

const NEW_BOX: (usize, (f32, f32, f32, f32)) = (0, (640., 440., 640., 200.));

fn written_box(words: &str) -> String {
    with_inserted(&format!("<Box x=\"640\" y=\"440\" w=\"640\" h=\"200\">\n\n{words}\n\n</Box>"))
}

#[gpui_kit::test]
fn a_text_box_writes_nothing_until_a_character_is_typed(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    insert_text_box(&typing, cx);
    assert_eq!(typing.source(cx), TALK);
    assert!(!unsaved(&typing, cx), "nothing to save");
    assert!(!can_undo(&typing, cx), "nothing to undo");
    assert_eq!(new_box(&typing, cx), Some(NEW_BOX));
    assert!(typing.editing(cx).is_some(), "typing has begun in it");
    assert_eq!(selected(&typing, cx), None);
    // Drawn where it'll be, with the caret at its corner.
    let drawn = bounds_of(&typing, cx, NEW_BOX_ID).expect("its outline is drawn");
    let (corner, far) = (typing.at(cx, 640., 440.), typing.at(cx, 1280., 640.));
    assert!((drawn.origin.x - corner.x).abs() <= px(1.) && (drawn.bottom() - far.y).abs() <= px(1.), "{drawn:?}");
    let caret = typing.caret(cx).expect("the caret shows");
    assert!((caret.center().x - corner.x).abs() <= px(2.) && (caret.origin.y - corner.y).abs() <= px(2.), "{caret:?} {corner:?}");
    assert!(caret.bottom() <= far.y);
}

#[gpui_kit::test]
fn the_first_character_writes_the_box_in_one_step(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    insert_text_box(&typing, cx);
    typing.type_text(cx, "H");
    assert_eq!(typing.source(cx), written_box("H"));
    assert_eq!(undo_label(&typing, cx), Some("Insert text box"));
    assert_eq!(new_box(&typing, cx), None);
    assert!(bounds_of(&typing, cx, NEW_BOX_ID).is_none());
    typing.type_text(cx, "i");
    assert_eq!(typing.source(cx), written_box("Hi"));
    assert_eq!(undo_label(&typing, cx), Some("Typing"), "later typing is a step of its own");

    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), written_box("H"));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TALK, "the box goes whole");
    assert_eq!(new_box(&typing, cx), None, "and nothing is left waiting");
    assert!(!can_undo(&typing, cx));
    typing.press(cx, "ctrl-shift-z");
    assert_eq!(typing.source(cx), written_box("H"));
    assert_eq!(typing.selection(cx), caret_at(pos(0, 1)), "the caret after what was typed");
    typing.type_text(cx, "o");
    assert_eq!(typing.source(cx), written_box("Ho"));
}

#[gpui_kit::test]
fn undoing_the_first_character_leaves_nothing(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    insert_text_box(&typing, cx);
    typing.type_text(cx, "a");
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TALK);
    typing.press(cx, "escape");
    assert_eq!(typing.source(cx), TALK);
    assert!(!can_undo(&typing, cx));
    assert_eq!(typing.document(cx).read_with(cx, |document, _| document.redo_label()), Some("Insert text box"));
}

#[gpui_kit::test]
fn escape_or_a_click_elsewhere_drops_a_pending_box(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    insert_text_box(&typing, cx);
    typing.press(cx, "escape");
    assert_eq!(new_box(&typing, cx), None);
    assert!(typing.editing(cx).is_none());

    insert_text_box(&typing, cx);
    // A click in it keeps it.
    let inside = typing.at(cx, 900., 540.);
    typing.click(cx, inside);
    assert_eq!(new_box(&typing, cx), Some(NEW_BOX));
    let away = typing.at(cx, 100., 1000.);
    typing.click(cx, away);
    assert_eq!(new_box(&typing, cx), None);

    insert_text_box(&typing, cx);
    let document = typing.document(cx);
    cx.update(|cx| document.update(cx, |document, cx| {
        document.go_to(1);
        cx.notify();
    }));
    typing.settle(cx);
    assert_eq!(typing.current(cx), 1);
    assert_eq!(new_box(&typing, cx), None, "leaving the slide drops it");

    cx.update(|cx| document.update(cx, |document, cx| {
        document.go_to(0);
        cx.notify();
    }));
    typing.settle(cx);
    insert_text_box(&typing, cx);
    insert_shape(&typing, cx, "rect");
    assert_eq!(new_box(&typing, cx), None, "another insert drops it");
    assert_eq!(undo_label(&typing, cx), Some("Insert shape"));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TALK);
    assert!(!can_undo(&typing, cx), "nothing of the boxes was ever a step");
}

#[gpui_kit::test]
fn a_second_text_box_replaces_the_pending_one_and_takes_the_caret(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    insert_text_box(&typing, cx);
    click_on(&typing, cx, INSERT_TEXT_BOX_ID);
    assert_eq!(new_box(&typing, cx), Some((0, (680., 480., 640., 200.))), "a step aside from the first");
    assert!(typing.editing(cx).is_some());
    assert_eq!(typing.source(cx), TALK);
    assert!(!can_undo(&typing, cx));
    typing.type_text(cx, "x");
    assert_eq!(typing.source(cx), with_inserted("<Box x=\"680\" y=\"480\" w=\"640\" h=\"200\">\n\nx\n\n</Box>"));
    typing.press(cx, "escape");
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TALK);
    assert!(!can_undo(&typing, cx));
}

#[gpui_kit::test]
fn save_present_and_export_with_a_pending_box_write_nothing(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    insert_shape(&typing, cx, "rect");
    let with_shape = typing.source(cx);
    insert_text_box(&typing, cx);
    typing.press(cx, "ctrl-s");
    cx.run_until_parked();
    typing.settle(cx);
    assert_eq!(std::fs::read_to_string(&typing.file).unwrap(), with_shape, "only the shape is saved");
    assert_eq!(typing.source(cx), with_shape);
    assert!(!unsaved(&typing, cx));
    assert_eq!(new_box(&typing, cx), None);
    assert_eq!(undo_label(&typing, cx), Some("Insert shape"));

    insert_text_box(&typing, cx);
    typing.press(cx, "f5");
    assert_eq!(new_box(&typing, cx), None, "a show drops it");
    assert_eq!(typing.source(cx), with_shape);

    insert_text_box(&typing, cx);
    typing.with_window(cx, |window, cx| window.dispatch_action(Box::new(ExportPdf), cx));
    typing.settle(cx);
    assert_eq!(new_box(&typing, cx), None, "an export drops it");
    assert_eq!(typing.source(cx), with_shape);
    assert!(!unsaved(&typing, cx));
    assert_eq!(undo_label(&typing, cx), Some("Insert shape"));
}

#[gpui_kit::test]
fn the_inspector_has_no_form_for_a_pending_box(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    insert_shape(&typing, cx, "rect");
    assert!(bounds_of(&typing, cx, INSPECTOR_HEADING_ID).is_some(), "the shape's");
    insert_text_box(&typing, cx);
    assert_eq!(selected(&typing, cx), None);
    assert!(bounds_of(&typing, cx, INSPECTOR_HEADING_ID).is_none(), "nothing selected, so no form");
    assert!(bounds_of(&typing, cx, INSPECTOR_MESSAGE_ID).is_some(), "but the hint");
    // A setting changed meanwhile, as from a field, changes nothing and loses nothing.
    let with_shape = typing.source(cx);
    let document = typing.document(cx);
    let changed = cx.update(|cx| document.update(cx, |document, _| document.set_selected_attr("w", Some("700"))));
    assert_eq!(changed, Ok(Vec::new()));
    assert_eq!(typing.source(cx), with_shape);
    assert_eq!(undo_label(&typing, cx), Some("Insert shape"));
}

#[gpui_kit::test]
fn a_dead_key_into_a_pending_box_writes_the_composed_character(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    insert_text_box(&typing, cx);
    compose(&typing, cx, "´");
    assert_eq!(typing.source(cx), TALK, "what's composed isn't written");
    assert!(new_box(&typing, cx).is_some());
    commit(&typing, cx, "é");
    assert_eq!(typing.source(cx), written_box("é"));
    assert_eq!(undo_label(&typing, cx), Some("Insert text box"));
}

#[gpui_kit::test]
fn pasting_text_into_a_pending_box_writes_it(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    insert_text_box(&typing, cx);
    put_on_clipboard(cx, vec![ClipboardEntry::String("Pasted words".to_string().into())]);
    typing.press(cx, "ctrl-v");
    assert_eq!(typing.source(cx), written_box("Pasted words"));
    assert_eq!(undo_label(&typing, cx), Some("Insert text box"));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), TALK);
    assert!(!can_undo(&typing, cx));

    // A picture drops the box, and goes on the slide.
    insert_text_box(&typing, cx);
    put_on_clipboard(cx, vec![picture_entry(png(100, 100, [255, 0, 0, 255]))]);
    typing.press(cx, "ctrl-v");
    assert_eq!(new_box(&typing, cx), None);
    let line = "<Picture src=\"assets/pasted-picture.png\" x=\"910\" y=\"490\" w=\"100\" h=\"100\" />";
    assert_eq!(typing.source(cx), with_inserted(line));
}

#[gpui_kit::test]
fn an_emptied_text_box_stays_as_an_empty_box(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    insert_text_box(&typing, cx);
    typing.type_text(cx, "ab");
    typing.press(cx, "backspace");
    typing.press(cx, "backspace");
    typing.press(cx, "escape");
    let source = typing.source(cx);
    assert!(source.contains("<Box x=\"640\" y=\"440\" w=\"640\" h=\"200\">"), "{source}");
    // An ordinary box: it can be picked and taken away.
    let start = source.find("<Box").unwrap();
    let end = source.find("</Box>").unwrap() + "</Box>".len();
    let document = typing.document(cx);
    assert!(cx.update(|cx| document.update(cx, |document, _| document.select(0, start..end))));
    let taken = cx.update(|cx| document.update(cx, |document, _| document.delete_selected()));
    assert!(taken.is_ok());
    assert_eq!(typing.source(cx), TALK);
}

#[gpui_kit::test]
fn a_pending_box_schedules_only_the_caret_blink(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    insert_text_box(&typing, cx);
    let editor = editor(&typing, cx);
    // Past the time the caret blinks for, nothing more is drawn.
    wait(cx, 11_000);
    let (count, _watching) = redraws(cx, &editor);
    let (doc_count, _watching_doc) = redraws(cx, &typing.document(cx));
    wait(cx, 3_000);
    assert_eq!(count.get(), 0, "the editor is idle");
    assert_eq!(doc_count.get(), 0, "the talk is idle");
    assert!(new_box(&typing, cx).is_some(), "still waiting");
}

#[gpui_kit::test]
fn more_than_twenty_dropped_files_take_the_first_twenty(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    let dir = tempfile::tempdir().unwrap();
    let paths: Vec<PathBuf> = (0..21).map(|at| outside_file(&dir, &format!("p{at}.png"), &png(10, 10, [at as u8, 0, 0, 255]))).collect();
    let middle = typing.at(cx, 960., 540.);
    drop_at(&typing, cx, &paths, middle);
    assert_eq!(typing.source(cx).matches("<Picture").count(), 20);
    assert_eq!(typing.status(cx).as_deref(), Some(ONLY_THE_FIRST));
    assert_eq!(ONLY_THE_FIRST, "Only the first 20 pictures were added.");
    assert_eq!(DIALOG_FAILED, "The picture couldn't be opened.");
}

// ---------------------------------------------------------------------------------------
// Bytes

#[gpui_kit::test]
fn inserting_changes_no_other_byte(cx: &mut TestAppContext) {
    let talk = "---\r\ntitle: T\r\n---\r\n\r\n# One\r\n\r\n- a\r\n- b\r\n\r\n---\r\n\r\n# Two  \r\n\r\nWords *here*\r\n\r\n---\r\n\r\n# Three\r\n";
    let typing = open(cx, talk);
    typing.press(cx, "pagedown");
    assert_eq!(typing.current(cx), 1);
    insert_shape(&typing, cx, "rect");
    let after = typing.source(cx);
    let added = "\r\n\r\n<Shape kind=\"rect\" fill=\"#2563eb\" x=\"720\" y=\"380\" w=\"480\" h=\"320\" />";
    let at = talk.find("Words *here*").unwrap() + "Words *here*".len();
    assert_eq!(after, format!("{}{added}{}", &talk[..at], &talk[at..]));
}

#[gpui_kit::test]
fn inserting_on_a_slide_with_notes_goes_before_them(cx: &mut TestAppContext) {
    let talk = "# One\n\nWords\n\n<Notes>\nSay it\n</Notes>\n";
    let typing = open(cx, talk);
    insert_shape(&typing, cx, "rect");
    let shape = "<Shape kind=\"rect\" fill=\"#2563eb\" x=\"720\" y=\"380\" w=\"480\" h=\"320\" />";
    assert_eq!(typing.source(cx), format!("# One\n\nWords\n\n{shape}\n\n<Notes>\nSay it\n</Notes>\n"));
    let notes = typing.document(cx).read_with(cx, |document, _| document.notes_of(0));
    assert_eq!(notes.as_deref(), Some("Say it"));
}

// ---------------------------------------------------------------------------------------
// The tab

#[gpui_kit::test]
fn the_insert_tab_fits_at_1024_pixels_or_scrolls(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    cx.simulate_window_resize(typing.handle, size(px(1024.), px(600.)));
    typing.settle(cx);
    insert_tab(&typing, cx);
    let ribbon = bounds_of(&typing, cx, RIBBON_ID).unwrap();
    let scrolls = bounds_of(&typing, cx, MORE_RIGHT_ID).is_some();
    for tool in [INSERT_PICTURE_ID, INSERT_SHAPES_ID, INSERT_TEXT_BOX_ID, INSERT_ICON_ID] {
        let bounds = bounds_of(&typing, cx, tool).unwrap_or_else(|| panic!("{tool} is there"));
        assert!(scrolls || bounds.right() <= ribbon.right(), "{tool} is cut off");
    }
    // The tools work from there.
    click_on(&typing, cx, INSERT_TEXT_BOX_ID);
    assert!(new_box(&typing, cx).is_some());
}

// ---------------------------------------------------------------------------------------
// A crash

#[gpui_kit::test]
fn a_picture_inserted_and_not_saved_survives_a_crash(cx: &mut TestAppContext) {
    let app_data = tempfile::tempdir().unwrap();
    let recovery = Recovery::new(app_data.path().join("recovery"));
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("slides.md"), TALK).unwrap();
    let typing = open_folder(cx, folder, Some(recovery.clone()));
    let dir = tempfile::tempdir().unwrap();
    let path = outside_file(&dir, "red.png", &png(100, 100, [255, 0, 0, 255]));
    choose(&typing, cx, vec![path]);
    wait(cx, 1_000);
    let inserted = typing.source(cx);
    let Typing { _folder: folder, handle, view, .. } = typing;
    drop(view);
    cx.update_window(handle, |_, window, _| window.remove_window()).unwrap();
    cx.run_until_parked();
    assert!(!folder.path().join("assets").exists(), "nothing written until a save");

    let typing = open_folder(cx, folder, Some(recovery));
    click_on(&typing, cx, RECOVER_ID);
    assert_eq!(typing.source(cx), inserted);
    let document = typing.document(cx);
    let drawn = cx.update(|cx| {
        document.update(cx, |document, _| {
            assert!(document.has_file("assets/red.png"));
            assert!(document.notices().is_empty());
            let layers = document.layers(1.0);
            layers.iter().any(|layer| {
                let pixmap = &layer.pixmap;
                pixmap.pixels().iter().any(|pixel| pixel.red() > 200 && pixel.green() < 50 && pixel.blue() < 50 && pixel.alpha() == 255)
            })
        })
    });
    assert!(drawn, "the picture draws");
}

// ---------------------------------------------------------------------------------------
// What writes a new text box

#[gpui_kit::test]
fn blank_input_leaves_a_text_box_pending(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    insert_text_box(&typing, cx);
    let still_pending = |typing: &Typing, cx: &mut TestAppContext, what: &str| {
        assert_eq!(typing.source(cx), TALK, "{what}");
        assert!(!can_undo(typing, cx), "{what}");
        assert_eq!(new_box(typing, cx), Some(NEW_BOX), "{what}");
        assert!(typing.caret(cx).is_some(), "{what}: the caret shows");
    };
    for key in ["enter", "shift-enter", "tab", "backspace", "delete"] {
        typing.press(cx, key);
        still_pending(&typing, cx, key);
    }
    typing.type_text(cx, " ");
    still_pending(&typing, cx, "a space");
    typing.type_text(cx, "\u{a0}");
    still_pending(&typing, cx, "a no-break space");
    put_on_clipboard(cx, vec![ClipboardEntry::String("\n\n".to_string().into())]);
    typing.press(cx, "ctrl-v");
    still_pending(&typing, cx, "a paste of blank lines");
}

#[gpui_kit::test]
fn the_first_visible_character_after_blanks_writes_only_it(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    insert_text_box(&typing, cx);
    typing.press(cx, "enter");
    typing.type_text(cx, " ");
    typing.type_text(cx, "H");
    assert_eq!(typing.source(cx), written_box("H"));
    assert_eq!(typing.selection(cx), caret_at(pos(0, 1)));
    assert_eq!(undo_label(&typing, cx), Some("Insert text box"));
}

#[gpui_kit::test]
fn a_paste_starting_with_blank_lines_writes_from_its_first_words(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    insert_text_box(&typing, cx);
    put_on_clipboard(cx, vec![ClipboardEntry::String("\n\n  Hello\nWorld".to_string().into())]);
    typing.press(cx, "ctrl-v");
    assert_eq!(typing.source(cx), written_box("Hello\n\nWorld"));
    assert_eq!(typing.selection(cx), caret_at(pos(1, "World".len())));
    assert_eq!(undo_label(&typing, cx), Some("Insert text box"));
}

/// What can't be seen: white space, and characters that draw nothing. The property below takes
/// its inputs from these and from `VISIBLE`, so it knows which inputs should write a box without
/// asking the app.
const INVISIBLE: [&str; 15] = [
    " ", "\u{a0}", "\t", "\n", "\u{2028}", "\u{200b}", "\u{200c}", "\u{200d}", "\u{2060}", "\u{feff}", "\u{ad}",
    "\u{fffc}", "\u{fe0f}", "\u{e0100}", "\u{3000}",
];
/// What can be seen, a lone combining mark among them, as it draws.
const VISIBLE: [&str; 3] = ["a", "é", "\u{301}"];

/// A step of typing into a new text box, for the property below.
#[derive(Clone, Debug)]
enum Input {
    Text(Vec<(&'static str, bool)>),
    Enter,
    LineBreak,
    Backspace,
}

fn input() -> impl proptest::strategy::Strategy<Value = Input> {
    use proptest::prelude::*;
    let piece = prop_oneof![
        4 => prop::sample::select(INVISIBLE.to_vec()).prop_map(|text| (text, false)),
        1 => prop::sample::select(VISIBLE.to_vec()).prop_map(|text| (text, true)),
    ];
    prop_oneof![
        proptest::collection::vec(piece, 1..4).prop_map(Input::Text),
        Just(Input::Enter),
        Just(Input::LineBreak),
        Just(Input::Backspace),
    ]
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig { cases: 64, ..Default::default() })]

    #[test]
    fn typing_into_a_new_box_writes_it_only_with_something_visible(inputs in proptest::collection::vec(input(), 1..6)) {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("slides.md"), TALK).unwrap();
        let mut document = stagepack_studio::document::Document::open(folder.path(), Fonts::bundled_only()).unwrap();
        document.insert_text_box().unwrap();
        let mut seen = false;
        for input in inputs {
            if let Input::Text(pieces) = &input {
                seen |= pieces.iter().any(|(_, visible)| *visible);
            }
            let _ = document.apply_text(|edit| match &input {
                Input::Text(pieces) => edit.insert(&pieces.iter().map(|(text, _)| *text).collect::<String>()),
                Input::Enter => edit.enter(),
                Input::LineBreak => edit.soft_break(),
                Input::Backspace => edit.backspace(),
            });
            let source = document.source().to_string();
            if seen {
                proptest::prop_assert!(source.contains("<Box"), "something visible was typed: {:?}", source);
            } else {
                proptest::prop_assert_eq!(&source, TALK);
                proptest::prop_assert!(document.new_box().is_some());
            }
        }
    }
}

#[gpui_kit::test]
fn invisible_characters_leave_a_text_box_pending(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    insert_text_box(&typing, cx);
    for text in ["\u{200b}", "\u{200c}", "\u{200d}", "\u{2060}", "\u{fffc}", "\u{fe0f}", "\u{e0100}", "\u{ad}"] {
        commit(&typing, cx, text);
        assert_eq!(typing.source(cx), TALK, "{text:?}");
        assert!(!can_undo(&typing, cx), "{text:?}");
        assert!(new_box(&typing, cx).is_some(), "{text:?}");
        put_on_clipboard(cx, vec![ClipboardEntry::String(format!("{text}\n{text}").into())]);
        typing.press(cx, "ctrl-v");
        assert_eq!(typing.source(cx), TALK, "pasted {text:?}");
    }
    // A combining mark draws, even alone.
    commit(&typing, cx, "\u{301}");
    assert!(typing.source(cx).contains("<Box"));
}

#[test]
fn what_is_visible_is_what_draws() {
    use slides_format::is_visible_char;
    for c in [' ', '\u{a0}', '\t', '\n', '\u{2028}', '\u{3000}', '\u{200b}', '\u{200c}', '\u{200d}', '\u{2060}', '\u{feff}', '\u{ad}', '\u{fffc}', '\u{fe00}', '\u{fe0f}', '\u{e0100}', '\u{e01ef}', '\u{202e}', '\u{7}'] {
        assert!(!is_visible_char(c), "{c:?}");
    }
    for c in ['a', 'é', '\u{301}', '😀', '-', '中'] {
        assert!(is_visible_char(c), "{c:?}");
    }
}

#[gpui_kit::test]
fn formatting_chosen_for_a_pending_box_is_written_with_it(cx: &mut TestAppContext) {
    for (tool, written) in [
        (BULLETS_ID, "- x".to_string()),
        (NUMBERING_ID, "1. x".to_string()),
        (BOLD_ID, "**x**".to_string()),
    ] {
        let typing = open(cx, TALK);
        insert_text_box(&typing, cx);
        click_on(&typing, cx, TAB_HOME_ID);
        click_on(&typing, cx, tool);
        assert_eq!(typing.source(cx), TALK, "{tool}: nothing written yet");
        assert!(!can_undo(&typing, cx), "{tool}");
        assert!(new_box(&typing, cx).is_some(), "{tool}: still pending");
        // Blank input doesn't lose it.
        typing.type_text(cx, " ");
        typing.type_text(cx, "x");
        assert_eq!(typing.source(cx), written_box(&written), "{tool}");
        assert_eq!(undo_label(&typing, cx), Some("Insert text box"), "{tool}");
    }
}

#[gpui_kit::test]
fn an_alignment_chosen_for_a_pending_box_is_written_with_it(cx: &mut TestAppContext) {
    let typing = open(cx, TALK);
    insert_text_box(&typing, cx);
    click_on(&typing, cx, TAB_HOME_ID);
    click_on(&typing, cx, ALIGN_CENTER_ID);
    assert_eq!(typing.source(cx), TALK);
    assert!(!can_undo(&typing, cx));
    let shown = typing.document(cx).read_with(cx, |document, _| document.text_align());
    assert_eq!(shown, Some(slides_engine::Align::Center), "the tool shows it");
    typing.type_text(cx, "x");
    assert_eq!(
        typing.source(cx),
        with_inserted("<Box x=\"640\" y=\"440\" w=\"640\" h=\"200\" align=\"center\">\n\nx\n\n</Box>")
    );
    assert_eq!(undo_label(&typing, cx), Some("Insert text box"));
}

#[gpui_kit::test]
fn a_styled_paste_into_a_pending_box_keeps_what_the_paste_rules_give(cx: &mut TestAppContext) {
    use slides_format::{Para, ParaKind, Span};
    let para = |kind, text: &str| Para { kind, spans: vec![Span { text: text.to_string(), style: Default::default() }] };
    let cases = [
        (vec![para(ParaKind::Heading(2), "Head"), para(ParaKind::Body, "words")], "Head\n\nwords"),
        (vec![para(ParaKind::Bullet { level: 0 }, "one"), para(ParaKind::Bullet { level: 0 }, "two")], "- one\n- two"),
    ];
    for (paras, written) in cases {
        let typing = open(cx, TALK);
        insert_text_box(&typing, cx);
        let entry = stagepack_studio::clipboard::text_entry(&paras);
        cx.write_to_clipboard(ClipboardItem::new_string_with_metadata(entry.text, entry.metadata.expect("the app's own")));
        typing.press(cx, "ctrl-v");
        assert_eq!(typing.source(cx), written_box(written));
    }
}
