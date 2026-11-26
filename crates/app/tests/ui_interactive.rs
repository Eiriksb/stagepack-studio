//! Where a press goes in the show: to an interactive thing under it, which keeps it from the show,
//! or else to the show as a click. The keys stay the show's, an interactive thing is outlined while
//! the pointer is over it, and the pointer hides on a slide with nothing to press.
//!
//! The interactive thing is a plain box the tests put on a slide, which says what reached it, and,
//! where it matters, a model. And trying a button or a component in the editor, which works it by hand
//! there as the show does.

#[path = "support/typing.rs"]
mod typing;
#[path = "support/components.rs"]
mod components;

use std::ops::Range;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use tempfile::TempDir;

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Entity, InputEvent, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, RenderImage, TestAppContext, point, px,
};
use slides_components::{Bundle, compile, source_hash};
use slides_engine::{BUTTON_ON_NOTHING, Fonts, Rect};
use slides_model::{AdapterKind, Drawer, DrawerOptions};
use stagepack_studio::document::Document;
use stagepack_studio::model_view;
use stagepack_studio::present::Place;
use stagepack_studio::present_view::{
    AudienceView, BUTTON_PRESSED_ID, COUNTER_ID, INTERACTIVE_HOVER_ID, MAX_EVENTS, PRESSED_DARKER, NEXT_ID, POINTER_HIDES_AFTER, PressedIn, Presentation, StandInPress,
    start_show,
};
use toolkit::{Event, PointerKind};
use stagepack_studio::inspector;
use stagepack_studio::slide_canvas::SlideCanvas;
use stagepack_studio::slide_editor::{SlideEditor, TRY_DONE_ID, TRY_ID};
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
    _folder: Option<TempDir>,
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
    show_of(cx, Some(folder), document, show)
}

/// The show `show` of `document`, in the folder `folder` if the show keeps it.
fn show_of(cx: &mut TestAppContext, folder: Option<TempDir>, document: Entity<Document>, show: Entity<Presentation>) -> Show {
    let (audience, presenter) = cx.read(|cx| {
        let show = show.read(cx);
        (show.audience().expect("its window").0, show.presenter().expect("its window").0)
    });
    let show = Show { _folder: folder, document, show, audience, presenter };
    show.settle(cx);
    show
}

/// A rehearsal started from the talk's window, as *Rehearse* does: the window shares its live
/// terminals with the show and says on its status line what the show says.
fn rehearse_from(typing: &typing::Typing, cx: &mut TestAppContext) -> Show {
    typing.press(cx, "shift-f5");
    cx.run_until_parked();
    let show = typing.view.read_with(cx, |view, _| view.current_show()).expect("a show was started");
    show_of(cx, None, typing.document(cx), show)
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

impl Show {
    /// Where the pressed look is in `window`, while it's drawn.
    fn pressed_look(&self, cx: &mut TestAppContext, window: AnyWindowHandle) -> Option<gpui_kit::Bounds<Pixels>> {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.try_find(BUTTON_PRESSED_ID).map(|shade| shade.bounds())
        })
        .unwrap()
    }
}

#[gpui_kit::test]
fn a_button_looks_pressed_only_while_the_pointer_is_down_on_it(cx: &mut TestAppContext) {
    assert_eq!(PRESSED_DARKER, 0.15, "darker by 15 %");
    let show = rehearse(cx, &stepped("2"));
    let canvas = show.canvas(cx);
    let before = fingerprints(cx, &canvas);
    let on = show.at(cx, &canvas, ON_IT);
    let off = show.at(cx, &canvas, BESIDE_IT);
    show.move_to(cx, show.audience, on, None);
    assert!(show.pressed_look(cx, show.audience).is_none(), "not before it's pressed");

    // Down on it: it looks pressed, over its box, in this window only and never in the slide.
    show.down(cx, show.audience, on);
    let shade = show.pressed_look(cx, show.audience).expect("it looks pressed");
    let (top_left, bottom_right) = (show.at(cx, &canvas, (BOX.x, BOX.y)), show.at(cx, &canvas, (BOX.x + BOX.w, BOX.y + BOX.h)));
    let near = |a: Pixels, b: Pixels| (f32::from(a) - f32::from(b)).abs() <= 1.5;
    assert!(near(shade.origin.x, top_left.x) && near(shade.origin.y, top_left.y), "{shade:?} from {top_left:?}");
    assert!(near(shade.origin.x + shade.size.width, bottom_right.x) && near(shade.origin.y + shade.size.height, bottom_right.y));
    assert_eq!(fingerprints(cx, &canvas), before, "the slide's layers are as they were");
    assert!(show.pressed_look(cx, show.presenter).is_none());

    // Leaving it with the button down, it looks as it did; back over it, pressed again.
    show.move_to(cx, show.audience, off, Some(MouseButton::Left));
    assert!(show.pressed_look(cx, show.audience).is_none(), "not while the pointer is off it");
    show.move_to(cx, show.audience, on, Some(MouseButton::Left));
    assert!(show.pressed_look(cx, show.audience).is_some());

    // Let go of: as it was, and it acted.
    show.up(cx, show.audience, on);
    show.settle(cx);
    assert!(show.pressed_look(cx, show.audience).is_none(), "not once let go of");
    assert_eq!(place(&show, cx), Place { slide: 0, step: 2 });

    // Let go of off it: as it was, and it didn't act.
    show.show.update(cx, |show, cx| show.go_to_step(0, cx));
    show.move_to(cx, show.audience, on, None);
    show.down(cx, show.audience, on);
    show.move_to(cx, show.audience, off, Some(MouseButton::Left));
    show.up(cx, show.audience, off);
    show.settle(cx);
    show.move_to(cx, show.audience, on, None);
    assert!(show.pressed_look(cx, show.audience).is_none());
    assert_eq!(place(&show, cx), Place { slide: 0, step: 0 });

    // In the presenter's window it looks pressed there, and not in the audience's.
    let theirs = show.presenter_canvas(cx);
    let at = show.at(cx, &theirs, ON_IT);
    show.move_to(cx, show.presenter, at, None);
    show.down(cx, show.presenter, at);
    assert!(show.pressed_look(cx, show.presenter).is_some());
    assert!(show.pressed_look(cx, show.audience).is_none());
    show.up(cx, show.presenter, at);
    assert!(show.pressed_look(cx, show.presenter).is_none());
}

/// A talk's own component that counts: a press let go of on its plus adds one, and the action
/// `add` adds its value, and `reset` makes it 0. Its count is on its left, and its plus on its right.
const COUNTER: &str = r#"use toolkit::prelude::*;

#[component(interactive)]
fn Counter(ctx: &Ctx) -> Element {
    let mut count: u32 = ctx.state();
    for event in ctx.events() {
        match event {
            Event::Pointer { kind: PointerKind::Up, hit: Some(hit), .. } if hit == "plus" => count += 1,
            Event::Action { name, value } if name == "add" => count += value.parse::<u32>().unwrap_or(0),
            Event::Action { name, .. } if name == "reset" => count = 0,
            _ => {}
        }
    }
    ctx.set_state(&count);
    let plus = shape(ShapeKind::Rect, 200.0, 200.0).fill(Color::rgb(37, 99, 235));
    canvas(400.0, 200.0, vec![at(0.0, 0.0, text(count.to_string()).size(64.0)), at(200.0, 0.0, hit("plus", plus))])
}
"#;

/// A counter at the top left of a slide, and where its plus and the space beside it are.
const COUNTER_TAG: &str = r#"<Counter id="c" x="200" y="200" w="400" h="200" />"#;
const PLUS: (f32, f32) = (500., 300.);
const BESIDE_PLUS: (f32, f32) = (300., 300.);

/// [`COUNTER`], built with the compiler bundle once for all the tests, or `None` if there's no
/// bundle, which the tests that need it say and skip.
fn built_counter() -> Option<&'static [u8]> {
    static BUILT: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    let built = BUILT.get_or_init(|| {
        let bundled = std::env::var_os("STUDIO_BUNDLE").is_some() && Bundle::find().is_some();
        if !bundled {
            return None;
        }
        let bundle = Bundle::find().expect("a bundle");
        Some(compile(&bundle, "components/counter.rs", COUNTER).unwrap_or_else(|errors| panic!("the counter doesn't build: {errors:?}")))
    });
    if built.is_none() {
        eprintln!("skipping the counter: no compiler bundle (set STUDIO_BUNDLE)");
    }
    built.as_deref()
}

/// A rehearsal of `source`, with the counter built in its folder: `None` with no compiler bundle.
fn with_counter(cx: &mut TestAppContext, source: &str) -> Option<Show> {
    let wasm = built_counter()?;
    let index = format!(r#"{{"format":1,"components":[{{"tag":"Counter","file":"counter","source_hash":"{}"}}]}}"#, source_hash(COUNTER.as_bytes()));
    let files: [(&str, &[u8]); 3] = [("components/counter.rs", COUNTER.as_bytes()), ("build/counter.wasm", wasm), ("build/index.json", index.as_bytes())];
    Some(rehearse_with(cx, source, &files))
}

/// Where the tag that starts `opening` is written in `source`, `/>` and all.
fn tag_at(source: &str, opening: &str) -> Range<usize> {
    let start = source.find(opening).expect("the tag is in the talk");
    start..start + source[start..].find("/>").expect("the tag closes") + 2
}

impl Show {
    /// The lines of text the layer of the tag at `source` draws now.
    fn words(&self, cx: &mut TestAppContext, source: &Range<usize>) -> Vec<String> {
        self.show.update(cx, |show, cx| show.words_on(source, cx))
    }

    /// A click on the slide's point `(x, y)` in the audience's window, and time for what reached a
    /// component to be drawn.
    fn press_slide(&self, cx: &mut TestAppContext, slide_at: (f32, f32)) {
        self.click_slide(cx, slide_at);
        self.wait(cx, 50);
    }

    fn reload(&self, cx: &mut TestAppContext, source: &str) {
        self.document.update(cx, |document, cx| {
            document.reload(Some(source.to_string()));
            cx.notify();
        });
        cx.run_until_parked();
        self.settle(cx);
    }
}

#[gpui_kit::test]
fn a_counter_counts_clicks_on_its_plus(cx: &mut TestAppContext) {
    let source = format!("# One\n\n{COUNTER_TAG}\n\n---\n\n# Two\n");
    let Some(show) = with_counter(cx, &source) else { return };
    let counter = tag_at(&source, "<Counter");
    assert_eq!(show.words(cx, &counter), ["0"]);
    show.press_slide(cx, PLUS);
    show.press_slide(cx, PLUS);
    assert_eq!(show.words(cx, &counter), ["2"]);
    // Beside the plus, inside the counter, nothing changes.
    show.press_slide(cx, BESIDE_PLUS);
    assert_eq!(show.words(cx, &counter), ["2"]);
    assert_eq!(place(&show, cx), Place { slide: 0, step: 0 }, "the show didn't move");
    assert_eq!(show.counter(cx), "1 / 2");
}

#[gpui_kit::test]
fn leaving_the_slide_clears_state(cx: &mut TestAppContext) {
    let source = format!("# One\n\n{COUNTER_TAG}\n\n---\n\n# Two\n");
    let Some(show) = with_counter(cx, &source) else { return };
    let counter = tag_at(&source, "<Counter");
    show.press_slide(cx, PLUS);
    assert_eq!(show.words(cx, &counter), ["1"]);
    show.show.update(cx, |show, cx| show.next(cx));
    show.settle(cx);
    assert_eq!(place(&show, cx), Place { slide: 1, step: 0 });
    show.show.update(cx, |show, cx| show.previous(cx));
    show.settle(cx);
    assert_eq!(show.words(cx, &counter), ["0"]);
    assert!(cx.read(|cx| show.show.read(cx).components().is_empty()));
}

#[gpui_kit::test]
fn a_do_button_sends_its_action(cx: &mut TestAppContext) {
    let source = format!("# One\n\n{COUNTER_TAG}\n\n<Button label=\"Add\" do=\"add\" on=\"c\" value=\"5\" x=\"800\" y=\"600\" w=\"300\" h=\"100\" />\n\n---\n\n# Two\n");
    let Some(show) = with_counter(cx, &source) else { return };
    let counter = tag_at(&source, "<Counter");
    assert!(!show.document.update(cx, |document, _| document.notices()).iter().any(|notice| notice == BUTTON_ON_NOTHING), "its `on` names the counter");
    show.press_slide(cx, (950., 650.));
    assert_eq!(show.words(cx, &counter), ["5"]);
    assert_eq!(place(&show, cx), Place { slide: 0, step: 0 });
}

/// A component of interface version 2 that says it's interactive and is a plain pad, named `pad`
/// under the pointer, and stops working when it's given what `trap_on` says, if it says anything. It
/// says it looks at the click step, so each of its slide's clicks is laid out apart.
fn pad_files(trap_on: Option<&str>) -> Vec<(&'static str, Vec<u8>)> {
    let schema = r#"{"name":"Pad","props":[],"children":false,"interactive":true}"#;
    let element = r#"{"Hit":{"name":"pad","child":{"Shape":{"kind":"Rect","width":400.0,"height":200.0,"fill":{"r":37,"g":99,"b":235,"a":255},"outline":null,"outline_width":0.0,"radius":0.0,"padding":0.0,"child":null}}}}"#;
    let index = r#"{"format":1,"components":[{"tag":"Pad","file":"pad","source_hash":"built from nothing here"}]}"#;
    vec![("build/pad.wasm", components::interactive_module(schema, element, trap_on, true)), ("build/index.json", index.as_bytes().to_vec())]
}

const PAD_SLIDES: &str = "# One\n\n<Pad x=\"200\" y=\"200\" w=\"400\" h=\"200\" />\n\n---\n\n# Two\n";

/// [`PAD_SLIDES`] with a box coming in on a click after the pad: the presenter's window shows that
/// click of the same slide as what's next.
const PAD_AND_A_CLICK: &str = "# One\n\n<Pad x=\"200\" y=\"200\" w=\"400\" h=\"200\" />\n\n<Box x=\"1200\" y=\"700\" w=\"200\" h=\"100\" click=\"1\">\n\nLater\n\n</Box>\n\n---\n\n# Two\n";

fn with_the_pad(cx: &mut TestAppContext, trap_on: Option<&str>) -> Show {
    with_the_pad_in(cx, PAD_SLIDES, trap_on)
}

fn with_the_pad_in(cx: &mut TestAppContext, source: &str, trap_on: Option<&str>) -> Show {
    let files = pad_files(trap_on);
    let files: Vec<(&str, &[u8])> = files.iter().map(|(name, bytes)| (*name, bytes.as_slice())).collect();
    rehearse_with(cx, source, &files)
}

#[gpui_kit::test]
fn events_are_coalesced_to_sixty_renders_a_second(cx: &mut TestAppContext) {
    // On a slide with a further click, which the presenter's window shows as what's next: the
    // module runs once for each time it's given something, however many windows lay it out.
    let show = with_the_pad_in(cx, PAD_AND_A_CLICK, None);
    let pad = tag_at(PAD_AND_A_CLICK, "<Pad");
    let next = cx.read(|cx| show.show.read(cx).presenter().expect("the presenter's window").1.read(cx).upcoming().clone());
    assert!(cx.read(|cx| !next.read(cx).images().is_empty()), "the presenter's window shows the next click");
    let canvas = show.canvas(cx);
    let along = |i: usize| 250. + i as f32 * 0.3;
    // 1,000 moves over it in a second.
    for i in 0..1000 {
        let at = show.at(cx, &canvas, (along(i), 300.));
        cx.update_window(show.audience, |_, window, cx| {
            let event = MouseMoveEvent { position: at, pressed_button: None, modifiers: Modifiers::default() };
            window.dispatch_event(event.to_platform_input(), cx);
        })
        .unwrap();
        cx.executor().advance_clock(Duration::from_millis(1));
        cx.run_until_parked();
    }
    show.wait(cx, 100);
    let (renders, last) = cx.read(|cx| {
        let components = show.show.read(cx).components();
        (components.renders(&pad), components.last_fed(&pad).and_then(|fed| fed.events.last().cloned()))
    });
    // Each render is a run of the module.
    assert!((30..=61).contains(&renders), "{renders} runs of the module");
    let Some(Event::Pointer { kind: PointerKind::Move, x, hit, .. }) = last else { panic!("the last move wasn't given it: {last:?}") };
    assert!((x - (along(999) - 200.)).abs() < 2.0, "the last move, at {x}");
    assert_eq!(hit.as_deref(), Some("pad"));
    assert_eq!(place(&show, cx), Place { slide: 0, step: 0 });
}

#[gpui_kit::test]
fn a_component_that_fails_under_events_shows_its_problem_and_the_show_moves_on(cx: &mut TestAppContext) {
    // Its first render with events is fine; it stops working on its second, which is given an Up.
    let show = with_the_pad(cx, Some(r#""Up""#));
    let pad = tag_at(PAD_SLIDES, "<Pad");
    let down = |x: f32| Event::Pointer { kind: PointerKind::Down, x, y: 10., hit: Some("pad".into()) };
    let up = |x: f32| Event::Pointer { kind: PointerKind::Up, x, y: 10., hit: Some("pad".into()) };
    show.show.update(cx, |show, cx| show.send_to(&pad, down(0.), cx));
    show.settle(cx);
    assert_eq!(cx.read(|cx| show.show.read(cx).components().renders(&pad)), 1);
    // A flood before it can be drawn again: no more than the most wait.
    show.show.update(cx, |show, cx| {
        for i in 0..500 {
            show.send_to(&pad, if i % 2 == 0 { down(i as f32) } else { up(i as f32) }, cx);
        }
    });
    assert!(cx.read(|cx| show.show.read(cx).components().queued(&pad)) <= MAX_EVENTS);
    show.wait(cx, 50);
    assert!(show.words(cx, &pad).iter().any(|line| line.contains("Pad: stopped working")), "{:?}", show.words(cx, &pad));
    // What comes after is dropped, not queued.
    show.show.update(cx, |show, cx| {
        for i in 0..500 {
            show.send_to(&pad, down(i as f32), cx);
        }
    });
    show.wait(cx, 50);
    let (renders, queued) = cx.read(|cx| {
        let components = show.show.read(cx).components();
        (components.renders(&pad), components.queued(&pad))
    });
    assert_eq!((renders, queued), (2, 0));
    cx.simulate_keystrokes(show.audience, "right");
    show.settle(cx);
    assert_eq!(show.counter(cx), "2 / 2");
}

#[gpui_kit::test]
fn an_edit_keeps_state_only_for_an_unchanged_component(cx: &mut TestAppContext) {
    let second = r#"<Counter id="d" x="200" y="600" w="400" h="200" />"#;
    let button = r#"<Button label="Add" do="add" on="d" x="800" y="600" w="300" h="100" />"#;
    let source = format!("# One\n\n{COUNTER_TAG}\n\n{second}\n\n{button}\n\n---\n\n# Two\n");
    let Some(show) = with_counter(cx, &source) else { return };
    let (first, second_at) = (tag_at(&source, "<Counter id=\"c\""), tag_at(&source, "<Counter id=\"d\""));
    show.press_slide(cx, PLUS);
    show.press_slide(cx, (PLUS.0, PLUS.1 + 400.));
    assert_eq!((show.words(cx, &first), show.words(cx, &second_at)), (vec!["1".to_string()], vec!["1".to_string()]));

    // Saved elsewhere with the second counter's id changed: the first keeps its count, as it's
    // written as it was where it was, and the second starts again. The button names nothing now.
    let edited = source.replace(r#"id="d""#, r#"id="e""#);
    show.reload(cx, &edited);
    let second_now = tag_at(&edited, "<Counter id=\"e\"");
    assert_eq!(show.words(cx, &first), ["1"]);
    assert_eq!(show.words(cx, &second_now), ["0"]);
    assert!(show.document.update(cx, |document, _| document.notices()).iter().any(|notice| notice == BUTTON_ON_NOTHING));
    show.press_slide(cx, PLUS);
    assert_eq!(show.words(cx, &first), ["2"]);
}

#[gpui_kit::test]
fn an_interactive_component_inside_another_tag_takes_no_presses(cx: &mut TestAppContext) {
    let source = "# One\n\n<Box x=\"200\" y=\"200\" w=\"400\" h=\"200\">\n\n<Pad />\n\n</Box>\n\n---\n\n# Two\n";
    let show = with_the_pad_in(cx, source, None);
    let notices = show.document.update(cx, |document, _| document.notices());
    assert!(notices.iter().any(|notice| notice == slides_engine::INTERACTIVE_NOT_PLACED), "{notices:?}");
    assert_eq!(slides_engine::INTERACTIVE_NOT_PLACED, "An interactive component works only when it's placed on the slide itself.");
    // The box it's in isn't interactive either: a click on it is the show's.
    show.click_slide(cx, (400., 300.));
    assert_eq!(show.counter(cx), "2 / 2");
    assert!(cx.read(|cx| show.show.read(cx).components().is_empty()));
}

// Trying in the editor.

/// A window on `source`, in a folder holding `files` as well.
fn edit_with(cx: &mut TestAppContext, source: &str, files: &[(&str, &[u8])]) -> typing::Typing {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("slides.md"), source).unwrap();
    for (name, bytes) in files {
        let path = folder.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    typing::open_folder(cx, folder, None)
}

/// A window on `source`, with the counter built in its folder: `None` with no compiler bundle.
fn edit_counter(cx: &mut TestAppContext, source: &str) -> Option<typing::Typing> {
    let wasm = built_counter()?;
    let index = format!(r#"{{"format":1,"components":[{{"tag":"Counter","file":"counter","source_hash":"{}"}}]}}"#, source_hash(COUNTER.as_bytes()));
    let files: [(&str, &[u8]); 3] = [("components/counter.rs", COUNTER.as_bytes()), ("build/counter.wasm", wasm), ("build/index.json", index.as_bytes())];
    Some(edit_with(cx, source, &files))
}

/// A window on `source`, with the pad in its folder.
fn edit_pad(cx: &mut TestAppContext, source: &str) -> typing::Typing {
    let files = pad_files(None);
    let files: Vec<(&str, &[u8])> = files.iter().map(|(name, bytes)| (*name, bytes.as_slice())).collect();
    edit_with(cx, source, &files)
}

fn slide_editor(typing: &typing::Typing, cx: &TestAppContext) -> Entity<SlideEditor> {
    cx.read(|cx| typing.view.read(cx).slide_editor().clone())
}

/// What's being tried in the editor, by where its tag is written.
fn trying(typing: &typing::Typing, cx: &TestAppContext) -> Option<Range<usize>> {
    let editor = slide_editor(typing, cx);
    cx.read(|cx| editor.read(cx).trying())
}

/// The lines of text the layer of the tag at `source` draws in the editor now.
fn edited_words(typing: &typing::Typing, cx: &mut TestAppContext, source: &Range<usize>) -> Vec<String> {
    let editor = slide_editor(typing, cx);
    editor.update(cx, |editor, cx| editor.words_on(source, cx))
}

/// A click on the slide's point `(x, y)` in the editor, and time for what reached a component to be
/// drawn.
fn click_editor(typing: &typing::Typing, cx: &mut TestAppContext, (x, y): (f32, f32)) {
    let at = typing.at(cx, x, y);
    typing.click(cx, at);
    typing::wait(cx, 50);
    typing.settle(cx);
}

fn selected_source(typing: &typing::Typing, cx: &TestAppContext) -> Option<Range<usize>> {
    typing::selected(typing, cx).map(|(_, source)| source)
}

const ELSEWHERE: &str = "<Box x=\"1200\" y=\"700\" w=\"300\" h=\"200\">\n\nElse\n\n</Box>";

#[gpui_kit::test]
fn try_lets_the_counter_count_in_the_editor_and_selection_still_works_after(cx: &mut TestAppContext) {
    let source = format!("# One\n\n{COUNTER_TAG}\n\n{ELSEWHERE}\n");
    let Some(typing) = edit_counter(cx, &source) else { return };
    let counter = tag_at(&source, "<Counter");
    // Not tried, a click on its plus selects it, and counts nothing.
    click_editor(&typing, cx, PLUS);
    assert_eq!(selected_source(&typing, cx), Some(counter.clone()));
    assert_eq!(edited_words(&typing, cx, &counter), ["0"]);
    assert!(typing::bounds_of(&typing, cx, TRY_ID).is_some(), "Try shows on the counter selected");
    assert!(typing::bounds_of(&typing, cx, TRY_DONE_ID).is_none());
    typing::click_on(&typing, cx, TRY_ID);
    assert_eq!(trying(&typing, cx), Some(counter.clone()));
    assert!(typing::bounds_of(&typing, cx, TRY_DONE_ID).is_some(), "Done shows while it's tried");
    click_editor(&typing, cx, PLUS);
    click_editor(&typing, cx, PLUS);
    assert_eq!(edited_words(&typing, cx, &counter), ["2"]);
    // Beside the plus, inside the counter, nothing changes, and nothing moved or was written.
    click_editor(&typing, cx, BESIDE_PLUS);
    assert_eq!(edited_words(&typing, cx, &counter), ["2"]);
    assert_eq!(typing.source(cx), source, "nothing written");
    assert_eq!(selected_source(&typing, cx), Some(counter.clone()), "still selected");
    // Done: it's drawn as the talk has it again.
    typing::click_on(&typing, cx, TRY_DONE_ID);
    assert_eq!(trying(&typing, cx), None);
    assert_eq!(edited_words(&typing, cx, &counter), ["0"]);
    // And a click selects again: the box, then the counter, by its plus.
    click_editor(&typing, cx, (1350., 800.));
    let boxed = selected_source(&typing, cx).expect("the box is selected");
    assert!(source[boxed].starts_with("<Box"));
    click_editor(&typing, cx, PLUS);
    assert_eq!(selected_source(&typing, cx), Some(counter.clone()));
    assert_eq!(edited_words(&typing, cx, &counter), ["0"]);
}

#[gpui_kit::test]
fn space_tries_and_stops_a_selected_component(cx: &mut TestAppContext) {
    let typing = edit_pad(cx, PAD_SLIDES);
    let pad = tag_at(PAD_SLIDES, "<Pad");
    click_editor(&typing, cx, (300., 300.));
    assert_eq!(selected_source(&typing, cx), Some(pad.clone()));
    let editor = slide_editor(&typing, cx);
    assert_eq!(cx.read(|cx| editor.read(cx).tried_components().renders(&pad)), 0, "a click selecting it isn't the pad's");
    typing.press(cx, "space");
    assert_eq!(trying(&typing, cx), Some(pad.clone()));
    click_editor(&typing, cx, (300., 300.));
    assert!(cx.read(|cx| editor.read(cx).tried_components().renders(&pad)) >= 1, "the press reached it");
    typing.press(cx, "space");
    assert_eq!(trying(&typing, cx), None);
    assert!(cx.read(|cx| editor.read(cx).tried_components().is_empty()));
    assert!(typing.editing(cx).is_none(), "Space typed nothing");
    assert_eq!(typing.source(cx), PAD_SLIDES);
    assert_eq!(selected_source(&typing, cx), Some(pad));
}

#[gpui_kit::test]
fn a_run_or_step_button_does_nothing_in_the_editor_and_its_form_says_so(cx: &mut TestAppContext) {
    let source = "# One\n\n<Terminal id=\"t\" run=\"echo 31367\" cols=\"40\" rows=\"6\" x=\"1000\" y=\"100\" w=\"640\" h=\"360\" />\n\n\
        <Button label=\"Run\" run=\"echo 31368\" in=\"t\" x=\"200\" y=\"200\" w=\"400\" h=\"200\" />\n\n\
        <Button label=\"Go\" step=\"1\" x=\"200\" y=\"600\" w=\"400\" h=\"200\" />\n\n\
        <Box x=\"1200\" y=\"700\" w=\"200\" h=\"100\" click=\"1\">\n\nLater\n\n</Box>\n\n---\n\n# Two\n";
    let typing = edit_with(cx, source, &[]);
    let live = typing.view.read_with(cx, |view, _| view.live());
    for (at, opening) in [((400., 300.), "<Button label=\"Run\""), ((400., 700.), "<Button label=\"Go\"")] {
        click_editor(&typing, cx, at);
        assert_eq!(selected_source(&typing, cx), Some(tag_at(source, opening)));
        // Its form says what its `run` and `step` do.
        for attr in ["run", "step"] {
            let note = typing.with_window(cx, |window, cx| {
                window.render_frame(cx);
                window.try_find(gpui_kit::SharedString::from(inspector::note_id(attr))).and_then(|found| found.label().map(str::to_string))
            });
            assert_eq!(note.as_deref(), Some(inspector::RUNS_IN_SHOW), "under {attr}");
        }
        typing::click_on(&typing, cx, TRY_ID);
        assert_eq!(trying(&typing, cx), Some(tag_at(source, opening)));
        click_editor(&typing, cx, at);
        assert_eq!(typing.status(cx).as_deref(), Some(inspector::RUNS_IN_SHOW));
        assert_eq!(live.running(), 0, "no terminal started");
        assert_eq!(live.sessions(), 0);
        assert_eq!(typing.current(cx), 0, "the editor stays on its slide");
        assert_eq!(typing.source(cx), source, "nothing written");
        typing::click_on(&typing, cx, TRY_DONE_ID);
    }
    assert_eq!(inspector::RUNS_IN_SHOW, "Runs in the show.");
}

#[gpui_kit::test]
fn a_do_button_tried_in_the_editor_reaches_a_target_being_tried(cx: &mut TestAppContext) {
    let source = "# One\n\n<Pad id=\"p\" x=\"200\" y=\"200\" w=\"400\" h=\"200\" />\n\n<Button label=\"Go\" do=\"go\" on=\"p\" value=\"7\" x=\"800\" y=\"600\" w=\"300\" h=\"100\" />\n\n---\n\n# Two\n";
    let typing = edit_pad(cx, source);
    let pad = tag_at(source, "<Pad");
    let button = tag_at(source, "<Button");
    let editor = slide_editor(&typing, cx);
    click_editor(&typing, cx, (950., 650.));
    assert_eq!(selected_source(&typing, cx), Some(button.clone()));
    // Not tried, a press selects it and sends nothing.
    assert!(cx.read(|cx| editor.read(cx).tried_components().is_empty()));
    typing::click_on(&typing, cx, TRY_ID);
    assert_eq!(trying(&typing, cx), Some(button.clone()));
    click_editor(&typing, cx, (950., 650.));
    let fed = cx.read(|cx| editor.read(cx).tried_components().last_fed(&pad).map(|fed| fed.events.clone()));
    assert_eq!(fed, Some(vec![Event::Action { name: "go".into(), value: "7".into() }]), "the action reached the pad");
    // The pad is tried with its button: a press on it is its own, and it stays unselected.
    click_editor(&typing, cx, (300., 300.));
    let last = cx.read(|cx| editor.read(cx).tried_components().last_fed(&pad).and_then(|fed| fed.events.last().cloned()));
    assert!(matches!(last, Some(Event::Pointer { kind: PointerKind::Up, .. })), "{last:?}");
    assert_eq!(selected_source(&typing, cx), Some(button));
    assert_eq!(typing.source(cx), source);
}

#[gpui_kit::test]
fn selecting_something_else_stops_trying_and_clears_its_state(cx: &mut TestAppContext) {
    let source = format!("# One\n\n{COUNTER_TAG}\n\n{ELSEWHERE}\n");
    let Some(typing) = edit_counter(cx, &source) else { return };
    let counter = tag_at(&source, "<Counter");
    let editor = slide_editor(&typing, cx);
    click_editor(&typing, cx, PLUS);
    typing::click_on(&typing, cx, TRY_ID);
    click_editor(&typing, cx, PLUS);
    assert_eq!(edited_words(&typing, cx, &counter), ["1"]);
    assert!(cx.read(|cx| editor.read(cx).tried_components().state_of(&counter).is_some()));
    // The box selected: the counter's tried no more, and what it kept is gone.
    click_editor(&typing, cx, (1350., 800.));
    assert_ne!(selected_source(&typing, cx), Some(counter.clone()));
    assert_eq!(trying(&typing, cx), None);
    assert!(cx.read(|cx| editor.read(cx).tried_components().is_empty()));
    assert!(typing::bounds_of(&typing, cx, TRY_DONE_ID).is_none());
    assert_eq!(edited_words(&typing, cx, &counter), ["0"]);
    // Tried again, it starts from nothing.
    click_editor(&typing, cx, PLUS);
    typing::click_on(&typing, cx, TRY_ID);
    click_editor(&typing, cx, PLUS);
    assert_eq!(edited_words(&typing, cx, &counter), ["1"]);
    // Escape lets go of it, which stops trying too.
    typing.press(cx, "escape");
    assert_eq!(trying(&typing, cx), None);
    assert!(cx.read(|cx| editor.read(cx).tried_components().is_empty()));
}

// What the final review found.

/// A pad `p`, and a button that sends it `go`, at (800, 600) to (1100, 700).
const PAD_AND_GO: &str = "# One\n\n<Pad id=\"p\" x=\"200\" y=\"200\" w=\"400\" h=\"200\" />\n\n<Button label=\"Go\" do=\"go\" on=\"p\" x=\"800\" y=\"600\" w=\"300\" h=\"100\" />\n\n---\n\n# Two\n";
const ON_GO: (f32, f32) = (950., 650.);

#[gpui_kit::test]
fn the_same_do_three_times_keeps_working(cx: &mut TestAppContext) {
    // The pad keeps nothing, so each press gives it what the one before did.
    let show = with_the_pad_in(cx, PAD_AND_GO, None);
    let pad = tag_at(PAD_AND_GO, "<Pad");
    for _ in 0..3 {
        show.press_slide(cx, ON_GO);
        show.wait(cx, 100);
    }
    let (renders, queued) = cx.read(|cx| {
        let components = show.show.read(cx).components();
        (components.renders(&pad), components.queued(&pad))
    });
    assert_eq!((renders, queued), (3, 0));
}

#[gpui_kit::test]
fn a_counter_reset_three_times_still_counts(cx: &mut TestAppContext) {
    let buttons = "<Button label=\"Reset\" do=\"reset\" on=\"c\" x=\"800\" y=\"600\" w=\"300\" h=\"100\" />\n\n\
        <Button label=\"Add 1\" do=\"add\" on=\"c\" value=\"1\" x=\"800\" y=\"800\" w=\"300\" h=\"100\" />";
    let source = format!("# One\n\n{COUNTER_TAG}\n\n{buttons}\n\n---\n\n# Two\n");
    let Some(show) = with_counter(cx, &source) else { return };
    let counter = tag_at(&source, "<Counter");
    for _ in 0..3 {
        show.press_slide(cx, PLUS);
    }
    assert_eq!(show.words(cx, &counter), ["3"]);
    for _ in 0..3 {
        show.press_slide(cx, (950., 650.));
        show.wait(cx, 100);
    }
    assert_eq!(show.words(cx, &counter), ["0"]);
    show.press_slide(cx, (950., 850.));
    assert_eq!(show.words(cx, &counter), ["1"]);
    show.press_slide(cx, PLUS);
    assert_eq!(show.words(cx, &counter), ["2"]);
}

#[gpui_kit::test]
fn rehearsing_twice_the_second_shows_first_press_acts(cx: &mut TestAppContext) {
    let first = with_the_pad_in(cx, PAD_AND_GO, None);
    let pad = tag_at(PAD_AND_GO, "<Pad");
    first.press_slide(cx, ON_GO);
    assert_eq!(cx.read(|cx| first.show.read(cx).components().renders(&pad)), 1);
    first.show.update(cx, |show, cx| show.end(cx));
    cx.run_until_parked();
    // The same talk, rehearsed again.
    let document = first.document.clone();
    let show = cx.update(|cx| start_show(document.clone(), true, cx));
    let second = show_of(cx, None, document, show);
    second.press_slide(cx, ON_GO);
    let (renders, queued) = cx.read(|cx| {
        let components = second.show.read(cx).components();
        (components.renders(&pad), components.queued(&pad))
    });
    assert_eq!((renders, queued), (1, 0), "the second show's first press reached the pad");
}

#[gpui_kit::test]
fn a_counter_tried_in_the_editor_is_not_the_shows(cx: &mut TestAppContext) {
    let source = format!("# One\n\n{COUNTER_TAG}\n\n{ELSEWHERE}\n");
    let Some(typing) = edit_counter(cx, &source) else { return };
    let counter = tag_at(&source, "<Counter");
    click_editor(&typing, cx, PLUS);
    typing::click_on(&typing, cx, TRY_ID);
    click_editor(&typing, cx, PLUS);
    click_editor(&typing, cx, PLUS);
    assert_eq!(edited_words(&typing, cx, &counter), ["2"]);
    // Rehearsed from the window: trying stops, and the show starts from nothing.
    let show = rehearse_from(&typing, cx);
    assert_eq!(trying(&typing, cx), None);
    assert!(cx.read(|cx| show.show.read(cx).components().is_empty()));
    assert_eq!(show.words(cx, &counter), ["0"]);
    // While the show runs nothing is tried in the editor.
    assert_eq!(selected_source(&typing, cx), Some(counter.clone()));
    typing.press(cx, "space");
    assert_eq!(trying(&typing, cx), None);
    // And the show's own counter counts from 0.
    show.press_slide(cx, PLUS);
    assert_eq!(show.words(cx, &counter), ["1"]);
}

#[gpui_kit::test]
fn a_do_button_naming_a_duplicated_id_acts_on_neither(cx: &mut TestAppContext) {
    let source = "# One\n\n<Pad id=\"p\" x=\"200\" y=\"200\" w=\"400\" h=\"200\" />\n\n<Pad id=\"p\" x=\"200\" y=\"500\" w=\"400\" h=\"200\" />\n\n\
        <Button label=\"Go\" do=\"go\" on=\"p\" x=\"800\" y=\"600\" w=\"300\" h=\"100\" />\n\n---\n\n# Two\n";
    let duplicated = slides_engine::duplicate_id_notice("p");
    let typing = edit_pad(cx, source);
    let editor = slide_editor(&typing, cx);
    // Tried in the editor, with its targets: nothing reaches either.
    click_editor(&typing, cx, ON_GO);
    typing::click_on(&typing, cx, TRY_ID);
    click_editor(&typing, cx, ON_GO);
    assert!(cx.read(|cx| editor.read(cx).tried_components().is_empty()));
    assert_eq!(typing.status(cx).as_deref(), Some(duplicated.as_str()));
    typing::click_on(&typing, cx, TRY_DONE_ID);
    // In the show: the same.
    let show = rehearse_from(&typing, cx);
    show.press_slide(cx, ON_GO);
    assert!(cx.read(|cx| show.show.read(cx).components().is_empty()), "nothing was sent");
    assert_eq!(typing.status(cx).as_deref(), Some(duplicated.as_str()));
    assert_eq!(place(&show, cx), Place { slide: 0, step: 0 });
}
