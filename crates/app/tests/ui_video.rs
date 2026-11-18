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
    next_frame(cx, typing.handle, 1000);
    assert_eq!(frame_times(cx, &canvas), vec![Some(1.0)], "the picture due, ten on");
    assert!(shows(frames(cx, &canvas)[0].as_ref().unwrap(), 10));
    // Only the one due was made to show, and the one it replaced was released.
    assert_eq!(images_made_and_released(cx, &canvas), (made + 1, released + 1));
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
    let (started_now, ended_now) = decodings_started_and_ended_aside();
    assert_eq!(started_now - started, 1, "it was opened");
    assert_eq!(ended_now - ended, 1, "and ended on a background task");
    // And stopped as it plays, the same.
    play_selected(&typing, cx);
    click_on(&typing, cx, VIDEO_STOP_ID);
    cx.run_until_parked();
    let (started_now, ended_now) = decodings_started_and_ended_aside();
    assert_eq!(started_now - started, ended_now - ended);
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
    for _ in 0..40 {
        decoded_ahead(cx, show.audience, &canvas, 1);
        next_frame(cx, show.audience, 34);
        times.push(cx.read(|cx| canvas.read(cx).last_video_advance()));
    }
    times.sort();
    let median = times[times.len() / 2];
    eprintln!("the window's thread, a turned 1080p picture: median {median:?}, most {:?}", times.last().unwrap());
    assert!(median < Duration::from_millis(2), "{median:?}");
}
