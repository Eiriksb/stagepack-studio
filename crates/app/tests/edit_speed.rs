//! How long a keystroke takes on a long talk, before anything is drawn. Only meaningful in a
//! release build: `scripts/cargo-safe.sh test --release -p stagepack-studio --test edit_speed -- --ignored`.

use std::time::{Duration, Instant};

use slides_engine::Fonts;
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

    times.sort();
    let median = times[times.len() / 2];
    println!(
        "median keystroke on a {SLIDES}-slide talk: {median:?} (slowest {:?})",
        times.last().unwrap()
    );
    assert!(median < MOST, "a keystroke took {median:?}");
}
