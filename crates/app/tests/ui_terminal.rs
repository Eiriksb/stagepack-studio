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
        workspace_view::hold_recording_checks_for_tests(false);
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
    document.update(cx, |document, _| document.terminals_at(slide, Moment::at(step, 0.0))[0].layer)
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
    let still = show.document.update(cx, |document, _| document.layers_at(0, Moment::at(0, 0.0), scale))[layer].clone();
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

// Putting a recording on a slide.

const PLAIN_TALK: &str = "# One\n\nSome words\n\n---\n\n# Two\n";
const NOT_A_RECORDING: &str = "This file isn't a terminal recording.";

/// An 80 by 24 recording, and what it's put on the slide as: its own size, in the middle.
const INSERTED: &str = "<Terminal src=\"assets/demo-run.cast\" x=\"614\" y=\"324\" w=\"691\" h=\"432\" />";

/// What [`PLAIN_TALK`] says once `inserted` is put after its first slide's content.
fn with_inserted(inserted: &str) -> String {
    PLAIN_TALK.replacen("Some words", &format!("Some words\n\n{inserted}"), 1)
}

/// A short recording of 80 by 24 that prints a word.
fn eighty() -> Vec<u8> {
    slides_terminal::make_test_cast(80, 24, &[(0.0, "o", "$ make\r\n"), (0.1, "o", "built\r\n")])
}

/// A recording 80 by 24, 3 minutes 20 seconds long as it's played, that stops twice.
fn three_minutes() -> Vec<u8> {
    let mut lines: Vec<(f64, &str, String)> = (0..=200).map(|at| (f64::from(at), "o", format!("line {at}\r\n"))).collect();
    lines.insert(51, (50.0, "m", String::new()));
    lines.insert(102, (100.0, "m", String::new()));
    let lines: Vec<(f64, &str, &str)> = lines.iter().map(|(at, code, text)| (*at, *code, text.as_str())).collect();
    slides_terminal::make_test_cast(80, 24, &lines)
}

/// A folder talk of `source` in a window that keeps its working copies under `root`.
fn keeping_editor(cx: &mut TestAppContext, source: &str, root: &std::path::Path) -> Typing {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("slides.md"), source).unwrap();
    open_folder(cx, folder, Some(stagepack_studio::recovery::Recovery::new(root.join("recovery"))))
}

/// A file named `name` holding `bytes` in `dir`, outside any talk.
fn outside(dir: &std::path::Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

/// Hands `paths` to the window as the Terminal recording tool's dialog would, and lets them be
/// looked at and put on the slide.
fn choose_recordings(typing: &Typing, cx: &mut TestAppContext, paths: Vec<std::path::PathBuf>) {
    let view = typing.view.clone();
    cx.update(|cx| view.update(cx, |view, cx| view.chosen_recordings(paths, cx)));
    cx.run_until_parked();
    typing.settle(cx);
}

/// Drops `paths` from outside the app on the middle of the slide.
fn drop_on_slide(typing: &Typing, cx: &mut TestAppContext, paths: &[std::path::PathBuf]) {
    use gpui_kit::InputEvent as _;
    let at = typing.at(cx, 960., 540.);
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        let paths = gpui_kit::ExternalPaths(paths.iter().cloned().collect());
        window.dispatch_event(gpui_kit::FileDropEvent::Entered { position: at, paths }.to_platform_input(), cx);
        window.dispatch_event(gpui_kit::FileDropEvent::Pending { position: at }.to_platform_input(), cx);
        window.dispatch_event(gpui_kit::FileDropEvent::Submit { position: at }.to_platform_input(), cx);
        window.render_frame(cx);
    });
    cx.run_until_parked();
    typing.settle(cx);
}

/// Pastes `paths`, as files copied from outside the app are.
fn paste_paths(typing: &Typing, cx: &mut TestAppContext, paths: &[std::path::PathBuf]) {
    cx.write_to_clipboard(gpui_kit::ClipboardItem {
        entries: vec![gpui_kit::ClipboardEntry::ExternalPaths(gpui_kit::ExternalPaths(paths.iter().cloned().collect()))],
    });
    click_on(typing, cx, stagepack_studio::clipboard_tools::PASTE_ID);
    cx.run_until_parked();
    typing.settle(cx);
}

fn selected_text(typing: &Typing, cx: &TestAppContext) -> Option<String> {
    let source = typing.source(cx);
    selected(typing, cx).map(|(_, range)| source[range].to_string())
}

fn can_undo(typing: &Typing, cx: &TestAppContext) -> bool {
    typing.document(cx).read_with(cx, |document, _| document.can_undo())
}

fn png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
    let picture = image::RgbaImage::from_pixel(width, height, image::Rgba(rgba));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(picture).write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

/// The recording's tag, as the talk has it now.
fn the_tag(typing: &Typing, cx: &mut TestAppContext) -> String {
    let source = typing.source(cx);
    let at = source.find("<Terminal").unwrap();
    let end = at + source[at..].find("/>").unwrap() + 2;
    source[at..end].to_string()
}

fn has_file(typing: &Typing, cx: &TestAppContext, path: &str) -> bool {
    typing.document(cx).read_with(cx, |document, _| document.has_file(path))
}

/// Every file under `root`, by its path.
fn files_under(root: &std::path::Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else {
                found.push(path.display().to_string());
            }
        }
    }
    found
}

/// The words of the element `id`, if it's there.
fn label_of(typing: &Typing, cx: &mut TestAppContext, id: &str) -> Option<String> {
    let id = gpui_kit::SharedString::from(id.to_string());
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find(id).and_then(|found| found.label().map(str::to_string))
    })
}

/// Waits for the form's line `id`, read off the window's thread, to say something.
fn said_by_form(typing: &Typing, cx: &mut TestAppContext, id: &'static str) -> String {
    let mut said = None;
    until(cx, typing.handle, id, |cx| {
        said = typing.with_window(cx, |window, cx| {
            window.render_frame(cx);
            window.try_find(id).and_then(|found| found.label().map(str::to_string))
        });
        said.is_some()
    });
    said.unwrap()
}

/// Why the recording on slide `slide` can't be played, once its still has been tried.
fn problem_of(typing: &Typing, cx: &mut TestAppContext, slide: usize) -> Option<String> {
    stills_made(typing, cx);
    let source = typing.document(cx).update(cx, |document, _| {
        document.terminals_at(slide, Moment::at(0, 0.0)).first().and_then(|terminal| terminal.source.clone())
    });
    let source = source.expect("a recording on the slide");
    typing.document(cx).update(cx, |document, _| document.terminal_problem_on(slide, &source))
}

#[gpui_kit::test]
fn a_chosen_recording_is_put_on_the_slide(cx: &mut TestAppContext) {
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let typing = keeping_editor(cx, PLAIN_TALK, work.path());
    // The Insert tab has it.
    click_on(&typing, cx, stagepack_studio::ribbon::TAB_INSERT_ID);
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        let tool = window.find(stagepack_studio::insert_tools::INSERT_TERMINAL_ID);
        assert_eq!(tool.label(), Some("Terminal recording"));
        assert!(tool.visible());
    });

    let from = outside(work.path(), "Demo Run.CAST", &eighty());
    choose_recordings(&typing, cx, vec![from.clone()]);
    assert_eq!(typing.source(cx), with_inserted(INSERTED));
    assert_eq!(selected_text(&typing, cx).as_deref(), Some(INSERTED));
    assert_eq!(typing.status(cx).as_deref(), Some("Terminal recording selected"));
    assert!(has_file(&typing, cx, "assets/demo-run.cast"));
    assert_eq!(problem_of(&typing, cx, 0), None, "it plays");
    // One step, which Undo takes away.
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), PLAIN_TALK);
    assert!(!can_undo(&typing, cx), "it was one step");
    // Made again, it's there and selected again.
    typing.press(cx, "ctrl-shift-z");
    assert_eq!(typing.source(cx), with_inserted(INSERTED));
    assert_eq!(selected_text(&typing, cx).as_deref(), Some(INSERTED));
    typing.press(cx, "ctrl-z");

    // On the slide shown when it was asked for, though another is shown before it's looked at.
    let view = typing.view.clone();
    cx.update(|cx| view.update(cx, |view, cx| view.chosen_recordings(vec![from], cx)));
    let document = typing.document(cx);
    cx.update(|cx| {
        document.update(cx, |document, cx| {
            document.go_to(1);
            cx.notify();
        })
    });
    assert_eq!(typing.current(cx), 1);
    cx.run_until_parked();
    typing.settle(cx);
    assert_eq!(typing.source(cx), with_inserted(INSERTED), "on the first slide");
    assert_eq!(typing.current(cx), 0, "shown again, as any edit shows its slide");
    assert_eq!(selected_text(&typing, cx).as_deref(), Some(INSERTED));
    typing.press(cx, "ctrl-z");

    // A screen larger than the slide is made smaller to fit it, keeping its shape.
    let wide = outside(work.path(), "wide.cast", &slides_terminal::make_test_cast(500, 200, &[(0.0, "o", "wide")]));
    choose_recordings(&typing, cx, vec![wide]);
    assert_eq!(the_tag(&typing, cx), "<Terminal src=\"assets/wide.cast\" x=\"312\" y=\"0\" w=\"1296\" h=\"1080\" />");
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), PLAIN_TALK);
}

#[gpui_kit::test]
fn a_dropped_or_pasted_recording_is_too(cx: &mut TestAppContext) {
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let typing = keeping_editor(cx, PLAIN_TALK, work.path());
    // A file that's none of what can be dropped is said, in words that name a recording too.
    // (Dropped first: in a test window, a drop after a key press doesn't reach the slide.)
    let notes = outside(work.path(), "notes.txt", b"words");
    drop_on_slide(&typing, cx, &[notes]);
    assert_eq!(typing.status(cx).as_deref(), Some("Only pictures, videos, 3D models and terminal recordings can be dropped on a slide."));

    let red = outside(work.path(), "red.png", &png(100, 100, [255, 0, 0, 255]));
    let run = outside(work.path(), "demo run.cast", &eighty());
    drop_on_slide(&typing, cx, &[red, run.clone()]);
    let source = typing.source(cx);
    assert_eq!(source.matches("<Picture").count(), 1, "{source}");
    assert_eq!(source.matches(INSERTED).count(), 1, "{source}");
    assert!(has_file(&typing, cx, "assets/demo-run.cast"));
    // Each its own step.
    typing.press(cx, "ctrl-z");
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), PLAIN_TALK);
    assert!(!can_undo(&typing, cx));

    // Its path pasted is a drop.
    paste_paths(&typing, cx, &[run]);
    assert_eq!(typing.source(cx), with_inserted(INSERTED));
    assert_eq!(selected_text(&typing, cx).as_deref(), Some(INSERTED));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), PLAIN_TALK);
    // One that isn't a recording, pasted or dropped, says why.
    let words = outside(work.path(), "words.cast", b"these are words, not a recording");
    paste_paths(&typing, cx, std::slice::from_ref(&words));
    assert_eq!(typing.status(cx).as_deref(), Some(NOT_A_RECORDING));
    assert_eq!(typing.source(cx), PLAIN_TALK);
}

#[gpui_kit::test]
fn a_file_that_isnt_a_recording_is_refused_saying_why(cx: &mut TestAppContext) {
    use slides_terminal::CantPlay;
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let typing = keeping_editor(cx, PLAIN_TALK, work.path());
    let mut cut = eighty();
    cut.extend_from_slice(b"[0.2, \"o\", \"cut");
    let too_long = slides_terminal::make_test_cast(80, 24, &[(0.0, "o", "start"), (25.0 * 3600.0, "o", "end")]);
    let mut over = b"{\"version\": 2, \"width\": 80, \"height\": 24}\n".to_vec();
    over.resize(33 * 1024 * 1024, b' ');
    let said = |why: CantPlay| why.to_string();
    let cases = [
        ("words.cast", b"these are words, not a recording".to_vec(), said(CantPlay::NotARecording)),
        ("empty.cast", Vec::new(), said(CantPlay::NotARecording)),
        ("picture.cast", png(10, 10, [0, 0, 255, 255]), said(CantPlay::NotARecording)),
        ("notes.txt", eighty(), said(CantPlay::NotARecording)),
        ("old.cast", b"{\"version\": 1, \"width\": 80, \"height\": 24, \"stdout\": []}".to_vec(), said(CantPlay::KindNotPlayed)),
        ("cut.cast", cut, said(CantPlay::Damaged)),
        ("huge.cast", slides_terminal::make_test_cast(1000, 24, &[]), said(CantPlay::TooLarge)),
        ("long.cast", too_long, said(CantPlay::TooLarge)),
        ("over.cast", over, said(CantPlay::TooLarge)),
    ];
    for (name, bytes, why) in &cases {
        let from = outside(work.path(), name, bytes);
        choose_recordings(&typing, cx, vec![from]);
        assert_eq!(typing.source(cx), PLAIN_TALK, "{name}");
        assert_eq!(typing.status(cx).as_deref(), Some(why.as_str()), "{name}");
        let stem = name.split('.').next().unwrap();
        assert!(!has_file(&typing, cx, &format!("assets/{stem}.cast")), "{name} wasn't added");
    }
    assert!(!can_undo(&typing, cx));
    // Dropped, the same.
    let words = outside(work.path(), "words.cast", b"these are words, not a recording");
    drop_on_slide(&typing, cx, &[words]);
    assert_eq!(typing.status(cx).as_deref(), Some(NOT_A_RECORDING));
    assert_eq!(typing.source(cx), PLAIN_TALK);
    wait(cx, 1_000);
    let kept: Vec<String> = files_under(&work.path().join("recovery")).into_iter().filter(|name| name.ends_with(".cast")).collect();
    assert!(kept.is_empty(), "nothing refused was kept: {kept:?}");
}

#[gpui_kit::test]
fn the_form_says_what_the_recording_is(cx: &mut TestAppContext) {
    use stagepack_studio::inspector::{INSPECTOR_MESSAGE_ID, INSPECTOR_TERMINAL_FACTS_ID, INSPECTOR_TERMINAL_PROBLEM_ID};
    let _alone = alone();
    let typing = editor(cx, &talk(), &three_minutes());
    select_terminal(&typing, cx);
    assert_eq!(said_by_form(&typing, cx, INSPECTOR_TERMINAL_FACTS_ID), "80 by 24, 3 min 20 s, 2 stops");
    assert_eq!(label_of(&typing, cx, INSPECTOR_MESSAGE_ID).as_deref(), Some("run.cast"), "its file, by its name");
    assert_eq!(label_of(&typing, cx, INSPECTOR_TERMINAL_PROBLEM_ID), None, "it plays");
    let form = cx.read(|cx| typing.view.read(cx).inspector().clone());
    let labels = cx.read(|cx| form.read(cx).labels());
    assert_eq!(labels, ["Left", "Top", "Width", "Height", "Turn"], "its place, and nothing else");
    // Moved, only its place changes, and its facts are as they were.
    let tag = the_tag(&typing, cx);
    typing.press(cx, "right");
    assert_ne!(the_tag(&typing, cx), tag);
    assert_eq!(said_by_form(&typing, cx, INSPECTOR_TERMINAL_FACTS_ID), "80 by 24, 3 min 20 s, 2 stops");

    // Short ones, and those that stop once or never.
    use stagepack_studio::inspector::terminal_facts;
    let facts = |length: f64, marks: usize| slides_terminal::RecordingFacts { width: 120, height: 40, length: Duration::from_secs_f64(length), marks };
    assert_eq!(terminal_facts(facts(12.4, 1)), "120 by 40, 12 s, 1 stop");
    assert_eq!(terminal_facts(facts(0.2, 0)), "120 by 40, under a second, no stops");
    assert_eq!(terminal_facts(facts(180.0, 3)), "120 by 40, 3 min, 3 stops");
    assert_eq!(terminal_facts(facts(3_725.0, 0)), "120 by 40, 1 h 2 min 5 s, no stops");

    // One that can't be played says why, and has no facts.
    let typing = editor(cx, &talk(), b"these are words, not a recording");
    select_terminal(&typing, cx);
    assert_eq!(said_by_form(&typing, cx, INSPECTOR_TERMINAL_PROBLEM_ID), NOT_A_RECORDING);
    assert_eq!(label_of(&typing, cx, INSPECTOR_TERMINAL_FACTS_ID), None);

    // Read from a pack that holds it compressed, as it's read to be played.
    let work = tempfile::tempdir().unwrap();
    let pack = compressed_pack(work.path(), &talk(), &three_minutes());
    let document = Document::open_packed(&pack, Fonts::bundled_only()).unwrap();
    let typing = open_untitled_talk(cx, document, work.path());
    stills_made(&typing, cx);
    select_terminal(&typing, cx);
    assert_eq!(said_by_form(&typing, cx, INSPECTOR_TERMINAL_FACTS_ID), "80 by 24, 3 min 20 s, 2 stops");
}

/// A pack made by another program, its talk of `source` and its `assets/run.cast`, `cast`, both
/// compressed, in `work`.
fn compressed_pack(work: &std::path::Path, source: &str, cast: &[u8]) -> std::path::PathBuf {
    use std::io::Write as _;
    let at = work.join("compressed.sptx");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&at).unwrap());
    let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("talk.json", deflated).unwrap();
    zip.write_all(br#"{"format":1,"app":"another","entry":"slides.md"}"#).unwrap();
    zip.start_file("slides.md", deflated).unwrap();
    zip.write_all(source.as_bytes()).unwrap();
    zip.start_file("assets/run.cast", deflated).unwrap();
    zip.write_all(cast).unwrap();
    zip.finish().unwrap();
    at
}

/// The file `path` in the pack `pack`, and whether it's compressed there.
fn in_pack(pack: &std::path::Path, path: &str) -> Option<(Vec<u8>, bool)> {
    use std::io::Read as _;
    let mut zip = zip::ZipArchive::new(std::fs::File::open(pack).unwrap()).unwrap();
    let mut entry = zip.by_name(path).ok()?;
    let compressed = entry.compression() != zip::CompressionMethod::Stored;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).unwrap();
    Some((bytes, compressed))
}

/// Whether `document`'s first slide's recording plays, once its still has been made.
fn plays(document: &mut Document) -> bool {
    let Some(source) = document.terminals_at(0, Moment::at(0, 0.0)).first().and_then(|terminal| terminal.source.clone()) else {
        return false;
    };
    // Its still is made here, as the window makes it aside.
    document.notices();
    for (job, open) in document.still_jobs() {
        match make_one(job, open) {
            Making::Made(made) => {
                document.take_still(made);
            }
            Making::Waits(_) => return false,
        }
    }
    document.terminal_problem_on(0, &source).is_none() && !document.terminal_pending(0)
}

#[gpui_kit::test]
fn a_recording_is_kept_saved_and_brought_back(cx: &mut TestAppContext) {
    use stagepack_studio::saving::SaveTarget;
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let recovery = stagepack_studio::recovery::Recovery::new(work.path().join("recovery"));
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("slides.md"), PLAIN_TALK).unwrap();
    let typing = open_folder(cx, folder, Some(recovery.clone()));
    let from = outside(work.path(), "demo-run.cast", &eighty());
    choose_recordings(&typing, cx, vec![from]);
    let inserted = typing.source(cx);
    assert_eq!(inserted, with_inserted(INSERTED));

    // A crash: the working copy keeps it, and it's brought back.
    wait(cx, 1_000);
    let Typing { _folder: folder, handle, view, .. } = typing;
    drop(view);
    cx.update_window(handle, |_, window, _| window.remove_window()).unwrap();
    cx.run_until_parked();
    assert!(!folder.path().join("assets").exists(), "nothing written until a save");
    let typing = open_folder(cx, folder, Some(recovery));
    click_on(&typing, cx, workspace_view::RECOVER_ID);
    assert_eq!(typing.source(cx), inserted);
    assert!(has_file(&typing, cx, "assets/demo-run.cast"));
    assert_eq!(problem_of(&typing, cx, 0), None, "brought back, it plays");

    // Saved in place, in a folder.
    typing.press(cx, "ctrl-s");
    wait(cx, 500);
    typing.settle(cx);
    let saved = typing._folder.path().join("assets/demo-run.cast");
    assert_eq!(std::fs::read(&saved).unwrap(), eighty());

    // Copied with its slide into another talk, it comes too, and plays there.
    typing.press(cx, "pageup");
    click_on(&typing, cx, stagepack_studio::slide_strip::strip_row_id(0));
    typing.press(cx, "ctrl-c");
    let into = open(cx, "# A\n");
    click_on(&into, cx, stagepack_studio::slide_strip::strip_row_id(0));
    into.press(cx, "ctrl-v");
    cx.run_until_parked();
    into.settle(cx);
    assert!(into.source(cx).contains("<Terminal src=\"assets/demo-run.cast\""), "{}", into.source(cx));
    assert!(has_file(&into, cx, "assets/demo-run.cast"), "its file came with it");
    assert_eq!(problem_of(&into, cx, 1), None, "it plays there");

    // Save As a pack: it's in the pack, compressed, and read from there.
    let mut document = Document::open(typing._folder.path(), Fonts::bundled_only()).unwrap();
    let pack = work.path().join("talk.sptx");
    let saved = document.save_as_job(SaveTarget::Packed(pack.clone())).run().unwrap();
    document.saved(saved);
    assert_eq!(in_pack(&pack, "assets/demo-run.cast"), Some((eighty(), true)));
    assert!(plays(&mut document), "read from the pack now");
    let mut fresh = Document::open_packed(&pack, Fonts::bundled_only()).unwrap();
    assert!(plays(&mut fresh), "the pack opened again");

    // Added to a pack and saved in place, it's in the pack.
    let longer = slides_terminal::make_test_cast(80, 24, &[(0.0, "o", "$ again\r\n")]);
    fresh.insert_terminal("again.cast", longer.clone(), (80, 24)).unwrap();
    let saved = fresh.save_job().run().unwrap();
    fresh.saved(saved);
    assert_eq!(in_pack(&pack, "assets/again.cast").map(|(bytes, _)| bytes), Some(longer.clone()));
    assert!(fresh.source().contains("<Terminal src=\"assets/again.cast\""));

    // Save As a folder from the pack: both are there, and read from there.
    let unpacked = work.path().join("unpacked");
    let saved = fresh.save_as_job(SaveTarget::Folder(unpacked.clone())).run().unwrap();
    fresh.saved(saved);
    assert_eq!(std::fs::read(unpacked.join("assets/demo-run.cast")).unwrap(), eighty());
    assert_eq!(std::fs::read(unpacked.join("assets/again.cast")).unwrap(), longer);
    assert!(plays(&mut fresh));
    assert!(plays(&mut Document::open(&unpacked, Fonts::bundled_only()).unwrap()));

    // A slide copied from a talk in a pack brings its recording, unpacked.
    let packed = Document::open_packed(&pack, Fonts::bundled_only()).unwrap();
    let from_pack = open_untitled_talk(cx, packed, work.path());
    click_on(&from_pack, cx, stagepack_studio::slide_strip::strip_row_id(0));
    from_pack.press(cx, "ctrl-c");
    let into = open(cx, "# B\n");
    click_on(&into, cx, stagepack_studio::slide_strip::strip_row_id(0));
    into.press(cx, "ctrl-v");
    cx.run_until_parked();
    into.settle(cx);
    assert!(has_file(&into, cx, "assets/demo-run.cast"), "{}", into.source(cx));
    assert_eq!(problem_of(&into, cx, 1), None, "it plays there");
}

#[gpui_kit::test]
fn a_hand_written_terminal_reads_or_says_why(cx: &mut TestAppContext) {
    use stagepack_studio::inspector::INSPECTOR_TERMINAL_PROBLEM_ID;
    let _alone = alone();
    let place = "x=\"100\" y=\"100\" w=\"640\" h=\"360\"";
    let written = [
        format!("<Terminal {place} />"),
        format!("<Terminal src=\"../outside.cast\" {place} />"),
        format!("<Terminal src=\"assets/clip.mp4\" {place} />"),
        format!("<Terminal src=\"assets/gone.cast\" {place} />"),
    ];
    let slides: Vec<String> = written.iter().enumerate().map(|(at, tag)| format!("# Slide {at}\n\n{tag}\n\n{BOX}\n")).collect();
    let source = slides.join("\n---\n\n");
    // The talk in a folder of its own, with a recording beside it, outside it, which is never read.
    let beside = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir_in(beside.path()).unwrap();
    std::fs::write(folder.path().join("slides.md"), &source).unwrap();
    std::fs::create_dir(folder.path().join("assets")).unwrap();
    std::fs::write(folder.path().join("assets/run.cast"), eighty()).unwrap();
    std::fs::write(folder.path().join("assets/clip.mp4"), b"\0\0\0\x18ftypmp42 not really a video").unwrap();
    std::fs::write(beside.path().join("outside.cast"), eighty()).unwrap();
    let typing = open_folder(cx, folder, None);
    let why = [
        slides_engine::TERMINAL_NO_FILE.to_string(),
        slides_terminal::CantPlay::CouldNotRead.to_string(),
        NOT_A_RECORDING.to_string(),
        slides_terminal::CantPlay::CouldNotRead.to_string(),
    ];
    for (at, (tag, why)) in written.iter().zip(&why).enumerate() {
        if at > 0 {
            typing.press(cx, "pagedown");
        }
        assert_eq!(typing.current(cx), at);
        assert_eq!(problem_of(&typing, cx, at).as_deref(), Some(why.as_str()), "{tag}");
        let notices = typing.document(cx).update(cx, |document, _| document.notices());
        assert!(notices.iter().any(|notice| notice.contains(why.as_str())), "{tag}: {notices:?}");
        select_terminal(&typing, cx);
        assert_eq!(said_by_form(&typing, cx, INSPECTOR_TERMINAL_PROBLEM_ID), *why, "{tag}");
        // Moved, only its place changes; deleted, only its bytes go; Undo puts each back exactly.
        let before = typing.source(cx);
        typing.press(cx, "right");
        let moved = typing.source(cx);
        let start = before.find(tag.as_str()).unwrap();
        assert_eq!(moved[..start], before[..start], "{tag}");
        let end = before.len() - start - tag.len();
        assert_eq!(moved[moved.len() - end..], before[before.len() - end..], "{tag}");
        assert_ne!(moved, before, "{tag}");
        typing.press(cx, "delete");
        assert_eq!(typing.source(cx), before.replacen(&format!("{tag}\n\n"), "", 1), "{tag}");
        typing.press(cx, "ctrl-z");
        assert_eq!(typing.source(cx), moved, "{tag}");
        typing.press(cx, "ctrl-z");
        assert_eq!(typing.source(cx), before, "{tag}");
    }
}

/// A talk whose first slide has `first` boxes of the recording and whose second has `second`.
fn many_talk(first: usize, second: usize) -> String {
    let boxes = |count: usize| {
        (0..count)
            .map(|at| format!("<Terminal src=\"assets/run.cast\" x=\"{}\" y=\"{}\" w=\"300\" h=\"150\" />", 10 + (at % 5) * 380, 10 + (at / 5) * 200))
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    format!("# Many\n\n{}\n\n---\n\n# More\n\n{}\n", boxes(first), boxes(second))
}

/// The layers of the recordings on slide `slide` at its start, in their order.
fn recording_layers(document: &Entity<Document>, slide: usize, cx: &mut TestAppContext) -> Vec<usize> {
    let mut layers: Vec<usize> = document.update(cx, |document, _| document.terminals_at(slide, Moment::at(0, 0.0))).iter().map(|terminal| terminal.layer).collect();
    layers.sort();
    layers
}

#[gpui_kit::test]
fn no_more_than_four_recordings_play_at_once_and_their_file_is_read_once(cx: &mut TestAppContext) {
    let _alone = alone();
    let read = replay::recordings_read();
    let show = rehearse(cx, &many_talk(25, 3), &marked());
    let canvas = show.canvas(cx);
    until(cx, show.audience, "four pictures", |cx| pictures(cx, &canvas).iter().flatten().count() == replay::MOST_PLAYING);
    for_a_while(cx, show.audience, 200);
    assert_eq!(replay::threads_running(), replay::MOST_PLAYING, "four play, and the next slide's wait");
    // The first four by their order on the slide; the rest show their last screens.
    let mut playing = cx.read(|cx| canvas.read(cx).playing_recordings());
    playing.sort();
    let layers = recording_layers(&show.document, 0, cx);
    assert_eq!(layers.len(), 25);
    assert_eq!(playing, layers[..replay::MOST_PLAYING]);
    assert_eq!(replay::recordings_read() - read, 1, "the four share what's read of their file");
}

#[gpui_kit::test]
fn the_next_slides_recordings_are_readied_within_the_same_four(cx: &mut TestAppContext) {
    let _alone = alone();
    let show = rehearse(cx, &many_talk(2, 5), &running(1));
    let canvas = show.canvas(cx);
    until(cx, show.audience, "two pictures", |cx| pictures(cx, &canvas).iter().flatten().count() == 2);
    for_a_while(cx, show.audience, 200);
    assert_eq!(replay::threads_running(), replay::MOST_PLAYING, "two play, and two of the next slide's are readied");
    // On the next slide, its first four play.
    show.press(cx, "right");
    assert_eq!(show.step(cx).0, 1);
    let canvas = show.canvas(cx);
    until(cx, show.audience, "four pictures", |cx| pictures(cx, &canvas).iter().flatten().count() == replay::MOST_PLAYING);
    assert!(replay::threads_running() <= replay::MOST_PLAYING, "{}", replay::threads_running());
    let mut playing = cx.read(|cx| canvas.read(cx).playing_recordings());
    playing.sort();
    assert_eq!(playing, recording_layers(&show.document, 1, cx)[..replay::MOST_PLAYING]);
}

fn window_open(cx: &TestAppContext, handle: AnyWindowHandle) -> bool {
    cx.read(|cx| cx.windows().iter().any(|window| window.window_id() == handle.window_id()))
}

fn progress_said(typing: &Typing, cx: &mut TestAppContext) -> Option<String> {
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find(workspace_view::PROGRESS_ID).and_then(|found| found.label().map(str::to_string))
    })
}

#[gpui_kit::test]
fn recordings_are_checked_one_at_a_time_saying_so(cx: &mut TestAppContext) {
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let typing = keeping_editor(cx, PLAIN_TALK, work.path());
    workspace_view::hold_recording_checks_for_tests(true);
    let paths = vec![outside(work.path(), "one.cast", &eighty()), outside(work.path(), "two.cast", &three_minutes())];
    choose_recordings(&typing, cx, paths);
    assert_eq!(workspace_view::recording_checks_running(), 1);
    assert_eq!(progress_said(&typing, cx).as_deref(), Some("Reading the terminal recording… 50%"));
    wait(cx, 50);
    assert_eq!(workspace_view::recording_checks_running(), 1, "one at a time");
    assert_eq!(typing.source(cx), PLAIN_TALK);
    workspace_view::hold_recording_checks_for_tests(false);
    wait(cx, 10);
    typing.settle(cx);
    assert_eq!(typing.source(cx).matches("<Terminal").count(), 2, "both added");
    assert_eq!(workspace_view::recording_checks_running(), 0);
    assert_eq!(progress_said(&typing, cx), None);
    let source = typing.source(cx);
    assert!(source.find("assets/one.cast") < source.find("assets/two.cast"), "in the order chosen: {source}");
}

#[gpui_kit::test]
fn cancel_stops_the_recording_being_checked_and_those_waiting(cx: &mut TestAppContext) {
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let typing = keeping_editor(cx, PLAIN_TALK, work.path());
    workspace_view::hold_recording_checks_for_tests(true);
    choose_recordings(&typing, cx, vec![outside(work.path(), "one.cast", &eighty()), outside(work.path(), "two.cast", &three_minutes())]);
    assert_eq!(workspace_view::recording_checks_running(), 1);
    click_on(&typing, cx, workspace_view::CANCEL_PROGRESS_ID);
    wait(cx, 10);
    workspace_view::hold_recording_checks_for_tests(false);
    wait(cx, 10);
    typing.settle(cx);
    assert_eq!(workspace_view::recording_checks_running(), 0);
    assert_eq!(progress_said(&typing, cx), None);
    assert_eq!(typing.source(cx), PLAIN_TALK, "neither was added");
}

#[gpui_kit::test]
fn closing_the_window_stops_a_recording_being_checked(cx: &mut TestAppContext) {
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let typing = keeping_editor(cx, PLAIN_TALK, work.path());
    workspace_view::hold_recording_checks_for_tests(true);
    choose_recordings(&typing, cx, vec![outside(work.path(), "one.cast", &eighty()), outside(work.path(), "two.cast", &eighty())]);
    assert_eq!(workspace_view::recording_checks_running(), 1);
    let document = typing.document(cx);
    let handle = typing.handle;
    assert!(!VisualTestContext::from_window(handle, cx).simulate_close(), "it waits for the check it stops");
    cx.run_until_parked();
    assert!(window_open(cx, handle), "until the check has stopped");
    // The check sees it's to stop, though it's still held, and the window closes once it has,
    // the other never checked.
    wait(cx, 10);
    assert_eq!(workspace_view::recording_checks_running(), 0);
    assert!(!window_open(cx, handle), "closed once the check stopped");
    assert_eq!(document.read_with(cx, |document, _| document.source().to_string()), PLAIN_TALK, "nothing was added");
}

/// A recording of the largest screen erased again and again in nine colours: more work to play
/// than any recording is played with.
fn too_costly() -> Vec<u8> {
    let erases: String = ["41", "42", "43", "44", "45", "46", "47", "100", "101"].iter().map(|bg| format!("\u{1b}[{bg}m\u{1b}[2J")).collect();
    let output = erases.repeat(1_000_000 / erases.len());
    let lines: Vec<(f64, &str, &str)> = (0..3).map(|at| (f64::from(at) * 0.1, "o", output.as_str())).collect();
    slides_terminal::make_test_cast(500, 200, &lines)
}

#[gpui_kit::test]
fn a_recording_too_costly_to_play_is_refused_saying_why(cx: &mut TestAppContext) {
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let typing = keeping_editor(cx, PLAIN_TALK, work.path());
    let cast = too_costly();
    let recording = slides_terminal::read(&cast).unwrap();
    assert!(slides_terminal::Player::new(std::sync::Arc::new(recording)).last().is_err(), "too costly to play");
    choose_recordings(&typing, cx, vec![outside(work.path(), "costly.cast", &cast)]);
    assert_eq!(workspace_view::recording_checks_running(), 0);
    assert_eq!(typing.status(cx), Some(slides_terminal::CantPlay::TooLarge.to_string()));
    assert_eq!(typing.source(cx), PLAIN_TALK);
    assert!(!has_file(&typing, cx, "assets/costly.cast"));
}

#[gpui_kit::test]
fn a_recording_that_cant_be_read_says_so_in_its_own_words(cx: &mut TestAppContext) {
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let typing = keeping_editor(cx, PLAIN_TALK, work.path());
    // A folder named as a recording can't be read as one.
    let folder = work.path().join("folder.cast");
    std::fs::create_dir(&folder).unwrap();
    choose_recordings(&typing, cx, vec![folder]);
    assert_eq!(workspace_view::recording_checks_running(), 0);
    assert_eq!(typing.status(cx), Some(slides_terminal::CantPlay::CouldNotRead.to_string()));
    assert_eq!(typing.source(cx), PLAIN_TALK);
}
