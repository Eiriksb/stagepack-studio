//! Where a press goes in the show: to an interactive thing under it, which keeps it from the show,
//! or else to the show as a click. The keys stay the show's, an interactive thing is outlined while
//! the pointer is over it, and the pointer hides on a slide with nothing to press.
//!
//! The interactive thing is a plain box the tests put on a slide, which says what reached it, and,
//! where it matters, a model.

#[path = "support/typing.rs"]
mod typing;

use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use tempfile::TempDir;

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Entity, InputEvent, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, RenderImage, TestAppContext, point, px,
};
use slides_engine::{Fonts, Rect};
use slides_model::{AdapterKind, Drawer, DrawerOptions};
use stagepack_studio::document::Document;
use stagepack_studio::model_view;
use stagepack_studio::present::Place;
use stagepack_studio::present_view::{
    AudienceView, COUNTER_ID, INTERACTIVE_HOVER_ID, NEXT_ID, POINTER_HIDES_AFTER, PressedIn, Presentation, StandInPress, start_show,
};
use stagepack_studio::slide_canvas::SlideCanvas;
use stagepack_studio::workspace_view;

const TWO_SLIDES: &str = "# One\n\n---\n\n# Two\n";

/// Where the tests' box sits on the first slide, in slide units.
const BOX: Rect = Rect { x: 200., y: 200., w: 400., h: 300. };

/// The middle of [`BOX`].
const ON_IT: (f32, f32) = (400., 350.);

/// Beside [`BOX`], on the slide.
const BESIDE_IT: (f32, f32) = (1400., 800.);

/// A show of a talk, rehearsed, in both its windows.
struct Show {
    _folder: TempDir,
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

    fn audience_view(&self, cx: &TestAppContext) -> Entity<AudienceView> {
        cx.read(|cx| self.show.read(cx).audience()).expect("the audience's window").1
    }

    fn canvas(&self, cx: &TestAppContext) -> Entity<SlideCanvas> {
        let view = self.audience_view(cx);
        cx.read(|cx| view.read(cx).current().clone())
    }

    fn presenter_canvas(&self, cx: &TestAppContext) -> Entity<SlideCanvas> {
        let view = cx.read(|cx| self.show.read(cx).presenter()).expect("the presenter's window").1;
        cx.read(|cx| view.read(cx).current().clone())
    }

    /// Puts the tests' box on slide `slide`.
    fn put_box(&self, cx: &mut TestAppContext, slide: usize) {
        self.show.update(cx, |show, cx| show.insert_for_tests(slide, BOX, cx));
        self.settle(cx);
    }

    fn presses(&self, cx: &TestAppContext) -> Vec<StandInPress> {
        cx.read(|cx| self.show.read(cx).stand_in_presses().to_vec())
    }

    fn counter(&self, cx: &mut TestAppContext) -> String {
        cx.update_window(self.presenter, |_, window, cx| {
            window.render_frame(cx);
            window.find(COUNTER_ID).label().map(str::to_string).expect("a counter")
        })
        .unwrap()
    }

    /// Where the slide's point `(x, y)` is in the window `canvas` is drawn in.
    fn at(&self, cx: &TestAppContext, canvas: &Entity<SlideCanvas>, (x, y): (f32, f32)) -> Point<Pixels> {
        let slide = cx.read(|cx| canvas.read(cx).slide_bounds()).expect("the slide is drawn");
        let unit = f32::from(slide.size.width) / 1920.;
        slide.origin + point(px(x * unit), px(y * unit))
    }

    fn mouse(&self, cx: &mut TestAppContext, window: AnyWindowHandle, event: impl InputEvent) {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.dispatch_event(event.to_platform_input(), cx);
            window.render_frame(cx);
        })
        .unwrap();
        cx.run_until_parked();
    }

    fn move_to(&self, cx: &mut TestAppContext, window: AnyWindowHandle, at: Point<Pixels>, pressed_button: Option<MouseButton>) {
        self.mouse(cx, window, MouseMoveEvent { position: at, pressed_button, modifiers: Modifiers::default() });
    }

    fn down(&self, cx: &mut TestAppContext, window: AnyWindowHandle, at: Point<Pixels>) {
        let event = MouseDownEvent { button: MouseButton::Left, position: at, modifiers: Modifiers::default(), click_count: 1, first_mouse: false };
        self.mouse(cx, window, event);
    }

    fn up(&self, cx: &mut TestAppContext, window: AnyWindowHandle, at: Point<Pixels>) {
        self.mouse(cx, window, MouseUpEvent { button: MouseButton::Left, position: at, modifiers: Modifiers::default(), click_count: 1 });
    }

    /// A click at `at` in `window`, the pointer coming there first.
    fn click(&self, cx: &mut TestAppContext, window: AnyWindowHandle, at: Point<Pixels>) {
        self.move_to(cx, window, at, None);
        self.down(cx, window, at);
        self.up(cx, window, at);
        self.settle(cx);
    }

    /// A click on the slide's point `(x, y)` in the audience's window.
    fn click_slide(&self, cx: &mut TestAppContext, slide_at: (f32, f32)) {
        let canvas = self.canvas(cx);
        let at = self.at(cx, &canvas, slide_at);
        self.click(cx, self.audience, at);
    }

    fn outline(&self, cx: &mut TestAppContext, window: AnyWindowHandle) -> Option<gpui_kit::Bounds<Pixels>> {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.try_find(INTERACTIVE_HOVER_ID).map(|outline| outline.bounds())
        })
        .unwrap()
    }

    fn pointer_hidden(&self, cx: &mut TestAppContext) -> bool {
        self.settle(cx);
        let view = self.audience_view(cx);
        cx.read(|cx| view.read(cx).pointer_hidden())
    }

    /// Lets `millis` of the test clock pass, and draws what they brought.
    fn wait(&self, cx: &mut TestAppContext, millis: u64) {
        cx.executor().advance_clock(Duration::from_millis(millis));
        cx.run_until_parked();
        self.settle(cx);
    }
}

fn set_up(cx: &mut TestAppContext) {
    cx.update(|cx: &mut App| {
        gpui_kit::init(cx);
        workspace_view::bind_keys(cx);
    });
}

/// Starts a rehearsal of `source`, in a folder holding `files` as well.
fn rehearse_with(cx: &mut TestAppContext, source: &str, files: &[(&str, &[u8])]) -> Show {
    set_up(cx);
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("slides.md"), source).unwrap();
    for (name, bytes) in files {
        let path = folder.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    let document = Document::open(folder.path(), Fonts::bundled_only()).unwrap();
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

fn rehearse(cx: &mut TestAppContext, source: &str) -> Show {
    rehearse_with(cx, source, &[])
}

/// A rehearsal of two plain slides, the tests' box on the first.
fn with_the_box(cx: &mut TestAppContext) -> Show {
    let show = rehearse(cx, TWO_SLIDES);
    show.put_box(cx, 0);
    show
}

#[gpui_kit::test]
fn a_click_on_an_interactive_thing_does_not_advance(cx: &mut TestAppContext) {
    let show = with_the_box(cx);
    show.click_slide(cx, ON_IT);
    assert_eq!(show.counter(cx), "1 / 2");
    assert_eq!(show.presses(cx), vec![StandInPress::Down(PressedIn::Audience), StandInPress::Up(PressedIn::Audience)]);
}

#[gpui_kit::test]
fn a_click_beside_it_still_advances(cx: &mut TestAppContext) {
    let show = with_the_box(cx);
    show.click_slide(cx, BESIDE_IT);
    assert_eq!(show.counter(cx), "2 / 2");
    assert!(show.presses(cx).is_empty());
}

#[gpui_kit::test]
fn a_press_that_leaves_the_button_does_nothing(cx: &mut TestAppContext) {
    let show = with_the_box(cx);
    let canvas = show.canvas(cx);
    let on = show.at(cx, &canvas, ON_IT);
    let off = on + point(px(300.), px(0.));
    show.move_to(cx, show.audience, on, None);
    show.down(cx, show.audience, on);
    show.move_to(cx, show.audience, off, Some(MouseButton::Left));
    show.up(cx, show.audience, off);
    show.settle(cx);
    assert_eq!(show.counter(cx), "1 / 2");
    assert_eq!(show.presses(cx), vec![StandInPress::Down(PressedIn::Audience)], "no Up reached it");

    // Nor does a press beside it let go of on it.
    let beside = show.at(cx, &canvas, BESIDE_IT);
    show.move_to(cx, show.audience, beside, None);
    show.down(cx, show.audience, beside);
    show.move_to(cx, show.audience, on, Some(MouseButton::Left));
    show.up(cx, show.audience, on);
    show.settle(cx);
    assert_eq!(show.counter(cx), "1 / 2");
    assert_eq!(show.presses(cx), vec![StandInPress::Down(PressedIn::Audience)]);

    // And a click after them all is a click again.
    show.click_slide(cx, BESIDE_IT);
    assert_eq!(show.counter(cx), "2 / 2");
}

#[gpui_kit::test]
fn keys_still_move_the_show_with_the_pointer_over_it(cx: &mut TestAppContext) {
    let show = with_the_box(cx);
    let canvas = show.canvas(cx);
    let on = show.at(cx, &canvas, ON_IT);
    show.move_to(cx, show.audience, on, None);
    assert!(show.outline(cx, show.audience).is_some(), "the pointer is over it");
    cx.simulate_keystrokes(show.audience, "right");
    show.settle(cx);
    assert_eq!(show.counter(cx), "2 / 2");
}

#[gpui_kit::test]
fn the_presenter_current_slide_routes_presses_too(cx: &mut TestAppContext) {
    let show = with_the_box(cx);
    let canvas = show.presenter_canvas(cx);
    let on = show.at(cx, &canvas, ON_IT);
    show.click(cx, show.presenter, on);
    assert_eq!(show.presses(cx), vec![StandInPress::Down(PressedIn::Presenter), StandInPress::Up(PressedIn::Presenter)]);
    assert_eq!(show.counter(cx), "1 / 2");

    // The next slide's picture is a still: the same place on it takes nothing.
    let next = cx
        .update_window(show.presenter, |_, window, cx| {
            window.render_frame(cx);
            window.find(NEXT_ID).bounds()
        })
        .unwrap();
    let on_next = next.origin + point(next.size.width * (ON_IT.0 / 1920.), next.size.height * (ON_IT.1 / 1080.));
    show.click(cx, show.presenter, on_next);
    assert_eq!(show.presses(cx).len(), 2);
    assert_eq!(show.counter(cx), "1 / 2");
}

fn fingerprints(cx: &TestAppContext, canvas: &Entity<SlideCanvas>) -> Vec<Vec<u8>> {
    let images: Vec<std::sync::Arc<RenderImage>> = cx.read(|cx| canvas.read(cx).images());
    images.iter().map(|image| image.as_bytes(0).expect("one frame").to_vec()).collect()
}

#[gpui_kit::test]
fn the_hover_outline_is_drawn_only_in_the_window_and_never_in_the_slide_bitmap(cx: &mut TestAppContext) {
    let show = with_the_box(cx);
    let canvas = show.canvas(cx);
    let before = fingerprints(cx, &canvas);
    assert!(!before.is_empty());
    assert!(show.outline(cx, show.audience).is_none(), "nothing is outlined until the pointer is over it");

    let on = show.at(cx, &canvas, ON_IT);
    show.move_to(cx, show.audience, on, None);
    let outline = show.outline(cx, show.audience).expect("the outline is drawn");
    // Around the box: from its top left to its bottom right.
    let (top_left, bottom_right) = (show.at(cx, &canvas, (BOX.x, BOX.y)), show.at(cx, &canvas, (BOX.x + BOX.w, BOX.y + BOX.h)));
    let near = |a: Pixels, b: Pixels| (f32::from(a) - f32::from(b)).abs() <= 1.5;
    assert!(near(outline.origin.x, top_left.x) && near(outline.origin.y, top_left.y), "{outline:?} from {top_left:?}");
    assert!(near(outline.origin.x + outline.size.width, bottom_right.x) && near(outline.origin.y + outline.size.height, bottom_right.y));
    assert_eq!(fingerprints(cx, &canvas), before, "the slide's layers are as they were");
    assert!(show.outline(cx, show.presenter).is_none(), "the presenter's pointer is elsewhere");

    // It goes with the pointer.
    let beside = show.at(cx, &canvas, BESIDE_IT);
    show.move_to(cx, show.audience, beside, None);
    assert!(show.outline(cx, show.audience).is_none());

    // The presenter's window outlines it too, under its own pointer.
    let theirs = show.presenter_canvas(cx);
    let on = show.at(cx, &theirs, ON_IT);
    show.move_to(cx, show.presenter, on, None);
    assert!(show.outline(cx, show.presenter).is_some());
    assert_eq!(fingerprints(cx, &show.canvas(cx)), before);
}

#[gpui_kit::test]
fn the_pointer_hides_after_two_seconds_on_a_plain_slide_and_stays_on_an_interactive_one(cx: &mut TestAppContext) {
    let show = rehearse(cx, TWO_SLIDES);
    let half = POINTER_HIDES_AFTER.as_millis() as u64 / 2;
    assert!(!show.pointer_hidden(cx), "shown as the show starts");
    show.wait(cx, half);
    assert!(!show.pointer_hidden(cx));
    show.wait(cx, half + 100);
    assert!(show.pointer_hidden(cx), "hidden two seconds on");

    // Moving it brings it back, for two seconds more.
    let canvas = show.canvas(cx);
    let at = show.at(cx, &canvas, BESIDE_IT);
    show.move_to(cx, show.audience, at, None);
    assert!(!show.pointer_hidden(cx));
    show.wait(cx, half);
    assert!(!show.pointer_hidden(cx));
    show.wait(cx, half + 100);
    assert!(show.pointer_hidden(cx));

    // On a slide with something to press, it shows, and stays.
    show.put_box(cx, 0);
    assert!(!show.pointer_hidden(cx));
    show.wait(cx, 3 * half);
    assert!(!show.pointer_hidden(cx));

    // On the plain one after it, it hides again.
    cx.simulate_keystrokes(show.audience, "right");
    show.settle(cx);
    show.wait(cx, 2 * half + 100);
    assert!(show.pointer_hidden(cx));
}

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// A test running alone with the app's drawer on the software adapter, or on any there is, which
/// is closed as it ends, as in `ui_model`.
struct Alone(#[allow(dead_code)] MutexGuard<'static, ()>);

impl Drop for Alone {
    fn drop(&mut self) {
        let closed = model_view::close_drawer(Duration::from_secs(30));
        if !std::thread::panicking() {
            assert!(closed, "the drawer's thread never ended");
        }
    }
}

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

#[gpui_kit::test]
fn a_click_on_a_model_does_not_advance(cx: &mut TestAppContext) {
    let Some(_alone) = alone("a_click_on_a_model_does_not_advance") else { return };
    let model = slides_model::make_test_glb(slides_model::MadeGlb::Boxes);
    let source = "# Model\n\n<Model src=\"assets/box.glb\" x=\"200\" y=\"200\" w=\"400\" h=\"300\" />\n\n---\n\n# Two\n";
    let show = rehearse_with(cx, source, &[("assets/box.glb", &model)]);
    let began = Instant::now();
    while show.document.read_with(cx, |document, _| document.stills_coming()) {
        assert!(began.elapsed() < Duration::from_secs(30), "waited too long for the stills");
        std::thread::sleep(Duration::from_millis(2));
        show.settle(cx);
    }
    show.click_slide(cx, ON_IT);
    assert_eq!(show.counter(cx), "1 / 2", "a click on the model is the model's");
    show.click_slide(cx, BESIDE_IT);
    assert_eq!(show.counter(cx), "2 / 2");
}

// The Insert tab's Button.

#[gpui_kit::test]
fn insert_button_adds_a_button_tag_and_selects_it(cx: &mut TestAppContext) {
    let editor = typing::open(cx, "# One\n\nSome words\n");
    typing::click_on(&editor, cx, stagepack_studio::ribbon::TAB_INSERT_ID);
    typing::click_on(&editor, cx, stagepack_studio::insert_tools::INSERT_BUTTON_ID);
    let source = editor.source(cx);
    let selected = editor.document(cx).read_with(cx, |document, _| document.selected().map(|selected| selected.source.clone()));
    let selected = selected.expect("the new button is selected");
    let written = &source[selected];
    assert!(written.starts_with("<Button label=\"Button\" step=\"0\" x=\""), "{written}");
    assert!(written.ends_with(" w=\"320\" h=\"96\" />"), "{written}");
    assert!(source.starts_with("# One\n\nSome words\n\n<Button "), "it goes after the slide's content: {source}");
    let (undo, notices) = editor.document(cx).update(cx, |document, _| (document.undo_label(), document.notices()));
    assert_eq!(undo, Some("Insert button"));
    assert!(notices.is_empty(), "a new button has nothing wrong with it: {notices:?}");
    let name = editor.document(cx).read_with(cx, |document, _| {
        let selected = document.selected().unwrap();
        stagepack_studio::selection::name_of(document.talk(), selected.slide, &selected.source)
    });
    assert_eq!(name.as_deref(), Some("Button"));
}

// Buttons in the show.

/// A slide of three clicks, each bringing a box, with a button at [`BOX`] that goes to click
/// `step`, then two plain slides.
fn stepped(step: &str) -> String {
    let boxes: Vec<String> =
        (1..=3).map(|click| format!("<Box x=\"{}\" y=\"700\" w=\"200\" h=\"100\" click=\"{click}\">\n\nBox {click}\n\n</Box>", 300 * click)).collect();
    format!(
        "# One\n\n{}\n\n<Button label=\"Go\" step=\"{step}\" x=\"200\" y=\"200\" w=\"400\" h=\"300\" />\n\n---\n\n# Two\n\n---\n\n# Three\n",
        boxes.join("\n\n")
    )
}

/// How many of the layers drawn on `canvas` are shown.
fn shown_layers(cx: &TestAppContext, canvas: &Entity<SlideCanvas>) -> usize {
    cx.read(|cx| canvas.read(cx).looks().iter().filter(|look| look.visible).count())
}

fn place(show: &Show, cx: &TestAppContext) -> Place {
    cx.read(|cx| show.show.read(cx).place())
}

#[gpui_kit::test]
fn a_step_button_goes_to_its_step(cx: &mut TestAppContext) {
    let show = rehearse(cx, &stepped("2"));
    assert_eq!(show.counter(cx), "1 / 3");
    let canvas = show.canvas(cx);
    let at_first = shown_layers(cx, &canvas);
    show.click_slide(cx, ON_IT);
    assert_eq!(place(&show, cx), Place { slide: 0, step: 2 });
    assert_eq!(show.counter(cx), "1 / 3", "the same slide");
    // Once the boxes have come in, the first two show, and not the third.
    show.wait(cx, 1000);
    assert_eq!(shown_layers(cx, &show.canvas(cx)), at_first + 2);
    // A click beside it goes on from there, to the third box.
    show.click_slide(cx, BESIDE_IT);
    assert_eq!(place(&show, cx), Place { slide: 0, step: 3 });
    // Pressed again, it goes back to its step.
    show.click_slide(cx, ON_IT);
    assert_eq!(place(&show, cx), Place { slide: 0, step: 2 });
    show.settle(cx);
    assert_eq!(shown_layers(cx, &show.canvas(cx)), at_first + 2);
}

#[gpui_kit::test]
fn a_step_button_past_the_last_click_does_nothing(cx: &mut TestAppContext) {
    let show = rehearse(cx, &stepped("9"));
    show.click_slide(cx, ON_IT);
    assert_eq!(place(&show, cx), Place { slide: 0, step: 0 }, "neither its step nor a click of the show's");
}

#[gpui_kit::test]
fn a_press_in_the_presenter_window_acts_once_and_both_windows_agree(cx: &mut TestAppContext) {
    let show = rehearse(cx, &stepped("2"));
    let canvas = show.presenter_canvas(cx);
    let on = show.at(cx, &canvas, ON_IT);
    show.click(cx, show.presenter, on);
    assert_eq!(place(&show, cx), Place { slide: 0, step: 2 }, "it moved once, and no click of the show's followed");
    assert_eq!(show.counter(cx), "1 / 3");
    show.wait(cx, 1000);
    let (theirs, ours) = (show.canvas(cx), show.presenter_canvas(cx));
    assert_eq!(shown_layers(cx, &theirs), shown_layers(cx, &ours), "both windows show the same");
    let looks = |canvas: &Entity<SlideCanvas>| cx.read(|cx| canvas.read(cx).looks().iter().map(|look| look.visible).collect::<Vec<_>>());
    assert_eq!(looks(&theirs), looks(&ours));
    // The audience's window held no press of it: a click there is the show's again.
    show.click_slide(cx, BESIDE_IT);
    assert_eq!(place(&show, cx), Place { slide: 0, step: 3 });
}

#[gpui_kit::test]
fn go_to_step_goes_straight_there_and_back(cx: &mut TestAppContext) {
    let show = rehearse(cx, &stepped("2"));
    let at_first = shown_layers(cx, &show.canvas(cx));
    let go = |cx: &mut TestAppContext, step: u32| {
        show.show.update(cx, |show, cx| show.go_to_step(step, cx));
        show.settle(cx);
        place(&show, cx)
    };
    assert_eq!(go(cx, 3), Place { slide: 0, step: 3 });
    assert_eq!(go(cx, 9), Place { slide: 0, step: 3 }, "no further than the slide's last click");
    show.wait(cx, 1000);
    assert_eq!(shown_layers(cx, &show.canvas(cx)), at_first + 3);
    // Going back shows the place at once, with nothing coming in.
    assert_eq!(go(cx, 1), Place { slide: 0, step: 1 });
    assert_eq!(shown_layers(cx, &show.canvas(cx)), at_first + 1);
    assert_eq!(go(cx, 0), Place { slide: 0, step: 0 });
    assert_eq!(shown_layers(cx, &show.canvas(cx)), at_first);
    // On a slide with no clicks it stays.
    cx.simulate_keystrokes(show.audience, "right right right right");
    show.settle(cx);
    assert_eq!(place(&show, cx), Place { slide: 1, step: 0 });
    assert_eq!(go(cx, 1), Place { slide: 1, step: 0 });
}

#[gpui_kit::test]
#[ignore = "Task 5"]
fn a_do_button_sends_its_action(_cx: &mut TestAppContext) {}
