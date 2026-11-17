//! How long a keystroke takes on a long talk, before anything is drawn. Only meaningful in a
//! release build: `scripts/cargo-safe.sh test --release -p stagepack-studio --test edit_speed -- --ignored`.

use std::time::{Duration, Instant};

use slides_engine::{Fonts, Rect};
use slides_format::{FlowPlace, Pos, text_flows};
use stagepack_studio::arrange::{Placed, bounds};
use stagepack_studio::document::{Document, Thing};
use stagepack_studio::snap::snap_move;

const SLIDES: usize = 500;
const KEYSTROKES: usize = 200;
const MOST: Duration = Duration::from_millis(8);

/// A talk of `SLIDES` slides, each like one written by hand: settings on some, a heading,
/// a few paragraphs and a list, and notes.
fn long_talk() -> String {
    let mut talk = String::from("---\ntitle: A long talk\n---\n\n");
    for n in 0..SLIDES {
        if n > 0 {
            talk.push_str(if n % 7 == 0 {
                "---\nlayout: section\n---\n\n"
            } else {
                "---\n\n"
            });
        }
        talk.push_str(&format!(
            "# Slide {n}\n\nSome **bold** words and some *others*, about point {n}.\n\n\
             - the first thing\n- the second thing\n- the third\n\nA closing line with `code` in it.\n\n\
             <Notes>\nWhat to say on slide {n}.\n</Notes>\n\n"
        ));
    }
    talk
}

fn report(what: &str, mut times: Vec<Duration>) {
    times.sort();
    let median = times[times.len() / 2];
    println!(
        "median {what} on a {SLIDES}-slide talk: {median:?} (slowest {:?})",
        times.last().unwrap()
    );
    assert!(median < MOST, "a keystroke took {median:?}");
}

/// A letter typed on a slide: written to the talk, read back, and the slide's text laid out
/// again for its caret, as the window does before it draws.
#[test]
#[ignore = "timing; run in release"]
fn a_keystroke_typed_on_a_slide_of_a_long_talk_is_quick() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), long_talk()).unwrap();
    let mut doc = Document::open(dir.path(), Fonts::bundled_only()).unwrap();
    let slide = SLIDES / 2;
    doc.go_to(slide);
    let flows = text_flows(doc.talk(), slide);
    let body = flows
        .iter()
        .position(|flow| flow.place == FlowPlace::Region("body".into()))
        .expect("the slide has words");
    let words: usize = flows[body].paras[0].spans.iter().map(|span| span.text.len()).sum();
    assert!(doc.begin_text(slide, body, Pos { para: 0, offset: words }));
    // The slide is showing, so it's laid out already.
    doc.text_layout(slide);

    let mut times = Vec::with_capacity(KEYSTROKES);
    for key in 0..KEYSTROKES {
        let letter = if key % 6 == 5 { " " } else { "a" };
        let start = Instant::now();
        doc.apply_text(|edit| edit.insert(letter)).unwrap();
        let head = doc.text().unwrap().edit.selection.head;
        let layout = doc.text_layout(slide);
        assert!(layout.caret(body, head).is_some());
        times.push(start.elapsed());
    }
    let typed: String = (0..KEYSTROKES).map(|key| if key % 6 == 5 { ' ' } else { 'a' }).collect();
    let flows = text_flows(doc.talk(), slide);
    let text: String = flows[body].paras[0].spans.iter().map(|span| span.text.as_str()).collect();
    assert!(text.ends_with(typed.trim_end()), "{text}");
    assert_eq!(doc.undo_label(), Some("Typing"));
    report("keystroke typed on a slide", times);
}

#[test]
#[ignore = "timing; run in release"]
fn a_keystroke_on_a_long_talk_is_quick() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), long_talk()).unwrap();
    let mut doc = Document::open(dir.path(), Fonts::bundled_only()).unwrap();
    assert_eq!(doc.slide_count(), SLIDES);
    let slide = SLIDES / 2;
    let mut notes = doc.notes_of(slide).unwrap();

    let mut times = Vec::with_capacity(KEYSTROKES);
    for key in 0..KEYSTROKES {
        notes.push(if key % 6 == 5 { ' ' } else { 'a' });
        let start = Instant::now();
        doc.set_notes(slide, &notes).unwrap();
        times.push(start.elapsed());
    }
    assert_eq!(doc.notes_of(slide).as_deref(), Some(notes.trim_end()));
    report("keystroke in the notes", times);
}

/// A letter typed on a slide, as above, with three pictures added to the talk and not yet saved,
/// which the talk shows: the window asks after each whether there's anything unsaved.
#[test]
#[ignore = "timing; run in release"]
fn a_keystroke_with_added_pictures_to_save_is_quick() {
    let mut source = long_talk();
    for n in 1..=3 {
        source = source.replacen(&format!("# Slide {n}\n"), &format!("# Slide {n}\n\n![](assets/p{n}.png)\n"), 1);
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), source).unwrap();
    let mut doc = Document::open(dir.path(), Fonts::bundled_only()).unwrap();
    for n in 1..=3u8 {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(8, 8, image::Rgba([n, 0, 0, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let path = doc.add_file(stagepack_studio::added_files::FileKind::Picture, &format!("p{n}"), png).unwrap();
        assert_eq!(path, format!("assets/p{n}.png"));
    }
    assert!(doc.has_unsaved_changes(), "the pictures are to save");

    // Asked once after a change of what's added, with the text as it's saved.
    let start = Instant::now();
    assert_eq!(doc.added_files().len(), 3);
    println!("working out the files to save on a {SLIDES}-slide talk: {:?}", start.elapsed());

    let slide = SLIDES / 2;
    doc.go_to(slide);
    let flows = text_flows(doc.talk(), slide);
    let body = flows
        .iter()
        .position(|flow| flow.place == FlowPlace::Region("body".into()))
        .expect("the slide has words");
    let words: usize = flows[body].paras[0].spans.iter().map(|span| span.text.len()).sum();
    assert!(doc.begin_text(slide, body, Pos { para: 0, offset: words }));
    doc.text_layout(slide);

    let mut times = Vec::with_capacity(KEYSTROKES);
    for key in 0..KEYSTROKES {
        let letter = if key % 6 == 5 { " " } else { "a" };
        let start = Instant::now();
        doc.apply_text(|edit| edit.insert(letter)).unwrap();
        let head = doc.text().unwrap().edit.selection.head;
        assert!(doc.text_layout(slide).caret(body, head).is_some());
        assert!(doc.has_unsaved_changes());
        times.push(start.elapsed());
    }
    report("keystroke typed with three pictures to save", times);
}

/// A letter typed in a cell of a table of 6 columns and 8 rows under its heading row, on a
/// slide of the long talk: written to the talk, read back, and the slide laid out again.
#[test]
#[ignore = "timing; run in release"]
fn a_keystroke_in_a_table_cell_is_quick() {
    let slide = SLIDES / 2;
    let table = slides_format::new_table(6, 8).unwrap().replace("|  ", "| word ");
    let source = long_talk().replacen(
        &format!("A closing line with `code` in it.\n\n<Notes>\nWhat to say on slide {slide}."),
        &format!("A closing line with `code` in it.\n\n{table}\n\n<Notes>\nWhat to say on slide {slide}."),
        1,
    );
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), source).unwrap();
    let mut doc = Document::open(dir.path(), Fonts::bundled_only()).unwrap();
    doc.go_to(slide);
    let flows = text_flows(doc.talk(), slide);
    let cell = flows
        .iter()
        .position(|flow| matches!(flow.place, FlowPlace::Cell { row: 4, column: 3, .. }))
        .expect("the slide has the table");
    let cells: Vec<_> = flows.iter().filter(|flow| matches!(flow.place, FlowPlace::Cell { .. })).collect();
    assert_eq!(cells.len(), 6 * 9);
    // Every cell holds a word, as a table typed in does.
    assert!(cells.iter().all(|cell| cell.paras.iter().any(|para| !para.spans.is_empty())), "a cell is empty");
    assert!(doc.begin_text(slide, cell, Pos::default()));
    doc.text_layout(slide);

    let mut times = Vec::with_capacity(KEYSTROKES);
    for key in 0..KEYSTROKES {
        let letter = if key % 6 == 5 { " " } else { "a" };
        let start = Instant::now();
        doc.apply_text(|edit| edit.insert(letter)).unwrap();
        let head = doc.text().unwrap().edit.selection.head;
        let layout = doc.text_layout(slide);
        assert!(layout.caret(cell, head).is_some());
        times.push(start.elapsed());
    }
    let flows = text_flows(doc.talk(), slide);
    let text: String = flows[cell].paras[0].spans.iter().map(|span| span.text.as_str()).collect();
    let typed: String = (0..KEYSTROKES).map(|key| if key % 6 == 5 { ' ' } else { 'a' }).collect();
    assert_eq!(text, format!("{typed}word"));
    assert_eq!(doc.undo_label(), Some("Typing"));
    assert_eq!(doc.table_at_caret().map(|(_, row, column)| (row, column)), Some((4, 3)));
    report("keystroke typed in a table cell", times);
}

/// How long one frame of a drag may take, at the median, before anything is drawn.
const MOST_FOR_A_DRAG_FRAME: Duration = Duration::from_millis(6);

/// A long talk with a slide of sixty placed things at its end.
fn sixty_things() -> String {
    let mut talk = long_talk();
    talk.push_str("---\n\n# Sixty\n\n");
    for n in 0..60 {
        talk.push_str(&format!(
            "<Shape kind=\"rect\" fill=\"#2563eb\" x=\"{}\" y=\"{}\" w=\"100\" h=\"80\" />\n\n",
            100 + (n % 10) * 150,
            200 + (n / 10) * 120
        ));
    }
    talk
}

/// Every thing on a slide of sixty dragged: each frame snaps the box holding them all to sixty
/// others, shows them all at their new place, and the slide's layers and its text's places are
/// worked out again, as the window does before it draws.
#[test]
#[ignore = "timing; run in release"]
fn a_frame_of_a_drag_on_a_slide_of_sixty_things_is_quick() {
    let talk = sixty_things();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), &talk).unwrap();
    let mut doc = Document::open(dir.path(), Fonts::bundled_only()).unwrap();
    let slide = doc.slide_count() - 1;
    doc.go_to(slide);
    assert!(doc.select_all_things());
    let starts: Vec<(Thing, Placed)> = doc.selected_boxes().into_iter().map(|(thing, rect, rotate)| (thing, Placed { rect, rotate })).collect();
    assert_eq!(starts.len(), 60);
    // The window's scale: a slide 1280 pixels wide.
    let scale = 1280. / 1920.;
    doc.editor_layers(scale);
    // What's dragged snaps to the things it isn't: here, sixty more, as many lines as the sixty
    // have. They're out of reach, as every line is looked at all the same: frames held on one
    // line would show the same place, which isn't shown again, and time nothing.
    let others: Vec<Rect> = starts.iter().map(|(_, placed)| Rect { x: placed.rect.x + 5000., y: placed.rect.y + 5000., ..bounds(placed) }).collect();
    let within = 6. / scale;
    let mut times = Vec::new();
    for frame in 1..=200 {
        let by = frame as f32 * 1.5;
        let start = Instant::now();
        let covers: Vec<Rect> = starts.iter().map(|(_, placed)| bounds(&Placed { rect: Rect { x: placed.rect.x + by, y: placed.rect.y + by / 2., ..placed.rect }, rotate: placed.rotate })).collect();
        let left = covers.iter().map(|rect| rect.x).fold(f32::INFINITY, f32::min);
        let top = covers.iter().map(|rect| rect.y).fold(f32::INFINITY, f32::min);
        let right = covers.iter().map(|rect| rect.x + rect.w).fold(f32::NEG_INFINITY, f32::max);
        let bottom = covers.iter().map(|rect| rect.y + rect.h).fold(f32::NEG_INFINITY, f32::max);
        let snapped = snap_move(Rect { x: left, y: top, w: right - left, h: bottom - top }, &others, within);
        let places: Vec<(Thing, Placed)> = starts
            .iter()
            .map(|(thing, placed)| {
                let rect = Rect { x: placed.rect.x + by + snapped.dx, y: placed.rect.y + by / 2. + snapped.dy, ..placed.rect };
                (thing.clone(), Placed { rect, rotate: placed.rotate })
            })
            .collect();
        doc.preview_places(&places);
        let layers = doc.editor_layers(scale);
        let layout = doc.text_layout(slide);
        times.push(start.elapsed());
        assert!(layers.len() > 60, "frame {frame}: the heading and the sixty");
        assert_eq!(layout.elements().len(), 60);
    }
    doc.drop_preview();
    assert_eq!(doc.source(), talk, "nothing written");
    times.sort();
    let median = times[times.len() / 2];
    println!("median drag frame of sixty things on a {SLIDES}-slide talk: {median:?} (slowest {:?})", times.last().unwrap());
    assert!(median < MOST_FOR_A_DRAG_FRAME, "a drag frame took {median:?}");
}

/// How long one step on every thing of a slide of sixty may take, at the median: a step taken
/// once, by a key or a tool, not a frame. Taking a tag away is worked out by the format crate a
/// tag at a time, each reading the slide again, which is most of it.
const MOST_FOR_SIXTY: Duration = Duration::from_millis(250);

/// Every thing on a slide of sixty selected with Ctrl+A, then deleted, duplicated or pasted as one
/// step, on a long talk, and undone: each step is worked out a thing at a time.
#[test]
#[ignore = "timing; run in release"]
fn a_step_on_sixty_things_is_quick() {
    let talk = sixty_things();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), &talk).unwrap();
    let mut doc = Document::open(dir.path(), Fonts::bundled_only()).unwrap();
    let slide = doc.slide_count() - 1;
    doc.go_to(slide);
    for (what, step) in [
        ("delete", Document::delete_selected as fn(&mut Document) -> _),
        ("duplicate", Document::duplicate_selected),
        ("paste", |doc: &mut Document| {
            let (text, _) = doc.copy_selected()?;
            doc.paste_element(&text, Default::default()).map(|(changed, _)| changed)
        }),
    ] {
        let mut times = Vec::new();
        for _ in 0..10 {
            assert!(doc.select_all_things());
            assert_eq!(doc.selection().map(|selection| selection.things.len()), Some(60));
            let start = Instant::now();
            step(&mut doc).unwrap();
            times.push(start.elapsed());
            doc.undo().unwrap();
            assert_eq!(doc.source(), talk, "{what}: one step");
        }
        times.sort();
        let median = times[times.len() / 2];
        println!("median {what} of sixty things on a {SLIDES}-slide talk: {median:?} (slowest {:?})", times.last().unwrap());
        assert!(median < MOST_FOR_SIXTY, "{what} of sixty things took {median:?}");
    }
}
