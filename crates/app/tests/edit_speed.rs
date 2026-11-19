//! How long a keystroke takes on a long talk, before anything is drawn, and on a talk of videos
//! and one of 3D models; how long opening those talks takes; and what a video playing and a model
//! turning cost the window's thread. Only meaningful in a release build:
//! `scripts/cargo-safe.sh test --release -p stagepack-studio --test edit_speed -- --ignored`.
//!
//! The models are drawn on the best graphics adapter there is, or with `ADAPTER=discrete`,
//! `integrated` or `software` on that kind alone. A test of models skips, saying so, where there's
//! none.

#[path = "support/typing.rs"]
mod typing;

use std::time::{Duration, Instant};

use slides_engine::{Fonts, Rect};
use slides_format::{FlowPlace, Pos, text_flows};
use stagepack_studio::arrange::{Placed, bounds, holding, moved};
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

/// Every thing on a slide of sixty dragged, by `by` at each of 200 frames: each frame snaps the
/// box holding them all to `others`, made from where the sixty start, shows them all at their
/// new place, and the slide's layers and its text's places are worked out again, as the window
/// does before it draws. Gives the median and the slowest frame, how many frames snapped, and
/// how many showed the same place as the frame before.
fn drag_frames(others: impl Fn(&[(Thing, Placed)]) -> Vec<Rect>, by: impl Fn(usize) -> (f32, f32)) -> (Duration, Duration, usize, usize) {
    drag_frames_on(sixty_things(), others, by, |_| {})
}

/// [`drag_frames`] on `talk`, whose last slide has sixty placed things, with `more` worked out
/// at each frame too.
fn drag_frames_on(
    talk: String,
    others: impl Fn(&[(Thing, Placed)]) -> Vec<Rect>,
    by: impl Fn(usize) -> (f32, f32),
    more: impl Fn(&mut Document),
) -> (Duration, Duration, usize, usize) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), &talk).unwrap();
    let mut doc = Document::open(dir.path(), Fonts::bundled_only()).unwrap();
    let slide = doc.slide_count() - 1;
    drag_frames_of(&mut doc, &talk, slide, 60, others, by, more)
}

/// [`drag_frames_on`] for the `count` things on slide `slide` of `doc`, a talk of `talk`.
fn drag_frames_of(
    doc: &mut Document,
    talk: &str,
    slide: usize,
    count: usize,
    others: impl Fn(&[(Thing, Placed)]) -> Vec<Rect>,
    by: impl Fn(usize) -> (f32, f32),
    more: impl Fn(&mut Document),
) -> (Duration, Duration, usize, usize) {
    doc.go_to(slide);
    assert!(doc.select_all_things());
    let starts: Vec<(Thing, Placed)> = doc.selected_boxes().into_iter().map(|(thing, rect, rotate)| (thing, Placed { rect, rotate })).collect();
    assert_eq!(starts.len(), count);
    // The window's scale: a slide 1280 pixels wide.
    let scale = 1280. / 1920.;
    doc.editor_layers(scale);
    let others = others(&starts);
    let within = 6. / scale;
    let (mut times, mut snapped_frames, mut held, mut last) = (Vec::new(), 0, 0, None);
    for frame in 1..=200 {
        let (dx, dy) = by(frame);
        let start = Instant::now();
        let held_by = holding(starts.iter().map(|(_, placed)| moved(placed, (dx, dy)))).expect("sixty");
        let snapped = snap_move(held_by, &others, within);
        let places: Vec<(Thing, Placed)> = starts.iter().map(|(thing, placed)| (thing.clone(), moved(placed, (dx + snapped.dx, dy + snapped.dy)))).collect();
        doc.preview_places(&places);
        let layers = doc.editor_layers(scale);
        let layout = doc.text_layout(slide);
        more(doc);
        times.push(start.elapsed());
        assert!(layers.len() > count, "frame {frame}: the heading and the {count}");
        assert_eq!(layout.elements().len(), count);
        snapped_frames += usize::from(!snapped.guides.is_empty());
        held += usize::from(last.as_ref() == Some(&places));
        last = Some(places);
    }
    doc.drop_preview();
    assert_eq!(doc.source(), talk, "nothing written");
    times.sort();
    (times[times.len() / 2], *times.last().unwrap(), snapped_frames, held)
}

/// Where a drag of sixty has them at `frame`: each frame at another place, so that each is
/// shown afresh, as a place shown already isn't.
fn here_and_there(frame: usize) -> (f32, f32) {
    ((frame as f32 * 7.3) % 300., (frame as f32 * 3.1) % 150.)
}

/// Dragging sixty things, snapping to sixty others out of reach, so only now and then to the
/// slide: every line is looked at all the same.
#[test]
#[ignore = "timing; run in release"]
fn a_frame_of_a_drag_on_a_slide_of_sixty_things_is_quick() {
    let away = |starts: &[(Thing, Placed)]| starts.iter().map(|(_, placed)| Rect { x: placed.rect.x + 5000., y: placed.rect.y + 5000., ..bounds(placed) }).collect();
    let (median, slowest, snapped, held) = drag_frames(away, here_and_there);
    println!("median drag frame of sixty things on a {SLIDES}-slide talk: {median:?} (slowest {slowest:?}), {snapped} of 200 snapped, {held} held");
    assert!(snapped <= 100 && held <= 10, "{snapped} snapped, {held} held");
    assert!(median < MOST_FOR_A_DRAG_FRAME, "a drag frame took {median:?}");
}

/// Dragging sixty things, snapping to sixty others scattered over the slide, in reach of most
/// frames, each frame at another place, so that a snap and its guides are worked out each time.
#[test]
#[ignore = "timing; run in release"]
fn a_frame_of_a_snapping_drag_on_a_slide_of_sixty_things_is_quick() {
    let scattered = |_: &[(Thing, Placed)]| {
        let mut seed = 12345u32;
        let mut next = move |most: f32| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (seed >> 8) as f32 / (1u32 << 24) as f32 * most
        };
        (0..60).map(|_| Rect { x: next(1800.), y: next(1000.), w: 20. + next(200.), h: 20. + next(150.) }).collect()
    };
    let (median, slowest, snapped, held) = drag_frames(scattered, here_and_there);
    println!("median snapping drag frame of sixty things on a {SLIDES}-slide talk: {median:?} (slowest {slowest:?}), {snapped} of 200 snapped, {held} held");
    assert!(snapped >= 150, "most frames snap: {snapped}");
    assert!(held <= 10, "most frames show a place of their own: {held} held");
    assert!(median < MOST_FOR_A_DRAG_FRAME, "a drag frame took {median:?}");
}

/// Dragging sixty things, each animated, on the Animate tab, beside a list shown an item at a
/// time: each frame the slide's badges and the tab's tools are worked out again, as the window
/// does before it draws.
#[test]
#[ignore = "timing; run in release"]
fn a_frame_of_a_drag_on_the_animate_tab_is_quick() {
    let talk = sixty_things()
        .replacen("---\n\n# Sixty\n\n", "---\nregion.body: click=each\n---\n\n# Sixty\n\n- one\n- two\n- three\n\n", 1)
        .replace("h=\"80\" />", "h=\"80\" click=\"2\" enter=\"fade\" />");
    let away = |starts: &[(Thing, Placed)]| starts.iter().map(|(_, placed)| Rect { x: placed.rect.x + 5000., y: placed.rect.y + 5000., ..bounds(placed) }).collect();
    let badges = |doc: &mut Document| {
        assert_eq!(doc.step_badges().len(), 61);
        assert_eq!(doc.animate_here().targets.len(), 60);
    };
    let (median, slowest, snapped, held) = drag_frames_on(talk, away, here_and_there, badges);
    println!("median drag frame of sixty things on the Animate tab on a {SLIDES}-slide talk: {median:?} (slowest {slowest:?}), {snapped} of 200 snapped, {held} held");
    assert!(median < MOST_FOR_A_DRAG_FRAME, "a drag frame took {median:?}");
}

/// A letter typed in a list shown an item at a time, on the Animate tab: the slide's badges and
/// the tab's tools are worked out again after it, as the window does before it draws.
#[test]
#[ignore = "timing; run in release"]
fn a_keystroke_on_the_animate_tab_is_quick() {
    let slide = SLIDES / 2;
    let talk = long_talk().replacen(&format!("---\n\n# Slide {slide}\n"), &format!("---\nregion.body: click=each\n---\n\n# Slide {slide}\n"), 1);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), talk).unwrap();
    let mut doc = Document::open(dir.path(), Fonts::bundled_only()).unwrap();
    doc.go_to(slide);
    let flows = text_flows(doc.talk(), slide);
    let body = flows.iter().position(|flow| flow.place == FlowPlace::Region("body".into())).expect("the slide has words");
    let words: usize = flows[body].paras[0].spans.iter().map(|span| span.text.len()).sum();
    assert!(doc.begin_text(slide, body, Pos { para: 0, offset: words }));
    doc.text_layout(slide);
    let mut times = Vec::with_capacity(KEYSTROKES);
    for key in 0..KEYSTROKES {
        let letter = if key % 6 == 5 { " " } else { "a" };
        let start = Instant::now();
        doc.apply_text(|edit| edit.insert(letter)).unwrap();
        doc.text_layout(slide);
        let badges = doc.step_badges();
        let here = doc.animate_here();
        times.push(start.elapsed());
        assert_eq!(badges.len(), 1);
        assert_eq!(here.each, Some(true));
    }
    report("keystroke typed on the Animate tab", times);
}

/// Every slide of a long talk given a transition, as one step: a step taken once, by a tool.
#[test]
#[ignore = "timing; run in release"]
fn apply_to_all_slides_on_a_long_talk_is_quick() {
    let talk = long_talk();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), &talk).unwrap();
    let mut doc = Document::open(dir.path(), Fonts::bundled_only()).unwrap();
    let mut times = Vec::new();
    for _ in 0..10 {
        let start = Instant::now();
        doc.set_slide_transition(slides_engine::Transition::Fade, true).unwrap();
        times.push(start.elapsed());
        assert_eq!(doc.timeline_of(SLIDES - 1).transition, slides_engine::Transition::Fade);
        doc.undo().unwrap();
        assert_eq!(doc.source(), talk, "one step");
    }
    times.sort();
    let median = times[times.len() / 2];
    println!("median Apply to all slides on a {SLIDES}-slide talk: {median:?} (slowest {:?})", times.last().unwrap());
    assert!(median < MOST_FOR_SIXTY, "Apply to all slides took {median:?}");
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

/// How many slides, each holding a video, the talk of videos has.
const VIDEO_SLIDES: usize = 20;

/// A talk of [`VIDEO_SLIDES`] slides, each a heading, some words and a list, with a video beside
/// them when `with_videos`.
fn video_talk(with_videos: bool) -> String {
    let mut talk = String::from("---\ntitle: A talk of videos\n---\n\n");
    for n in 0..VIDEO_SLIDES {
        if n > 0 {
            talk.push_str("---\n\n");
        }
        talk.push_str(&format!("# Slide {n}\n\nSome **bold** words about clip {n}.\n\n- the first thing\n- the second thing\n\n"));
        if with_videos {
            talk.push_str(&format!("<Video src=\"assets/clip{n}.mp4\" x=\"1000\" y=\"300\" w=\"800\" h=\"450\" />\n\n"));
        }
    }
    talk
}

/// A folder holding [`video_talk`], and with videos, its twenty videos: each 1280 by 720, a
/// second long, its pictures one colour of its own. Made pictures of one colour each are far
/// easier to decode than filmed ones.
fn video_talk_folder(with_videos: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), video_talk(with_videos)).unwrap();
    if with_videos {
        std::fs::create_dir(dir.path().join("assets")).unwrap();
        for n in 0..VIDEO_SLIDES {
            let colour = [(n * 12) as u8, 200 - (n * 9) as u8, 40 + (n * 7) as u8, 255];
            let clip = slides_video::make_test_video(1280, 720, 30, 30, |_| colour.repeat(1280 * 720));
            std::fs::write(dir.path().join(format!("assets/clip{n}.mp4")), clip).unwrap();
        }
    }
    dir
}

/// Makes every still wanted, one after another, as the window's background tasks do, and says
/// how many were made.
fn make_stills_now(doc: &mut Document) -> usize {
    let mut made = 0;
    loop {
        let jobs = doc.still_jobs();
        if jobs.is_empty() {
            return made;
        }
        for (job, open) in jobs {
            let Ok(reader) = open.expect("the video's file").open_for_still() else { panic!("the video doesn't open") };
            let stagepack_studio::video_player::Making::Made(still) = stagepack_studio::video_player::make_still_from(job, reader) else {
                panic!("a still waits");
            };
            assert!(!still.failed(), "a still couldn't be made");
            doc.take_still(still);
            made += 1;
        }
    }
}

/// The talk of videos opened as the window opens it, its stills made, and shown at slide `slide`.
fn open_video_talk(dir: &std::path::Path, slide: usize) -> Document {
    let mut doc = Document::open(dir, Fonts::bundled_only()).unwrap();
    doc.make_stills_aside(std::sync::Arc::new(|| {}));
    for index in 0..doc.slide_count() {
        doc.thumbnail(index, 192);
    }
    doc.go_to(slide);
    doc.editor_layers(1280. / 1920.);
    assert_eq!(make_stills_now(&mut doc), VIDEO_SLIDES, "a still for each video");
    doc
}

/// A letter typed on a slide of a talk of twenty videos, none playing, beside its video.
#[test]
#[ignore = "timing; run in release"]
fn a_keystroke_on_a_talk_of_twenty_videos_is_quick() {
    let dir = video_talk_folder(true);
    let slide = VIDEO_SLIDES / 2;
    let mut doc = open_video_talk(dir.path(), slide);
    let flows = text_flows(doc.talk(), slide);
    let body = flows.iter().position(|flow| flow.place == FlowPlace::Region("body".into())).expect("the slide has words");
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
        doc.editor_layers(1280. / 1920.);
        times.push(start.elapsed());
    }
    assert!(doc.still_jobs().is_empty(), "typing beside a video doesn't make its still again");
    times.sort();
    let median = times[times.len() / 2];
    println!("median keystroke on a talk of {VIDEO_SLIDES} videos: {median:?} (slowest {:?})", times.last().unwrap());
    assert!(median < MOST, "a keystroke took {median:?}");
}

/// A video dragged about its slide, on a talk of twenty videos, none playing.
#[test]
#[ignore = "timing; run in release"]
fn a_frame_of_a_drag_of_a_video_is_quick() {
    let dir = video_talk_folder(true);
    let slide = VIDEO_SLIDES / 2;
    let mut doc = open_video_talk(dir.path(), slide);
    let talk = doc.source().to_string();
    let away = |starts: &[(Thing, Placed)]| starts.iter().map(|(_, placed)| Rect { x: placed.rect.x + 5000., y: placed.rect.y + 5000., ..bounds(placed) }).collect();
    let (median, slowest, _, _) = drag_frames_of(&mut doc, &talk, slide, 1, away, here_and_there, |doc| {
        assert!(doc.still_jobs().is_empty(), "a video moved keeps its still");
    });
    println!("median drag frame of a video on a talk of {VIDEO_SLIDES} videos: {median:?} (slowest {slowest:?})");
    assert!(median < MOST_FOR_A_DRAG_FRAME, "a drag frame took {median:?}");
}

/// Opening a talk of twenty videos, as the window does: the talk read, every slide drawn small
/// for the list of slides and the first drawn for the editor, a video shown as a plain panel
/// until its still is made aside. That takes under half a second more than the same talk without
/// its videos. Making the twenty stills after, one after another, is timed too.
#[test]
#[ignore = "timing; run in release"]
fn opening_a_talk_of_twenty_videos_is_quick() {
    let (plain, videos) = (video_talk_folder(false), video_talk_folder(true));
    let open = |dir: &std::path::Path| {
        let start = Instant::now();
        let mut doc = Document::open(dir, Fonts::bundled_only()).unwrap();
        doc.make_stills_aside(std::sync::Arc::new(|| {}));
        for index in 0..doc.slide_count() {
            assert!(doc.thumbnail(index, 192).is_some());
        }
        doc.editor_layers(1280. / 1920.);
        (start.elapsed(), doc)
    };
    let (mut without, mut with, mut stills) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..5 {
        without.push(open(plain.path()).0);
        let (took, mut doc) = open(videos.path());
        with.push(took);
        let start = Instant::now();
        assert_eq!(make_stills_now(&mut doc), VIDEO_SLIDES);
        for index in 0..doc.slide_count() {
            assert!(doc.thumbnail(index, 192).is_some());
        }
        stills.push(start.elapsed());
    }
    for times in [&mut without, &mut with, &mut stills] {
        times.sort();
    }
    let (without, with, stills) = (without[2], with[2], stills[2]);
    println!(
        "opening a talk of {VIDEO_SLIDES} slides: {without:?} without videos, {with:?} with one on each; \
         its {VIDEO_SLIDES} stills made after in {stills:?} (made videos, each picture one colour)"
    );
    assert!(with < without + Duration::from_millis(500), "opening took {with:?}, against {without:?} without videos");
}

/// A video 1920 by 1080, thirty pictures a second, played in the editor for 300 pictures, each
/// waited for and shown in turn: how long the window's thread takes to take each picture from
/// the decoding and make it the window's image, and how long the whole frame drawn with it takes.
/// The window here draws nothing to a screen, so painting the picture isn't in it. The made
/// video's pictures are one colour each, far easier to decode than filmed ones, which only the
/// decoding's own thread feels.
#[gpui_kit::test]
#[ignore = "timing; run in release"]
fn a_1080p_video_plays_to_the_canvas_quickly(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::AppContext as _;
    use gpui_kit::test::TestWindowExt as _;
    use stagepack_studio::slide_editor::VIDEO_PLAY_ID;
    const FRAMES: u32 = 300;
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("slides.md"), "# Played\n\n<Video src=\"assets/clip.mp4\" x=\"0\" y=\"0\" w=\"1920\" h=\"1080\" />\n").unwrap();
    std::fs::create_dir(folder.path().join("assets")).unwrap();
    let clip = slides_video::make_test_video(1920, 1080, FRAMES, 30, |at| [(at % 256) as u8, (255 - at % 256) as u8, 128, 255].repeat(1920 * 1080));
    std::fs::write(folder.path().join("assets/clip.mp4"), clip).unwrap();
    let typing = typing::open_folder(cx, folder, None);
    let handle = typing.handle;
    let draw = |cx: &mut gpui_kit::TestAppContext| {
        cx.run_until_parked();
        let start = Instant::now();
        cx.update_window(handle, |_, window, cx| {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
        })
        .unwrap();
        start.elapsed()
    };
    let until = |cx: &mut gpui_kit::TestAppContext, what: &str, done: &dyn Fn(&mut gpui_kit::TestAppContext) -> bool| {
        let began = Instant::now();
        while !done(cx) {
            draw(cx);
            assert!(began.elapsed() < Duration::from_secs(30), "waited too long for {what}");
            std::thread::sleep(Duration::from_millis(1));
        }
    };
    let document = typing.document(cx);
    until(cx, "the still", &|cx| document.read_with(cx, |document, _| !document.stills_coming()));
    typing.settle(cx);
    let video = typing::thing(&typing, cx, "<Video");
    let at = typing.at(cx, video.rect.x + video.rect.w / 5., video.rect.y + video.rect.h / 5.);
    typing.click(cx, at);
    typing::click_on(&typing, cx, VIDEO_PLAY_ID);
    let canvas = cx.read(|cx| typing.view.read(cx).slide_canvas().clone());
    let made = |cx: &gpui_kit::TestAppContext| cx.read(|cx| canvas.read(cx).video_images().0);
    until(cx, "the first picture", &|cx| made(cx) >= 1);
    let (mut taking, mut frames) = (Vec::new(), Vec::new());
    let mut shown = made(cx);
    while shown < FRAMES as usize {
        // The next picture decoded, or the video done.
        until(cx, "a picture decoded", &|cx| cx.read(|cx| canvas.read(cx).videos_queued().iter().all(|queued| *queued >= 1) || canvas.read(cx).playing_videos().is_empty()));
        if cx.read(|cx| canvas.read(cx).playing_videos().is_empty()) {
            break;
        }
        // A thirtieth of a second, a hair over, so that each picture is due in turn and none is skipped.
        cx.executor().advance_clock(Duration::from_micros(33_334));
        let took = draw(cx);
        let now = made(cx);
        if now > shown {
            taking.push(cx.read(|cx| canvas.read(cx).last_video_advance()));
            frames.push(took);
        }
        shown = now;
    }
    let (made, released) = cx.read(|cx| canvas.read(cx).video_images());
    assert!(made >= FRAMES as usize - 1, "{made} of {FRAMES} pictures shown");
    assert!(released + 1 >= made, "{made} made and {released} released");
    taking.sort();
    frames.sort();
    let at = |times: &[Duration], part: f64| times[((times.len() - 1) as f64 * part).round() as usize];
    println!(
        "a 1920 by 1080 video played for {} pictures: taking each picture to the window, median {:?}, 99th percentile {:?}; \
         the whole frame drawn with it, median {:?}, 99th percentile {:?} (made pictures, one colour each)",
        taking.len(),
        at(&taking, 0.5),
        at(&taking, 0.99),
        at(&frames, 0.5),
        at(&frames, 0.99)
    );
    assert!(at(&taking, 0.5) < MOST_FOR_A_DRAG_FRAME, "taking a picture took {:?}", at(&taking, 0.5));
}

/// How many slides, each holding a model, the talk of models has.
const MODEL_SLIDES: usize = 10;

/// Model tests one at a time, as they share the app's one drawer.
static MODELS_ALONE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A test of models running alone, the app's drawer closed as it ends, as a program that ends
/// while a driver lets go of a device can crash in the driver.
struct ModelsAlone(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

impl Drop for ModelsAlone {
    fn drop(&mut self) {
        let closed = stagepack_studio::model_view::close_drawer(Duration::from_secs(30));
        if !std::thread::panicking() {
            assert!(closed, "the drawer's thread never ended");
        }
    }
}

/// Runs alone with the app's drawer on the adapter `ADAPTER` names, or the best there is with
/// none, and says which; or `None`, after saying why `test` is skipped. An `ADAPTER` it doesn't
/// know stops the test, saying so.
fn models_alone(test: &str) -> Option<ModelsAlone> {
    use slides_model::{AdapterKind, Drawer, DrawerOptions};
    let alone = ModelsAlone(MODELS_ALONE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
    let only = match std::env::var("ADAPTER") {
        Err(std::env::VarError::NotPresent) => None,
        Ok(kind) if kind.is_empty() => None,
        Ok(kind) if kind == "discrete" => Some(AdapterKind::Discrete),
        Ok(kind) if kind == "integrated" => Some(AdapterKind::Integrated),
        Ok(kind) if kind == "software" => Some(AdapterKind::Software),
        // Not the best there is in its place, which would time another card under its name.
        other => panic!("ADAPTER is {other:?}: it can be discrete, integrated or software, or left unset for the best there is"),
    };
    let drawer = Drawer::start_with(DrawerOptions { only, ..DrawerOptions::default() });
    let began = Instant::now();
    let ready = loop {
        if let Some(ready) = drawer.ready() {
            break ready;
        }
        assert!(began.elapsed() < Duration::from_secs(60), "the drawer never said whether it can draw");
        std::thread::sleep(Duration::from_millis(5));
    };
    if let Some(old) = stagepack_studio::model_view::use_drawer(drawer) {
        assert!(old.join().unwrap(), "the drawer before let go of its device");
    }
    match ready {
        Ok(kind) => {
            println!("{test}: models drawn on a {kind:?} adapter");
            Some(alone)
        }
        Err(why) => {
            println!("{test} skipped: {why}");
            None
        }
    }
}

/// The model on slide `n` of the talk of models: a textured grid of 20,000 triangles on even
/// slides, and a box of three materials on odd ones.
fn made_model(n: usize) -> Vec<u8> {
    if n.is_multiple_of(2) {
        slides_model::make_test_glb(slides_model::MadeGlb::Grid { cells: 100, texture: 512, jpeg: n.is_multiple_of(4) })
    } else {
        slides_model::make_test_glb(slides_model::MadeGlb::Boxes)
    }
}

/// A talk of [`MODEL_SLIDES`] slides, each a heading, some words and a list, with a model beside
/// them when `with_models`.
fn model_talk(with_models: bool) -> String {
    let mut talk = String::from("---\ntitle: A talk of models\n---\n\n");
    for n in 0..MODEL_SLIDES {
        if n > 0 {
            talk.push_str("---\n\n");
        }
        talk.push_str(&format!("# Slide {n}\n\nSome **bold** words about model {n}.\n\n- the first thing\n- the second thing\n\n"));
        if with_models {
            talk.push_str(&format!("<Model src=\"assets/model{n}.glb\" x=\"1000\" y=\"300\" w=\"800\" h=\"450\" view=\"30 20 4 0 0.5 0\" />\n\n"));
        }
    }
    talk
}

/// A folder holding [`model_talk`], and with models, its ten models.
fn model_talk_folder(with_models: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), model_talk(with_models)).unwrap();
    if with_models {
        std::fs::create_dir(dir.path().join("assets")).unwrap();
        for n in 0..MODEL_SLIDES {
            std::fs::write(dir.path().join(format!("assets/model{n}.glb")), made_model(n)).unwrap();
        }
    }
    dir
}

/// The talk of models opened as the window opens it, its stills drawn, and shown at slide `slide`.
fn open_model_talk(dir: &std::path::Path, slide: usize) -> Document {
    let mut doc = Document::open(dir, Fonts::bundled_only()).unwrap();
    doc.make_stills_aside(std::sync::Arc::new(|| {}));
    for index in 0..doc.slide_count() {
        doc.thumbnail(index, 192);
    }
    doc.go_to(slide);
    doc.editor_layers(1280. / 1920.);
    assert_eq!(make_stills_now(&mut doc), MODEL_SLIDES, "a still for each model");
    doc
}

/// A letter typed on a slide of a talk of ten models, none turned, beside its model.
#[test]
#[ignore = "timing; run in release"]
fn a_keystroke_on_a_talk_of_ten_models_is_quick() {
    let Some(_alone) = models_alone("a_keystroke_on_a_talk_of_ten_models_is_quick") else { return };
    let dir = model_talk_folder(true);
    let slide = MODEL_SLIDES / 2;
    let mut doc = open_model_talk(dir.path(), slide);
    let asked = stagepack_studio::model_view::draws_asked();
    let flows = text_flows(doc.talk(), slide);
    let body = flows.iter().position(|flow| flow.place == FlowPlace::Region("body".into())).expect("the slide has words");
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
        doc.editor_layers(1280. / 1920.);
        times.push(start.elapsed());
    }
    assert!(doc.still_jobs().is_empty(), "typing beside a model doesn't draw its still again");
    assert_eq!(stagepack_studio::model_view::draws_asked(), asked, "nothing is drawn while typing");
    times.sort();
    let median = times[times.len() / 2];
    println!("median keystroke on a talk of {MODEL_SLIDES} models: {median:?} (slowest {:?})", times.last().unwrap());
    assert!(median < MOST, "a keystroke took {median:?}");
}

/// A model dragged about its slide, on a talk of ten models, none turned.
#[test]
#[ignore = "timing; run in release"]
fn a_frame_of_a_drag_of_a_model_is_quick() {
    let Some(_alone) = models_alone("a_frame_of_a_drag_of_a_model_is_quick") else { return };
    let dir = model_talk_folder(true);
    let slide = MODEL_SLIDES / 2;
    let mut doc = open_model_talk(dir.path(), slide);
    let talk = doc.source().to_string();
    let asked = stagepack_studio::model_view::draws_asked();
    let away = |starts: &[(Thing, Placed)]| starts.iter().map(|(_, placed)| Rect { x: placed.rect.x + 5000., y: placed.rect.y + 5000., ..bounds(placed) }).collect();
    let (median, slowest, _, _) = drag_frames_of(&mut doc, &talk, slide, 1, away, here_and_there, |doc| {
        assert!(doc.still_jobs().is_empty(), "a model moved keeps its still");
    });
    assert_eq!(stagepack_studio::model_view::draws_asked(), asked, "nothing is drawn while it's dragged");
    println!("median drag frame of a model on a talk of {MODEL_SLIDES} models: {median:?} (slowest {slowest:?})");
    assert!(median < MOST_FOR_A_DRAG_FRAME, "a drag frame took {median:?}");
}

/// Opening a talk of ten models, as the window does: the talk read, every slide drawn small for
/// the list of slides and the first drawn for the editor, a model shown as a plain panel until
/// its still is drawn aside. That takes under half a second more than the same talk without its
/// models, and draws nothing. Drawing the ten stills after, one after another, is timed too.
#[test]
#[ignore = "timing; run in release"]
fn opening_a_talk_of_ten_models_is_quick() {
    let Some(_alone) = models_alone("opening_a_talk_of_ten_models_is_quick") else { return };
    let (plain, models) = (model_talk_folder(false), model_talk_folder(true));
    let open = |dir: &std::path::Path| {
        let start = Instant::now();
        let mut doc = Document::open(dir, Fonts::bundled_only()).unwrap();
        doc.make_stills_aside(std::sync::Arc::new(|| {}));
        for index in 0..doc.slide_count() {
            assert!(doc.thumbnail(index, 192).is_some());
        }
        doc.editor_layers(1280. / 1920.);
        (start.elapsed(), doc)
    };
    let (mut without, mut with, mut stills) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..5 {
        without.push(open(plain.path()).0);
        let asked = stagepack_studio::model_view::draws_asked();
        let (took, mut doc) = open(models.path());
        assert_eq!(stagepack_studio::model_view::draws_asked(), asked, "opening draws nothing");
        with.push(took);
        let start = Instant::now();
        assert_eq!(make_stills_now(&mut doc), MODEL_SLIDES);
        for index in 0..doc.slide_count() {
            assert!(doc.thumbnail(index, 192).is_some());
        }
        stills.push(start.elapsed());
    }
    for times in [&mut without, &mut with, &mut stills] {
        times.sort();
    }
    let (without, with, stills) = (without[2], with[2], stills[2]);
    println!(
        "opening a talk of {MODEL_SLIDES} slides: {without:?} without models, {with:?} with one on each; \
         its {MODEL_SLIDES} stills drawn after in {stills:?}"
    );
    assert!(with < without + Duration::from_millis(500), "opening took {with:?}, against {without:?} without models");
}

/// A textured grid of a million triangles, turning on its own in the show, for 300 of its
/// pictures, each waited for and shown in turn: how long each frame the window draws takes its
/// thread, the view moved on, the newest picture taken in and the next asked for. The drawing and
/// the read-back are on the drawer's thread, and making the picture the window's image on a
/// background task. The window here draws nothing to a screen, so painting the picture isn't in
/// it.
#[gpui_kit::test]
#[ignore = "timing; run in release"]
fn a_model_turning_for_300_pictures_leaves_the_window_free(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::AppContext as _;
    use gpui_kit::test::TestWindowExt as _;
    use stagepack_studio::present_view::start_show;
    const PICTURES: usize = 300;
    let Some(_alone) = models_alone("a_model_turning_for_300_pictures_leaves_the_window_free") else { return };
    cx.update(|cx| {
        gpui_kit::init(cx);
        stagepack_studio::workspace_view::bind_keys(cx);
    });
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(
        folder.path().join("slides.md"),
        "# Turned\n\n<Model src=\"assets/terrain.glb\" x=\"0\" y=\"0\" w=\"1920\" h=\"1080\" turn=\"auto\" view=\"0 30 3.5 0 0 0\" />\n",
    )
    .unwrap();
    std::fs::create_dir(folder.path().join("assets")).unwrap();
    let terrain = slides_model::make_test_glb(slides_model::MadeGlb::Grid { cells: 707, texture: 2048, jpeg: true });
    std::fs::write(folder.path().join("assets/terrain.glb"), terrain).unwrap();
    let document = cx.new(|_| Document::open(folder.path(), Fonts::bundled_only()).unwrap());
    let show = cx.update(|cx| start_show(document.clone(), false, cx));
    let (audience, view) = cx.read(|cx| show.read(cx).audience()).expect("the audience's window");
    let draw = |cx: &mut gpui_kit::TestAppContext| {
        cx.run_until_parked();
        let start = Instant::now();
        cx.update_window(audience, |_, window, cx| {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
        })
        .unwrap();
        let took = start.elapsed();
        cx.run_until_parked();
        took
    };
    let canvas_now = |cx: &gpui_kit::TestAppContext| cx.read(|cx| view.read(cx).current().clone());
    let began = Instant::now();
    while canvas_now(cx).read_with(cx, |canvas, _| canvas.model_images().0) == 0 {
        draw(cx);
        assert!(began.elapsed() < Duration::from_secs(60), "the first picture never came");
        std::thread::sleep(Duration::from_millis(1));
    }
    let canvas = canvas_now(cx);
    let made = |cx: &gpui_kit::TestAppContext| canvas.read_with(cx, |canvas, _| canvas.model_images().0);
    // Every frame drawn, and those that showed a new picture.
    let (mut frames, mut showing) = (Vec::new(), Vec::new());
    let began = Instant::now();
    let first = made(cx);
    while made(cx) < first + PICTURES {
        // A sixtieth of a second on: the view moves on, and the next picture is asked for.
        cx.executor().advance_clock(Duration::from_micros(16_667));
        let before = made(cx);
        loop {
            let took = draw(cx);
            frames.push(took);
            if made(cx) > before {
                showing.push(took);
                break;
            }
            assert!(began.elapsed() < Duration::from_secs(300), "waited too long for a picture");
            std::thread::sleep(Duration::from_micros(500));
        }
    }
    let took = began.elapsed();
    let pictures = made(cx) - first;
    let (made, released) = canvas.read_with(cx, |canvas, _| canvas.model_images());
    assert!(released + 1 >= made, "{made} made and {released} released");
    frames.sort();
    showing.sort();
    let at = |times: &[Duration], part: f64| times[((times.len() - 1) as f64 * part).round() as usize];
    println!(
        "a million-triangle model turning for {pictures} pictures in {took:?}: of the {} frames the window drew, those showing a \
         new picture took its thread a median {:?}, 99th percentile {:?}; every frame, median {:?}, 99th percentile {:?}, slowest {:?}",
        frames.len(),
        at(&showing, 0.5),
        at(&showing, 0.99),
        at(&frames, 0.5),
        at(&frames, 0.99),
        frames.last().unwrap()
    );
    assert!(at(&showing, 0.5) < MOST_FOR_A_DRAG_FRAME, "a frame took {:?}", at(&showing, 0.5));
    cx.update_window(audience, |_, window, _| window.remove_window()).ok();
    cx.run_until_parked();
}
