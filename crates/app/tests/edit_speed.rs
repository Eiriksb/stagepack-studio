//! How long a keystroke takes on a long talk, before anything is drawn. Only meaningful in a
//! release build: `scripts/cargo-safe.sh test --release -p stagepack-studio --test edit_speed -- --ignored`.

use std::time::{Duration, Instant};

use slides_engine::Fonts;
use slides_format::{FlowPlace, Pos, text_flows};
use stagepack_studio::document::Document;

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
