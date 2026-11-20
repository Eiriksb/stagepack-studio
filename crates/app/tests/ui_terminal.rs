//! A terminal recording playing on a slide: in the show from when its slide shows, stopping at its
//! marks until the next click and going back to them, in the editor when asked, and every way it
//! stops, with its thread and its pictures let go of.
//!
//! Each test makes its own recordings. They run one at a time, as they count the replays' threads
//! and set what slows or quickens them, which another test's would see.

#[path = "support/typing.rs"]
mod typing;

use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use typing::*;

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, RenderImage, TestAppContext, VisualTestContext, px, size};
use slides_engine::{Fonts, Moment};
use stagepack_studio::document::Document;
use stagepack_studio::present_view::{AudienceView, Presentation, PresenterView, start_show};
use stagepack_studio::replay;
use stagepack_studio::slide_canvas::{RecordingShows, SlideCanvas};
use stagepack_studio::slide_editor::{TERMINAL_PLAY_ID, TERMINAL_STOP_ID};
use stagepack_studio::video_player::{Making, make_one};
use stagepack_studio::workspace_view;

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// Held by each test: the replays' counts and test settings are the process's.
struct Alone(#[allow(dead_code)] MutexGuard<'static, ()>);

impl Drop for Alone {
    fn drop(&mut self) {
        replay::play_faster_for_tests(1);
        replay::slow_for_tests(Duration::ZERO);
    }
}

fn alone() -> Alone {
    Alone(ONE_AT_A_TIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner()))
}

const TERMINAL: &str = "<Terminal src=\"assets/run.cast\" x=\"100\" y=\"100\" w=\"640\" h=\"360\" />";
const BOX: &str = "<Box x=\"1000\" y=\"600\" w=\"300\" h=\"200\" fill=\"#888888\" />";

/// Where the short recording stops, and where it ends, in seconds.
const STOPS: [f64; 2] = [0.1, 0.25];
const END: f64 = 0.35;

/// A short recording that stops twice: three commands, a mark after each of the first two.
fn marked() -> Vec<u8> {
    slides_terminal::make_test_cast(
        40,
        6,
        &[
            (0.0, "o", "$ one\r\n"),
            (0.05, "o", "first\r\n"),
            (0.1, "m", ""),
            (0.15, "o", "$ two\r\n"),
            (0.2, "o", "second\r\n"),
            (0.25, "m", ""),
            (0.3, "o", "$ three\r\n"),
            (0.35, "o", "third\r\n"),
        ],
    )
}

/// A recording that goes on writing for `seconds`, a line every 50 ms, with no marks.
fn running(seconds: u32) -> Vec<u8> {
    let lines: Vec<(f64, &str, String)> = (0..seconds * 20).map(|at| (f64::from(at) * 0.05, "o", format!("line {at}\r\n"))).collect();
    let lines: Vec<(f64, &str, &str)> = lines.iter().map(|(at, code, text)| (*at, *code, text.as_str())).collect();
    slides_terminal::make_test_cast(40, 6, &lines)
}

/// One slide with the recording and a box on it, then another.
fn talk() -> String {
    format!("# Run\n\n{TERMINAL}\n\n{BOX}\n\n---\n\n# Two\n")
}

/// A folder holding a talk of `source`, and `cast` as its `assets/run.cast`.
fn talk_folder(source: &str, cast: &[u8]) -> tempfile::TempDir {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("slides.md"), source).unwrap();
    std::fs::create_dir(folder.path().join("assets")).unwrap();
    std::fs::write(folder.path().join("assets/run.cast"), cast).unwrap();
    folder
}

/// Waits, in real time, for `done`, delivering the frames the window asks for and drawing it
/// meanwhile, as a replay runs on a thread of its own.
fn until(cx: &mut TestAppContext, window: AnyWindowHandle, what: &str, mut done: impl FnMut(&mut TestAppContext) -> bool) {
    let began = Instant::now();
    loop {
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
        })
        .unwrap();
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        assert!(began.elapsed() < Duration::from_secs(60), "waited too long for {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Draws `window` for `millis` of real time, as it would be drawn meanwhile.
fn for_a_while(cx: &mut TestAppContext, window: AnyWindowHandle, millis: u64) {
    let began = Instant::now();
    until(cx, window, "a while", |_| began.elapsed() >= Duration::from_millis(millis));
}

/// The editor on a talk of `source` with `cast`, its recordings' stills made.
fn editor(cx: &mut TestAppContext, source: &str, cast: &[u8]) -> Typing {
    let typing = open_folder(cx, talk_folder(source, cast), None);
    stills_made(&typing, cx);
    typing
}

/// Waits for every still to be made, off the window's thread, and taken in.
fn stills_made(typing: &Typing, cx: &mut TestAppContext) {
    let document = typing.document(cx);
    until(cx, typing.handle, "the stills", |cx| document.read_with(cx, |document, _| !document.stills_coming()));
    typing.settle(cx);
}

fn editor_canvas(typing: &Typing, cx: &TestAppContext) -> Entity<SlideCanvas> {
    cx.read(|cx| typing.view.read(cx).slide_canvas().clone())
}

/// When in its recording the picture each replay on `canvas` shows is from, in seconds.
fn times(cx: &TestAppContext, canvas: &Entity<SlideCanvas>) -> Vec<Option<f64>> {
    cx.read(|cx| canvas.read(cx).replay_times())
}

fn pictures(cx: &TestAppContext, canvas: &Entity<SlideCanvas>) -> Vec<Option<std::sync::Arc<RenderImage>>> {
    cx.read(|cx| canvas.read(cx).replay_pictures())
}

/// Whether a replay on `canvas` has pictures still to come.
fn moving(cx: &TestAppContext, canvas: &Entity<SlideCanvas>) -> bool {
    cx.read(|cx| canvas.read(cx).replays_moving())
}

fn made_and_released(cx: &TestAppContext, canvas: &Entity<SlideCanvas>) -> (usize, usize) {
    cx.read(|cx| canvas.read(cx).replay_images())
}

/// Whether `time`, a picture's, is `seconds` into its recording.
fn is_at(time: Option<f64>, seconds: f64) -> bool {
    time.is_some_and(|time| (time - seconds).abs() < 1e-6)
}

/// Waits until the replay on `canvas` holds still, at `seconds` into its recording.
fn holds_at(cx: &mut TestAppContext, window: AnyWindowHandle, canvas: &Entity<SlideCanvas>, seconds: f64) {
    until(cx, window, &format!("the screen at {seconds} s"), |cx| {
        let times = times(cx, canvas);
        times.len() == 1 && is_at(times[0], seconds) && !moving(cx, canvas)
    });
}

/// Waits, in real time, for every replay's thread to end, and says how long that took.
fn threads_end(cx: &mut TestAppContext, window: Option<AnyWindowHandle>, how: &str) -> Duration {
    let began = Instant::now();
    loop {
        cx.run_until_parked();
        if let Some(window) = window {
            cx.update_window(window, |_, window, cx| window.render_frame(cx)).ok();
        }
        if replay::threads_running() == 0 {
            return began.elapsed();
        }
        assert!(began.elapsed() < Duration::from_secs(10), "{how}: a replay's thread didn't end");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Checks that no screen is drawn for a while once nothing plays.
fn nothing_drawn(cx: &mut TestAppContext, window: AnyWindowHandle, how: &str) {
    let drawn = replay::screens_drawn();
    for_a_while(cx, window, 100);
    assert_eq!(replay::screens_drawn(), drawn, "{how}: nothing is drawn for a slide not shown");
}

/// Selects the recording, by a click a little way in from its corner, away from its button.
fn select_terminal(typing: &Typing, cx: &mut TestAppContext) {
    let terminal = thing(typing, cx, "<Terminal");
    let at = typing.at(cx, terminal.rect.x + terminal.rect.w / 5., terminal.rect.y + terminal.rect.h / 5.);
    typing.click(cx, at);
    assert_eq!(selected(typing, cx).map(|(_, source)| source), Some(terminal.source));
}

/// Selects the recording and plays it, and waits for its first picture.
fn play_selected(typing: &Typing, cx: &mut TestAppContext) -> Entity<SlideCanvas> {
    select_terminal(typing, cx);
    click_on(typing, cx, TERMINAL_PLAY_ID);
    let canvas = editor_canvas(typing, cx);
    until(cx, typing.handle, "the first picture", |cx| pictures(cx, &canvas).first().is_some_and(Option::is_some));
    canvas
}

/// Closes every window but `typing`'s.
fn close_others(typing: &Typing, cx: &mut TestAppContext) {
    let others: Vec<AnyWindowHandle> =
        cx.read(|cx| cx.windows()).into_iter().filter(|window| window.window_id() != typing.handle.window_id()).collect();
    for window in others {
        cx.update_window(window, |_, window, _| window.remove_window()).ok();
    }
    cx.run_until_parked();
}

// In the show.

/// A show of a talk, rehearsed, in both its windows.
struct Show {
    _folder: tempfile::TempDir,
    document: Entity<Document>,
    show: Entity<Presentation>,
    audience: AnyWindowHandle,
    presenter: AnyWindowHandle,
}

impl Show {
    fn settle(&self, cx: &mut TestAppContext) {
        for window in [self.audience, self.presenter] {
            for _ in 0..2 {
                cx.update_window(window, |_, window, cx| window.render_frame(cx)).ok();
                cx.run_until_parked();
            }
        }
    }

    fn canvas(&self, cx: &TestAppContext) -> Entity<SlideCanvas> {
        let view: Entity<AudienceView> = cx.read(|cx| self.show.read(cx).audience()).expect("the audience's window").1;
        cx.read(|cx| view.read(cx).current().clone())
    }

    fn presenters(&self, cx: &TestAppContext) -> Entity<SlideCanvas> {
        let view: Entity<PresenterView> = cx.read(|cx| self.show.read(cx).presenter()).expect("the presenter's window").1;
        cx.read(|cx| view.read(cx).current().clone())
    }

    fn press(&self, cx: &mut TestAppContext, key: &str) {
        cx.simulate_keystrokes(self.audience, key);
        self.settle(cx);
    }

    fn step(&self, cx: &TestAppContext) -> (usize, u32) {
        self.show.read_with(cx, |show, _| (show.place().slide, show.place().step))
    }

    /// Waits for the replay on the audience's slide to show its first picture.
    fn first_picture(&self, cx: &mut TestAppContext) -> Entity<SlideCanvas> {
        let canvas = self.canvas(cx);
        until(cx, self.audience, "the first picture", |cx| {
            let pictures = pictures(cx, &canvas);
            pictures.len() == 1 && pictures[0].is_some()
        });
        canvas
    }
}

/// A rehearsed show of a talk of `source` with `cast`.
fn rehearse(cx: &mut TestAppContext, source: &str, cast: &[u8]) -> Show {
    let folder = talk_folder(source, cast);
    let document = Document::open(folder.path(), Fonts::bundled_only()).unwrap();
    show_of(cx, folder, document)
}

/// A rehearsed show of `document`, whose files `folder` holds.
fn show_of(cx: &mut TestAppContext, folder: tempfile::TempDir, document: Document) -> Show {
    cx.update(|cx| {
        gpui_kit::init(cx);
        workspace_view::bind_keys(cx);
    });
    let document = cx.new(|_| document);
    let show = cx.update(|cx| start_show(document.clone(), true, cx));
    let (audience, presenter) = cx.read(|cx| {
        let show = show.read(cx);
        (show.audience().expect("its window").0, show.presenter().expect("its window").0)
    });
    let show = Show { _folder: folder, document, show, audience, presenter };
    show.settle(cx);
    show
}

/// The image of layer `layer`, on `canvas`.
fn image_of(cx: &TestAppContext, canvas: &Entity<SlideCanvas>, layer: usize) -> std::sync::Arc<RenderImage> {
    cx.read(|cx| canvas.read(cx).images())[layer].clone()
}

/// The place among the show's layers of the first slide's recording.
fn shown_layer(document: &Entity<Document>, cx: &mut TestAppContext) -> usize {
    layer_on(document, 0, 0, cx)
}

/// The place among the layers of slide `slide` at step `step` of its recording.
fn layer_on(document: &Entity<Document>, slide: usize, step: u32, cx: &mut TestAppContext) -> usize {
    document.update(cx, |document, _| document.terminals_at(slide, Moment { step, time: 0.0 })[0].layer)
}

/// What shows of layer `layer` on `canvas`.
fn shows(cx: &TestAppContext, canvas: &Entity<SlideCanvas>, layer: usize) -> RecordingShows {
    cx.read(|cx| canvas.read(cx).recording_shows(layer))
}

fn same_picture(a: &RenderImage, b: &RenderImage) -> bool {
    a.size(0) == b.size(0) && a.as_bytes(0) == b.as_bytes(0)
}

#[gpui_kit::test]
fn the_show_plays_a_recording_and_stops_at_its_marks(cx: &mut TestAppContext) {
    let _alone = alone();
    let show = rehearse(cx, &talk(), &marked());
    // Its marks are its slide's clicks.
    assert_eq!(show.step(cx), (0, 0));
    let canvas = show.first_picture(cx);
    holds_at(cx, show.audience, &canvas, STOPS[0]);
    // It stays at the mark, drawing nothing more, with no frame asked for.
    let drawn = replay::screens_drawn();
    for_a_while(cx, show.audience, 300);
    assert!(is_at(times(cx, &canvas)[0], STOPS[0]), "it stays at its mark: {:?}", times(cx, &canvas));
    assert_eq!(replay::screens_drawn(), drawn, "nothing is drawn while it waits");
    let asked = cx.update_window(show.audience, |_, window, cx| window.simulate_next_frame(cx)).unwrap();
    assert_eq!(asked, 0, "no frame is asked for while it waits");
    assert_eq!(replay::threads_running(), 1, "one thread plays it");
    assert_eq!(show.step(cx), (0, 0));
}

#[gpui_kit::test]
fn a_click_goes_on_from_a_mark(cx: &mut TestAppContext) {
    let _alone = alone();
    let show = rehearse(cx, &talk(), &marked());
    let canvas = show.first_picture(cx);
    holds_at(cx, show.audience, &canvas, STOPS[0]);
    show.press(cx, "right");
    assert_eq!(show.step(cx), (0, 1), "a click on the same slide");
    holds_at(cx, show.audience, &canvas, STOPS[1]);
    show.press(cx, "right");
    assert_eq!(show.step(cx), (0, 2));
    holds_at(cx, show.audience, &canvas, END);
    // Its last screen is drawn as its still is, by the same items and the same drawing.
    let last = pictures(cx, &canvas)[0].clone().unwrap();
    let layer = shown_layer(&show.document, cx);
    assert!(same_picture(&last, &image_of(cx, &canvas, layer)), "the replay's last screen is the still");
    // Past the last mark, the next slide.
    show.press(cx, "right");
    assert_eq!(show.step(cx), (1, 0));
    assert!(times(cx, &show.canvas(cx)).is_empty(), "nothing plays on the next slide");
    threads_end(cx, Some(show.audience), "the show");
}

#[gpui_kit::test]
fn going_back_a_step_shows_the_mark_before(cx: &mut TestAppContext) {
    let _alone = alone();
    let show = rehearse(cx, &talk(), &marked());
    let canvas = show.first_picture(cx);
    holds_at(cx, show.audience, &canvas, STOPS[0]);
    show.press(cx, "right");
    show.press(cx, "right");
    holds_at(cx, show.audience, &canvas, END);
    show.press(cx, "left");
    assert_eq!(show.step(cx), (0, 1));
    holds_at(cx, show.audience, &canvas, STOPS[1]);
    show.press(cx, "left");
    holds_at(cx, show.audience, &canvas, STOPS[0]);
    // And on again from there.
    show.press(cx, "right");
    holds_at(cx, show.audience, &canvas, STOPS[1]);
}

/// A recording of two hours and 500,000 events, with a mark every 4,000.
fn long() -> Vec<u8> {
    let mut lines: Vec<(f64, &str, String)> = Vec::with_capacity(500_200);
    let step = 7200.0 / 500_000.0;
    for i in 0..500_000u32 {
        let at = f64::from(i) * step;
        let text = match i % 5 {
            0 => format!("\u{1b}[32mok\u{1b}[0m test {i}\r\n"),
            1 => "$ ".to_owned(),
            2 => "l".to_owned(),
            3 => "s".to_owned(),
            _ => "\r\n".to_owned(),
        };
        lines.push((at, "o", text));
        if i % 4000 == 3999 {
            lines.push((at, "m", "next".to_owned()));
        }
    }
    let lines: Vec<(f64, &str, &str)> = lines.iter().map(|(at, code, text)| (*at, *code, text.as_str())).collect();
    slides_terminal::make_test_cast(160, 48, &lines)
}

#[gpui_kit::test]
#[cfg_attr(debug_assertions, ignore = "timed in a release build")]
fn going_back_to_a_mark_is_quick(cx: &mut TestAppContext) {
    let _alone = alone();
    let cast = long();
    let stops: Vec<f64> = {
        let recording = std::sync::Arc::new(slides_terminal::read(&cast).unwrap());
        slides_terminal::Player::new(recording).stops().iter().map(Duration::as_secs_f64).collect()
    };
    assert!(stops.len() >= 100);
    // Two hours in a few seconds.
    replay::play_faster_for_tests(4000);
    let show = rehearse(cx, &talk(), &cast);
    let canvas = show.first_picture(cx);
    holds_at(cx, show.audience, &canvas, stops[0]);
    for _ in 0..100 {
        show.press(cx, "right");
    }
    holds_at(cx, show.audience, &canvas, stops[100]);
    let began = Instant::now();
    cx.simulate_keystrokes(show.audience, "left");
    cx.update_window(show.audience, |_, window, cx| window.render_frame(cx)).unwrap();
    let frame = began.elapsed();
    assert!(frame < Duration::from_millis(16), "the window's frame took {frame:?}");
    holds_at(cx, show.audience, &canvas, stops[99]);
    let shown = began.elapsed();
    eprintln!("back a step on the long recording: the window's frame {frame:?}, the mark's screen in {shown:?}");
    assert!(shown < Duration::from_millis(200), "the mark's screen took {shown:?}");
}

#[gpui_kit::test]
fn the_presenter_shows_the_last_screen(cx: &mut TestAppContext) {
    let _alone = alone();
    let show = rehearse(cx, &talk(), &marked());
    let canvas = show.first_picture(cx);
    holds_at(cx, show.audience, &canvas, STOPS[0]);
    let presenter = show.presenters(cx);
    assert!(times(cx, &presenter).is_empty(), "no second copy plays");
    assert_eq!(replay::threads_running(), 1);
    // Its slide shows the still, the last screen, drawn at its own scale.
    let layer = shown_layer(&show.document, cx);
    let scale = cx.read(|cx| presenter.read(cx).scale()).expect("drawn");
    let still = show.document.update(cx, |document, _| document.layers_at(0, Moment { step: 0, time: 0.0 }, scale))[layer].clone();
    let shown = image_of(cx, &presenter, layer);
    let wanted = slides_engine::to_bgra_frame(&still.pixmap);
    assert_eq!((shown.size(0).width.0 as u32, shown.size(0).height.0 as u32), (wanted.buffer().width(), wanted.buffer().height()));
    assert!(shown.as_bytes(0) == Some(wanted.buffer().as_raw().as_slice()), "the presenter's slide shows the last screen");
    assert_eq!(shows(cx, &presenter, layer), RecordingShows::Still);
}

#[gpui_kit::test]
fn the_window_never_waits_for_the_replay(cx: &mut TestAppContext) {
    let _alone = alone();
    replay::slow_for_tests(Duration::from_millis(200));
    let show = rehearse(cx, &talk(), &running(30));
    let canvas = show.canvas(cx);
    let mut slowest = Duration::ZERO;
    let began = Instant::now();
    let mut seen = 0;
    while began.elapsed() < Duration::from_secs(2) {
        let frame = Instant::now();
        cx.run_until_parked();
        cx.update_window(show.audience, |_, window, cx| {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
        })
        .unwrap();
        cx.run_until_parked();
        slowest = slowest.max(frame.elapsed());
        seen = seen.max(cx.read(|cx| canvas.read(cx).replay_images().0));
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(seen >= 2, "pictures still come, slowly: {seen}");
    assert!(slowest < Duration::from_millis(50), "a window frame took {slowest:?}");
    // Leaving doesn't wait for it either.
    let frame = Instant::now();
    show.press(cx, "right");
    assert!(frame.elapsed() < Duration::from_millis(100), "leaving took {:?}", frame.elapsed());
    threads_end(cx, Some(show.audience), "the show");
}

#[gpui_kit::test]
fn a_click_waits_for_a_recording_being_read(cx: &mut TestAppContext) {
    let _alone = alone();
    let folder = talk_folder(&talk(), &marked());
    let mut document = Document::open(folder.path(), Fonts::bundled_only()).unwrap();
    // Stills made aside, by nobody yet: the recording is being read.
    document.make_stills_aside(std::sync::Arc::new(|| {}));
    let show = show_of(cx, folder, document);
    assert!(show.document.update(cx, |document, _| document.terminal_pending(0)));
    show.press(cx, "right");
    assert_eq!(show.step(cx), (0, 0), "no click passes marks not yet known");
    assert!(times(cx, &show.canvas(cx)).is_empty(), "nothing plays before it's read");
    // Read now, its stops count.
    let jobs = show.document.update(cx, |document, _| document.still_jobs());
    for (job, open) in jobs {
        let Making::Made(made) = std::thread::spawn(move || make_one(job, open)).join().unwrap() else { panic!("a still waits") };
        show.document.update(cx, |document, cx| {
            document.take_still(made);
            cx.notify();
        });
    }
    show.settle(cx);
    let canvas = show.first_picture(cx);
    holds_at(cx, show.audience, &canvas, STOPS[0]);
    show.press(cx, "right");
    assert_eq!(show.step(cx), (0, 1));
}

#[gpui_kit::test]
fn a_recording_that_cannot_play_starts_nothing(cx: &mut TestAppContext) {
    let _alone = alone();
    let show = rehearse(cx, &talk(), b"these are words, not a recording");
    let document = show.document.clone();
    until(cx, show.audience, "the still", |cx| document.read_with(cx, |document, _| !document.stills_coming()));
    show.settle(cx);
    for_a_while(cx, show.audience, 100);
    assert!(times(cx, &show.canvas(cx)).is_empty());
    assert_eq!(replay::threads_running(), 0, "no thread");
    // Nor in the editor.
    let typing = editor(cx, &talk(), b"these are words, not a recording");
    select_terminal(&typing, cx);
    assert!(bounds_of(&typing, cx, TERMINAL_PLAY_ID).is_none(), "nothing to press");
}

// In the editor.

#[gpui_kit::test]
fn play_in_the_editor_plays_through_and_holds_the_end(cx: &mut TestAppContext) {
    let _alone = alone();
    let typing = editor(cx, &talk(), &marked());
    select_terminal(&typing, cx);
    assert!(bounds_of(&typing, cx, TERMINAL_PLAY_ID).is_some(), "Play shows on a recording selected");
    let canvas = play_selected(&typing, cx);
    assert!(bounds_of(&typing, cx, TERMINAL_STOP_ID).is_some(), "and Stop while it plays");
    // Through its marks, with no click, to its end, where it holds.
    holds_at(cx, typing.handle, &canvas, END);
    threads_end(cx, Some(typing.handle), "the editor");
    for_a_while(cx, typing.handle, 100);
    assert!(is_at(times(cx, &canvas)[0], END), "its last screen holds");
    let last = pictures(cx, &canvas)[0].clone().unwrap();
    let layer = typing.document(cx).update(cx, |document, _| document.terminals_here()[0].layer);
    assert!(same_picture(&last, &image_of(cx, &canvas, layer)), "drawn as its still is");
    assert!(bounds_of(&typing, cx, TERMINAL_PLAY_ID).is_some(), "Play again once it's done");
    // Stop lets go of it.
    let canvas = play_selected(&typing, cx);
    click_on(&typing, cx, TERMINAL_STOP_ID);
    assert!(times(cx, &canvas).is_empty());
    threads_end(cx, Some(typing.handle), "the editor");
    let (made, released) = made_and_released(cx, &canvas);
    assert_eq!(made, released, "every picture is released");
}

/// Plays the recording on a talk of `source`, does `leave`, and checks its thread ends within
/// 300 ms, nothing is drawn after, and, if the window's still open, no replay is left and every
/// picture is released.
fn left_by(cx: &mut TestAppContext, how: &str, source: &str, leave: impl FnOnce(Typing, &mut TestAppContext) -> Option<Typing>) {
    let typing = editor(cx, source, &running(30));
    let canvas = play_selected(&typing, cx);
    assert_eq!(replay::threads_running(), 1, "{how}: one thread plays it");
    drop(canvas);
    let kept = leave(typing, cx);
    let took = threads_end(cx, kept.as_ref().map(|typing| typing.handle), how);
    assert!(took <= Duration::from_millis(300), "{how}: its thread took {took:?} to end");
    if let Some(typing) = kept {
        nothing_drawn(cx, typing.handle, how);
        let canvas = editor_canvas(&typing, cx);
        assert!(times(cx, &canvas).is_empty(), "{how}: no replay is left");
        typing.settle(cx);
        let (made, released) = made_and_released(cx, &canvas);
        assert_eq!(made, released, "{how}: every picture is released");
        assert!(bounds_of(&typing, cx, TERMINAL_STOP_ID).is_none(), "{how}");
    }
}

#[gpui_kit::test]
fn every_way_of_leaving_stops_the_replay(cx: &mut TestAppContext) {
    let _alone = alone();
    let talk = talk();
    left_by(cx, "selecting something else", &talk, |typing, cx| {
        let shape = thing(&typing, cx, "<Box");
        let at = middle(&typing, cx, &shape);
        typing.click(cx, at);
        Some(typing)
    });
    left_by(cx, "another slide", &talk, |typing, cx| {
        typing.press(cx, "pagedown");
        assert_eq!(typing.current(cx), 1);
        Some(typing)
    });
    left_by(cx, "deleting the recording", &talk, |typing, cx| {
        typing.press(cx, "delete");
        assert!(!typing.source(cx).contains("<Terminal"));
        Some(typing)
    });
    left_by(cx, "a change to the talk's text", &talk, |typing, cx| {
        let changed = talk.replace("# Run", "# Changed");
        typing.view.update(cx, |view, cx| view.reload_for_test(Some(changed), cx));
        typing.settle(cx);
        Some(typing)
    });
    left_by(cx, "the window losing the keyboard", &talk, |typing, cx| {
        typing.with_window(cx, |window, _| window.activate_window());
        cx.run_until_parked();
        VisualTestContext::from_window(typing.handle, cx).deactivate_window();
        typing.settle(cx);
        Some(typing)
    });
    left_by(cx, "starting the show", &talk, |typing, cx| {
        typing.press(cx, "shift-f5");
        cx.run_until_parked();
        assert!(cx.read(|cx| cx.windows().len()) > 1, "the show is on");
        close_others(&typing, cx);
        Some(typing)
    });
    left_by(cx, "closing the window", &talk, |typing, cx| {
        let Typing { _folder, handle, view, .. } = typing;
        drop(view);
        cx.update_window(handle, |_, window, _| window.remove_window()).unwrap();
        cx.run_until_parked();
        None
    });
    // Undo of the recording's insert.
    let typing = editor(cx, "# Run\n\n---\n\n# Two\n", &running(30));
    typing.document(cx).update(cx, |document, cx| {
        document.insert_tag("Insert terminal recording", "Terminal", &[("src", "assets/run.cast")], None).unwrap();
        cx.notify();
    });
    stills_made(&typing, cx);
    let canvas = play_selected(&typing, cx);
    typing.press(cx, "ctrl-z");
    assert!(!typing.source(cx).contains("<Terminal"), "undone");
    let took = threads_end(cx, Some(typing.handle), "undo");
    assert!(took <= Duration::from_millis(300), "undo: its thread took {took:?} to end");
    assert!(times(cx, &canvas).is_empty());

    // A jump to a slide further on.
    left_by(cx, "a jump to another slide", &format!("{talk}\n---\n\n# Three\n"), |typing, cx| {
        typing.document(cx).update(cx, |document, cx| {
            document.go_to(2);
            cx.notify();
        });
        typing.settle(cx);
        assert_eq!(typing.current(cx), 2);
        Some(typing)
    });

    // In the show: the slide left, the slide come back to, and the show ended.
    let three = format!("# Zero\n\n---\n\n{talk}");
    let show = rehearse(cx, &three, &running(30));
    // Readied for the next slide, at its start.
    until(cx, show.audience, "the next slide's recording readied", |_| replay::threads_running() == 1);
    show.press(cx, "right");
    assert_eq!(show.step(cx), (1, 0));
    let canvas = show.first_picture(cx);
    until(cx, show.audience, "it plays on", |cx| times(cx, &canvas)[0].is_some_and(|at| at >= 0.3));
    show.press(cx, "right");
    assert_eq!(show.step(cx), (2, 0));
    assert!(times(cx, &show.canvas(cx)).is_empty(), "nothing plays on the next slide");
    let took = threads_end(cx, Some(show.audience), "the slide left");
    assert!(took <= Duration::from_millis(300), "the slide left: its thread took {took:?} to end");
    nothing_drawn(cx, show.audience, "the slide left");
    let (made, released) = made_and_released(cx, &canvas);
    assert_eq!(made, released, "the slide left: every picture is released");
    // Come back to from the slide after, it shows its end, its still, and nothing plays.
    show.press(cx, "left");
    assert_eq!(show.step(cx), (1, 0));
    for_a_while(cx, show.audience, 100);
    assert!(times(cx, &canvas).is_empty(), "come back to, it's its still");
    assert_eq!(replay::threads_running(), 0, "come back to: no thread");
    drop(canvas);
    // Come to again going on, it starts from its beginning.
    show.press(cx, "left");
    show.press(cx, "right");
    assert_eq!(show.step(cx), (1, 0));
    let canvas = show.first_picture(cx);
    assert!(times(cx, &canvas)[0].is_some_and(|at| at < 0.3), "from the beginning again: {:?}", times(cx, &canvas));
    // Held here, the canvas would outlive its window as the show ends.
    drop(canvas);
    cx.simulate_keystrokes(show.audience, "escape");
    cx.run_until_parked();
    assert!(show.show.read_with(cx, |show, _| show.has_ended()));
    let took = threads_end(cx, None, "the show ended");
    assert!(took <= Duration::from_millis(300), "the show ended: its thread took {took:?} to end");

    // Left through a transition: its last picture stays as the next slide comes in, and then goes.
    let faded = format!("# Run\n\n{TERMINAL}\n\n---\ntransition: fade\n---\n\n# Two\n");
    let show = rehearse(cx, &faded, &running(30));
    let canvas = show.first_picture(cx);
    show.press(cx, "right");
    let view: Entity<AudienceView> = cx.read(|cx| show.show.read(cx).audience()).unwrap().1;
    let leaving = cx.read(|cx| view.read(cx).outgoing(cx).cloned()).expect("the slide left is still seen");
    assert!(pictures(cx, &leaving)[0].is_some(), "its last picture stays");
    let took = threads_end(cx, Some(show.audience), "the transition");
    assert!(took <= Duration::from_millis(300), "the transition: its thread took {took:?} to end");
    // The transition's time is the show's clock.
    for _ in 0..30 {
        cx.executor().advance_clock(Duration::from_millis(50));
        cx.run_until_parked();
        cx.update_window(show.audience, |_, window, cx| {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
        })
        .unwrap();
    }
    show.settle(cx);
    assert!(times(cx, &leaving).is_empty(), "once the transition's done, it goes");
    let (made, released) = made_and_released(cx, &leaving);
    assert_eq!(made, released, "the transition: every picture is released");
    nothing_drawn(cx, show.audience, "the transition");
    drop((canvas, leaving, view));
    cx.update_window(show.audience, |_, window, _| window.remove_window()).ok();
    cx.run_until_parked();
    threads_end(cx, None, "the transition's show closed");
}

#[gpui_kit::test]
fn come_back_to_from_a_later_slide_it_shows_its_end(cx: &mut TestAppContext) {
    let _alone = alone();
    let show = rehearse(cx, &talk(), &marked());
    let canvas = show.first_picture(cx);
    holds_at(cx, show.audience, &canvas, STOPS[0]);
    show.press(cx, "right");
    show.press(cx, "right");
    show.press(cx, "right");
    assert_eq!(show.step(cx), (1, 0));
    threads_end(cx, Some(show.audience), "the slide left");
    show.press(cx, "left");
    assert_eq!(show.step(cx), (0, 2));
    let canvas = show.canvas(cx);
    let layer = shown_layer(&show.document, cx);
    for_a_while(cx, show.audience, 100);
    assert_eq!(shows(cx, &canvas, layer), RecordingShows::Still, "its last screen, its still");
    assert_eq!(replay::threads_running(), 0, "and no thread");
    // A step back from there goes to the mark before, soon.
    let began = Instant::now();
    show.press(cx, "left");
    holds_at(cx, show.audience, &canvas, STOPS[1]);
    assert!(began.elapsed() < Duration::from_secs(1), "the mark's screen took {:?}", began.elapsed());
    // Drawn again at the same place, it stays there.
    show.document.update(cx, |_, cx| cx.notify());
    show.settle(cx);
    for_a_while(cx, show.audience, 200);
    assert!(is_at(times(cx, &canvas)[0], STOPS[1]), "it stays at the mark: {:?}", times(cx, &canvas));
}

#[gpui_kit::test]
fn the_first_frame_never_shows_the_last_screen(cx: &mut TestAppContext) {
    let _alone = alone();
    let clicked = TERMINAL.replace(" />", " click=\"1\" />");
    let source = format!("# Zero\n\n---\n\n# Run\n\n{TERMINAL}\n\n---\n\n# Clicked\n\n{clicked}\n");
    let show = rehearse(cx, &source, &marked());
    let document = show.document.clone();
    until(cx, show.audience, "the stills", |cx| document.read_with(cx, |document, _| !document.stills_coming()));
    show.settle(cx);
    // The next slide's recording is readied, its first picture drawn, before it's come to.
    until(cx, show.audience, "the next slide's recording readied", |_| replay::threads_running() == 1);
    for_a_while(cx, show.audience, 300);
    let first_frame = |cx: &mut TestAppContext| {
        cx.simulate_keystrokes(show.audience, "right");
        cx.run_until_parked();
        cx.update_window(show.audience, |_, window, cx| window.render_frame(cx)).unwrap();
    };
    first_frame(cx);
    assert_eq!(show.step(cx), (1, 0));
    let layer = layer_on(&document, 1, 0, cx);
    let canvas = show.canvas(cx);
    let seen = shows(cx, &canvas, layer);
    assert!(matches!(seen, RecordingShows::Picture(at) if at < STOPS[0]), "arrived at, it shows its start at once: {seen:?}");
    holds_at(cx, show.audience, &canvas, STOPS[0]);
    show.press(cx, "right");
    show.press(cx, "right");
    show.press(cx, "right");
    assert_eq!(show.step(cx), (2, 0));
    // Its click comes: its panel, or its start, never its last screen.
    first_frame(cx);
    assert_eq!(show.step(cx), (2, 1));
    let layer = layer_on(&document, 2, 1, cx);
    let canvas = show.canvas(cx);
    let seen = shows(cx, &canvas, layer);
    assert!(seen == RecordingShows::Panel || matches!(seen, RecordingShows::Picture(at) if at < STOPS[0]), "on its click: {seen:?}");
}

#[gpui_kit::test]
fn space_plays_and_stops_a_selected_recording(cx: &mut TestAppContext) {
    let _alone = alone();
    let typing = editor(cx, &talk(), &running(30));
    select_terminal(&typing, cx);
    typing.press(cx, "space");
    let canvas = editor_canvas(&typing, cx);
    until(cx, typing.handle, "the first picture", |cx| pictures(cx, &canvas).first().is_some_and(Option::is_some));
    assert!(typing.editing(cx).is_none(), "nothing is typed");
    typing.press(cx, "space");
    assert!(times(cx, &canvas).is_empty(), "stopped");
    threads_end(cx, Some(typing.handle), "space");
}

#[gpui_kit::test]
fn a_finished_replay_gives_way_to_its_still_at_a_new_size(cx: &mut TestAppContext) {
    let _alone = alone();
    let typing = editor(cx, &talk(), &marked());
    let canvas = play_selected(&typing, cx);
    holds_at(cx, typing.handle, &canvas, END);
    threads_end(cx, Some(typing.handle), "the end");
    let layer = typing.document(cx).update(cx, |document, _| document.terminals_here()[0].layer);
    assert!(matches!(shows(cx, &canvas, layer), RecordingShows::Picture(_)));
    VisualTestContext::from_window(typing.handle, cx).simulate_resize(size(px(1000.), px(700.)));
    typing.settle(cx);
    until(cx, typing.handle, "the still at the new size", |cx| shows(cx, &canvas, layer) == RecordingShows::Still);
    typing.settle(cx);
    let (made, released) = made_and_released(cx, &canvas);
    assert_eq!(made, released, "its picture is released");
}

#[gpui_kit::test]
fn an_idle_talk_of_recordings_does_no_work(cx: &mut TestAppContext) {
    let _alone = alone();
    let source: Vec<String> = (0..10).map(|at| format!("# Slide {at}\n\n{TERMINAL}\n")).collect();
    let typing = editor(cx, &source.join("\n---\n\n"), &marked());
    let canvas = editor_canvas(&typing, cx);
    let idle = |typing: &Typing, cx: &mut TestAppContext, when: &str| {
        typing.settle(cx);
        cx.update_window(typing.handle, |_, window, cx| window.simulate_next_frame(cx)).unwrap();
        let drawn = replay::screens_drawn();
        let (redrawn, _watching) = redraws(cx, &canvas);
        let mut frames_asked = 0;
        for _ in 0..80 {
            cx.executor().advance_clock(Duration::from_millis(250));
            cx.run_until_parked();
            frames_asked += cx.update_window(typing.handle, |_, window, cx| window.simulate_next_frame(cx)).unwrap();
        }
        assert_eq!(redrawn.get(), 0, "{when}: nothing redraws");
        assert_eq!(frames_asked, 0, "{when}: no frame is asked for");
        assert_eq!(replay::threads_running(), 0, "{when}: no thread");
        assert_eq!(replay::screens_drawn(), drawn, "{when}: no screen is drawn");
    };
    idle(&typing, cx, "none played");
    play_selected(&typing, cx);
    click_on(&typing, cx, TERMINAL_STOP_ID);
    threads_end(cx, Some(typing.handle), "the editor");
    idle(&typing, cx, "one played and stopped");
}
