//! A video on a slide: its still, playing it in the editor when asked and in the show when its
//! slide appears, and every way it stops, with its thread and its pictures let go of.
//!
//! Each test makes its own videos, whose pictures are colours known. They run one at a time, as
//! some count the process's threads, which another test's videos would add to.

#[path = "support/typing.rs"]
mod typing;

use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use typing::*;

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, Modifiers, RenderImage, TestAppContext, VisualTestContext, point, px};
use slides_engine::{Fonts, stills_decoded_on_this_thread};
use stagepack_studio::arrange::Handle;
use stagepack_studio::document::Document;
use stagepack_studio::present_view::{AudienceView, Presentation, PresenterView, start_show};
use stagepack_studio::saving::SaveTarget;
use stagepack_studio::slide_canvas::SlideCanvas;
use stagepack_studio::video_player::decodings_started_and_ended_aside;
use stagepack_studio::slide_editor::{VIDEO_PLAY_ID, VIDEO_STOP_ID, handle_id};
use stagepack_studio::workspace_view;

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn alone() -> MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The colours the pictures of a made video take in turn, as blue, green, red and alpha.
const COLOURS: [[u8; 4]; 6] = [
    [0, 0, 255, 255],
    [0, 255, 0, 255],
    [255, 0, 0, 255],
    [0, 255, 255, 255],
    [255, 255, 0, 255],
    [255, 0, 255, 255],
];

const WIDTH: u32 = 64;
const HEIGHT: u32 = 36;

/// A video of `frames` pictures, `per_second` a second, each the colour of [`COLOURS`] its
/// place gives it, round again after the sixth.
fn clip(frames: u32, per_second: u32) -> Vec<u8> {
    slides_video::make_test_video(WIDTH, HEIGHT, frames, per_second, |at| COLOURS[at as usize % COLOURS.len()].repeat((WIDTH * HEIGHT) as usize))
}

const VIDEO: &str = "<Video src=\"assets/clip.mp4\" x=\"100\" y=\"100\" w=\"640\" h=\"360\" />";
const BOX: &str = "<Box x=\"1000\" y=\"600\" w=\"300\" h=\"200\" fill=\"#888888\" />";

/// One slide with the video and a box on it, then another.
fn talk() -> String {
    format!("# Video\n\n{VIDEO}\n\n{BOX}\n\n---\n\n# Two\n")
}

/// A folder holding a talk of `source`, and `video` as its `assets/clip.mp4`.
fn talk_folder(source: &str, video: &[u8]) -> tempfile::TempDir {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("slides.md"), source).unwrap();
    std::fs::create_dir(folder.path().join("assets")).unwrap();
    std::fs::write(folder.path().join("assets/clip.mp4"), video).unwrap();
    folder
}

/// The editor on a talk of `source` with `video`, its videos' stills made.
fn editor(cx: &mut TestAppContext, source: &str, video: &[u8]) -> Typing {
    let typing = open_folder(cx, talk_folder(source, video), None);
    stills_made(&typing, cx);
    typing
}

/// Waits, in real time, for `done`, delivering the frames the window asks for and drawing it
/// meanwhile, as decoding runs on threads of its own.
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
        assert!(began.elapsed() < Duration::from_secs(20), "waited too long for {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Waits for every video's still to be made, off the window's thread, and taken in.
fn stills_made(typing: &Typing, cx: &mut TestAppContext) {
    let document = typing.document(cx);
    until(cx, typing.handle, "the stills", |cx| document.read_with(cx, |document, _| !document.stills_coming()));
    typing.settle(cx);
}

fn editor_canvas(typing: &Typing, cx: &TestAppContext) -> Entity<SlideCanvas> {
    cx.read(|cx| typing.view.read(cx).slide_canvas().clone())
}

/// What each video on `canvas` shows now.
fn frames(cx: &TestAppContext, canvas: &Entity<SlideCanvas>) -> Vec<Option<std::sync::Arc<RenderImage>>> {
    cx.read(|cx| canvas.read(cx).video_frames())
}

fn frame_times(cx: &TestAppContext, canvas: &Entity<SlideCanvas>) -> Vec<Option<f64>> {
    cx.read(|cx| canvas.read(cx).video_frame_times())
}

fn threads_of(cx: &TestAppContext, canvas: &Entity<SlideCanvas>) -> Vec<String> {
    cx.read(|cx| canvas.read(cx).video_threads())
}

fn queued(cx: &TestAppContext, canvas: &Entity<SlideCanvas>) -> Vec<usize> {
    cx.read(|cx| canvas.read(cx).videos_queued())
}

fn images_made_and_released(cx: &TestAppContext, canvas: &Entity<SlideCanvas>) -> (usize, usize) {
    cx.read(|cx| canvas.read(cx).video_images())
}

/// The colour in the middle of `image`, as red, green and blue.
fn middle_colour(image: &RenderImage) -> (u8, u8, u8) {
    let size = image.size(0);
    let (width, height) = (size.width.0 as usize, size.height.0 as usize);
    let bytes = image.as_bytes(0).expect("one picture");
    let at = (height / 2 * width + width / 2) * 4;
    (bytes[at + 2], bytes[at + 1], bytes[at])
}

/// Whether `image` shows colour `n` of [`COLOURS`], give or take what encoding does to it.
fn shows(image: &RenderImage, n: usize) -> bool {
    let [b, g, r, _] = COLOURS[n % COLOURS.len()];
    let (er, eg, eb) = middle_colour(image);
    er.abs_diff(r) <= 16 && eg.abs_diff(g) <= 16 && eb.abs_diff(b) <= 16
}

/// The threads of the process named as one of `names`.
fn threads_named(names: &[String]) -> usize {
    std::fs::read_dir("/proc/self/task")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|task| std::fs::read_to_string(task.path().join("comm")).ok())
        .filter(|name| names.iter().any(|wanted| name.trim_end() == wanted))
        .count()
}

/// How many threads the process has.
fn thread_count() -> usize {
    std::fs::read_dir("/proc/self/task").map(|tasks| tasks.count()).unwrap_or(0)
}

/// Selects the video, by a click a little way in from its corner, away from its button.
fn select_video(typing: &Typing, cx: &mut TestAppContext) {
    let video = thing(typing, cx, "<Video");
    let at = typing.at(cx, video.rect.x + video.rect.w / 5., video.rect.y + video.rect.h / 5.);
    typing.click(cx, at);
    assert_eq!(selected(typing, cx).map(|(_, source)| source), Some(video.source));
}

/// Selects the video and plays it, and waits for its first picture to show.
fn play_selected(typing: &Typing, cx: &mut TestAppContext) -> Entity<SlideCanvas> {
    select_video(typing, cx);
    click_on(typing, cx, VIDEO_PLAY_ID);
    let canvas = editor_canvas(typing, cx);
    until(cx, typing.handle, "the first picture", |cx| frames(cx, &canvas).first().is_some_and(Option::is_some));
    canvas
}

/// Moves the test clock on by `millis`, and delivers the frame the window asked for.
fn next_frame(cx: &mut TestAppContext, window: AnyWindowHandle, millis: u64) {
    cx.executor().advance_clock(Duration::from_millis(millis));
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.simulate_next_frame(cx);
        window.render_frame(cx);
    })
    .unwrap();
}

/// Waits until each video on `canvas` has `n` pictures decoded ahead, or all it has.
fn decoded_ahead(cx: &mut TestAppContext, window: AnyWindowHandle, canvas: &Entity<SlideCanvas>, n: usize) {
    until(cx, window, "pictures decoded ahead", |cx| queued(cx, canvas).iter().all(|queued| *queued >= n));
}

/// Waits, in real time, for `names`'s threads to end and the process to be back to `before`
/// threads at most, and says how long that took.
fn threads_end(cx: &mut TestAppContext, names: &[String], before: usize) -> Duration {
    let began = Instant::now();
    loop {
        cx.run_until_parked();
        if threads_named(names) == 0 && thread_count() <= before {
            return began.elapsed();
        }
        assert!(began.elapsed() < Duration::from_secs(5), "{names:?} still running");
        std::thread::sleep(Duration::from_millis(2));
    }
}

// In the editor.

#[gpui_kit::test]
fn play_shows_the_videos_frames_in_turn(cx: &mut TestAppContext) {
    let _alone = alone();
    let typing = editor(cx, &talk(), &clip(6, 10));
    let canvas = play_selected(&typing, cx);
    assert!(shows(frames(cx, &canvas)[0].as_ref().unwrap(), 0));
    // Playing, the button says Stop.
    assert!(bounds_of(&typing, cx, VIDEO_STOP_ID).is_some());
    decoded_ahead(cx, typing.handle, &canvas, 5);
    for at in 1..6 {
        next_frame(cx, typing.handle, 100);
        let shown = frames(cx, &canvas)[0].clone().expect("a picture shows");
        assert!(shows(&shown, at), "picture {at}: {:?}", middle_colour(&shown));
    }
    // At its end its last picture stays, and it can be played again.
    next_frame(cx, typing.handle, 100);
    assert!(shows(frames(cx, &canvas)[0].as_ref().unwrap(), 5));
    assert!(bounds_of(&typing, cx, VIDEO_PLAY_ID).is_some());
}

#[gpui_kit::test]
fn stop_goes_back_to_the_still(cx: &mut TestAppContext) {
    let _alone = alone();
    let typing = editor(cx, &talk(), &clip(30, 10));
    let canvas = play_selected(&typing, cx);
    let still = cx.read(|cx| canvas.read(cx).images());
    assert!(bounds_of(&typing, cx, VIDEO_PLAY_ID).is_none());
    click_on(&typing, cx, VIDEO_STOP_ID);
    assert!(frames(cx, &canvas).is_empty(), "no picture over the still");
    assert!(bounds_of(&typing, cx, VIDEO_PLAY_ID).is_some(), "Play again");
    let after = cx.read(|cx| canvas.read(cx).images());
    assert!(still.iter().zip(&after).all(|(a, b)| a.id == b.id), "the still is the layer it was");
    // Still selected.
    assert!(selected(&typing, cx).is_some());
}

#[gpui_kit::test]
fn space_plays_and_stops_a_selected_video(cx: &mut TestAppContext) {
    let _alone = alone();
    let typing = editor(cx, &talk(), &clip(30, 10));
    let canvas = editor_canvas(&typing, cx);
    select_video(&typing, cx);
    typing.press(cx, "space");
    assert_eq!(cx.read(|cx| canvas.read(cx).playing_videos()).len(), 1, "playing");
    assert!(bounds_of(&typing, cx, VIDEO_STOP_ID).is_some());
    typing.press(cx, "space");
    assert!(cx.read(|cx| canvas.read(cx).playing_videos()).is_empty(), "stopped");
    assert!(typing.editing(cx).is_none(), "a video takes no typing");
    assert_eq!(typing.source(cx), talk(), "nothing written");
    // A box selected still starts typing with Space.
    let shape = thing(&typing, cx, "<Box");
    let at = middle(&typing, cx, &shape);
    typing.click(cx, at);
    typing.press(cx, "space");
    assert!(typing.editing(cx).is_some(), "the box is typed in");
}

/// Plays the video on a talk of `source`, does `leave`, and checks its thread ends within 300 ms
/// and, if the window's still open, that no player is left.
fn left_by(cx: &mut TestAppContext, how: &str, source: &str, leave: impl FnOnce(Typing, &mut TestAppContext) -> Option<Typing>) {
    let typing = editor(cx, source, &clip(300, 30));
    let before = thread_count();
    let names = {
        let canvas = play_selected(&typing, cx);
        threads_of(cx, &canvas)
    };
    assert_eq!(names.len(), 1, "{how}: one thread decodes");
    assert_eq!(threads_named(&names), 1, "{how}: and it's running");
    let kept = leave(typing, cx);
    let took = threads_end(cx, &names, before);
    assert!(took <= Duration::from_millis(300), "{how}: its thread took {took:?} to end");
    if let Some(typing) = kept {
        let canvas = editor_canvas(&typing, cx);
        assert!(frames(cx, &canvas).is_empty(), "{how}: no player is left");
        assert!(bounds_of(&typing, cx, VIDEO_STOP_ID).is_none(), "{how}");
    }
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

#[gpui_kit::test]
fn every_way_of_leaving_stops_the_decoding(cx: &mut TestAppContext) {
    let _alone = alone();
    let talk = talk();
    left_by(cx, "selecting something else", &talk, |typing, cx| {
        let shape = thing(&typing, cx, "<Box");
        let at = middle(&typing, cx, &shape);
        typing.click(cx, at);
        Some(typing)
    });
    // Its heading reaches past the video, to be clicked where the video isn't.
    let long_heading = talk.replace("# Video", "# Video, with a heading long enough to reach past it");
    left_by(cx, "a click in the slide's words, to type there", &long_heading, |typing, cx| {
        typing.click_text(cx, 0, pos(0, 48));
        assert!(typing.editing(cx).is_some(), "typing in the heading");
        let canvas = editor_canvas(&typing, cx);
        assert!(cx.read(|cx| canvas.read(cx).playing_videos()).is_empty(), "it stops at once");
        Some(typing)
    });
    left_by(cx, "another slide", &talk, |typing, cx| {
        typing.press(cx, "pagedown");
        assert_eq!(typing.current(cx), 1);
        Some(typing)
    });
    left_by(cx, "deleting the video", &talk, |typing, cx| {
        typing.press(cx, "delete");
        assert!(!typing.source(cx).contains("<Video"));
        Some(typing)
    });
    left_by(cx, "a change to the talk from outside", &talk, |typing, cx| {
        let changed = talk.replace("# Video", "# Changed");
        typing.view.update(cx, |view, cx| view.reload_for_test(Some(changed), cx));
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
    // Undo of the video's insert.
    let typing = editor(cx, "# Video\n\n---\n\n# Two\n", &clip(300, 30));
    typing.document(cx).update(cx, |document, cx| {
        document.insert_tag("Insert video", "Video", &[("src", "assets/clip.mp4")], None).unwrap();
        cx.notify();
    });
    stills_made(&typing, cx);
    let before = thread_count();
    let names = {
        let canvas = play_selected(&typing, cx);
        threads_of(cx, &canvas)
    };
    typing.press(cx, "ctrl-z");
    assert!(!typing.source(cx).contains("<Video"), "undone");
    let took = threads_end(cx, &names, before);
    assert!(took <= Duration::from_millis(300), "undo: its thread took {took:?} to end");
    assert!(frames(cx, &editor_canvas(&typing, cx)).is_empty());
}

#[gpui_kit::test]
fn frames_are_released(cx: &mut TestAppContext) {
    let _alone = alone();
    let source = talk().replace(VIDEO, &VIDEO.replace(" />", " loop />"));
    let typing = editor(cx, &source, &clip(20, 30));
    let canvas = play_selected(&typing, cx);
    let layers = cx.read(|cx| canvas.read(cx).images().len());
    for _ in 0..200 {
        decoded_ahead(cx, typing.handle, &canvas, 1);
        next_frame(cx, typing.handle, 34);
        let (made, released) = images_made_and_released(cx, &canvas);
        assert_eq!(made, released + 1, "only the picture shown is held");
    }
    let (made, _) = images_made_and_released(cx, &canvas);
    assert!(made >= 200, "{made} pictures shown");
    click_on(&typing, cx, VIDEO_STOP_ID);
    let (made, released) = images_made_and_released(cx, &canvas);
    assert_eq!(made, released, "every picture is released");
    assert_eq!(cx.read(|cx| canvas.read(cx).images().len()), layers, "the layers are as they were");
}

#[gpui_kit::test]
fn a_late_frame_is_skipped(cx: &mut TestAppContext) {
    let _alone = alone();
    let typing = editor(cx, &talk(), &clip(30, 10));
    let canvas = play_selected(&typing, cx);
    decoded_ahead(cx, typing.handle, &canvas, 11);
    let (made, released) = images_made_and_released(cx, &canvas);
    let skipped = cx.read(|cx| canvas.read(cx).video_pictures_skipped());
    next_frame(cx, typing.handle, 1000);
    assert_eq!(frame_times(cx, &canvas), vec![Some(1.0)], "the picture due, ten on");
    assert!(shows(frames(cx, &canvas)[0].as_ref().unwrap(), 10));
    // Only the one due was made to show, and the one it replaced was released.
    assert_eq!(images_made_and_released(cx, &canvas), (made + 1, released + 1));
    // The nine between were skipped, and counted.
    assert_eq!(cx.read(|cx| canvas.read(cx).video_pictures_skipped()), skipped + 9);
}

#[gpui_kit::test]
fn a_video_in_a_turned_box_plays_turned(cx: &mut TestAppContext) {
    let _alone = alone();
    let source = talk().replace(VIDEO, &VIDEO.replace(" />", " rotate=\"30\" />"));
    let typing = editor(cx, &source, &clip(30, 10));
    let canvas = play_selected(&typing, cx);
    let shown = frames(cx, &canvas)[0].clone().unwrap();
    let scale = cx.read(|cx| canvas.read(cx).scale()).unwrap();
    // Drawn turned, as the layer is: as big as the turned box, not as the video.
    let (sin, cos) = 30f32.to_radians().sin_cos();
    let (w, h) = (640. * cos + 360. * sin, 640. * sin + 360. * cos);
    let size = shown.size(0);
    assert!((size.width.0 as f32 - w * scale).abs() <= 3., "{size:?} for {w} by {h} at {scale}");
    assert!((size.height.0 as f32 - h * scale).abs() <= 3., "{size:?} for {w} by {h} at {scale}");
    assert!(shows(&shown, 0), "{:?}", middle_colour(&shown));
    // Its corners are clear, as the turned box doesn't reach them.
    assert_eq!(shown.as_bytes(0).unwrap()[3], 0);
}

#[gpui_kit::test]
fn a_talk_of_twenty_videos_sits_idle(cx: &mut TestAppContext) {
    let _alone = alone();
    let source: Vec<String> = (0..20).map(|at| format!("# Slide {at}\n\n{VIDEO}\n")).collect();
    let typing = editor(cx, &source.join("\n---\n\n"), &clip(30, 10));
    let canvas = editor_canvas(&typing, cx);
    let idle = |typing: &Typing, cx: &mut TestAppContext, when: &str| {
        typing.settle(cx);
        cx.update_window(typing.handle, |_, window, cx| window.simulate_next_frame(cx)).unwrap();
        let threads = thread_count();
        let (redrawn, _watching) = redraws(cx, &canvas);
        let mut frames_asked = 0;
        for _ in 0..80 {
            cx.executor().advance_clock(Duration::from_millis(250));
            cx.run_until_parked();
            frames_asked += cx.update_window(typing.handle, |_, window, cx| window.simulate_next_frame(cx)).unwrap();
        }
        assert_eq!(redrawn.get(), 0, "{when}: nothing redraws");
        assert_eq!(frames_asked, 0, "{when}: no frame is asked for");
        assert!(thread_count() <= threads, "{when}: no thread more");
    };
    idle(&typing, cx, "none played");
    let before = thread_count();
    let names = {
        let canvas = play_selected(&typing, cx);
        threads_of(cx, &canvas)
    };
    click_on(&typing, cx, VIDEO_STOP_ID);
    threads_end(cx, &names, before);
    idle(&typing, cx, "one played and stopped");
}

#[gpui_kit::test]
fn a_video_that_cannot_play_says_why_when_selected(cx: &mut TestAppContext) {
    let _alone = alone();
    let typing = editor(cx, &talk(), b"these are words, not pictures");
    select_video(&typing, cx);
    assert_eq!(typing.status(cx).as_deref(), Some("Video selected. This file isn't a video."));
    // Its panel shows, and Play plays nothing.
    click_on(&typing, cx, VIDEO_PLAY_ID);
    let canvas = editor_canvas(&typing, cx);
    until(cx, typing.handle, "opening it", |cx| cx.read(|cx| canvas.read(cx).playing_videos()).is_empty());
    assert!(frames(cx, &canvas).iter().all(Option::is_none));
    assert_eq!(threads_of(cx, &canvas), Vec::<String>::new());
}

#[gpui_kit::test]
fn a_video_moves_resizes_and_keeps_its_shape_like_a_picture(cx: &mut TestAppContext) {
    let _alone = alone();
    let typing = editor(cx, &talk(), &clip(3, 10));
    assert!(thing(&typing, cx, "<Video").keeps_shape);
    select_video(&typing, cx);
    // Moved by a drag from a little way in, away from its button.
    let video = thing(&typing, cx, "<Video");
    let from = typing.at(cx, video.rect.x + video.rect.w / 4., video.rect.y + video.rect.h / 4.);
    typing.drag(cx, from, from + point(px(40.), px(20.)));
    let moved = thing(&typing, cx, "<Video");
    assert!(moved.rect.x > video.rect.x && moved.rect.y > video.rect.y, "{:?} from {:?}", moved.rect, video.rect);
    assert_eq!((moved.rect.w, moved.rect.h), (640., 360.));
    // Resized by a corner, it keeps its shape.
    let corner = bounds_of(&typing, cx, gpui_kit::SharedString::from(handle_id(Handle::BottomRight))).expect("a handle").center();
    typing.mouse_down(cx, corner, 1, Modifiers::default());
    typing.move_held(cx, corner + point(px(30.), px(5.)), Modifiers::default());
    typing.move_held(cx, corner + point(px(60.), px(10.)), Modifiers::default());
    typing.mouse_up(cx, corner + point(px(60.), px(10.)), 1, Modifiers::default());
    let resized = thing(&typing, cx, "<Video");
    assert!(resized.rect.w > 640., "{:?}", resized.rect);
    assert!((resized.rect.w / resized.rect.h - 16. / 9.).abs() < 0.01, "{:?}", resized.rect);
}

#[gpui_kit::test]
fn a_video_playing_in_a_packed_talk_lets_go_of_the_pack_for_a_save_in_place(cx: &mut TestAppContext) {
    let _alone = alone();
    let folder = talk_folder(&talk(), &clip(300, 30));
    let work = tempfile::tempdir().unwrap();
    let pack = work.path().join("talk.sptx");
    let saved = Document::open(folder.path(), Fonts::bundled_only()).unwrap().save_as_job(SaveTarget::Packed(pack.clone())).run();
    assert!(saved.is_ok());
    let document = Document::open_packed(&pack, Fonts::bundled_only()).unwrap();
    let typing = open_untitled_talk(cx, document, work.path());
    stills_made(&typing, cx);
    let canvas = play_selected(&typing, cx);
    let names = threads_of(cx, &canvas);
    let held = |typing: &Typing, cx: &TestAppContext| typing.document(cx).read_with(cx, |document, _| document.readers_held());
    assert_eq!(held(&typing, cx), 1, "the video reads the pack");
    typing.press(cx, "ctrl-s");
    wait(cx, 500);
    typing.settle(cx);
    assert_eq!(typing.status(cx).as_deref(), Some("Saved talk.sptx."));
    // Its reader was let go of for the save, and the video plays on, read from the new pack.
    assert_eq!(threads_named(&names), 0, "the old reading ended");
    until(cx, typing.handle, "playing again", |cx| threads_of(cx, &canvas).len() == 1 && frames(cx, &canvas)[0].is_some());
    assert_ne!(threads_of(cx, &canvas), names);
    assert_eq!(held(&typing, cx), 1, "one reader, of the pack as saved");
    decoded_ahead(cx, typing.handle, &canvas, 2);
    let at = frame_times(cx, &canvas)[0].unwrap();
    next_frame(cx, typing.handle, 34);
    assert!(frame_times(cx, &canvas)[0].unwrap() > at, "it goes on");
}

// In the show.

/// A show of a talk of `source` with `video`, rehearsed, in both its windows.
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
                cx.update_window(window, |_, window, cx| window.render_frame(cx)).unwrap();
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

    /// Waits for every video on the audience's slide to show its first picture.
    fn first_pictures(&self, cx: &mut TestAppContext, count: usize) -> Entity<SlideCanvas> {
        let canvas = self.canvas(cx);
        until(cx, self.audience, "the first pictures", |cx| {
            let frames = frames(cx, &canvas);
            frames.len() == count && frames.iter().all(Option::is_some)
        });
        canvas
    }
}

fn rehearse(cx: &mut TestAppContext, source: &str, video: &[u8]) -> Show {
    rehearse_with(cx, source, video, &[])
}

/// [`rehearse`], with `more` files in the talk's folder too.
fn rehearse_with(cx: &mut TestAppContext, source: &str, video: &[u8], more: &[(&str, Vec<u8>)]) -> Show {
    cx.update(|cx| {
        gpui_kit::init(cx);
        workspace_view::bind_keys(cx);
    });
    let folder = talk_folder(source, video);
    for (path, bytes) in more {
        std::fs::write(folder.path().join(path), bytes).unwrap();
    }
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

#[gpui_kit::test]
fn in_the_show_it_starts_with_its_slide_and_again_when_shown_again(cx: &mut TestAppContext) {
    let _alone = alone();
    let show = rehearse(cx, &talk(), &clip(300, 30));
    let canvas = show.first_pictures(cx, 1);
    assert!(shows(frames(cx, &canvas)[0].as_ref().unwrap(), 0));
    let first = threads_of(cx, &canvas);
    decoded_ahead(cx, show.audience, &canvas, 3);
    for _ in 0..3 {
        next_frame(cx, show.audience, 34);
    }
    assert!(frame_times(cx, &canvas)[0].unwrap() > 0.0, "it plays");
    let before = thread_count();
    show.press(cx, "right");
    assert!(frames(cx, &show.canvas(cx)).is_empty(), "nothing plays on the next slide");
    threads_end(cx, &first, before);
    show.press(cx, "left");
    let canvas = show.first_pictures(cx, 1);
    assert_eq!(frame_times(cx, &canvas), vec![Some(0.0)], "from the start again");
    assert_ne!(threads_of(cx, &canvas), first);
}

#[gpui_kit::test]
fn it_waits_for_its_click(cx: &mut TestAppContext) {
    let _alone = alone();
    let source = talk().replace(VIDEO, &VIDEO.replace(" />", " click=\"1\" />"));
    let show = rehearse(cx, &source, &clip(30, 10));
    let canvas = show.canvas(cx);
    show.settle(cx);
    assert!(frames(cx, &canvas).is_empty(), "not before its click");
    show.press(cx, "right");
    assert_eq!(show.show.read_with(cx, |show, _| show.place().step), 1);
    let canvas = show.first_pictures(cx, 1);
    assert!(shows(frames(cx, &canvas)[0].as_ref().unwrap(), 0));
    // Back before its click, it stops.
    show.press(cx, "left");
    assert!(frames(cx, &show.canvas(cx)).is_empty());
}

#[gpui_kit::test]
fn loop_goes_round_and_without_it_the_last_frame_holds(cx: &mut TestAppContext) {
    let _alone = alone();
    for looped in [true, false] {
        let source = if looped { talk().replace(VIDEO, &VIDEO.replace(" />", " loop />")) } else { talk() };
        let show = rehearse(cx, &source, &clip(3, 10));
        let canvas = show.first_pictures(cx, 1);
        let mut seen = Vec::new();
        for _ in 0..6 {
            decoded_ahead(cx, show.audience, &canvas, usize::from(looped));
            seen.push(frame_times(cx, &canvas)[0].unwrap());
            next_frame(cx, show.audience, 100);
        }
        if looped {
            assert_eq!(seen, [0.0, 0.1, 0.2, 0.0, 0.1, 0.2], "round again");
            assert!(cx.read(|cx| canvas.read(cx).playing_videos()).len() == 1);
        } else {
            assert_eq!(seen, [0.0, 0.1, 0.2, 0.2, 0.2, 0.2], "the last picture holds");
            assert!(shows(frames(cx, &canvas)[0].as_ref().unwrap(), 2));
            assert!(cx.read(|cx| canvas.read(cx).playing_videos()).is_empty(), "it's done");
            let asked = cx.update_window(show.audience, |_, window, cx| window.simulate_next_frame(cx)).unwrap();
            next_frame(cx, show.audience, 100);
            let asked_again = cx.update_window(show.audience, |_, window, cx| window.simulate_next_frame(cx)).unwrap();
            assert_eq!((asked.min(1), asked_again), (asked.min(1), 0), "no frames asked for once it's done");
        }
        cx.update_window(show.audience, |_, window, _| window.remove_window()).ok();
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn the_presenters_window_shows_the_still(cx: &mut TestAppContext) {
    let _alone = alone();
    let show = rehearse(cx, &talk(), &clip(30, 10));
    show.first_pictures(cx, 1);
    let presenter = show.presenters(cx);
    assert!(frames(cx, &presenter).is_empty(), "no second copy plays");
    assert!(!cx.read(|cx| presenter.read(cx).has_videos()));
    // Its slide shows the still, the video's first picture, in the video's layer.
    let layers = cx.read(|cx| presenter.read(cx).images());
    assert!(layers.iter().any(|image| shows(image, 0)), "the still shows");
    assert_eq!(show.document.read_with(cx, |document, _| document.readers_held()), 1, "one reader, the audience's");
}

#[gpui_kit::test]
fn two_videos_on_a_slide_both_play(cx: &mut TestAppContext) {
    let _alone = alone();
    let second = "<Video src=\"assets/other.mp4\" x=\"900\" y=\"100\" w=\"640\" h=\"360\" />";
    let source = talk().replace(BOX, second);
    let show = rehearse_with(cx, &source, &clip(30, 10), &[("assets/other.mp4", clip(20, 10))]);
    let canvas = show.first_pictures(cx, 2);
    let threads = threads_of(cx, &canvas);
    assert_eq!(threads.len(), 2);
    assert_eq!(threads_named(&threads), 2, "a thread each");
}

#[gpui_kit::test]
fn the_same_file_twice_plays_twice(cx: &mut TestAppContext) {
    let _alone = alone();
    let again = VIDEO.replace("x=\"100\"", "x=\"900\"");
    let source = talk().replace(BOX, &again);
    let show = rehearse(cx, &source, &clip(30, 10));
    let canvas = show.first_pictures(cx, 2);
    let threads = threads_of(cx, &canvas);
    assert_eq!(threads.len(), 2);
    assert_ne!(threads[0], threads[1]);
    assert_eq!(threads_named(&threads), 2);
    decoded_ahead(cx, show.audience, &canvas, 1);
    next_frame(cx, show.audience, 100);
    assert_eq!(frame_times(cx, &canvas), vec![Some(0.1), Some(0.1)], "both go on");
}

// What the first review found.

/// A packed talk of `source` with `video`, kept in `work`, opened from its pack.
fn packed(source: &str, video: &[u8], work: &std::path::Path) -> (Document, std::path::PathBuf) {
    let folder = talk_folder(source, video);
    let pack = work.join("talk.sptx");
    let saved = Document::open(folder.path(), Fonts::bundled_only()).unwrap().save_as_job(SaveTarget::Packed(pack.clone())).run();
    assert!(saved.is_ok());
    (Document::open_packed(&pack, Fonts::bundled_only()).unwrap(), pack)
}

#[gpui_kit::test]
fn a_video_that_fails_part_way_says_why_and_is_not_opened_again(cx: &mut TestAppContext) {
    let _alone = alone();
    let typing = editor(cx, &talk(), &clip(300, 30));
    let canvas = play_selected(&typing, cx);
    let file = typing._folder.path().join("assets/clip.mp4");
    let len = std::fs::metadata(&file).unwrap().len();
    // The file is cut short under it, as a disk going away would.
    std::fs::OpenOptions::new().write(true).open(&file).unwrap().set_len(len / 3).unwrap();
    until(cx, typing.handle, "it to fail", |cx| {
        cx.executor().advance_clock(Duration::from_millis(34));
        cx.read(|cx| canvas.read(cx).video_failures()) == vec![Some(slides_video::CantPlay::CouldNotRead)]
    });
    typing.settle(cx);
    assert_eq!(typing.status(cx).as_deref(), Some("This video couldn't be read."));
    assert!(frames(cx, &canvas)[0].is_none(), "its still shows");
    // Nothing opens it again, whatever the talk does.
    let (opened, _) = decodings_started_and_ended_aside();
    typing.document(cx).update(cx, |_, cx| cx.notify());
    for _ in 0..20 {
        next_frame(cx, typing.handle, 34);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(decodings_started_and_ended_aside().0, opened, "never opened again");
    assert!(threads_of(cx, &canvas).is_empty());
}

#[gpui_kit::test]
fn opening_a_talk_decodes_no_still_on_its_thread(cx: &mut TestAppContext) {
    let _alone = alone();
    let source = format!("# Video\n\n{}\n", VIDEO.replace(" w=\"640\" h=\"360\"", ""));
    let folder = talk_folder(&source, &clip(3, 10));
    let before = stills_decoded_on_this_thread();
    let mut document = Document::open(folder.path(), Fonts::bundled_only()).unwrap();
    document.notices();
    let video = document.videos_here().pop().expect("the video has a layer");
    assert!(document.stills_coming(), "its still is to be made aside");
    assert_eq!(stills_decoded_on_this_thread(), before, "nothing was decoded here");
    // Unsized, it has its own shape already, read from the file without decoding.
    assert_eq!((video.on.rect.w, video.on.rect.h), (WIDTH as f32, HEIGHT as f32));
    // In a window, it's made aside, and the slide keeps its shape when it comes.
    let typing = open_document_for_test(cx, folder, document);
    stills_made(&typing, cx);
    let made = typing.document(cx).update(cx, |document, _| document.videos_here().pop().unwrap());
    assert_eq!(made.on.rect, video.on.rect);
}

/// `document` in a window, its files in `folder`.
fn open_document_for_test(cx: &mut TestAppContext, folder: tempfile::TempDir, document: Document) -> Typing {
    let root = tempfile::tempdir().unwrap();
    let typing = open_untitled_talk(cx, document, root.path());
    std::mem::forget(root);
    Typing { _folder: folder, ..typing }
}

#[gpui_kit::test]
fn a_video_stopped_while_it_opens_ends_off_the_windows_thread(cx: &mut TestAppContext) {
    let _alone = alone();
    let typing = editor(cx, &talk(), &clip(300, 30));
    select_video(&typing, cx);
    let canvas = editor_canvas(&typing, cx);
    let (started, ended) = decodings_started_and_ended_aside();
    let document = typing.document(cx);
    cx.update(|cx| {
        let video = document.update(cx, |document, _| document.selected_video()).unwrap();
        let open = document.read(cx).open_large(&video.on.src).unwrap();
        canvas.update(cx, |canvas, cx| {
            canvas.play(video.layer, video.on, open, cx);
            // Stopped before it's open.
            canvas.stop_videos(cx);
        });
    });
    cx.run_until_parked();
    // Ended on a thread of its own, which may take a moment.
    let ended_as_started = |cx: &mut TestAppContext, count: usize| {
        until(cx, typing.handle, "its decoding ended aside", |_| {
            let (started_now, ended_now) = decodings_started_and_ended_aside();
            assert_eq!(started_now - started, count, "opened");
            ended_now - ended == count
        });
    };
    ended_as_started(cx, 1);
    // And stopped as it plays, the same.
    play_selected(&typing, cx);
    click_on(&typing, cx, VIDEO_STOP_ID);
    ended_as_started(cx, 2);
}

#[gpui_kit::test]
fn play_pressed_during_a_save_opens_the_saved_pack_after_it(cx: &mut TestAppContext) {
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let (document, pack) = packed(&talk(), &clip(300, 30), work.path());
    let typing = open_untitled_talk(cx, document, work.path());
    stills_made(&typing, cx);
    select_video(&typing, cx);
    let canvas = editor_canvas(&typing, cx);
    // The save starts, and Play is pressed before it's done.
    typing.with_window(cx, |window, cx| {
        window.press("ctrl-s", cx);
        window.press("space", cx);
    });
    let saving = typing.document(cx).read_with(cx, |document, _| document.saving_the_pack());
    typing.settle(cx);
    wait(cx, 500);
    assert!(saving, "Play came while the save ran");
    assert_eq!(typing.status(cx).as_deref(), Some("Saved talk.sptx."));
    until(cx, typing.handle, "playing", |cx| frames(cx, &canvas).first().is_some_and(Option::is_some));
    assert_eq!(typing.document(cx).read_with(cx, |document, _| document.readers_held()), 1);
    // It reads the pack as saved: the one there now.
    assert!(slides_engine::open_packed(&pack).is_ok());
    decoded_ahead(cx, typing.handle, &canvas, 2);
    let at = frame_times(cx, &canvas)[0].unwrap();
    next_frame(cx, typing.handle, 34);
    assert!(frame_times(cx, &canvas)[0].unwrap() > at);
}

#[gpui_kit::test]
fn a_save_in_place_keeps_every_still_and_shape(cx: &mut TestAppContext) {
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let source = format!("# Video\n\n{}\n\n---\n\n# Two\n", VIDEO.replace(" w=\"640\" h=\"360\"", ""));
    let (document, _) = packed(&source, &clip(3, 10), work.path());
    let typing = open_untitled_talk(cx, document, work.path());
    stills_made(&typing, cx);
    let shape = typing.document(cx).update(cx, |document, _| document.videos_here().pop().unwrap().on.rect);
    let made = stills_decoded_on_this_thread();
    typing.press(cx, "ctrl-s");
    wait(cx, 500);
    assert_eq!(typing.status(cx).as_deref(), Some("Saved talk.sptx."));
    assert!(!typing.document(cx).read_with(cx, |document, _| document.stills_coming()), "no still is made again");
    assert_eq!(stills_decoded_on_this_thread(), made, "none was decoded again");
    let after = typing.document(cx).update(cx, |document, _| document.videos_here().pop().unwrap().on.rect);
    assert_eq!(after, shape);
}

#[gpui_kit::test]
fn a_video_in_the_text_says_it_plays_once_placed_when_selected(cx: &mut TestAppContext) {
    let _alone = alone();
    let typing = editor(cx, "# Video\n\n<Video src=\"assets/clip.mp4\" />\n", &clip(3, 10));
    let video = thing(&typing, cx, "<Video");
    let at = middle(&typing, cx, &video);
    typing.click(cx, at);
    assert_eq!(
        typing.status(cx).as_deref(),
        Some("Video selected. This video plays only when it's placed on the slide. Drag it out of the text to place it.")
    );
    assert!(bounds_of(&typing, cx, VIDEO_PLAY_ID).is_none(), "nothing to press");
}

#[gpui_kit::test]
fn the_window_losing_the_keyboard_stops_a_video(cx: &mut TestAppContext) {
    let _alone = alone();
    let typing = editor(cx, &talk(), &clip(300, 30));
    let before = thread_count();
    let names = {
        let canvas = play_selected(&typing, cx);
        threads_of(cx, &canvas)
    };
    typing.with_window(cx, |window, _| window.activate_window());
    cx.run_until_parked();
    VisualTestContext::from_window(typing.handle, cx).deactivate_window();
    typing.settle(cx);
    threads_end(cx, &names, before);
    assert!(frames(cx, &editor_canvas(&typing, cx)).is_empty());
}

#[gpui_kit::test]
fn ending_the_show_ends_its_videos(cx: &mut TestAppContext) {
    let _alone = alone();
    let show = rehearse(cx, &talk(), &clip(300, 30));
    let before = thread_count();
    let names = {
        let canvas = show.first_pictures(cx, 1);
        threads_of(cx, &canvas)
    };
    cx.simulate_keystrokes(show.audience, "escape");
    cx.run_until_parked();
    assert!(show.show.read_with(cx, |show, _| show.has_ended()));
    let took = threads_end(cx, &names, before);
    assert!(took <= Duration::from_millis(300), "{took:?}");
}

#[gpui_kit::test]
fn the_show_opens_a_video_again_after_a_save_in_place(cx: &mut TestAppContext) {
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let (document, _) = packed(&talk(), &clip(300, 30), work.path());
    let show = show_of(cx, tempfile::tempdir().unwrap(), document);
    let canvas = show.first_pictures(cx, 1);
    let names = threads_of(cx, &canvas);
    // A save in place of the pack, as the editor's window runs one.
    show.document.update(cx, |document, _| assert_eq!(document.begin_saving_the_pack(), 1));
    until(cx, show.audience, "its file let go of", |cx| {
        // Pictures are taken, so its thread reads on, and finds its file let go of.
        cx.executor().advance_clock(Duration::from_millis(34));
        cx.read(|cx| canvas.read(cx).videos_let_go()).len() == 1
    });
    let job = show.document.read_with(cx, |document, _| document.save_job());
    let saved = job.run().expect("saved");
    show.document.update(cx, |document, cx| {
        document.pack_save_ended();
        document.saved(saved);
        cx.notify();
    });
    show.settle(cx);
    until(cx, show.audience, "playing again", |cx| threads_of(cx, &canvas).len() == 1 && frames(cx, &canvas)[0].is_some());
    assert_ne!(threads_of(cx, &canvas), names);
    assert_eq!(show.document.read_with(cx, |document, _| document.readers_held()), 1);
}

#[gpui_kit::test]
fn a_slide_left_keeps_its_videos_last_picture_through_its_transition(cx: &mut TestAppContext) {
    let _alone = alone();
    let source = format!("# Video\n\n{VIDEO}\n\n---\ntransition: fade\n---\n\n# Two\n");
    let show = rehearse(cx, &source, &clip(300, 30));
    let canvas = show.first_pictures(cx, 1);
    let names = threads_of(cx, &canvas);
    let before = thread_count();
    show.press(cx, "right");
    let view: Entity<AudienceView> = cx.read(|cx| show.show.read(cx).audience()).unwrap().1;
    let leaving = cx.read(|cx| view.read(cx).outgoing(cx).cloned()).expect("the slide left is still seen");
    assert!(frames(cx, &leaving)[0].is_some(), "its last picture stays");
    threads_end(cx, &names, before);
    // Once the transition's done, it goes.
    for _ in 0..30 {
        next_frame(cx, show.audience, 50);
    }
    assert!(frames(cx, &leaving).is_empty());
}

/// How long the window's thread takes for each picture of a turned 1080p video shown full
/// screen. Timed in a release build only.
#[gpui_kit::test]
#[cfg_attr(debug_assertions, ignore = "timed in a release build")]
fn a_turned_full_size_video_costs_the_window_little(cx: &mut TestAppContext) {
    let _alone = alone();
    let video = slides_video::make_test_video(1920, 1080, 60, 30, |at| COLOURS[at as usize % COLOURS.len()].repeat(1920 * 1080));
    let source = "# Video\n\n<Video src=\"assets/clip.mp4\" x=\"0\" y=\"0\" w=\"1920\" h=\"1080\" rotate=\"5\" />\n";
    let show = rehearse(cx, source, &video);
    let canvas = show.first_pictures(cx, 1);
    let mut times = Vec::new();
    let mut shown = Vec::new();
    for _ in 0..40 {
        decoded_ahead(cx, show.audience, &canvas, 1);
        next_frame(cx, show.audience, 34);
        times.push(cx.read(|cx| canvas.read(cx).last_video_advance()));
        shown.extend(frame_times(cx, &canvas)[0]);
    }
    shown.dedup();
    assert!(shown.len() >= 20, "turned pictures come: {shown:?}");
    times.sort();
    let median = times[times.len() / 2];
    eprintln!("the window's thread, a turned 1080p picture: median {median:?}, most {:?}", times.last().unwrap());
    assert!(median < Duration::from_millis(2), "{median:?}");
}

// What the second review found.

#[test]
fn a_save_letting_go_while_a_video_is_read_waits_for_it_rather_than_failing() {
    let _alone = alone();
    use stagepack_studio::video_player::{Making, Unopened, make_still_from, play_from};
    let work = tempfile::tempdir().unwrap();
    let (mut document, _) = packed(&talk(), &clip(30, 10), work.path());
    // Its index being read as a save lets go of it.
    let reader = document.open_large("assets/clip.mp4").unwrap().open_unless_saving().unwrap();
    assert_eq!(document.begin_saving_the_pack(), 1);
    assert_eq!(play_from(reader, 0.0, false).err(), Some(Unopened::Saving), "it's opened again after the save");
    document.pack_save_ended();
    // Its still being made as a save lets go of it.
    document.notices();
    let (job, open) = document.still_jobs().pop().expect("its still is wanted");
    let reader = open.unwrap().open_for_still().unwrap();
    document.begin_saving_the_pack();
    assert!(matches!(make_still_from(job, reader), Making::Waits(_)), "made again after the save, not said to be unreadable");
    document.pack_save_ended();
    // A file that isn't a video still says so.
    let folder = talk_folder(&talk(), b"words, not pictures");
    let mut words = Document::open(folder.path(), Fonts::bundled_only()).unwrap();
    let reader = words.open_large("assets/clip.mp4").unwrap().open_unless_saving().unwrap();
    assert_eq!(play_from(reader, 0.0, false).err(), Some(Unopened::Cannot(slides_video::CantPlay::NotAVideo)));
    words.notices();
    let (job, open) = words.still_jobs().pop().unwrap();
    assert!(matches!(make_still_from(job, open.unwrap().open_for_still().unwrap()), Making::Made(_)));
}

#[gpui_kit::test]
fn a_save_that_fails_or_is_cancelled_leaves_the_video_playing(cx: &mut TestAppContext) {
    let _alone = alone();
    use std::os::unix::fs::PermissionsExt as _;
    let work = tempfile::tempdir().unwrap();
    let place = work.path().join("place");
    std::fs::create_dir(&place).unwrap();
    let (document, _) = packed(&talk(), &clip(300, 30), &place);
    let typing = open_untitled_talk(cx, document, work.path());
    stills_made(&typing, cx);
    let canvas = play_selected(&typing, cx);
    let playing_again = |cx: &mut TestAppContext, names: &[String], how: &str| {
        until(cx, typing.handle, how, |cx| {
            cx.executor().advance_clock(Duration::from_millis(34));
            let threads = threads_of(cx, &canvas);
            threads.len() == 1 && threads != names && frames(cx, &canvas)[0].is_some()
        });
        assert!(!typing.document(cx).read_with(cx, |document, _| document.saving_the_pack()), "{how}");
    };
    // Failed: the folder the pack is in can't be written to.
    let names = threads_of(cx, &canvas);
    std::fs::set_permissions(&place, std::fs::Permissions::from_mode(0o500)).unwrap();
    typing.press(cx, "ctrl-s");
    wait(cx, 500);
    std::fs::set_permissions(&place, std::fs::Permissions::from_mode(0o700)).unwrap();
    let said = typing.status(cx).unwrap_or_default();
    assert!(!said.starts_with("Saved"), "{said}");
    playing_again(cx, &names, "after a failed save");
    // Cancelled as it starts.
    let names = threads_of(cx, &canvas);
    typing.with_window(cx, |window, cx| {
        window.press("ctrl-s", cx);
        window.render_frame(cx);
        window.click(stagepack_studio::workspace_view::CANCEL_PROGRESS_ID, cx);
    });
    wait(cx, 500);
    playing_again(cx, &names, "after a cancelled save");
}

#[test]
fn a_save_beginning_while_a_video_is_unpacked_does_not_wait_for_it() {
    let _alone = alone();
    use std::io::Write as _;
    let work = tempfile::tempdir().unwrap();
    // A pack from another program, its video compressed, large enough to take a while to unpack.
    let pack = work.path().join("odd.sptx");
    {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&pack).unwrap());
        let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("talk.json", deflated).unwrap();
        zip.write_all(br#"{"format":1,"app":"another","entry":"slides.md"}"#).unwrap();
        zip.start_file("slides.md", deflated).unwrap();
        zip.write_all(b"# Odd\n\n<Video src=\"assets/clip.mp4\" x=\"0\" w=\"640\" h=\"360\" />\n").unwrap();
        zip.start_file("assets/clip.mp4", deflated).unwrap();
        let mut state = 7u32;
        let mut piece = vec![0u8; 1 << 20];
        for _ in 0..256 {
            for byte in piece.iter_mut() {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                *byte = state as u8;
            }
            zip.write_all(&piece).unwrap();
        }
        zip.finish().unwrap();
    }
    let mut document = Document::open_packed(&pack, Fonts::bundled_only()).unwrap();
    document.use_video_cache(stagepack_studio::large_files::VideoCache::new(work.path().join("cache"), u64::MAX));
    let open = document.open_large("assets/clip.mp4").unwrap();
    let unpacking = std::thread::spawn(move || open.open_unless_saving().err());
    let began = Instant::now();
    while document.opens_for_test().0 == 0 {
        assert!(began.elapsed() < Duration::from_secs(10), "it never started unpacking");
        std::thread::yield_now();
    }
    // The save begins on the window's thread at once, whatever is being opened.
    let began = Instant::now();
    document.begin_saving_the_pack();
    assert!(began.elapsed() < Duration::from_millis(20), "{:?}", began.elapsed());
    // Its own work waits for the unpacking, which gives up, and then saves.
    let job = document.save_job().waiting_for_opens(document.readers());
    let saved = std::thread::spawn(move || job.run()).join().unwrap();
    assert_eq!(unpacking.join().unwrap(), Some(stagepack_studio::large_files::NotOpened::Saving), "it gave up for the save");
    assert!(saved.is_ok(), "{:?}", saved.err());
    document.pack_save_ended();
}

#[test]
fn an_export_waits_for_a_save_replacing_the_pack() {
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let (document, _) = packed(&talk(), &clip(3, 10), work.path());
    let readers = document.readers();
    document.begin_saving_the_pack();
    let exporting = {
        let readers = readers.clone();
        std::thread::spawn(move || readers.while_reading(Instant::now))
    };
    std::thread::sleep(Duration::from_millis(50));
    let ended = Instant::now();
    let mut document = document;
    document.pack_save_ended();
    let exported = exporting.join().unwrap();
    assert!(exported >= ended, "it read the talk only once the save was done");
}

#[gpui_kit::test]
fn a_turned_video_opened_again_after_a_save_goes_on_playing(cx: &mut TestAppContext) {
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let source = talk().replace(VIDEO, &VIDEO.replace(" />", " rotate=\"20\" />"));
    let (document, _) = packed(&source, &clip(300, 30), work.path());
    let typing = open_untitled_talk(cx, document, work.path());
    stills_made(&typing, cx);
    let canvas = play_selected(&typing, cx);
    let names = threads_of(cx, &canvas);
    typing.press(cx, "ctrl-s");
    wait(cx, 500);
    until(cx, typing.handle, "playing again", |cx| {
        cx.executor().advance_clock(Duration::from_millis(34));
        let threads = threads_of(cx, &canvas);
        threads.len() == 1 && threads != names
    });
    let mut seen = Vec::new();
    until(cx, typing.handle, "new pictures", |cx| {
        cx.executor().advance_clock(Duration::from_millis(34));
        seen.extend(frame_times(cx, &canvas)[0]);
        seen.dedup();
        seen.len() >= 4
    });
}

// What the third review found.

#[test]
fn a_save_beginning_as_a_file_is_opened_never_misses_it() {
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let (mut document, _) = packed(&talk(), &clip(3, 10), work.path());
    let readers = document.readers();
    let waited = std::sync::Arc::new(std::sync::Mutex::new(None));
    {
        // The file is open, and not yet held, as the save begins.
        let (readers, waited) = (readers.clone(), waited.clone());
        let hooked = readers.clone();
        hooked.when_opened_for_test(move || {
            if waited.lock().unwrap().is_some() {
                return;
            }
            assert_eq!(readers.begin_saving_for_test(), 0, "nothing is held yet");
            // The save's own work waits for it.
            let waiting = readers.clone();
            let saving = std::thread::spawn(move || {
                waiting.wait_for_opens_for_test();
                Instant::now()
            });
            std::thread::sleep(Duration::from_millis(50));
            assert!(!saving.is_finished(), "the save waits for what's being opened");
            *waited.lock().unwrap() = Some((saving, Instant::now()));
        });
    }
    let open = document.open_large("assets/clip.mp4").unwrap();
    assert_eq!(open.open_unless_saving().err(), Some(stagepack_studio::large_files::NotOpened::Saving), "refused, never held");
    let (saving, hook_done) = waited.lock().unwrap().take().unwrap();
    assert!(saving.join().unwrap() >= hook_done, "the save went on only once it was refused");
    assert_eq!(document.readers_held(), 0, "nothing holds the file");
    document.pack_save_ended();
}

#[test]
fn a_save_waiting_on_an_export_can_be_cancelled() {
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let (document, _) = packed(&talk(), &clip(3, 10), work.path());
    let readers = document.readers();
    // An export reading the talk, which doesn't end.
    let (release, released) = std::sync::mpsc::channel::<()>();
    let exporting = {
        let readers = readers.clone();
        std::thread::spawn(move || readers.while_reading(|| released.recv().ok()))
    };
    while document.opens_for_test().0 == 0 {
        std::thread::yield_now();
    }
    document.begin_saving_the_pack();
    let job = document.save_job().waiting_for_opens(readers.clone());
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let saving = {
        let stop = stop.clone();
        std::thread::spawn(move || job.run_with(&|_, _| {}, &stop))
    };
    std::thread::sleep(Duration::from_millis(50));
    let cancelled = Instant::now();
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    let saved = saving.join().unwrap();
    assert!(cancelled.elapsed() < Duration::from_millis(500), "{:?}", cancelled.elapsed());
    assert!(saved.is_err(), "cancelled, nothing written");
    release.send(()).unwrap();
    exporting.join().unwrap();
}

#[test]
fn a_video_coded_taller_than_it_is_shown_lays_out_at_its_shown_size_from_the_start() {
    let _alone = alone();
    let video = clip(3, 10);
    // Its pictures said to be coded 48 rows tall, shown 36 tall, as 1088 for 1080.
    let mut coded = video.clone();
    let at = coded.windows(4).position(|kind| kind == b"avc1").unwrap() - 4;
    coded[at + 34..at + 36].copy_from_slice(&48u16.to_be_bytes());
    let source = format!("# Video\n\n{}\n", VIDEO.replace(" w=\"640\" h=\"360\"", ""));
    let folder = talk_folder(&source, &coded);
    let mut document = Document::open(folder.path(), Fonts::bundled_only()).unwrap();
    document.notices();
    let video = document.videos_here().pop().unwrap();
    assert_eq!((video.on.rect.w, video.on.rect.h), (WIDTH as f32, HEIGHT as f32));
}

// Putting a video on a slide: from the Insert tab, a drop or a paste, and its form.

const PLAIN_TALK: &str = "# One\n\nSome words\n\n---\n\n# Two\n";
const NOT_A_VIDEO: &str = "This file isn't a video.";
/// The video [`clip`] makes, as it's put in the middle of the slide at its own size.
const INSERTED: &str = "<Video src=\"assets/clip.mp4\" x=\"928\" y=\"522\" w=\"64\" h=\"36\" />";

/// What [`PLAIN_TALK`] says once `inserted` is put after its first slide's content.
fn with_inserted(inserted: &str) -> String {
    PLAIN_TALK.replacen("Some words", &format!("Some words\n\n{inserted}"), 1)
}

/// A folder talk of `source` in a window that keeps its working copies under `root`, so that a
/// video can be copied in.
fn copying_editor(cx: &mut TestAppContext, source: &str, root: &std::path::Path) -> Typing {
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

/// Hands `paths` to the window as the Video tool's dialog would, and lets the copies run.
fn choose_videos(typing: &Typing, cx: &mut TestAppContext, paths: Vec<std::path::PathBuf>) {
    let view = typing.view.clone();
    cx.update(|cx| view.update(cx, |view, cx| view.chosen_videos(paths, cx)));
    cx.run_until_parked();
    typing.settle(cx);
}

/// Drops `paths` from outside the app on the middle of the slide, and lets the copies run.
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

fn selected_text(typing: &Typing, cx: &TestAppContext) -> Option<String> {
    let source = typing.source(cx);
    selected(typing, cx).map(|(_, range)| source[range].to_string())
}

fn can_undo(typing: &Typing, cx: &TestAppContext) -> bool {
    typing.document(cx).read_with(cx, |document, _| document.can_undo())
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

fn png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
    let picture = image::RgbaImage::from_pixel(width, height, image::Rgba(rgba));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(picture).write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

#[gpui_kit::test]
fn the_insert_tab_has_video(cx: &mut TestAppContext) {
    let typing = open(cx, PLAIN_TALK);
    click_on(&typing, cx, stagepack_studio::ribbon::TAB_INSERT_ID);
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        let video = window.find(stagepack_studio::insert_tools::INSERT_VIDEO_ID);
        assert_eq!(video.label(), Some("Video"));
        assert!(video.visible());
    });
}

#[gpui_kit::test]
fn a_chosen_video_is_copied_in_and_put_on_the_slide_selected(cx: &mut TestAppContext) {
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let typing = copying_editor(cx, PLAIN_TALK, work.path());
    let from = outside(work.path(), "Clip.MP4", &clip(3, 10));
    choose_videos(&typing, cx, vec![from]);
    assert_eq!(typing.source(cx), with_inserted(INSERTED));
    assert_eq!(selected_text(&typing, cx).as_deref(), Some(INSERTED));
    assert_eq!(typing.status(cx).as_deref(), Some("Video selected"));
    assert!(typing.document(cx).read_with(cx, |document, _| document.has_file("assets/clip.mp4")));

    // One step, which Undo takes away.
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), PLAIN_TALK);
    assert!(!can_undo(&typing, cx), "it was one step");
    // A save after that doesn't write the file.
    typing.press(cx, "ctrl-s");
    wait(cx, 500);
    let folder = typing.file.parent().unwrap().to_path_buf();
    assert!(!folder.join("assets/clip.mp4").exists(), "{:?}", files_under(&folder));
    assert_eq!(std::fs::read_to_string(folder.join("slides.md")).unwrap(), PLAIN_TALK);
}

#[gpui_kit::test]
fn a_dropped_video_is_inserted_among_dropped_pictures(cx: &mut TestAppContext) {
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let typing = copying_editor(cx, PLAIN_TALK, work.path());
    let red = outside(work.path(), "red.png", &png(100, 100, [255, 0, 0, 255]));
    let video = outside(work.path(), "clip.mp4", &clip(3, 10));
    let blue = outside(work.path(), "blue.png", &png(100, 100, [0, 0, 255, 255]));
    drop_on_slide(&typing, cx, &[red, video.clone(), blue]);
    let source = typing.source(cx);
    assert_eq!(source.matches("<Picture").count(), 2, "{source}");
    assert_eq!(source.matches("<Video src=\"assets/clip.mp4\"").count(), 1, "{source}");
    // Each its own step.
    for _ in 0..3 {
        typing.press(cx, "ctrl-z");
    }
    assert_eq!(typing.source(cx), PLAIN_TALK);
    assert!(!can_undo(&typing, cx));

    // Its path pasted is a drop.
    typing.press(cx, "ctrl-y");
    cx.write_to_clipboard(gpui_kit::ClipboardItem {
        entries: vec![gpui_kit::ClipboardEntry::ExternalPaths(gpui_kit::ExternalPaths([video].into_iter().collect()))],
    });
    click_on(&typing, cx, stagepack_studio::clipboard_tools::PASTE_ID);
    cx.run_until_parked();
    typing.settle(cx);
    assert_eq!(typing.source(cx).matches("<Video").count(), 1, "{}", typing.source(cx));
}

#[gpui_kit::test]
fn dropped_videos_are_copied_one_at_a_time_in_turn(cx: &mut TestAppContext) {
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let typing = copying_editor(cx, PLAIN_TALK, work.path());
    let wide = outside(work.path(), "wide.mp4", &clip(3, 10));
    let small = slides_video::make_test_video(32, 18, 3, 10, |_| COLOURS[1].repeat(32 * 18));
    let small = outside(work.path(), "small.mp4", &small);
    let view = typing.view.clone();
    cx.update(|cx| view.update(cx, |view, cx| view.chosen_videos(vec![wide, small], cx)));
    // One copy runs, the other waits for it.
    let label = typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.find(workspace_view::PROGRESS_ID).label().map(str::to_string)
    });
    assert_eq!(label.as_deref(), Some("Copying video… 0%"));
    cx.run_until_parked();
    typing.settle(cx);
    let source = typing.source(cx);
    let (first, second) = (source.find("assets/wide.mp4").unwrap(), source.find("assets/small.mp4").unwrap());
    assert!(first < second, "in the order asked for: {source}");
}

#[gpui_kit::test]
fn a_video_goes_on_the_slide_it_was_asked_for_on(cx: &mut TestAppContext) {
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let typing = copying_editor(cx, PLAIN_TALK, work.path());
    let from = outside(work.path(), "clip.mp4", &clip(3, 10));
    let view = typing.view.clone();
    cx.update(|cx| view.update(cx, |view, cx| view.chosen_videos(vec![from], cx)));
    // The next slide is shown before the copy ends.
    let document = typing.document(cx);
    cx.update(|cx| document.update(cx, |document, cx| {
        document.go_to(1);
        cx.notify();
    }));
    assert_eq!(typing.current(cx), 1);
    cx.run_until_parked();
    typing.settle(cx);
    assert_eq!(typing.source(cx), with_inserted(INSERTED), "on the first slide");
    // Shown again, as any edit shows its slide, with it selected.
    assert_eq!(typing.current(cx), 0);
    assert_eq!(selected_text(&typing, cx).as_deref(), Some(INSERTED));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), PLAIN_TALK);
}

#[gpui_kit::test]
fn a_drop_of_more_than_twenty_files_with_videos_among_them_takes_the_first_twenty(cx: &mut TestAppContext) {
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let typing = copying_editor(cx, PLAIN_TALK, work.path());
    let video = clip(3, 10);
    let paths: Vec<std::path::PathBuf> = (0..21)
        .map(|at| match at {
            5 | 10 | 20 => outside(work.path(), &format!("v{at}.mp4"), &video),
            at => outside(work.path(), &format!("p{at}.png"), &png(10, 10, [at as u8, 0, 0, 255])),
        })
        .collect();
    drop_on_slide(&typing, cx, &paths);
    let source = typing.source(cx);
    assert_eq!(source.matches("<Picture").count(), 18, "{source}");
    assert_eq!(source.matches("<Video").count(), 2, "the 21st was left out: {source}");
    assert!(!source.contains("v20"), "{source}");
    // Said once the videos are in, so it's what the line says.
    assert_eq!(typing.status(cx).as_deref(), Some("Only the first 20 files were added."));
}

#[gpui_kit::test]
fn a_video_that_fails_in_a_drop_leaves_the_rest_to_go_on(cx: &mut TestAppContext) {
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let typing = copying_editor(cx, PLAIN_TALK, work.path());
    let red = outside(work.path(), "red.png", &png(100, 100, [255, 0, 0, 255]));
    let words = outside(work.path(), "words.mp4", b"these are words, not pictures");
    // A container with no video track in it.
    let no_track = outside(work.path(), "empty.mp4", &[0, 0, 0, 8, b'm', b'o', b'o', b'v']);
    let video = outside(work.path(), "clip.mp4", &clip(3, 10));
    let blue = outside(work.path(), "blue.png", &png(100, 100, [0, 0, 255, 255]));
    drop_on_slide(&typing, cx, &[red, words, no_track, video, blue]);
    let source = typing.source(cx);
    assert_eq!(source.matches("<Picture").count(), 2, "{source}");
    assert_eq!(source.matches("<Video").count(), 1, "{source}");
    assert!(source.contains("assets/clip.mp4") && !source.contains("words.mp4") && !source.contains("empty.mp4"), "{source}");
    for _ in 0..3 {
        typing.press(cx, "ctrl-z");
    }
    assert_eq!(typing.source(cx), PLAIN_TALK, "each one whole step");
    assert!(!can_undo(&typing, cx));
    let root = work.path().join("recovery");
    let videos: Vec<String> = files_under(&root).into_iter().filter(|name| name.ends_with(".mp4")).collect();
    assert!(videos.iter().all(|name| name.ends_with("clip.mp4")), "{videos:?}");
}

#[gpui_kit::test]
fn a_cancelled_copy_inserts_nothing(cx: &mut TestAppContext) {
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let typing = copying_editor(cx, PLAIN_TALK, work.path());
    let from = outside(work.path(), "clip.mp4", &clip(3, 10));
    let view = typing.view.clone();
    cx.update(|cx| view.update(cx, |view, cx| view.chosen_videos(vec![from], cx)));
    let label = typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        let label = window.find(workspace_view::PROGRESS_ID).label().map(str::to_string);
        window.click(workspace_view::CANCEL_PROGRESS_ID, cx);
        label
    });
    assert_eq!(label.as_deref(), Some("Copying video… 0%"));
    cx.run_until_parked();
    typing.settle(cx);
    assert_eq!(typing.source(cx), PLAIN_TALK);
    assert!(!can_undo(&typing, cx));
    assert_eq!(typing.status(cx).as_deref(), Some("The copy was cancelled, so nothing was added."));
    let root = work.path().join("recovery");
    assert!(files_under(&root).iter().all(|name| !name.ends_with(".mp4")), "{:?}", files_under(&root));
}

#[gpui_kit::test]
fn a_file_that_is_not_a_video_says_so_and_inserts_nothing(cx: &mut TestAppContext) {
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let typing = copying_editor(cx, PLAIN_TALK, work.path());
    let root = work.path().join("recovery");
    // Words, under a video's name.
    let words = outside(work.path(), "words.mp4", b"these are words, not pictures");
    choose_videos(&typing, cx, vec![words.clone()]);
    assert_eq!(typing.source(cx), PLAIN_TALK);
    assert_eq!(typing.status(cx).as_deref(), Some(NOT_A_VIDEO));
    assert!(files_under(&root).iter().all(|name| !name.ends_with(".mp4")), "{:?}", files_under(&root));
    // Nothing at all.
    let empty = outside(work.path(), "empty.mov", b"");
    choose_videos(&typing, cx, vec![empty]);
    assert_eq!(typing.status(cx).as_deref(), Some(NOT_A_VIDEO));
    // Another kind of file, chosen for a video.
    let notes = outside(work.path(), "notes.txt", b"words");
    choose_videos(&typing, cx, vec![notes.clone()]);
    assert_eq!(typing.status(cx).as_deref(), Some(NOT_A_VIDEO));
    // Dropped, as any file that isn't a picture or a video.
    drop_on_slide(&typing, cx, &[notes]);
    assert_eq!(typing.status(cx).as_deref(), Some("Only pictures, videos, 3D models and terminal recordings can be dropped on a slide."));
    drop_on_slide(&typing, cx, &[words]);
    assert_eq!(typing.status(cx).as_deref(), Some(NOT_A_VIDEO));
    assert_eq!(typing.source(cx), PLAIN_TALK);
    assert!(!can_undo(&typing, cx));
}

/// The inspector of `typing`'s window.
fn inspector(typing: &Typing, cx: &TestAppContext) -> Entity<stagepack_studio::inspector::Inspector> {
    cx.read(|cx| typing.view.read(cx).inspector().clone())
}

/// The words of the element `id`, if it's there.
fn label_of(typing: &Typing, cx: &mut TestAppContext, id: &str) -> Option<String> {
    let id = gpui_kit::SharedString::from(id.to_string());
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find(id).and_then(|found| found.label().map(str::to_string))
    })
}

/// Waits for the form's line of facts, read off the window's thread, to say something.
fn facts(typing: &Typing, cx: &mut TestAppContext) -> String {
    use stagepack_studio::inspector::INSPECTOR_VIDEO_FACTS_ID;
    let mut said = None;
    until(cx, typing.handle, "the video's facts", |cx| {
        said = typing.with_window(cx, |window, cx| {
            window.render_frame(cx);
            window.try_find(INSPECTOR_VIDEO_FACTS_ID).and_then(|found| found.label().map(str::to_string))
        });
        said.is_some()
    });
    said.unwrap()
}

fn shown(typing: &Typing, cx: &mut TestAppContext, name: &str) -> Option<String> {
    typing.settle(cx);
    let inspector = inspector(typing, cx);
    cx.read(|cx| inspector.read(cx).shown(name, cx))
}

/// The video's tag, as the talk has it now.
fn video_tag(typing: &Typing, cx: &mut TestAppContext) -> String {
    let source = typing.source(cx);
    let at = source.find("<Video").unwrap();
    let end = at + source[at..].find("/>").unwrap() + 2;
    source[at..end].to_string()
}

#[gpui_kit::test]
fn the_form_says_how_long_and_how_big_and_sets_loop_and_the_still(cx: &mut TestAppContext) {
    use stagepack_studio::inspector::{INSPECTOR_MESSAGE_ID, INSPECTOR_VIDEO_PROBLEM_ID, field_id};
    let _alone = alone();
    // Fourteen seconds, a picture a second, 1120 by 630.
    let video = slides_video::make_test_video(1120, 630, 14, 1, |at| COLOURS[at as usize % COLOURS.len()].repeat(1120 * 630));
    let folder = talk_folder(&talk(), &video);
    std::fs::write(folder.path().join("assets/poster.png"), png(16, 9, [0, 200, 0, 255])).unwrap();
    let typing = open_folder(cx, folder, None);
    stills_made(&typing, cx);
    select_video(&typing, cx);
    assert_eq!(facts(&typing, cx), "14 seconds, 1120 by 630");
    assert_eq!(label_of(&typing, cx, INSPECTOR_MESSAGE_ID).as_deref(), Some("clip.mp4"), "its file, by its name");
    assert_eq!(label_of(&typing, cx, INSPECTOR_VIDEO_PROBLEM_ID), None, "it plays");
    let form = inspector(&typing, cx);
    let labels = cx.read(|cx| form.read(cx).labels());
    assert_eq!(labels, ["Loop", "Still picture", "Left", "Top", "Width", "Height", "Turn"]);

    // Loop, ticked and unticked.
    assert_eq!(shown(&typing, cx, "loop").as_deref(), Some("false"));
    click_on(&typing, cx, gpui_kit::SharedString::from(field_id("loop")));
    assert!(video_tag(&typing, cx).contains("loop=\"true\""), "{}", video_tag(&typing, cx));
    assert_eq!(shown(&typing, cx, "loop").as_deref(), Some("true"));
    click_on(&typing, cx, gpui_kit::SharedString::from(field_id("loop")));
    assert!(!video_tag(&typing, cx).contains("loop=\"true\""), "{}", video_tag(&typing, cx));

    // The still: its first frame, or a picture of the talk's.
    assert_eq!(shown(&typing, cx, "poster").as_deref(), Some("First frame"));
    let choose = |typing: &Typing, cx: &mut TestAppContext, value: &str| {
        let inspector = inspector(typing, cx);
        typing.with_window(cx, |window, cx| inspector.update(cx, |inspector, cx| inspector.choose("poster", value, window, cx)));
        typing.settle(cx);
    };
    // The talk's pictures are read off the window's thread: chosen once they're in.
    until(cx, typing.handle, "the talk's pictures", |cx| {
        choose(&typing, cx, "assets/poster.png");
        video_tag(&typing, cx).contains("poster=")
    });
    assert!(video_tag(&typing, cx).contains("poster=\"assets/poster.png\""), "{}", video_tag(&typing, cx));
    assert_eq!(shown(&typing, cx, "poster").as_deref(), Some("poster.png"));
    choose(&typing, cx, "");
    assert!(!video_tag(&typing, cx).contains("poster"), "{}", video_tag(&typing, cx));
    assert_eq!(shown(&typing, cx, "poster").as_deref(), Some("First frame"));
    // A file that's no picture of the talk's is no choice.
    choose(&typing, cx, "assets/clip.mp4");
    assert!(!video_tag(&typing, cx).contains("poster"), "{}", video_tag(&typing, cx));
    // A picture added to the talk meanwhile is one, once it's been read.
    let document = typing.document(cx);
    let added = cx.update(|cx| document.update(cx, |document, cx| {
        let added = document.add_file(stagepack_studio::added_files::FileKind::Picture, "later.png", png(16, 9, [0, 0, 200, 255]));
        cx.notify();
        added
    }));
    assert_eq!(added.as_deref(), Ok("assets/later.png"));
    until(cx, typing.handle, "the picture added", |cx| {
        choose(&typing, cx, "assets/later.png");
        video_tag(&typing, cx).contains("poster=\"assets/later.png\"")
    });
    choose(&typing, cx, "");
    // The facts were read once.
    assert_eq!(facts(&typing, cx), "14 seconds, 1120 by 630");
}

#[test]
fn a_videos_length_and_size_read_as_words() {
    use stagepack_studio::inspector::video_facts;
    assert_eq!(video_facts(14.0, (1120, 630)), "14 seconds, 1120 by 630");
    assert_eq!(video_facts(1.2, (64, 36)), "1 second, 64 by 36");
    assert_eq!(video_facts(0.3, (64, 36)), "Under a second, 64 by 36");
    assert_eq!(video_facts(59.6, (64, 36)), "1 minute, 64 by 36");
    assert_eq!(video_facts(125.0, (64, 36)), "2 minutes 5 seconds, 64 by 36");
    assert_eq!(video_facts(3661.0, (1920, 1080)), "1 hour 1 minute 1 second, 1920 by 1080");
}

/// Whether `after` differs from `before` only in what was at `range`, give or take the blank
/// lines around it.
fn only_its_bytes(before: &str, after: &str, range: &std::ops::Range<usize>) -> bool {
    let (head, tail) = (before[..range.start].trim_end(), before[range.end..].trim_start());
    after.starts_with(head) && after.ends_with(tail) && after.len() >= head.len() + tail.len()
}

#[gpui_kit::test]
fn a_hand_written_video_reads_or_says_why(cx: &mut TestAppContext) {
    use stagepack_studio::inspector::INSPECTOR_VIDEO_PROBLEM_ID;
    let _alone = alone();
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join("assets")).unwrap();
    std::fs::write(folder.path().join("assets/clip.mp4"), clip(3, 10)).unwrap();
    std::fs::write(folder.path().join("assets/words.mp4"), b"these are words, not pictures").unwrap();
    // A video lying beside the talk, which it can't reach.
    let beside = tempfile::tempdir().unwrap();
    std::fs::write(beside.path().join("outside.mp4"), clip(3, 10)).unwrap();
    let outside = format!("../{}/outside.mp4", beside.path().file_name().unwrap().to_string_lossy());
    assert!(folder.path().join(&outside).exists());
    let cases: Vec<(String, Option<&str>)> = vec![
        ("<Video src=\"assets/clip.mp4\" />".to_string(), None),
        ("<Video src=\"assets/clip.mp4\" x=\"100\" y=\"100\" />".to_string(), None),
        ("<Video src=\"assets/gone.mp4\" x=\"100\" y=\"100\" w=\"640\" h=\"360\" />".to_string(), Some("This video couldn't be read.")),
        (format!("<Video src=\"{outside}\" x=\"100\" y=\"100\" w=\"640\" h=\"360\" />"), Some("This video couldn't be read.")),
        ("<Video src=\"assets/words.mp4\" x=\"100\" y=\"100\" w=\"640\" h=\"360\" />".to_string(), Some(NOT_A_VIDEO)),
        (
            "<Video src=\"assets/clip.mp4\" poster=\"assets/words.mp4\" x=\"100\" y=\"100\" w=\"640\" h=\"360\" />".to_string(),
            Some("The picture named to show for this video can't be shown, so its first picture shows instead."),
        ),
        ("<Video src=\"assets/clip.mp4\" loop=\"no\" x=\"100\" y=\"100\" w=\"640\" h=\"360\" />".to_string(), None),
        ("<Video x=\"100\" y=\"100\" w=\"640\" h=\"360\" />".to_string(), Some("This video has no file. Insert a video to choose one.")),
    ];
    let slides: Vec<String> = cases.iter().enumerate().map(|(at, (tag, _))| format!("# Case {at}\n\n{tag}\n\nAfter\n")).collect();
    std::fs::write(folder.path().join("slides.md"), slides.join("\n---\n\n")).unwrap();
    let typing = open_folder(cx, folder, None);
    stills_made(&typing, cx);
    for (at, (tag, why)) in cases.iter().enumerate() {
        go_to_slide(&typing, cx, at);
        let notices = label_of(&typing, cx, workspace_view::NOTICES_ID).unwrap_or_default();
        match why {
            Some(why) => assert!(notices.contains(why), "case {at}: {notices:?}"),
            None => assert!(!notices.contains("can't"), "case {at}: {notices:?}"),
        }
        // Drawn: the still, or a panel.
        let video = thing(&typing, cx, "<Video");
        assert!(video.rect.w > 0. && video.rect.h > 0., "case {at}: {:?}", video.rect);
        let placed = tag.contains(" x=");
        let at_in = if placed { (video.rect.x + video.rect.w / 5., video.rect.y + video.rect.h / 5.) } else { (video.rect.x + video.rect.w / 2., video.rect.y + video.rect.h / 2.) };
        let point = typing.at(cx, at_in.0, at_in.1);
        typing.click(cx, point);
        assert_eq!(selected(&typing, cx).map(|(_, source)| source), Some(video.source.clone()), "case {at}");
        let status = typing.status(cx).unwrap_or_default();
        assert!(status.starts_with("Video selected"), "case {at}: {status}");
        let problem = label_of(&typing, cx, INSPECTOR_VIDEO_PROBLEM_ID);
        if matches!(why, Some(why) if why.starts_with("This")) {
            assert_eq!(problem.as_deref(), *why, "case {at}: the form says why too");
        }
        let named = label_of(&typing, cx, stagepack_studio::inspector::INSPECTOR_MESSAGE_ID);
        assert_eq!(named.is_some(), tag.contains("src="), "case {at}: its file is named if it has one: {named:?}");
        if !tag.contains("src=") {
            assert_eq!(status, format!("Video selected. {}", why.unwrap()), "case {at}");
        }
        let before = typing.source(cx);
        if placed {
            typing.press(cx, "right");
            let after = typing.source(cx);
            assert_ne!(after, before, "case {at}: moved");
            assert!(only_its_bytes(&before, &after, &video.source), "case {at}: {after}");
            typing.press(cx, "ctrl-z");
            assert_eq!(typing.source(cx), before, "case {at}");
            let point = typing.at(cx, at_in.0, at_in.1);
            typing.click(cx, point);
        }
        typing.press(cx, "delete");
        let after = typing.source(cx);
        assert!(only_its_bytes(&before, &after, &video.source), "case {at}: {after}");
        assert_eq!(after.matches("<Video").count(), cases.len() - 1, "case {at}");
        typing.press(cx, "ctrl-z");
        assert_eq!(typing.source(cx), before, "case {at}");
    }
    // A loop of no is unticked.
    go_to_slide(&typing, cx, 6);
    select_video(&typing, cx);
    assert_eq!(shown(&typing, cx, "loop").as_deref(), Some("false"));
}

/// Goes to slide `index`, from wherever the window is.
fn go_to_slide(typing: &Typing, cx: &mut TestAppContext, index: usize) {
    while typing.current(cx) < index {
        typing.press(cx, "pagedown");
    }
    while typing.current(cx) > index {
        typing.press(cx, "pageup");
    }
    assert_eq!(typing.current(cx), index);
}

/// Every word the window shows that a person reads: what each thing is called, the status
/// line, the notices, and what the form's fields show.
fn words_shown(typing: &Typing, cx: &mut TestAppContext) -> Vec<String> {
    use gpui_kit::test::TestQueryExt as _;
    let mut words: Vec<String> = typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window
            .elements()
            .into_iter()
            .filter(|element| !format!("{:?}", element.path()).contains(workspace_view::MARKDOWN_ID))
            .filter_map(|element| element.label().map(str::to_string))
            .collect()
    });
    let inspector = inspector(typing, cx);
    let fields = cx.read(|cx| {
        let inspector = inspector.read(cx);
        ["loop", "poster", "src"].iter().filter_map(|name| inspector.shown(name, cx)).chain(inspector.labels()).collect::<Vec<_>>()
    });
    words.extend(fields);
    words
}

/// Whether `words` show code: a tag, a folder of the talk's, a video's file name other than
/// as the form names its file, or a codec's name without the plain words beside it.
fn shows_code(words: &str, file_name: bool) -> bool {
    let codec = ["H.264", "H.265", "HEVC", "avc1", "hvc1"].iter().any(|name| words.contains(name));
    words.contains('<')
        || words.contains("assets/")
        || (words.to_ascii_lowercase().contains(".mp4") && !file_name)
        || (codec && !words.contains("An MP4 made with the usual settings (H.264) can."))
}

#[gpui_kit::test]
fn nothing_about_a_video_shows_code(cx: &mut TestAppContext) {
    use stagepack_studio::inspector::INSPECTOR_MESSAGE_ID;
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let typing = copying_editor(cx, PLAIN_TALK, work.path());
    let from = outside(work.path(), "clip.mp4", &clip(3, 10));
    choose_videos(&typing, cx, vec![from]);
    stills_made(&typing, cx);
    facts(&typing, cx);
    let message = label_of(&typing, cx, INSPECTOR_MESSAGE_ID);
    assert_eq!(message.as_deref(), Some("clip.mp4"));
    let shown = words_shown(&typing, cx);
    for said in ["Video selected", "Under a second, 64 by 36", "clip.mp4", "Loop", "Still picture", "First frame"] {
        assert!(shown.iter().any(|words| words == said), "{said:?} among {shown:?}");
    }
    for words in shown {
        let file_name = Some(&words) == message.as_ref() && !words.contains('/');
        assert!(!shows_code(&words, file_name), "{words:?}");
    }

    // One that isn't a video, and one of a kind that isn't played.
    let mut other_kind = clip(3, 10);
    // The kind of its pictures' description, which comes just before their configuration.
    let config = other_kind.windows(4).position(|kind| kind == b"avcC").unwrap();
    let at = other_kind[..config].windows(4).rposition(|kind| kind == b"avc1").unwrap();
    other_kind[at..at + 4].copy_from_slice(b"hvc1");
    for (bytes, why) in [
        (b"these are words, not pictures".to_vec(), NOT_A_VIDEO),
        (other_kind, "This kind of video can't be played here. An MP4 made with the usual settings (H.264) can."),
    ] {
        let typing = editor(cx, &talk(), &bytes);
        select_video(&typing, cx);
        let message = label_of(&typing, cx, INSPECTOR_MESSAGE_ID);
        let shown = words_shown(&typing, cx);
        assert!(shown.iter().any(|words| words.contains(why)), "{why:?} among {shown:?}");
        for words in shown {
            let file_name = Some(&words) == message.as_ref() && !words.contains('/');
            assert!(!shows_code(&words, file_name), "{words:?}");
        }
    }
    assert!(!shows_code("This kind of video can't be played here. An MP4 made with the usual settings (H.264) can.", false));
    assert!(shows_code("Only H.264 plays", false));
}

#[test]
fn a_videos_facts_are_read_once_and_the_oldest_let_go_of() {
    let _alone = alone();
    let folder = talk_folder(&talk(), &clip(3, 10));
    let mut document = Document::open(folder.path(), Fonts::bundled_only()).unwrap();
    let to_read = document.video_facts_to_read("assets/clip.mp4").expect("read once");
    assert!(document.video_facts_to_read("assets/clip.mp4").is_none(), "not again while it's read");
    assert!(document.take_video_facts(to_read.read()));
    let facts = document.video_facts("assets/clip.mp4").unwrap().unwrap();
    assert_eq!(facts.size, (WIDTH, HEIGHT));
    assert!((facts.seconds - 0.3).abs() < 0.01, "{facts:?}");
    assert!(document.video_facts_to_read("assets/clip.mp4").is_none(), "known");
    // 256 more asked for, and the first goes.
    for at in 0..256 {
        document.video_facts_to_read(&format!("assets/v{at}.mp4"));
    }
    assert!(document.video_facts("assets/clip.mp4").is_none(), "let go of");
    assert!(document.video_facts_to_read("assets/clip.mp4").is_some(), "read again when wanted");

    // Read while a save holds the talk's files back, they wait for it, and are read after.
    let folder = talk_folder(&talk(), &clip(3, 10));
    let mut document = Document::open(folder.path(), Fonts::bundled_only()).unwrap();
    document.begin_saving_the_pack();
    let to_read = document.video_facts_to_read("assets/clip.mp4").unwrap();
    assert!(!document.take_video_facts(to_read.read()), "not read yet");
    assert!(document.video_facts_to_read("assets/clip.mp4").is_none(), "it waits");
    document.pack_save_ended();
    let to_read = document.video_facts_to_read("assets/clip.mp4").expect("read after the save");
    assert!(document.take_video_facts(to_read.read()));
    // One that waited says so, for the form to be drawn again.
    document.begin_saving_the_pack();
    let other = talk_folder(&talk(), &clip(3, 10));
    std::fs::copy(other.path().join("assets/clip.mp4"), folder.path().join("assets/again.mp4")).unwrap();
    let to_read = document.video_facts_to_read("assets/again.mp4").unwrap();
    document.take_video_facts(to_read.read());
    assert!(document.try_waiting_stills_again(), "one waited");
    assert!(!document.try_waiting_stills_again(), "and none waits now");
    document.pack_save_ended();
}

// What the final review found.

/// A reader whose first read waits until `release` is told, or three seconds, as a share that
/// has gone away does; and says when it was dropped.
struct Stuck {
    started: std::sync::mpsc::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
    dropped: std::sync::Arc<Mutex<Option<std::time::SystemTime>>>,
}

impl std::io::Read for Stuck {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let _ = self.started.send(());
        let _ = self.release.recv_timeout(Duration::from_secs(3));
        Ok(buf.len().min(1))
    }
}

impl std::io::Seek for Stuck {
    fn seek(&mut self, _: std::io::SeekFrom) -> std::io::Result<u64> {
        Ok(0)
    }
}

impl Drop for Stuck {
    fn drop(&mut self) {
        *self.dropped.lock().unwrap() = Some(std::time::SystemTime::now());
    }
}

#[gpui_kit::test]
fn a_save_pressed_during_a_read_never_waits_on_the_windows_thread(cx: &mut TestAppContext) {
    let _alone = alone();
    use std::io::Read as _;
    let work = tempfile::tempdir().unwrap();
    let (document, pack) = packed(&talk(), &clip(3, 10), work.path());
    let typing = open_untitled_talk(cx, document, work.path());
    stills_made(&typing, cx);
    let readers = typing.document(cx).read_with(cx, |document, _| document.readers());
    let (started, reading) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let dropped = std::sync::Arc::new(Mutex::new(None));
    let mut held = readers.hold_for_test(Box::new(Stuck { started, release: released, dropped: dropped.clone() }));
    // A read, part way, on a thread of its own.
    let reader = std::thread::spawn(move || {
        let first = held.read(&mut [0; 4]).map_err(|_| ());
        let after = held.read(&mut [0; 4]).map_err(|_| ());
        (first, after, held.is_let_go())
    });
    reading.recv().unwrap();
    let pressed = Instant::now();
    typing.with_window(cx, |window, cx| window.press("ctrl-s", cx));
    assert!(pressed.elapsed() < Duration::from_secs(1), "the window waited {:?}", pressed.elapsed());
    assert!(typing.document(cx).read_with(cx, |document, _| document.saving_the_pack()));
    // The read ends a moment later, and the save goes on once it's let go of.
    let ending = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        release.send(()).unwrap();
    });
    wait(cx, 500);
    typing.settle(cx);
    ending.join().unwrap();
    assert_eq!(typing.status(cx).as_deref(), Some("Saved talk.sptx."));
    assert_eq!(reader.join().unwrap(), (Ok(1), Err(()), true), "the read in progress ended, and nothing more was read");
    let dropped = dropped.lock().unwrap().expect("the reader was closed");
    assert!(std::fs::metadata(&pack).unwrap().modified().unwrap() >= dropped, "the pack was replaced after the reader closed");
    assert_eq!(typing.document(cx).read_with(cx, |document, _| document.readers_held()), 0);
}

#[test]
fn a_save_waiting_on_a_read_can_be_cancelled() {
    let _alone = alone();
    use std::io::Read as _;
    let work = tempfile::tempdir().unwrap();
    let (mut document, pack) = packed(&talk(), &clip(3, 10), work.path());
    let before = std::fs::read(&pack).unwrap();
    let readers = document.readers();
    let (started, reading) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let dropped = std::sync::Arc::new(Mutex::new(None));
    let mut held = readers.hold_for_test(Box::new(Stuck { started, release: released, dropped: dropped.clone() }));
    let reader = std::thread::spawn(move || held.read(&mut [0; 4]).is_ok());
    reading.recv().unwrap();
    assert_eq!(document.begin_saving_the_pack(), 1, "let go of, though it's being read");
    let job = document.save_job().waiting_for_opens(readers.clone());
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let saving = {
        let stop = stop.clone();
        std::thread::spawn(move || job.run_with(&|_, _| {}, &stop))
    };
    std::thread::sleep(Duration::from_millis(50));
    assert!(!saving.is_finished(), "the save waits for the read");
    let cancelled = Instant::now();
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    let saved = saving.join().unwrap();
    assert!(cancelled.elapsed() < Duration::from_millis(500), "{:?}", cancelled.elapsed());
    assert!(saved.is_err(), "cancelled, nothing written");
    assert_eq!(std::fs::read(&pack).unwrap(), before);
    release.send(()).unwrap();
    assert!(reader.join().unwrap(), "the read in progress ended as it would have");
    assert!(dropped.lock().unwrap().is_some(), "and closed what it read");
    document.pack_save_ended();
}

#[gpui_kit::test]
fn a_closing_window_copies_no_more_videos(cx: &mut TestAppContext) {
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let typing = copying_editor(cx, PLAIN_TALK, work.path());
    let words = outside(work.path(), "words.mp4", b"these are words, not pictures");
    let video = outside(work.path(), "clip.mp4", &clip(3, 10));
    let view = typing.view.clone();
    // The first copy held as the window is closed, which stops it; it comes back refused, as
    // not a video, rather than cancelled.
    cx.update(|cx| view.update(cx, |view, cx| {
        view.hold_work_for_test(false, true);
        view.chosen_videos(vec![words, video], cx);
    }));
    let handle = typing.handle;
    assert!(!VisualTestContext::from_window(handle, cx).simulate_close());
    cx.run_until_parked();
    assert!(is_open(cx, handle), "it waits for the copy it stopped");
    // A copy started after it would be held, and keep the window open.
    cx.update(|cx| view.update(cx, |view, cx| {
        view.release_held_work_for_test(cx);
        view.hold_work_for_test(false, true);
    }));
    cx.run_until_parked();
    assert!(!is_open(cx, handle), "the video waiting wasn't copied, and the window closed");
    let root = work.path().join("recovery");
    assert!(files_under(&root).iter().all(|name| !name.ends_with(".mp4")), "{:?}", files_under(&root));
}

fn is_open(cx: &TestAppContext, handle: AnyWindowHandle) -> bool {
    cx.read(|cx| cx.windows().iter().any(|window| window.window_id() == handle.window_id()))
}

/// A reader that takes a while to close, as one on a slow share does, and says when it had.
struct SlowToClose(std::sync::Arc<Mutex<Option<Instant>>>);

impl std::io::Read for SlowToClose {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Ok(0)
    }
}

impl std::io::Seek for SlowToClose {
    fn seek(&mut self, _: std::io::SeekFrom) -> std::io::Result<u64> {
        Ok(0)
    }
}

impl Drop for SlowToClose {
    fn drop(&mut self) {
        std::thread::sleep(Duration::from_millis(100));
        *self.0.lock().unwrap() = Some(Instant::now());
    }
}

#[test]
fn a_file_refused_as_a_save_begins_is_closed_before_the_save_goes_on() {
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let (mut document, _) = packed(&talk(), &clip(3, 10), work.path());
    let readers = document.readers();
    let saving = std::sync::Arc::new(Mutex::new(None));
    {
        let (readers, saving) = (readers.clone(), saving.clone());
        let hooked = readers.clone();
        hooked.when_opened_for_test(move || {
            readers.begin_saving_for_test();
            let waiting = readers.clone();
            *saving.lock().unwrap() = Some(std::thread::spawn(move || {
                waiting.wait_for_opens_for_test();
                Instant::now()
            }));
        });
    }
    let closed = std::sync::Arc::new(Mutex::new(None));
    let file = closed.clone();
    let opened = readers.open_for_test(move || Ok(Box::new(SlowToClose(file))));
    assert_eq!(opened.err(), Some(stagepack_studio::large_files::NotOpened::Saving));
    let went_on = saving.lock().unwrap().take().unwrap().join().unwrap();
    let closed = closed.lock().unwrap().expect("the file was closed");
    assert!(went_on >= closed, "the save went on {:?} before the file was closed", closed - went_on);
    document.pack_save_ended();
}

/// Whether this process holds open a file at `path` that's been deleted, or replaced.
#[cfg(target_os = "linux")]
fn holds_deleted(path: &std::path::Path) -> bool {
    std::fs::read_dir("/proc/self/fd")
        .unwrap()
        .flatten()
        .filter_map(|fd| std::fs::read_link(fd.path()).ok())
        .any(|target| target.to_string_lossy().starts_with(&*path.to_string_lossy()) && target.to_string_lossy().ends_with("(deleted)"))
}

#[cfg(target_os = "linux")]
#[gpui_kit::test]
fn a_pack_saved_in_place_with_only_its_text_changed_is_not_held_open(cx: &mut TestAppContext) {
    let _alone = alone();
    let work = tempfile::tempdir().unwrap();
    let (document, pack) = packed(&talk(), &clip(30, 10), work.path());
    let typing = open_untitled_talk(cx, document, work.path());
    stills_made(&typing, cx);
    assert_eq!(typing.document(cx).read_with(cx, |document, _| document.readers_held()), 0, "nothing plays");
    typing.press(cx, "ctrl-m");
    typing.press(cx, "ctrl-s");
    wait(cx, 500);
    typing.settle(cx);
    assert_eq!(typing.status(cx).as_deref(), Some("Saved talk.sptx."));
    assert!(!holds_deleted(&pack), "the pack it replaced, videos and all, is still held open");
    // It plays from the pack as saved.
    let document = typing.document(cx);
    cx.update(|cx| document.update(cx, |document, cx| {
        document.go_to(0);
        cx.notify();
    }));
    typing.settle(cx);
    let canvas = play_selected(&typing, cx);
    until(cx, typing.handle, "playing", |cx| frames(cx, &canvas)[0].is_some());
}
