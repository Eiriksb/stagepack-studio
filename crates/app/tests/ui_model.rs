//! A 3D model on a slide: its still, turning it by hand in the editor, and in the show turning on
//! its own, by a drag and the wheel, and flying through its views on clicks; every way it stops,
//! with its pictures and its place on the graphics card let go of; and the window never waiting
//! for the drawer.
//!
//! Each test makes its own models, and draws them on the software adapter where there is one. A
//! test skips, saying so, where this computer has no adapter at all. They run one at a time, as
//! they share the app's one drawer and count the process's threads.

#[path = "support/typing.rs"]
mod typing;

use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use typing::*;

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, InputEvent as _, Modifiers, RenderImage, ScrollDelta, ScrollEvent, TestAppContext, VisualTestContext, point, px};
use slides_engine::Fonts;
use slides_model::{AdapterKind, Drawer, DrawerOptions, View};
use stagepack_studio::document::Document;
use stagepack_studio::model_view::{self, ORBIT_DEGREES_PER_SECOND, draws_asked, scenes_held};
use stagepack_studio::present_view::{AudienceView, Presentation, PresenterView, start_show};
use stagepack_studio::saving::SaveTarget;
use stagepack_studio::slide_canvas::SlideCanvas;
use stagepack_studio::slide_editor::{MODEL_DONE_ID, MODEL_SET_VIEW_ID, MODEL_TURN_ID};
use stagepack_studio::workspace_view;

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// A test running alone, with the app's drawer on the software adapter, or on any there is. The
/// drawer is closed as it ends, as a program that ends while a driver lets go of a device can
/// crash in the driver.
struct Alone(#[allow(dead_code)] MutexGuard<'static, ()>);

impl Drop for Alone {
    fn drop(&mut self) {
        let closed = model_view::close_drawer(Duration::from_secs(30));
        if !std::thread::panicking() {
            assert!(closed, "the drawer's thread never ended");
        }
    }
}

/// Waits for `drawer` to say whether it can draw.
fn ready(drawer: &Drawer) -> bool {
    let began = Instant::now();
    loop {
        if let Some(ready) = drawer.ready() {
            return ready.is_ok();
        }
        assert!(began.elapsed() < Duration::from_secs(60), "the drawer never said whether it can draw");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Runs alone, drawing on the software adapter where there is one, else on any, else `None` after
/// saying why `test` is skipped.
fn alone(test: &str) -> Option<Alone> {
    let alone = Alone(ONE_AT_A_TIME.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
    for options in [DrawerOptions { only: Some(AdapterKind::Software), ..DrawerOptions::default() }, DrawerOptions::default()] {
        let drawer = Drawer::start_with(options);
        let can = ready(&drawer);
        if let Some(old) = model_view::use_drawer(drawer) {
            assert!(old.join().unwrap(), "the drawer before let go of its device");
        }
        if can {
            return Some(alone);
        }
    }
    eprintln!("{test} skipped: no graphics adapter, not even a software one, can be had here.");
    None
}

/// A box a unit a side standing on the ground: its faces along x red, along y green and along
/// z, its front, blue.
fn box_model() -> Vec<u8> {
    slides_model::make_test_glb(slides_model::MadeGlb::Boxes)
}

/// Seen from the front, the box filling most of its middle.
const FRONT: &str = "0 0 4 0 0.5 0";

fn model_tag(more: &str) -> String {
    format!("<Model src=\"assets/box.glb\" x=\"100\" y=\"100\" w=\"640\" h=\"360\" view=\"{FRONT}\"{more} />")
}

const BOX: &str = "<Box x=\"1000\" y=\"600\" w=\"300\" h=\"200\" fill=\"#888888\" />";

/// One slide with the model and a box on it, then another.
fn talk_with(more: &str) -> String {
    format!("# Model\n\n{}\n\n{BOX}\n\n---\n\n# Two\n", model_tag(more))
}

fn talk() -> String {
    talk_with("")
}

/// A folder holding a talk of `source`, and `model` as its `assets/box.glb`.
fn talk_folder(source: &str, model: &[u8]) -> tempfile::TempDir {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("slides.md"), source).unwrap();
    std::fs::create_dir(folder.path().join("assets")).unwrap();
    std::fs::write(folder.path().join("assets/box.glb"), model).unwrap();
    folder
}

/// The editor on a talk of `source` with the box, its stills drawn.
fn editor(cx: &mut TestAppContext, source: &str) -> Typing {
    let typing = open_folder(cx, talk_folder(source, &box_model()), None);
    stills_made(&typing, cx);
    typing
}

/// Waits, in real time, for `done`, delivering the frames the window asks for and drawing it
/// meanwhile, as the drawer draws on a thread of its own.
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
        assert!(began.elapsed() < Duration::from_secs(30), "waited too long for {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
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

/// The live picture each model on `canvas` shows now.
fn pictures(cx: &TestAppContext, canvas: &Entity<SlideCanvas>) -> Vec<Option<std::sync::Arc<RenderImage>>> {
    cx.read(|cx| canvas.read(cx).model_pictures())
}

fn made_and_released(cx: &TestAppContext, canvas: &Entity<SlideCanvas>) -> (usize, usize) {
    cx.read(|cx| canvas.read(cx).model_images())
}

/// Where the model of `layer` on `canvas` is seen from now.
fn view_of(cx: &TestAppContext, canvas: &Entity<SlideCanvas>, layer: usize) -> Option<View> {
    cx.read(|cx| canvas.read(cx).model_view(layer))
}

/// The place of the model's layer on the slide shown.
fn model_layer(typing: &Typing, cx: &mut TestAppContext) -> usize {
    typing.document(cx).update(cx, |document, _| document.models_here())[0].layer
}

/// Which of red, green and blue is the most of the colour in the middle of `image`.
fn most_in_the_middle(image: &RenderImage) -> usize {
    let size = image.size(0);
    let (width, height) = (size.width.0 as usize, size.height.0 as usize);
    let bytes = image.as_bytes(0).expect("one picture");
    let at = (height / 2 * width + width / 2) * 4;
    let (r, g, b) = (bytes[at + 2], bytes[at + 1], bytes[at]);
    [r, g, b].iter().enumerate().max_by_key(|(_, value)| **value).map(|(at, _)| at).unwrap()
}

/// How many threads the process has.
fn thread_count() -> usize {
    std::fs::read_dir("/proc/self/task").map(|tasks| tasks.count()).unwrap_or(0)
}

/// Selects the model, by a click a little way in from its corner, away from its buttons.
fn select_model(typing: &Typing, cx: &mut TestAppContext) {
    let model = thing(typing, cx, "<Model");
    let at = typing.at(cx, model.rect.x + model.rect.w / 5., model.rect.y + model.rect.h / 5.);
    typing.click(cx, at);
    assert_eq!(selected(typing, cx).map(|(_, source)| source), Some(model.source));
}

/// Selects the model and turns it, and waits for its first live picture.
fn turn_selected(typing: &Typing, cx: &mut TestAppContext) -> Entity<SlideCanvas> {
    select_model(typing, cx);
    click_on(typing, cx, MODEL_TURN_ID);
    let canvas = editor_canvas(typing, cx);
    until(cx, typing.handle, "the first live picture", |cx| pictures(cx, &canvas).first().is_some_and(Option::is_some));
    canvas
}

/// Waits until no picture of a model is still to come from the drawer.
fn drawn(cx: &mut TestAppContext, window: AnyWindowHandle, canvas: &Entity<SlideCanvas>) {
    until(cx, window, "the picture asked for", |cx| !cx.read(|cx| canvas.read(cx).models_waiting()));
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

/// A turn of the wheel by `lines` at `at` in `window`.
fn wheel(cx: &mut TestAppContext, window: AnyWindowHandle, at: gpui_kit::Point<gpui_kit::Pixels>, lines: f32) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let event = ScrollEvent { position: at, delta: ScrollDelta::Lines(point(0., lines)), ..ScrollEvent::default() };
        window.dispatch_event(event.to_platform_input(), cx);
    })
    .unwrap();
    cx.run_until_parked();
}

/// Whether two angles are within a tenth of a degree, the way round.
fn same_angle(a: f32, b: f32) -> bool {
    let off = (a - b).rem_euclid(360.0);
    off < 0.1 || off > 359.9
}

// In the editor.

#[gpui_kit::test]
fn turn_shows_live_pictures_and_done_goes_back_to_the_still(cx: &mut TestAppContext) {
    let Some(_alone) = alone("turn_shows_live_pictures_and_done_goes_back_to_the_still") else { return };
    let typing = editor(cx, &talk());
    let canvas = editor_canvas(&typing, cx);
    let still = cx.read(|cx| canvas.read(cx).images());
    select_model(&typing, cx);
    assert!(bounds_of(&typing, cx, MODEL_TURN_ID).is_some(), "Turn shows on the model selected");
    assert!(bounds_of(&typing, cx, MODEL_DONE_ID).is_none());
    let canvas = turn_selected(&typing, cx);
    let shown = pictures(cx, &canvas)[0].clone().unwrap();
    let size = shown.size(0);
    let scale = cx.read(|cx| canvas.read(cx).scale()).unwrap();
    assert!((size.width.0 as f32 - 640. * scale).abs() <= 1., "drawn as large as its box shows: {size:?}");
    assert_eq!(most_in_the_middle(&shown), 2, "its blue front");
    // While it turns, Set view and Done, not Turn.
    assert!(bounds_of(&typing, cx, MODEL_SET_VIEW_ID).is_some());
    assert!(bounds_of(&typing, cx, MODEL_DONE_ID).is_some());
    assert!(bounds_of(&typing, cx, MODEL_TURN_ID).is_none());
    click_on(&typing, cx, MODEL_DONE_ID);
    typing.settle(cx);
    assert!(pictures(cx, &canvas).is_empty(), "no picture over the still");
    assert!(bounds_of(&typing, cx, MODEL_TURN_ID).is_some(), "Turn again");
    let after = cx.read(|cx| canvas.read(cx).images());
    assert!(still.iter().zip(&after).all(|(a, b)| a.id == b.id), "the still is the layer it was");
    let (made, released) = made_and_released(cx, &canvas);
    assert_eq!(made, released, "every live picture is released");
    assert!(selected(&typing, cx).is_some(), "still selected");
    // Space turns it too, and takes no typing.
    typing.press(cx, "space");
    assert_eq!(cx.read(|cx| canvas.read(cx).models_turned()).len(), 1);
    typing.press(cx, "space");
    assert!(cx.read(|cx| canvas.read(cx).models_turned()).is_empty());
    assert!(typing.editing(cx).is_none());
    assert_eq!(typing.source(cx), talk(), "nothing written");
}

#[gpui_kit::test]
fn a_drag_turns_it_and_the_wheel_zooms(cx: &mut TestAppContext) {
    let Some(_alone) = alone("a_drag_turns_it_and_the_wheel_zooms") else { return };
    let typing = editor(cx, &talk());
    let canvas = turn_selected(&typing, cx);
    let layer = model_layer(&typing, cx);
    let front = View::parse(FRONT).unwrap();
    assert_eq!(view_of(cx, &canvas, layer), Some(front), "it starts at its view");
    let before = thing(&typing, cx, "<Model");
    let (made, _) = made_and_released(cx, &canvas);
    // A drag to the right turns it round, as far as the pointer goes.
    let from = middle(&typing, cx, &before) + point(px(-60.), px(40.));
    typing.mouse_down(cx, from, 1, Modifiers::default());
    typing.move_held(cx, from + point(px(50.), px(0.)), Modifiers::default());
    typing.move_held(cx, from + point(px(100.), px(20.)), Modifiers::default());
    typing.mouse_up(cx, from + point(px(100.), px(20.)), 1, Modifiers::default());
    let turned = view_of(cx, &canvas, layer).unwrap();
    assert!(same_angle(turned.yaw, -100. * model_view::DEGREES_PER_PIXEL), "{turned:?}");
    assert!((turned.pitch - 20. * model_view::DEGREES_PER_PIXEL).abs() < 0.01, "{turned:?}");
    assert_eq!(turned.distance, front.distance);
    drawn(cx, typing.handle, &canvas);
    assert!(made_and_released(cx, &canvas).0 > made, "drawn anew as it turned");
    // The model wasn't moved, nor anything written.
    assert_eq!(thing(&typing, cx, "<Model").rect, before.rect);
    assert_eq!(typing.source(cx), talk());
    // The wheel brings it nearer, and back.
    let over = middle(&typing, cx, &before) + point(px(-60.), px(40.));
    wheel(cx, typing.handle, over, 1.);
    let nearer = view_of(cx, &canvas, layer).unwrap();
    assert!((nearer.distance - front.distance * model_view::ZOOM_PER_STEP).abs() < 0.001, "{nearer:?}");
    wheel(cx, typing.handle, over, -1.);
    assert!((view_of(cx, &canvas, layer).unwrap().distance - front.distance).abs() < 0.001);
    assert_eq!(typing.source(cx), talk());
}

#[gpui_kit::test]
fn set_view_writes_six_numbers_as_one_step(cx: &mut TestAppContext) {
    let Some(_alone) = alone("set_view_writes_six_numbers_as_one_step") else { return };
    let typing = editor(cx, &talk());
    let canvas = turn_selected(&typing, cx);
    let model = thing(&typing, cx, "<Model");
    let from = middle(&typing, cx, &model) + point(px(-60.), px(40.));
    typing.drag(cx, from, from + point(px(100.), px(0.)));
    drawn(cx, typing.handle, &canvas);
    let before = typing.source(cx);
    click_on(&typing, cx, MODEL_SET_VIEW_ID);
    let written = typing.source(cx);
    let view = format!("view=\"{}\"", View { yaw: 330., ..View::parse(FRONT).unwrap() });
    assert_eq!(view, "view=\"330 0 4 0 0.5 0\"");
    assert_eq!(written, before.replace(&format!("view=\"{FRONT}\""), &view), "only its view is written");
    // It goes on turning, and one Undo takes the step back.
    assert!(bounds_of(&typing, cx, MODEL_DONE_ID).is_some(), "it goes on being turned");
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), before);
    typing.press(cx, "ctrl-shift-z");
    assert_eq!(typing.source(cx), written);
    // Its still is drawn at the view written.
    stills_made(&typing, cx);
    assert!(typing.document(cx).update(cx, |document, _| document.models_here())[0].on.view.is_some_and(|view| view.yaw == 330.));
}

/// A view turned to by hand is written as one put on a slide is: its angles to a thousandth of a
/// degree and its distance to four figures, not with every figure a drag's sums leave.
#[test]
fn a_view_turned_to_by_hand_is_written_with_a_few_figures() {
    let folder = talk_folder(&talk(), &box_model());
    let mut document = Document::open(folder.path(), Fonts::bundled_only()).unwrap();
    let source = |document: &Document| {
        let text = document.source();
        let at = text.find("<Model").unwrap();
        at..at + text[at..].find("/>").unwrap() + 2
    };
    let turned = View { yaw: 320.999_45, pitch: 26.000_08, distance: 15.515_446, target: [0.0, 0.500_002, 0.0] };
    document.set_model_view(source(&document), turned).unwrap();
    assert!(document.source().contains("view=\"320.999 26 15.52 0 0.5 0\""), "{}", document.source());
    document.add_model_view(source(&document), turned).unwrap();
    assert!(document.source().contains("views=\"320.999 26 15.52 0 0.5 0\""), "{}", document.source());
}

/// Turns the model on a talk of `source`, does `leave`, and checks nothing goes on drawing for
/// it: no live picture is left, every one made is released, it's off the card, nothing more is
/// asked of the drawer, and no thread is left.
fn left_by(cx: &mut TestAppContext, how: &str, source: &str, leave: impl FnOnce(Typing, &mut TestAppContext) -> Option<Typing>) {
    let typing = editor(cx, source);
    let before = thread_count();
    {
        let canvas = turn_selected(&typing, cx);
        drawn(cx, typing.handle, &canvas);
    }
    assert_eq!(scenes_held(), 1, "{how}: it's on the card while it's turned");
    let kept = leave(typing, cx);
    if let Some(typing) = &kept {
        typing.settle(cx);
        let canvas = editor_canvas(typing, cx);
        assert!(pictures(cx, &canvas).is_empty(), "{how}: no live picture is left");
        let (made, released) = made_and_released(cx, &canvas);
        assert_eq!(made, released, "{how}: every live picture is released");
        assert!(bounds_of(typing, cx, MODEL_DONE_ID).is_none(), "{how}");
    }
    cx.run_until_parked();
    assert_eq!(scenes_held(), 0, "{how}: it's let go of on the card");
    let asked = draws_asked();
    if let Some(typing) = &kept {
        for _ in 0..20 {
            next_frame(cx, typing.handle, 100);
        }
    }
    assert_eq!(draws_asked(), asked, "{how}: nothing more is drawn");
    assert!(thread_count() <= before, "{how}: no thread is left");
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
fn every_way_of_leaving_stops_the_drawing(cx: &mut TestAppContext) {
    let Some(_alone) = alone("every_way_of_leaving_stops_the_drawing") else { return };
    let talk = talk();
    left_by(cx, "selecting something else", &talk, |typing, cx| {
        let shape = thing(&typing, cx, "<Box");
        let at = middle(&typing, cx, &shape);
        typing.click(cx, at);
        Some(typing)
    });
    let long_heading = talk.replace("# Model", "# Model, with a heading long enough to reach past it");
    left_by(cx, "a click in the slide's words, to type there", &long_heading, |typing, cx| {
        typing.click_text(cx, 0, pos(0, 48));
        assert!(typing.editing(cx).is_some(), "typing in the heading");
        Some(typing)
    });
    left_by(cx, "another slide", &talk, |typing, cx| {
        typing.press(cx, "pagedown");
        assert_eq!(typing.current(cx), 1);
        Some(typing)
    });
    left_by(cx, "deleting the model", &talk, |typing, cx| {
        typing.press(cx, "delete");
        assert!(!typing.source(cx).contains("<Model"));
        Some(typing)
    });
    left_by(cx, "a change to the talk from outside", &talk, |typing, cx| {
        let changed = talk.replace("# Model", "# Changed");
        typing.view.update(cx, |view, cx| view.reload_for_test(Some(changed), cx));
        typing.settle(cx);
        Some(typing)
    });
    left_by(cx, "the window losing the keyboard", &talk, |typing, cx| {
        typing.with_window(cx, |window, _| window.activate_window());
        cx.run_until_parked();
        VisualTestContext::from_window(typing.handle, cx).deactivate_window();
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
    // Undo of the model's insert.
    let typing = editor(cx, "# Model\n\n---\n\n# Two\n");
    typing.document(cx).update(cx, |document, cx| {
        let place = [("src", "assets/box.glb"), ("x", "100"), ("y", "100"), ("w", "640"), ("h", "360")];
        document.insert_tag("Insert model", "Model", &place, None).unwrap();
        cx.notify();
    });
    stills_made(&typing, cx);
    let canvas = turn_selected(&typing, cx);
    drawn(cx, typing.handle, &canvas);
    typing.press(cx, "ctrl-z");
    assert!(!typing.source(cx).contains("<Model"), "undone");
    typing.settle(cx);
    assert!(pictures(cx, &canvas).is_empty());
    let (made, released) = made_and_released(cx, &canvas);
    assert_eq!(made, released);
    assert_eq!(scenes_held(), 0);
}

#[gpui_kit::test]
fn a_talk_of_ten_models_sits_idle(cx: &mut TestAppContext) {
    let Some(_alone) = alone("a_talk_of_ten_models_sits_idle") else { return };
    let source: Vec<String> = (0..10).map(|at| format!("# Slide {at}\n\n{}\n", model_tag(""))).collect();
    let typing = editor(cx, &source.join("\n---\n\n"));
    let canvas = editor_canvas(&typing, cx);
    let idle = |typing: &Typing, cx: &mut TestAppContext, when: &str| {
        typing.settle(cx);
        cx.update_window(typing.handle, |_, window, cx| window.simulate_next_frame(cx)).unwrap();
        let (threads, asked) = (thread_count(), draws_asked());
        let (redrawn, _watching) = redraws(cx, &canvas);
        let mut frames_asked = 0;
        for _ in 0..80 {
            cx.executor().advance_clock(Duration::from_millis(250));
            cx.run_until_parked();
            frames_asked += cx.update_window(typing.handle, |_, window, cx| window.simulate_next_frame(cx)).unwrap();
        }
        assert_eq!(redrawn.get(), 0, "{when}: nothing redraws");
        assert_eq!(frames_asked, 0, "{when}: no frame is asked for");
        assert_eq!(draws_asked(), asked, "{when}: nothing is drawn");
        assert!(thread_count() <= threads, "{when}: no thread more");
        assert_eq!(scenes_held(), 0, "{when}: nothing is on the card");
    };
    idle(&typing, cx, "none turned");
    let canvas_turned = turn_selected(&typing, cx);
    drawn(cx, typing.handle, &canvas_turned);
    // Turned but not moving, it asks for nothing more.
    let asked = draws_asked();
    for _ in 0..10 {
        next_frame(cx, typing.handle, 100);
    }
    assert_eq!(draws_asked(), asked, "a model turned but still draws nothing more");
    click_on(&typing, cx, MODEL_DONE_ID);
    idle(&typing, cx, "one turned and done");
}

#[gpui_kit::test]
fn the_window_never_waits_for_the_drawer(cx: &mut TestAppContext) {
    let Some(_alone) = alone("the_window_never_waits_for_the_drawer") else { return };
    let typing = editor(cx, &talk());
    // From now on the device takes two seconds to make.
    let slow = Drawer::start_with(DrawerOptions { device_delay: Duration::from_secs(2), ..DrawerOptions::default() });
    if let Some(old) = model_view::use_drawer(slow) {
        assert!(old.join().unwrap());
    }
    select_model(&typing, cx);
    let began = Instant::now();
    click_on(&typing, cx, MODEL_TURN_ID);
    let canvas = editor_canvas(&typing, cx);
    let mut frames = 0;
    let mut slowest = Duration::ZERO;
    while pictures(cx, &canvas).first().is_none_or(Option::is_none) {
        let frame = Instant::now();
        cx.run_until_parked();
        cx.update_window(typing.handle, |_, window, cx| {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
        })
        .unwrap();
        cx.run_until_parked();
        slowest = slowest.max(frame.elapsed());
        frames += 1;
        assert!(began.elapsed() < Duration::from_secs(60), "the picture never came");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(began.elapsed() >= Duration::from_millis(1500), "the picture came after the device: {:?}", began.elapsed());
    // Many times, even on a loaded machine, where each frame takes longer.
    assert!(frames >= 20, "the window drew {frames} times meanwhile");
    assert!(slowest < Duration::from_millis(500), "a frame took {slowest:?}");
    click_on(&typing, cx, MODEL_DONE_ID);
    assert!(pictures(cx, &canvas).is_empty());
}

// The reader gate.

fn packed(source: &str, work: &std::path::Path) -> (Document, std::path::PathBuf) {
    let folder = talk_folder(source, &box_model());
    let pack = work.join("talk.sptx");
    let saved = Document::open(folder.path(), Fonts::bundled_only()).unwrap().save_as_job(SaveTarget::Packed(pack.clone())).run();
    assert!(saved.is_ok());
    (Document::open_packed(&pack, Fonts::bundled_only()).unwrap(), pack)
}

#[gpui_kit::test]
fn a_save_in_place_lets_go_of_a_model_being_read(cx: &mut TestAppContext) {
    let Some(_alone) = alone("a_save_in_place_lets_go_of_a_model_being_read") else { return };
    use stagepack_studio::video_player::{Making, make_still_from};
    let work = tempfile::tempdir().unwrap();
    let (mut document, _) = packed(&talk(), work.path());
    // Read to be turned as a save lets go of it: it's read again after the save.
    let reader = document.open_large("assets/box.glb").unwrap().open_unless_saving().unwrap();
    assert_eq!(document.begin_saving_the_pack(), 1);
    assert!(model_view::read_model_from(reader).was_let_go(), "read again after the save, not said to be unreadable");
    document.pack_save_ended();
    // Its still being drawn as a save lets go of it.
    document.notices();
    let (job, open) = document.still_jobs().pop().expect("its still is wanted");
    assert!(job.is_model());
    let reader = open.unwrap().open_for_still().unwrap();
    document.begin_saving_the_pack();
    assert!(matches!(make_still_from(job, reader), Making::Waits(_)), "drawn again after the save");
    document.pack_save_ended();
    assert_eq!(scenes_held(), 0);

    // Turned in a packed talk, the pack is let go of once it's read, and a save in place runs.
    let (document, _) = packed(&talk(), work.path());
    let typing = open_untitled_talk(cx, document, work.path());
    stills_made(&typing, cx);
    let canvas = turn_selected(&typing, cx);
    let held = |typing: &Typing, cx: &TestAppContext| typing.document(cx).read_with(cx, |document, _| document.readers_held());
    assert_eq!(held(&typing, cx), 0, "its file isn't held once it's read");
    typing.press(cx, "ctrl-s");
    wait(cx, 500);
    typing.settle(cx);
    assert_eq!(typing.status(cx).as_deref(), Some("Saved talk.sptx."));
    // It goes on being turned.
    let layer = model_layer(&typing, cx);
    let model = thing(&typing, cx, "<Model");
    let from = middle(&typing, cx, &model) + point(px(-60.), px(40.));
    let (made, _) = made_and_released(cx, &canvas);
    typing.drag(cx, from, from + point(px(30.), px(0.)));
    drawn(cx, typing.handle, &canvas);
    assert!(made_and_released(cx, &canvas).0 > made);
    assert!(view_of(cx, &canvas, layer).is_some());
}

/// A pack made by another program, its talk of `source` and its `assets/box.glb`, `model`, both
/// compressed, in `work`.
fn compressed_pack(work: &std::path::Path, source: &str, model: &[u8]) -> std::path::PathBuf {
    use std::io::Write as _;
    let at = work.join("compressed.sptx");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&at).unwrap());
    let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("talk.json", deflated).unwrap();
    zip.write_all(br#"{"format":1,"app":"another","entry":"slides.md"}"#).unwrap();
    zip.start_file("slides.md", deflated).unwrap();
    zip.write_all(source.as_bytes()).unwrap();
    zip.start_file("assets/box.glb", deflated).unwrap();
    zip.write_all(model).unwrap();
    zip.finish().unwrap();
    at
}

#[gpui_kit::test]
fn a_model_a_pack_holds_compressed_is_drawn_read_and_exported(cx: &mut TestAppContext) {
    use stagepack_studio::export::{ExportKind, run_export};
    use stagepack_studio::large_files::{CACHE_LIMIT, VideoCache};
    let Some(_alone) = alone("a_model_a_pack_holds_compressed_is_drawn_read_and_exported") else { return };
    let work = tempfile::tempdir().unwrap();
    let pack = compressed_pack(work.path(), &talk(), &box_model());
    let mut document = Document::open_packed(&pack, Fonts::bundled_only()).unwrap();
    document.use_video_cache(VideoCache::new(work.path().join("cache"), CACHE_LIMIT));
    let typing = open_untitled_talk(cx, document, work.path());
    // On open, its still is drawn: unpacked to the cache, as Turn would.
    stills_made(&typing, cx);
    let model = thing(&typing, cx, "<Model");
    let problem = typing.document(cx).update(cx, |document, _| document.model_problem_on(0, &model.source));
    assert_eq!(problem, None, "its still is drawn");
    // Its facts show.
    select_model(&typing, cx);
    assert_eq!(facts(&typing, cx), "12 triangles");
    // A PDF and a PowerPoint export draw it.
    for kind in [ExportKind::Pdf, ExportKind::PowerPoint] {
        let input = typing.document(cx).read_with(cx, |document, _| document.export_input());
        let out = work.path().join(format!("talk.{}", kind.extension()));
        let notices = run_export(input, kind, &out).unwrap();
        assert!(notices.iter().any(|notice| notice == "1 model was exported as a picture."), "{kind:?}: {notices:?}");
        assert!(notices.iter().all(|notice| !notice.contains("couldn't be read")), "{kind:?}: {notices:?}");
    }
}

#[gpui_kit::test]
fn a_still_waiting_for_its_model_is_made_once_turn_has_read_it(cx: &mut TestAppContext) {
    let Some(_alone) = alone("a_still_waiting_for_its_model_is_made_once_turn_has_read_it") else { return };
    let folder = talk_folder(&talk(), &box_model());
    // A still of the model that couldn't be made yet, as one couldn't before its file was
    // unpacked.
    let mut other = Document::open(folder.path(), Fonts::bundled_only()).unwrap();
    other.notices();
    let (job, _) = other.still_jobs().pop().expect("its still is wanted");
    assert!(job.is_model());
    let typing = open_folder(cx, folder, None);
    stills_made(&typing, cx);
    let document = typing.document(cx);
    document.update(cx, |document, _| document.still_waits(job));
    cx.run_until_parked();
    assert_eq!(document.read_with(cx, |document, _| document.stills_waiting()), 1, "nothing has tried it again yet");
    // Turned, the model is read, which may have unpacked it: what waited is made then.
    turn_selected(&typing, cx);
    until(cx, typing.handle, "the still that waited", |cx| document.read_with(cx, |document, _| document.stills_waiting()) == 0);
}

// In the show.

/// A show of a talk of `source` with the box, rehearsed, in both its windows.
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

    fn place(&self, cx: &TestAppContext) -> (usize, u32) {
        let place = self.show.read_with(cx, |show, _| show.place());
        (place.slide, place.step)
    }

    /// The place of the model's layer on the first slide.
    fn model_layer(&self, cx: &mut TestAppContext) -> usize {
        self.document.update(cx, |document, _| document.models_at(0, slides_engine::Moment::START))[0].layer
    }

    /// Where on the audience's window the slide's point (`x`, `y`) is.
    fn at(&self, cx: &TestAppContext, x: f32, y: f32) -> gpui_kit::Point<gpui_kit::Pixels> {
        let canvas = self.canvas(cx);
        let slide = cx.read(|cx| canvas.read(cx).slide_bounds()).expect("the slide is drawn");
        let unit = f32::from(slide.size.width) / 1920.;
        slide.origin + point(px(x * unit), px(y * unit))
    }

    /// The model's live picture once it's come.
    fn first_picture(&self, cx: &mut TestAppContext) -> Entity<SlideCanvas> {
        let canvas = self.canvas(cx);
        until(cx, self.audience, "the model's live picture", |cx| pictures(cx, &canvas).first().is_some_and(Option::is_some));
        canvas
    }

    fn mouse(&self, cx: &mut TestAppContext, event: impl gpui_kit::InputEvent) {
        cx.update_window(self.audience, |_, window, cx| {
            window.render_frame(cx);
            window.dispatch_event(event.to_platform_input(), cx);
        })
        .unwrap();
        cx.run_until_parked();
    }

    /// A drag on the audience's window from `from` by `by`.
    fn drag(&self, cx: &mut TestAppContext, from: gpui_kit::Point<gpui_kit::Pixels>, by: gpui_kit::Point<gpui_kit::Pixels>) {
        use gpui_kit::{MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent};
        self.mouse(cx, MouseDownEvent { button: MouseButton::Left, position: from, modifiers: Modifiers::default(), click_count: 1, first_mouse: false });
        for step in 1..=4 {
            let at = from + point(by.x * (step as f32 / 4.), by.y * (step as f32 / 4.));
            self.mouse(cx, MouseMoveEvent { position: at, pressed_button: Some(MouseButton::Left), modifiers: Modifiers::default() });
        }
        self.mouse(cx, MouseUpEvent { button: MouseButton::Left, position: from + by, modifiers: Modifiers::default(), click_count: 1 });
        self.settle(cx);
    }
}

fn rehearse(cx: &mut TestAppContext, source: &str) -> Show {
    cx.update(|cx| {
        gpui_kit::init(cx);
        workspace_view::bind_keys(cx);
    });
    let folder = talk_folder(source, &box_model());
    let document = Document::open(folder.path(), Fonts::bundled_only()).unwrap();
    let document = cx.new(|_| document);
    let show = cx.update(|cx| start_show(document.clone(), true, cx));
    let (audience, presenter) = cx.read(|cx| {
        let show = show.read(cx);
        (show.audience().expect("its window").0, show.presenter().expect("its window").0)
    });
    let show = Show { _folder: folder, document, show, audience, presenter };
    show.settle(cx);
    let document = show.document.clone();
    until(cx, show.audience, "the stills", |cx| document.read_with(cx, |document, _| !document.stills_coming()));
    show.settle(cx);
    show
}

#[gpui_kit::test]
fn in_the_show_auto_turns_and_a_drag_turns_it(cx: &mut TestAppContext) {
    let Some(_alone) = alone("in_the_show_auto_turns_and_a_drag_turns_it") else { return };
    let show = rehearse(cx, &talk_with(" turn=\"auto\""));
    let canvas = show.first_picture(cx);
    let layer = show.model_layer(cx);
    let start = view_of(cx, &canvas, layer).unwrap();
    // A turn a minute: six degrees a second.
    for _ in 0..10 {
        next_frame(cx, show.audience, 100);
    }
    let later = view_of(cx, &canvas, layer).unwrap();
    let turned = (later.yaw - start.yaw).rem_euclid(360.);
    assert!((turned - ORBIT_DEGREES_PER_SECOND).abs() < 0.5, "turned {turned} degrees in a second");
    // The presenter sees its still.
    assert!(pictures(cx, &show.presenters(cx)).is_empty(), "the presenter's window shows the still");
    // A drag on it turns it, and isn't a click of the show's.
    let before = view_of(cx, &canvas, layer).unwrap();
    let on = show.at(cx, 300., 250.);
    show.drag(cx, on, point(px(80.), px(0.)));
    let dragged = view_of(cx, &canvas, layer).unwrap();
    assert!(same_angle(dragged.yaw, before.yaw - 80. * model_view::DEGREES_PER_PIXEL), "{before:?} to {dragged:?}");
    assert_eq!(show.place(cx), (0, 0), "the show stayed where it was");
    // The wheel zooms it.
    wheel(cx, show.audience, on, 2.);
    assert!(view_of(cx, &canvas, layer).unwrap().distance < dragged.distance);
    // The show's keys are its own still.
    show.press(cx, "right");
    assert_eq!(show.place(cx), (1, 0));
}

#[gpui_kit::test]
fn clicks_fly_through_the_views(cx: &mut TestAppContext) {
    let Some(_alone) = alone("clicks_fly_through_the_views") else { return };
    let views = " views=\"90 0 4 0 0.5 0; 0 80 4 0 0.5 0\"";
    let show = rehearse(cx, &talk_with(views));
    let canvas = show.canvas(cx);
    assert!(cx.read(|cx| canvas.read(cx).models_turned()).is_empty(), "standing still, it shows its still");
    let layer = show.model_layer(cx);
    let first = View::parse("90 0 4 0 0.5 0").unwrap();
    let second = View::parse("0 80 4 0 0.5 0").unwrap();
    // A click flies it to its first view, over a second.
    show.press(cx, "right");
    assert_eq!(show.place(cx), (0, 1), "a view is a click of its slide");
    let canvas = show.first_picture(cx);
    next_frame(cx, show.audience, 500);
    let halfway = view_of(cx, &canvas, layer).unwrap();
    assert!(halfway.yaw > 10. && halfway.yaw < 80., "on its way: {halfway:?}");
    for _ in 0..6 {
        next_frame(cx, show.audience, 100);
    }
    assert_eq!(view_of(cx, &canvas, layer), Some(first));
    // A click with the mouse, off the model, flies it to the next.
    let off = show.at(cx, 1500., 900.);
    show.mouse(cx, gpui_kit::MouseDownEvent { button: gpui_kit::MouseButton::Left, position: off, modifiers: Modifiers::default(), click_count: 1, first_mouse: false });
    show.mouse(cx, gpui_kit::MouseUpEvent { button: gpui_kit::MouseButton::Left, position: off, modifiers: Modifiers::default(), click_count: 1 });
    show.settle(cx);
    assert_eq!(show.place(cx), (0, 2));
    for _ in 0..12 {
        next_frame(cx, show.audience, 100);
    }
    assert_eq!(view_of(cx, &canvas, layer), Some(second));
    // Back a click, it flies back.
    show.press(cx, "left");
    for _ in 0..12 {
        next_frame(cx, show.audience, 100);
    }
    assert_eq!(view_of(cx, &canvas, layer), Some(first));
    // Past its last view, the next slide.
    show.press(cx, "right");
    show.press(cx, "right");
    assert_eq!(show.place(cx), (1, 0));
}

#[gpui_kit::test]
fn leaving_the_slide_puts_it_back(cx: &mut TestAppContext) {
    let Some(_alone) = alone("leaving_the_slide_puts_it_back") else { return };
    let show = rehearse(cx, &talk());
    let layer = show.model_layer(cx);
    let on = show.at(cx, 300., 250.);
    show.drag(cx, on, point(px(100.), px(0.)));
    let canvas_now = show.first_picture(cx);
    show.drag(cx, on, point(px(100.), px(0.)));
    assert_ne!(view_of(cx, &canvas_now, layer), View::parse(FRONT), "turned by hand");
    // Its last view drawn: a picture asked for during the drag may be of a view before the last,
    // so the last is asked for once that's come, and waited for too.
    drawn(cx, show.audience, &canvas_now);
    drawn(cx, show.audience, &canvas_now);
    let asked = draws_asked();
    show.press(cx, "right");
    assert_eq!(show.place(cx), (1, 0));
    cx.run_until_parked();
    assert_eq!(scenes_held(), 0, "let go of once its slide is left");
    for _ in 0..10 {
        next_frame(cx, show.audience, 100);
    }
    assert_eq!(draws_asked(), asked, "nothing drawn for a slide that isn't shown");
    show.press(cx, "left");
    assert_eq!(show.place(cx), (0, 0));
    let canvas = show.canvas(cx);
    assert!(cx.read(|cx| canvas.read(cx).models_turned()).is_empty(), "back at its still, at its view");
    drop(canvas_now);
    // Turning on its own, it starts again from its view.
    drop(show);
    let show = rehearse(cx, &talk_with(" turn=\"auto\""));
    let canvas = show.first_picture(cx);
    for _ in 0..20 {
        next_frame(cx, show.audience, 100);
    }
    assert!(view_of(cx, &canvas, layer).unwrap().yaw > 5.);
    show.press(cx, "right");
    show.press(cx, "left");
    let canvas = show.first_picture(cx);
    let back = view_of(cx, &canvas, layer).unwrap();
    assert!(back.yaw < 3. || back.yaw > 359., "from its view again: {back:?}");
}

// Quitting.

#[gpui_kit::test]
fn the_drawer_lets_go_of_its_device_off_the_windows_thread_as_the_app_quits(cx: &mut TestAppContext) {
    let Some(_alone) = alone("the_drawer_lets_go_of_its_device_off_the_windows_thread_as_the_app_quits") else { return };
    let _typing = editor(cx, &talk());
    let drawers = || {
        std::fs::read_dir("/proc/self/task")
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|task| std::fs::read_to_string(task.path().join("comm")).ok())
            .filter(|name| name.trim_end() == "model drawer")
            .count()
    };
    // The driver's own threads take the drawer's name.
    assert!(drawers() >= 1, "the drawer drew the still");
    // Begun as the app quits, closing returns at once, and the drawer's thread ends.
    let began = Instant::now();
    let closed = model_view::begin_closing();
    assert!(began.elapsed() < Duration::from_millis(50), "closing took {:?} on the window's thread", began.elapsed());
    assert!(model_view::wait_closed(), "it let go of its device");
    assert!(futures::executor::block_on(closed).is_ok());
    assert_eq!(drawers(), 0);
    // The app closes it as it quits.
    cx.update(model_view::close_on_quit);
    assert!(model_view::use_drawer(Drawer::start()).is_none(), "none was left to close");
    let drawer = model_view::drawer().expect("a drawer given to draw with is used");
    assert!(ready(&drawer));
    drop(drawer);
    assert!(drawers() >= 1);
    cx.quit();
    assert!(model_view::wait_closed());
    assert_eq!(drawers(), 0);
}

/// How many threads of the process are a model drawer's: the driver's own take its name too.
fn drawer_threads() -> usize {
    std::fs::read_dir("/proc/self/task")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|task| std::fs::read_to_string(task.path().join("comm")).ok())
        .filter(|name| name.trim_end() == "model drawer")
        .count()
}

#[test]
fn once_the_drawer_is_closing_nothing_starts_another() {
    use stagepack_studio::large_files::Readers;
    use stagepack_studio::model_view::Reading;
    let Some(_alone) = alone("once_the_drawer_is_closing_nothing_starts_another") else { return };
    assert!(ready(&model_view::drawer().expect("the drawer given")));
    for close in [true, false] {
        if close {
            // As the app quits.
            let closed = model_view::begin_closing();
            assert!(model_view::wait_closed());
            assert!(futures::executor::block_on(closed).is_ok());
        } else {
            // As a test ends.
            if let Some(old) = model_view::use_drawer(Drawer::start()) {
                assert!(old.join().unwrap());
            }
            assert!(model_view::close_drawer(Duration::from_secs(30)));
        }
        assert_eq!(drawer_threads(), 0, "closed");
        // A still asked for after a long read, as one being made as the app quits is.
        let began = Instant::now();
        let drawn = model_view::draw_still(Box::new(std::io::Cursor::new(box_model())), None, 64, 36);
        assert_eq!(drawn.err(), Some(slides_model::CantDraw::NoAdapter), "a plain answer");
        assert!(began.elapsed() < Duration::from_secs(2), "at once: {:?}", began.elapsed());
        // A model read to be turned.
        let reader = Readers::default().open_for_test(|| Ok(Box::new(std::io::Cursor::new(box_model())))).unwrap();
        assert!(matches!(model_view::read_model_from(reader), Reading::Cant(slides_model::CantDraw::NoAdapter)));
        assert!(model_view::drawer().is_none());
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(drawer_threads(), 0, "no drawer was started again");
        assert_eq!(scenes_held(), 0);
    }
}

// A card that refuses, or keeps losing what's on it.

#[gpui_kit::test]
fn a_model_the_card_refuses_is_not_asked_for_again(cx: &mut TestAppContext) {
    let Some(_alone) = alone("a_model_the_card_refuses_is_not_asked_for_again") else { return };
    let typing = editor(cx, &talk());
    let canvas = turn_selected(&typing, cx);
    drawn(cx, typing.handle, &canvas);
    model_view::drawer().unwrap().refuse_next_draw();
    let model = thing(&typing, cx, "<Model");
    let from = middle(&typing, cx, &model) + point(px(-60.), px(40.));
    typing.drag(cx, from, from + point(px(40.), px(0.)));
    drawn(cx, typing.handle, &canvas);
    typing.settle(cx);
    assert_eq!(typing.status(cx).as_deref(), Some(slides_model::CantDraw::CardRefused.to_string().as_str()), "it says why");
    assert!(pictures(cx, &canvas)[0].is_none(), "its still shows");
    // Turned on, it isn't asked for at that size again.
    let asked = draws_asked();
    typing.drag(cx, from, from + point(px(40.), px(0.)));
    for _ in 0..10 {
        next_frame(cx, typing.handle, 100);
    }
    assert_eq!(draws_asked(), asked, "not drawn and refused on every frame");
}

#[gpui_kit::test]
fn a_model_lost_from_the_card_is_read_again_once(cx: &mut TestAppContext) {
    let Some(_alone) = alone("a_model_lost_from_the_card_is_read_again_once") else { return };
    let typing = editor(cx, &talk());
    let canvas = turn_selected(&typing, cx);
    drawn(cx, typing.handle, &canvas);
    let model = thing(&typing, cx, "<Model");
    let from = middle(&typing, cx, &model) + point(px(-60.), px(40.));
    // Lost once, it's read again and drawn.
    let (made, _) = made_and_released(cx, &canvas);
    model_view::drawer().unwrap().lose_device();
    typing.drag(cx, from, from + point(px(20.), px(0.)));
    until(cx, typing.handle, "drawn again", |cx| made_and_released(cx, &canvas).0 > made);
    assert_eq!(scenes_held(), 1);
    // Lost again as soon as it's put on the card again, with no picture between, it can't be
    // drawn here, and isn't read again.
    let put_on = model_view::models_put_on_card();
    model_view::drawer().unwrap().lose_device();
    model_view::after_upload(Some(Box::new(|drawer: &Drawer| drawer.lose_device())));
    typing.drag(cx, from, from + point(px(20.), px(0.)));
    until(cx, typing.handle, "it can't be drawn", |cx| cx.read(|cx| canvas.read(cx).model_failures()) == vec![Some(slides_model::CantDraw::CardRefused)]);
    model_view::after_upload(None);
    assert_eq!(model_view::models_put_on_card(), put_on + 1, "read again once");
    typing.settle(cx);
    assert!(pictures(cx, &canvas)[0].is_none(), "its still shows");
    let asked = draws_asked();
    typing.drag(cx, from, from + point(px(20.), px(0.)));
    for _ in 0..10 {
        next_frame(cx, typing.handle, 100);
    }
    assert_eq!(draws_asked(), asked, "nothing more is asked of the drawer");
}

#[gpui_kit::test]
fn a_model_in_a_turned_box_is_drawn_turned(cx: &mut TestAppContext) {
    let Some(_alone) = alone("a_model_in_a_turned_box_is_drawn_turned") else { return };
    let typing = editor(cx, &talk_with(" rotate=\"30\""));
    let canvas = turn_selected(&typing, cx);
    let shown = pictures(cx, &canvas)[0].clone().unwrap();
    let scale = cx.read(|cx| canvas.read(cx).scale()).unwrap();
    // Drawn turned, as the layer is: as big as the turned box.
    let (sin, cos) = 30f32.to_radians().sin_cos();
    let (w, h) = (640. * cos + 360. * sin, 640. * sin + 360. * cos);
    let size = shown.size(0);
    assert!((size.width.0 as f32 - w * scale).abs() <= 3., "{size:?} for {w} by {h} at {scale}");
    assert!((size.height.0 as f32 - h * scale).abs() <= 3., "{size:?} for {w} by {h} at {scale}");
    assert_eq!(most_in_the_middle(&shown), 2, "its blue front");
    assert_eq!(shown.as_bytes(0).unwrap()[3], 0, "clear in its corners");
    click_on(&typing, cx, MODEL_DONE_ID);
    typing.settle(cx);
    let (made, released) = made_and_released(cx, &canvas);
    assert_eq!(made, released);
}

#[gpui_kit::test]
fn a_model_turning_on_its_own_that_cannot_be_drawn_asks_for_no_frames(cx: &mut TestAppContext) {
    let Some(_alone) = alone("a_model_turning_on_its_own_that_cannot_be_drawn_asks_for_no_frames") else { return };
    // Its still is drawn while there's an adapter; then there's none.
    let source = format!("# One\n\n---\n\n# Model\n\n{}\n", model_tag(" turn=\"auto\""));
    let show = rehearse(cx, &source);
    let no_adapter = Drawer::start_with(DrawerOptions { no_adapter: true, ..DrawerOptions::default() });
    assert!(!ready(&no_adapter));
    if let Some(old) = model_view::use_drawer(no_adapter) {
        assert!(old.join().unwrap());
    }
    show.press(cx, "right");
    let canvas = show.canvas(cx);
    until(cx, show.audience, "it can't be drawn", |cx| {
        cx.read(|cx| canvas.read(cx).model_failures()) == vec![Some(slides_model::CantDraw::NoAdapter)]
    });
    show.settle(cx);
    cx.update_window(show.audience, |_, window, cx| window.simulate_next_frame(cx)).unwrap();
    let asked = draws_asked();
    let mut frames_asked = 0;
    for _ in 0..40 {
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        frames_asked += cx.update_window(show.audience, |_, window, cx| {
            let asked = window.simulate_next_frame(cx);
            window.render_frame(cx);
            asked
        })
        .unwrap();
    }
    assert_eq!(frames_asked, 0, "no frame is asked for once it can't be drawn");
    assert_eq!(draws_asked(), asked, "and nothing more is asked of the drawer");
    assert!(pictures(cx, &canvas)[0].is_none(), "its still shows");
}

#[test]
fn a_still_lost_from_the_card_twice_cannot_be_drawn() {
    let Some(_alone) = alone("a_still_lost_from_the_card_twice_cannot_be_drawn") else { return };
    // Lost from the card each time it's put there.
    model_view::after_upload(Some(Box::new(|drawer: &Drawer| drawer.lose_device())));
    let put_on = model_view::models_put_on_card();
    let drawn = model_view::draw_still(Box::new(std::io::Cursor::new(box_model())), None, 64, 36);
    model_view::after_upload(None);
    assert_eq!(drawn.err(), Some(slides_model::CantDraw::CardRefused));
    assert_eq!(model_view::models_put_on_card(), put_on + 2, "read and put on again once");
    // Lost only the first time, it's drawn.
    let lost = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let once = lost.clone();
    model_view::after_upload(Some(Box::new(move |drawer: &Drawer| {
        if !once.swap(true, std::sync::atomic::Ordering::SeqCst) {
            drawer.lose_device();
        }
    })));
    let drawn = model_view::draw_still(Box::new(std::io::Cursor::new(box_model())), None, 64, 36);
    model_view::after_upload(None);
    assert_eq!(drawn.map(|picture| (picture.width, picture.height)).ok(), Some((64, 36)));
    assert_eq!(scenes_held(), 0);
}

// Putting a model on a slide, and its form.

const PLAIN_TALK: &str = "# One\n\nSome words\n\n---\n\n# Two\n";
const NOT_A_MODEL: &str = "This file isn't a 3D model.";

/// The box, framed whole, as it's put in the middle of the slide.
const INSERTED: &str = "<Model src=\"assets/box.glb\" x=\"480\" y=\"270\" w=\"960\" h=\"540\" view=\"0 20 3.681 0 0.5 0\" />";

/// What [`PLAIN_TALK`] says once `inserted` is put after its first slide's content.
fn with_inserted(inserted: &str) -> String {
    PLAIN_TALK.replacen("Some words", &format!("Some words\n\n{inserted}"), 1)
}

/// A folder talk of `source` in a window that keeps its working copies under `root`, so that a
/// model can be copied in.
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

/// Hands `paths` to the window as the Model tool's dialog would, and lets the copies run.
fn choose_models(typing: &Typing, cx: &mut TestAppContext, paths: Vec<std::path::PathBuf>) {
    let view = typing.view.clone();
    cx.update(|cx| view.update(cx, |view, cx| view.chosen_models(paths, cx)));
    cx.run_until_parked();
    typing.settle(cx);
}

/// Drops `paths` from outside the app on the middle of the slide, and lets the copies run.
fn drop_on_slide(typing: &Typing, cx: &mut TestAppContext, paths: &[std::path::PathBuf]) {
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

fn png(width: u32, height: u32, rgba: [u8; 4]) -> Vec<u8> {
    let picture = image::RgbaImage::from_pixel(width, height, image::Rgba(rgba));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(picture).write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

/// The model's tag, as the talk has it now.
fn the_tag(typing: &Typing, cx: &mut TestAppContext) -> String {
    let source = typing.source(cx);
    let at = source.find("<Model").unwrap();
    let end = at + source[at..].find("/>").unwrap() + 2;
    source[at..end].to_string()
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

/// Whether the element `id` is there and can be clicked.
fn enabled(typing: &Typing, cx: &mut TestAppContext, id: &str) -> bool {
    let id = gpui_kit::SharedString::from(id.to_string());
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some_and(|found| found.disabled() != Some(true))
    })
}

/// Waits for the form's line of facts, read off the window's thread, to say something.
fn facts(typing: &Typing, cx: &mut TestAppContext) -> String {
    use stagepack_studio::inspector::INSPECTOR_MODEL_FACTS_ID;
    let mut said = None;
    until(cx, typing.handle, "the model's facts", |cx| {
        said = typing.with_window(cx, |window, cx| {
            window.render_frame(cx);
            window.try_find(INSPECTOR_MODEL_FACTS_ID).and_then(|found| found.label().map(str::to_string))
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

#[gpui_kit::test]
fn the_insert_tab_has_model(cx: &mut TestAppContext) {
    let typing = open(cx, PLAIN_TALK);
    click_on(&typing, cx, stagepack_studio::ribbon::TAB_INSERT_ID);
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        let model = window.find(stagepack_studio::insert_tools::INSERT_MODEL_ID);
        assert_eq!(model.label(), Some("Model"));
        assert!(model.visible());
    });
}

#[gpui_kit::test]
fn a_chosen_model_is_copied_in_and_put_on_the_slide_framed(cx: &mut TestAppContext) {
    let Some(_alone) = alone("a_chosen_model_is_copied_in_and_put_on_the_slide_framed") else { return };
    let work = tempfile::tempdir().unwrap();
    let typing = copying_editor(cx, PLAIN_TALK, work.path());
    let from = outside(work.path(), "Box.GLB", &box_model());
    choose_models(&typing, cx, vec![from]);
    assert_eq!(typing.source(cx), with_inserted(INSERTED));
    assert_eq!(selected_text(&typing, cx).as_deref(), Some(INSERTED));
    assert_eq!(typing.status(cx).as_deref(), Some("Model selected"));
    assert!(typing.document(cx).read_with(cx, |document, _| document.has_file("assets/box.glb")));
    // Framed whole, as the view written says.
    let facts = slides_model::facts(Box::new(std::io::Cursor::new(box_model()))).unwrap();
    assert_eq!(stagepack_studio::insert::written_view(View::framing(&facts)).to_string(), "0 20 3.681 0 0.5 0");
    // One step, which Undo takes away.
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), PLAIN_TALK);
    assert!(!can_undo(&typing, cx), "it was one step");

    // A point cloud is framed from its points, which its header doesn't say where they lie.
    let cloud = slides_model::make_test_ply(500, true);
    let from = outside(work.path(), "cloud.ply", &cloud);
    choose_models(&typing, cx, vec![from]);
    let read = slides_model::read(Box::new(std::io::Cursor::new(cloud))).unwrap().facts;
    let framed = stagepack_studio::insert::written_view(View::framing(&read));
    assert!(the_tag(&typing, cx).contains(&format!("view=\"{framed}\"")), "{}", the_tag(&typing, cx));
    let header = slides_model::facts(Box::new(std::io::Cursor::new(slides_model::make_test_ply(500, true)))).unwrap();
    assert_ne!(framed, stagepack_studio::insert::written_view(View::framing(&header)), "not from the header's box of nothing");
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), PLAIN_TALK);

    // What isn't a model, or is one no computer could draw, says why, and nothing is put on the
    // slide or copied.
    let cut = box_model()[..200].to_vec();
    let needs_others = slides_model::glb_from(r#"{"asset":{"version":"2.0"},"buffers":[{"uri":"other.bin","byteLength":4}]}"#, &[]);
    let damaged = slides_model::CantDraw::Damaged.to_string();
    let needs = slides_model::CantDraw::NeedsOtherFiles.to_string();
    let kind = slides_model::CantDraw::KindNotDrawn.to_string();
    let cases = [
        ("words.glb", b"these are words, not a model".to_vec(), NOT_A_MODEL),
        ("empty.ply", Vec::new(), NOT_A_MODEL),
        ("notes.txt", b"words".to_vec(), NOT_A_MODEL),
        ("cut.glb", cut, damaged.as_str()),
        ("others.glb", needs_others, needs.as_str()),
        // Kinds of model this app doesn't read, by their names: why, rather than that they're none.
        ("scene.gltf", b"{}".to_vec(), needs.as_str()),
        ("Scene.GLTF", b"{}".to_vec(), needs.as_str()),
        ("mesh.obj", b"v 0 0 0".to_vec(), kind.as_str()),
        ("part.stl", b"solid part".to_vec(), kind.as_str()),
        ("rig.fbx", b"Kaydara FBX Binary".to_vec(), kind.as_str()),
    ];
    for (name, bytes, why) in cases {
        let from = outside(work.path(), name, &bytes);
        choose_models(&typing, cx, vec![from]);
        assert_eq!(typing.source(cx), PLAIN_TALK, "{name}");
        assert_eq!(typing.status(cx).as_deref(), Some(why), "{name}");
    }
    assert!(!can_undo(&typing, cx));
    let kept: Vec<String> = files_under(&work.path().join("recovery")).into_iter().filter(|name| name.ends_with(".glb") || name.ends_with(".ply")).collect();
    assert!(kept.iter().all(|name| name.ends_with("box.glb") || name.ends_with("cloud.ply")), "nothing refused was copied: {kept:?}");
}

#[gpui_kit::test]
fn a_dropped_model_is_inserted_among_dropped_pictures(cx: &mut TestAppContext) {
    let Some(_alone) = alone("a_dropped_model_is_inserted_among_dropped_pictures") else { return };
    let work = tempfile::tempdir().unwrap();
    let typing = copying_editor(cx, PLAIN_TALK, work.path());
    // A file that's none of them is said, in words that name a model too. (Dropped first: in a
    // test window, a drop after a key press doesn't reach the slide.)
    let notes = outside(work.path(), "notes.txt", b"words");
    drop_on_slide(&typing, cx, &[notes]);
    assert_eq!(typing.status(cx).as_deref(), Some("Only pictures, videos, 3D models and terminal recordings can be dropped on a slide."));
    let words = outside(work.path(), "words.glb", b"these are words, not a model");
    drop_on_slide(&typing, cx, &[words]);
    assert_eq!(typing.status(cx).as_deref(), Some(NOT_A_MODEL));
    assert_eq!(typing.source(cx), PLAIN_TALK);
    // Kinds of model this app doesn't read say why, dropped or their paths pasted.
    let needs = slides_model::CantDraw::NeedsOtherFiles.to_string();
    let kind = slides_model::CantDraw::KindNotDrawn.to_string();
    let others = [("scene.gltf", &needs), ("mesh.OBJ", &kind), ("plan.gltf", &needs), ("part.stl", &kind), ("parts.gltf", &needs), ("rig.fbx", &kind)];
    let others: Vec<_> = others.into_iter().map(|(name, why)| (name, outside(work.path(), name, b"not one this app reads"), why)).collect();
    for (name, from, why) in &others {
        drop_on_slide(&typing, cx, std::slice::from_ref(from));
        assert_eq!(typing.status(cx).as_deref(), Some(why.as_str()), "{name} dropped");
    }
    for (name, from, why) in &others {
        cx.write_to_clipboard(gpui_kit::ClipboardItem {
            entries: vec![gpui_kit::ClipboardEntry::ExternalPaths(gpui_kit::ExternalPaths([from.clone()].into_iter().collect()))],
        });
        click_on(&typing, cx, stagepack_studio::clipboard_tools::PASTE_ID);
        cx.run_until_parked();
        typing.settle(cx);
        assert_eq!(typing.status(cx).as_deref(), Some(why.as_str()), "{name} pasted");
    }
    assert_eq!(typing.source(cx), PLAIN_TALK);

    let red = outside(work.path(), "red.png", &png(100, 100, [255, 0, 0, 255]));
    let model = outside(work.path(), "box.glb", &box_model());
    let blue = outside(work.path(), "blue.png", &png(100, 100, [0, 0, 255, 255]));
    drop_on_slide(&typing, cx, &[red, model.clone(), blue]);
    let source = typing.source(cx);
    assert_eq!(source.matches("<Picture").count(), 2, "{source}");
    assert_eq!(source.matches("<Model src=\"assets/box.glb\"").count(), 1, "{source}");
    // Each its own step.
    for _ in 0..3 {
        typing.press(cx, "ctrl-z");
    }
    assert_eq!(typing.source(cx), PLAIN_TALK);
    assert!(!can_undo(&typing, cx));

    // Its path pasted is a drop.
    cx.write_to_clipboard(gpui_kit::ClipboardItem {
        entries: vec![gpui_kit::ClipboardEntry::ExternalPaths(gpui_kit::ExternalPaths([model].into_iter().collect()))],
    });
    click_on(&typing, cx, stagepack_studio::clipboard_tools::PASTE_ID);
    cx.run_until_parked();
    typing.settle(cx);
    assert_eq!(typing.source(cx).matches("<Model").count(), 1, "{}", typing.source(cx));
}

#[gpui_kit::test]
fn the_form_says_how_big_and_sets_turning(cx: &mut TestAppContext) {
    use stagepack_studio::inspector::{INSPECTOR_MESSAGE_ID, INSPECTOR_MODEL_PROBLEM_ID, field_id};
    let Some(_alone) = alone("the_form_says_how_big_and_sets_turning") else { return };
    let typing = editor(cx, &talk());
    select_model(&typing, cx);
    assert_eq!(facts(&typing, cx), "12 triangles");
    assert_eq!(label_of(&typing, cx, INSPECTOR_MESSAGE_ID).as_deref(), Some("box.glb"), "its file, by its name");
    assert_eq!(label_of(&typing, cx, INSPECTOR_MODEL_PROBLEM_ID), None, "it's drawn");
    let form = inspector(&typing, cx);
    let labels = cx.read(|cx| form.read(cx).labels());
    assert_eq!(labels, ["Turns on its own", "Left", "Top", "Width", "Height", "Rotation"], "its box turned on the slide is no turning of the model");

    // Turns on its own, ticked and unticked, each one step.
    assert_eq!(shown(&typing, cx, "turn").as_deref(), Some("false"));
    let before = typing.source(cx);
    click_on(&typing, cx, gpui_kit::SharedString::from(field_id("turn")));
    assert!(the_tag(&typing, cx).contains("turn=\"auto\""), "{}", the_tag(&typing, cx));
    assert_eq!(shown(&typing, cx, "turn").as_deref(), Some("true"));
    click_on(&typing, cx, gpui_kit::SharedString::from(field_id("turn")));
    assert!(!the_tag(&typing, cx).contains("turn"), "{}", the_tag(&typing, cx));
    assert_eq!(typing.source(cx), before);
    typing.press(cx, "ctrl-z");
    assert!(the_tag(&typing, cx).contains("turn=\"auto\""));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), before);
    // The facts were read once.
    assert_eq!(facts(&typing, cx), "12 triangles");

    // A point cloud says how many points.
    let folder = talk_folder(&talk().replace("box.glb", "cloud.ply"), &box_model());
    std::fs::write(folder.path().join("assets/cloud.ply"), slides_model::make_test_ply(1500, false)).unwrap();
    let typing = open_folder(cx, folder, None);
    stills_made(&typing, cx);
    select_model(&typing, cx);
    assert_eq!(facts(&typing, cx), "1,500 points");
}

#[test]
fn a_models_size_reads_as_words() {
    use slides_model::ModelKind::{Mesh, Points};
    use stagepack_studio::inspector::model_facts;
    assert_eq!(model_facts(Mesh, 42_000, 0), "42,000 triangles");
    assert_eq!(model_facts(Mesh, 1, 0), "1 triangle");
    assert_eq!(model_facts(Mesh, 0, 0), "No triangles");
    assert_eq!(model_facts(Mesh, 999_999, 0), "999,999 triangles");
    assert_eq!(model_facts(Points, 0, 1_200_000), "1.2 million points");
    assert_eq!(model_facts(Points, 0, 2_000_000), "2 million points");
    assert_eq!(model_facts(Points, 0, 20_000_000), "20 million points");
    assert_eq!(model_facts(Points, 0, 1), "1 point");
    assert_eq!(model_facts(Points, 0, 999), "999 points");
}

#[gpui_kit::test]
fn add_this_view_and_clear_views_each_one_step(cx: &mut TestAppContext) {
    use stagepack_studio::inspector::{INSPECTOR_ADD_VIEW_ID, INSPECTOR_CLEAR_VIEWS_ID, INSPECTOR_MODEL_VIEWS_ID};
    let Some(_alone) = alone("add_this_view_and_clear_views_each_one_step") else { return };
    let typing = editor(cx, &talk());
    select_model(&typing, cx);
    assert_eq!(label_of(&typing, cx, INSPECTOR_MODEL_VIEWS_ID).as_deref(), Some("No views"));
    // Only while it's turned is there a view to add, and with none, none to clear.
    assert!(!enabled(&typing, cx, INSPECTOR_ADD_VIEW_ID));
    assert!(!enabled(&typing, cx, INSPECTOR_CLEAR_VIEWS_ID));
    let canvas = turn_selected(&typing, cx);
    assert!(enabled(&typing, cx, INSPECTOR_ADD_VIEW_ID));
    let model = thing(&typing, cx, "<Model");
    let from = middle(&typing, cx, &model) + point(px(-60.), px(40.));
    typing.drag(cx, from, from + point(px(100.), px(0.)));
    drawn(cx, typing.handle, &canvas);
    let before = typing.source(cx);
    click_on(&typing, cx, INSPECTOR_ADD_VIEW_ID);
    let one = typing.source(cx);
    assert!(the_tag(&typing, cx).ends_with(" views=\"330 0 4 0 0.5 0\" />"), "{}", the_tag(&typing, cx));
    assert_eq!(label_of(&typing, cx, INSPECTOR_MODEL_VIEWS_ID).as_deref(), Some("1 view"));
    assert!(bounds_of(&typing, cx, MODEL_DONE_ID).is_some(), "it goes on being turned");
    // Another, after the first.
    typing.drag(cx, from, from + point(px(100.), px(0.)));
    drawn(cx, typing.handle, &canvas);
    click_on(&typing, cx, INSPECTOR_ADD_VIEW_ID);
    assert!(the_tag(&typing, cx).contains(" views=\"330 0 4 0 0.5 0; 300 0 4 0 0.5 0\""), "{}", the_tag(&typing, cx));
    assert_eq!(label_of(&typing, cx, INSPECTOR_MODEL_VIEWS_ID).as_deref(), Some("2 views"));
    let two = typing.source(cx);
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), one, "one step");
    typing.press(cx, "ctrl-shift-z");
    assert_eq!(typing.source(cx), two);
    // Cleared, as one step.
    assert!(enabled(&typing, cx, INSPECTOR_CLEAR_VIEWS_ID));
    click_on(&typing, cx, INSPECTOR_CLEAR_VIEWS_ID);
    assert_eq!(typing.source(cx), before);
    assert_eq!(label_of(&typing, cx, INSPECTOR_MODEL_VIEWS_ID).as_deref(), Some("No views"));
    typing.press(cx, "ctrl-z");
    assert_eq!(typing.source(cx), two);
    assert!(bounds_of(&typing, cx, MODEL_DONE_ID).is_none(), "an Undo is no change of its own: it stops being turned");

    // A model with as many views as it can hold takes no more, and says so.
    let full = vec![FRONT; 256].join("; ");
    let typing = editor(cx, &talk_with(&format!(" views=\"{full}\"")));
    select_model(&typing, cx);
    assert_eq!(label_of(&typing, cx, INSPECTOR_MODEL_VIEWS_ID).as_deref(), Some("256 views"));
    turn_selected(&typing, cx);
    let before = typing.source(cx);
    click_on(&typing, cx, INSPECTOR_ADD_VIEW_ID);
    assert_eq!(typing.source(cx), before);
    assert_eq!(typing.status(cx).as_deref(), Some("This 3D model has as many views as it can have."));
    click_on(&typing, cx, MODEL_DONE_ID);

    // What's written is kept as it was, the view added after it; and Clear views while it's
    // turned clears them, as the form writes any setting.
    let typing = editor(cx, &talk_with(&format!(" views=\"{FRONT} ,\"")));
    select_model(&typing, cx);
    turn_selected(&typing, cx);
    click_on(&typing, cx, INSPECTOR_ADD_VIEW_ID);
    assert!(the_tag(&typing, cx).contains(&format!(" views=\"{FRONT} , {FRONT}\"")), "{}", the_tag(&typing, cx));
    click_on(&typing, cx, INSPECTOR_CLEAR_VIEWS_ID);
    assert!(!the_tag(&typing, cx).contains("views"), "{}", the_tag(&typing, cx));
    assert_eq!(typing.source(cx), talk());
}

/// Whether `after` differs from `before` only in what was at `range`, give or take the blank
/// lines around it.
fn only_its_bytes(before: &str, after: &str, range: &std::ops::Range<usize>) -> bool {
    let (head, tail) = (before[..range.start].trim_end(), before[range.end..].trim_start());
    after.starts_with(head) && after.ends_with(tail) && after.len() >= head.len() + tail.len()
}

#[gpui_kit::test]
fn a_hand_written_model_reads_or_says_why(cx: &mut TestAppContext) {
    use stagepack_studio::inspector::INSPECTOR_MODEL_PROBLEM_ID;
    let Some(_alone) = alone("a_hand_written_model_reads_or_says_why") else { return };
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join("assets")).unwrap();
    std::fs::write(folder.path().join("assets/box.glb"), box_model()).unwrap();
    std::fs::write(folder.path().join("assets/words.glb"), b"these are words, not a model").unwrap();
    // A model lying beside the talk, which it can't reach.
    let beside = tempfile::tempdir().unwrap();
    std::fs::write(beside.path().join("outside.glb"), box_model()).unwrap();
    let outside = format!("../{}/outside.glb", beside.path().file_name().unwrap().to_string_lossy());
    assert!(folder.path().join(&outside).exists());
    let place = "x=\"100\" y=\"100\" w=\"640\" h=\"360\"";
    let many = vec![FRONT; 1000].join("; ");
    let views_left_out = "Some of this 3D model's views can't be read, or there are more than 256, so they're left out";
    let cases: Vec<(String, Option<&str>)> = vec![
        ("<Model src=\"assets/box.glb\" />".to_string(), Some(slides_engine::MODEL_IN_TEXT)),
        ("<Model src=\"assets/box.glb\" x=\"100\" y=\"100\" />".to_string(), None),
        (format!("<Model src=\"assets/gone.glb\" {place} />"), Some("This model couldn't be read.")),
        (format!("<Model src=\"{outside}\" {place} />"), Some("This model couldn't be read.")),
        (format!("<Model src=\"assets/words.glb\" {place} />"), Some(NOT_A_MODEL)),
        (format!("<Model src=\"assets/box.glb\" view=\"garbage\" {place} />"), Some(slides_engine::MODEL_VIEW_UNREAD)),
        (format!("<Model src=\"assets/box.glb\" view=\"NaN 0 4 0 0.5 0\" {place} />"), Some(slides_engine::MODEL_VIEW_UNREAD)),
        (format!("<Model src=\"assets/box.glb\" views=\"{many}\" {place} />"), Some(views_left_out)),
        (format!("<Model {place} />"), Some(slides_engine::MODEL_NO_FILE)),
    ];
    let slides: Vec<String> = cases.iter().enumerate().map(|(at, (tag, _))| format!("# Case {at}\n\n{tag}\n\nAfter\n")).collect();
    std::fs::write(folder.path().join("slides.md"), slides.join("\n---\n\n")).unwrap();
    let typing = open_folder(cx, folder, None);
    stills_made(&typing, cx);
    for (at, (tag, why)) in cases.iter().enumerate() {
        go_to_slide(&typing, cx, at);
        stills_made(&typing, cx);
        let notices = label_of(&typing, cx, workspace_view::NOTICES_ID).unwrap_or_default();
        match why {
            Some(why) => assert!(notices.contains(why), "case {at}: {notices:?}"),
            None => assert!(!notices.contains("can't"), "case {at}: {notices:?}"),
        }
        // Drawn: the still, or a panel.
        let model = thing(&typing, cx, "<Model");
        assert!(model.rect.w > 0. && model.rect.h > 0., "case {at}: {:?}", model.rect);
        let placed = tag.contains(" x=");
        let at_in = if placed { (model.rect.x + model.rect.w / 5., model.rect.y + model.rect.h / 5.) } else { (model.rect.x + model.rect.w / 2., model.rect.y + model.rect.h / 2.) };
        let point = typing.at(cx, at_in.0, at_in.1);
        typing.click(cx, point);
        assert_eq!(selected(&typing, cx).map(|(_, source)| source), Some(model.source.clone()), "case {at}");
        let status = typing.status(cx).unwrap_or_default();
        assert!(status.starts_with("Model selected"), "case {at}: {status}");
        let problem = label_of(&typing, cx, INSPECTOR_MODEL_PROBLEM_ID);
        if matches!(why, Some(why) if why.starts_with("This")) {
            assert_eq!(problem.as_deref(), *why, "case {at}: the form says why too");
        }
        let named = label_of(&typing, cx, stagepack_studio::inspector::INSPECTOR_MESSAGE_ID);
        assert_eq!(named.is_some(), tag.contains("src="), "case {at}: its file is named if it has one: {named:?}");
        let before = typing.source(cx);
        if placed {
            typing.press(cx, "right");
            let after = typing.source(cx);
            assert_ne!(after, before, "case {at}: moved");
            assert!(only_its_bytes(&before, &after, &model.source), "case {at}: {after}");
            typing.press(cx, "ctrl-z");
            assert_eq!(typing.source(cx), before, "case {at}");
            let point = typing.at(cx, at_in.0, at_in.1);
            typing.click(cx, point);
        }
        typing.press(cx, "delete");
        let after = typing.source(cx);
        assert!(only_its_bytes(&before, &after, &model.source), "case {at}: {after}");
        assert_eq!(after.matches("<Model").count(), cases.len() - 1, "case {at}");
        typing.press(cx, "ctrl-z");
        assert_eq!(typing.source(cx), before, "case {at}");
    }
}

#[gpui_kit::test]
fn a_model_that_cannot_be_drawn_offers_no_turn(cx: &mut TestAppContext) {
    let Some(_alone) = alone("a_model_that_cannot_be_drawn_offers_no_turn") else { return };
    let typing = open_folder(cx, talk_folder(&talk(), b"these are words, not a model"), None);
    stills_made(&typing, cx);
    select_model(&typing, cx);
    assert!(bounds_of(&typing, cx, MODEL_TURN_ID).is_none(), "Turn isn't offered for a model its still says can't be drawn");
    // Space says why, and reads nothing.
    let put_on = model_view::models_put_on_card();
    typing.press(cx, "space");
    typing.settle(cx);
    assert_eq!(typing.status(cx).as_deref(), Some(NOT_A_MODEL));
    let canvas = editor_canvas(&typing, cx);
    assert!(cx.read(|cx| canvas.read(cx).models_turned()).is_empty(), "nothing is turned");
    assert_eq!(model_view::models_put_on_card(), put_on);
    // One that can be drawn offers it.
    let typing = editor(cx, &talk());
    select_model(&typing, cx);
    assert!(bounds_of(&typing, cx, MODEL_TURN_ID).is_some());
}

#[gpui_kit::test]
fn the_show_reads_its_models_one_at_a_time(cx: &mut TestAppContext) {
    let Some(_alone) = alone("the_show_reads_its_models_one_at_a_time") else { return };
    let tags: Vec<String> = (0..3)
        .map(|at| format!("<Model src=\"assets/box.glb\" x=\"{}\" y=\"100\" w=\"400\" h=\"300\" view=\"{FRONT}\" turn=\"auto\" />", 100 + at * 500))
        .collect();
    let source = format!("# One\n\n---\n\n# Models\n\n{}\n", tags.join("\n\n"));
    let show = rehearse(cx, &source);
    show.press(cx, "right");
    let canvas = show.canvas(cx);
    until(cx, show.audience, "every model's live picture", |cx| {
        let shown = pictures(cx, &canvas);
        shown.len() == 3 && shown.iter().all(Option::is_some)
    });
    assert_eq!(cx.read(|cx| canvas.read(cx).most_models_read_at_once()), 1, "read one at a time");
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
        ["turn", "view", "views", "src"].iter().filter_map(|name| inspector.shown(name, cx)).chain(inspector.labels()).collect::<Vec<_>>()
    });
    words.extend(fields);
    words
}

/// Whether `words` show code: a tag, a folder of the talk's, a model's file name other than as
/// the form names its file, a kind of file's name without the plain words beside it, or a view's
/// six numbers.
fn shows_code(words: &str, file_name: bool) -> bool {
    let plain = words.contains("saved as one file (.glb)") || words.contains("point cloud (.ply)");
    let lower = words.to_ascii_lowercase();
    let format = ["glTF", "GLB", "PLY"].iter().any(|name| words.contains(name)) || lower.contains(".glb") || lower.contains(".ply");
    let numbers = words.split_whitespace().filter(|word| word.parse::<f32>().is_ok()).count() >= 6;
    words.contains('<') || words.contains("assets/") || (format && !file_name && !plain) || numbers
}

#[gpui_kit::test]
fn nothing_about_a_model_shows_code(cx: &mut TestAppContext) {
    use stagepack_studio::inspector::INSPECTOR_MESSAGE_ID;
    let Some(_alone) = alone("nothing_about_a_model_shows_code") else { return };
    let work = tempfile::tempdir().unwrap();
    let typing = copying_editor(cx, PLAIN_TALK, work.path());
    let from = outside(work.path(), "box.glb", &box_model());
    choose_models(&typing, cx, vec![from]);
    stills_made(&typing, cx);
    facts(&typing, cx);
    let message = label_of(&typing, cx, INSPECTOR_MESSAGE_ID);
    assert_eq!(message.as_deref(), Some("box.glb"));
    let shown = words_shown(&typing, cx);
    for said in ["Model selected", "12 triangles", "box.glb", "Turns on its own", "No views", "Add this view", "Clear views"] {
        assert!(shown.iter().any(|words| words == said), "{said:?} among {shown:?}");
    }
    for words in shown {
        let file_name = Some(&words) == message.as_ref() && !words.contains('/');
        assert!(!shows_code(&words, file_name), "{words:?}");
    }

    // One that isn't a model, and one that needs files beside it.
    let needs_others = slides_model::glb_from(r#"{"asset":{"version":"2.0"},"buffers":[{"uri":"other.bin","byteLength":4}]}"#, &[]);
    for (bytes, why) in [
        (b"these are words, not a model".to_vec(), NOT_A_MODEL),
        (needs_others, "This model needs other files kept beside it, so it can't be drawn. A model saved as one file (.glb) with everything inside it can."),
    ] {
        let typing = open_folder(cx, talk_folder(&talk(), &bytes), None);
        stills_made(&typing, cx);
        select_model(&typing, cx);
        let message = label_of(&typing, cx, INSPECTOR_MESSAGE_ID);
        let shown = words_shown(&typing, cx);
        assert!(shown.iter().any(|words| words.contains(why)), "{why:?} among {shown:?}");
        for words in shown {
            let file_name = Some(&words) == message.as_ref() && !words.contains('/');
            assert!(!shows_code(&words, file_name), "{words:?}");
        }
    }
    assert!(shows_code("Only GLB files", false));
    assert!(shows_code("0 20 3.681 0 0.5 0", false));
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

#[gpui_kit::test]
fn a_model_goes_on_the_slide_it_was_asked_for_on(cx: &mut TestAppContext) {
    let Some(_alone) = alone("a_model_goes_on_the_slide_it_was_asked_for_on") else { return };
    let work = tempfile::tempdir().unwrap();
    let typing = copying_editor(cx, PLAIN_TALK, work.path());
    let from = outside(work.path(), "box.glb", &box_model());
    let view = typing.view.clone();
    cx.update(|cx| view.update(cx, |view, cx| view.chosen_models(vec![from], cx)));
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
fn a_cancelled_model_copy_inserts_nothing(cx: &mut TestAppContext) {
    let Some(_alone) = alone("a_cancelled_model_copy_inserts_nothing") else { return };
    let work = tempfile::tempdir().unwrap();
    let typing = copying_editor(cx, PLAIN_TALK, work.path());
    // A point cloud, which is read whole to frame it before it's copied: Cancel stops that too.
    let from = outside(work.path(), "cloud.ply", &slides_model::make_test_ply(2000, true));
    let view = typing.view.clone();
    cx.update(|cx| view.update(cx, |view, cx| view.chosen_models(vec![from], cx)));
    let label = typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        let label = window.find(workspace_view::PROGRESS_ID).label().map(str::to_string);
        window.click(workspace_view::CANCEL_PROGRESS_ID, cx);
        label
    });
    assert_eq!(label.as_deref(), Some("Copying model… 0%"));
    cx.run_until_parked();
    typing.settle(cx);
    assert_eq!(typing.source(cx), PLAIN_TALK);
    assert!(!can_undo(&typing, cx));
    assert_eq!(typing.status(cx).as_deref(), Some("The copy was cancelled, so nothing was added."));
    let root = work.path().join("recovery");
    assert!(files_under(&root).iter().all(|name| !name.ends_with(".ply")), "{:?}", files_under(&root));
}

#[gpui_kit::test]
fn a_model_copy_cancelled_part_way_through_framing_ends_and_lets_the_next_in(cx: &mut TestAppContext) {
    let Some(_alone) = alone("a_model_copy_cancelled_part_way_through_framing_ends_and_lets_the_next_in") else { return };
    let work = tempfile::tempdir().unwrap();
    let typing = copying_editor(cx, PLAIN_TALK, work.path());
    // A binary cloud of some 300 KB, read whole to frame it, stopped as its Cancel stops it a
    // third of the way through its points, which are read a record at a time for exact lengths.
    let cloud = slides_model::make_test_ply(20_000, true);
    let from = outside(work.path(), "cloud.ply", &cloud);
    let stopping = workspace_view::stop_framing_after(cloud.len() as u64 / 3);
    let view = typing.view.clone();
    cx.update(|cx| view.update(cx, |view, cx| view.chosen_models(vec![from], cx)));
    // It ends, however long the test executor takes to run it.
    let began = Instant::now();
    loop {
        cx.run_until_parked();
        if typing.status(cx).as_deref() == Some("The copy was cancelled, so nothing was added.") {
            break;
        }
        assert!(began.elapsed() < Duration::from_secs(30), "the copy never ended: {:?}", typing.status(cx));
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(stopping);
    typing.settle(cx);
    assert_eq!(typing.source(cx), PLAIN_TALK);
    assert!(!can_undo(&typing, cx));
    let root = work.path().join("recovery");
    assert!(files_under(&root).iter().all(|name| !name.ends_with(".ply")), "{:?}", files_under(&root));
    // The next one goes in.
    let from = outside(work.path(), "box.glb", &box_model());
    choose_models(&typing, cx, vec![from]);
    assert_eq!(typing.source(cx), with_inserted(INSERTED));
}
