use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Entity, Point, RenderImage, TestAppContext,
    WindowBounds, WindowOptions, px, size,
};
use slides_engine::Fonts;
use stagepack_studio::document::Document;
use stagepack_studio::present::{LayerLook, SlideLook, ease};
use stagepack_studio::present_view::{
    AUDIENCE_ID, AudienceView, COUNTER_ID, CURRENT_ID, NEXT_ID, NOTES_ID, Presentation,
    PresenterView, TIMER_ID, start_show,
};
use stagepack_studio::slide_canvas::SlideCanvas;
use stagepack_studio::workspace_view::{self, PRESENT_ID, TITLE_ID, WorkspaceView};

const END_OF_THE_TALK: &str = "End of the talk";

/// One slide that takes two clicks, then one that takes none.
const TWO_CLICKS: &str = "<Box x=\"0\" y=\"0\" w=\"99\" h=\"99\" fill=\"red\" click=\"1\" />\n\n\
                          <Box x=\"100\" y=\"0\" w=\"99\" h=\"99\" fill=\"red\" click=\"2\" />\n\n\
                          ---\n\n# Two\n";

/// One slide with a shape that fades in on the first click.
const FADES_IN: &str =
    "# One\n\n<Box x=\"0\" y=\"0\" w=\"99\" h=\"99\" fill=\"red\" click=\"1\" enter=\"fade\" />\n";

/// Each slide is arrived at with a fade.
const FADES: &str =
    "# One\n\n---\ntransition: fade\n---\n\n# Two\n\n---\ntransition: fade\n---\n\n# Three\n";

const PUSHES: &str = "# One\n\n---\ntransition: push\n---\n\n# Two\n";

const NO_TRANSITION: &str = "# One\n\n---\n\n# Two\n";

const THREE_SLIDES: &str = "# One\n\n---\n\n# Two\n\n---\n\n# Three\n";

fn talk(source: &str) -> Document {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), source).unwrap();
    Document::open(dir.path(), Fonts::bundled_only()).unwrap()
}

fn set_up(cx: &mut TestAppContext) {
    cx.update(|cx: &mut App| {
        gpui_kit::init(cx);
        workspace_view::bind_keys(cx);
    });
}

/// Draws until what one frame asked for has reached the screen.
fn settle(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    for _ in 0..2 {
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        cx.run_until_parked();
    }
}

/// A show with both windows open, painted and ready for keys.
struct Rehearsal {
    document: Entity<Document>,
    show: Entity<Presentation>,
    audience: AnyWindowHandle,
    presenter: AnyWindowHandle,
}

impl Rehearsal {
    fn settle(&self, cx: &mut TestAppContext) {
        settle(cx, self.audience);
        settle(cx, self.presenter);
    }

    fn audience_view(&self, cx: &TestAppContext) -> Entity<AudienceView> {
        cx.read(|cx| self.show.read(cx).audience())
            .expect("the audience window is open")
            .1
    }

    fn presenter_view(&self, cx: &TestAppContext) -> Entity<PresenterView> {
        cx.read(|cx| self.show.read(cx).presenter())
            .expect("the presenter window is open")
            .1
    }

    fn canvas(&self, cx: &TestAppContext) -> Entity<SlideCanvas> {
        let view = self.audience_view(cx);
        cx.read(|cx| view.read(cx).current().clone())
    }
}

fn rehearse(cx: &mut TestAppContext, source: &str) -> Rehearsal {
    set_up(cx);
    let opened = talk(source);
    let document = cx.new(|_| opened);
    let show = cx.update(|cx| start_show(document.clone(), true, cx));
    let (audience, presenter) = cx.read(|cx| {
        let show = show.read(cx);
        (
            show.audience().expect("rehearsing opens it").0,
            show.presenter().expect("rehearsing opens it").0,
        )
    });
    let rehearsal = Rehearsal {
        document,
        show,
        audience,
        presenter,
    };
    rehearsal.settle(cx);
    rehearsal
}

/// What the element `id` of the window says.
fn label(cx: &mut TestAppContext, window: AnyWindowHandle, id: &'static str) -> String {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window
            .find(id)
            .label()
            .map(str::to_string)
            .expect("it has a label")
    })
    .unwrap()
}

fn counter(cx: &mut TestAppContext, show: &Rehearsal) -> String {
    label(cx, show.presenter, COUNTER_ID)
}

fn press(cx: &mut TestAppContext, window: AnyWindowHandle, key: &str) {
    cx.simulate_keystrokes(window, key);
}

/// Lets time pass, and draws what it brought.
fn wait(cx: &mut TestAppContext, show: &Rehearsal, millis: u64) {
    cx.executor().advance_clock(Duration::from_millis(millis));
    cx.run_until_parked();
    show.settle(cx);
}

/// Delivers the frames asked for so far, and says how many there were.
fn frames_asked_for(cx: &mut TestAppContext, window: AnyWindowHandle) -> usize {
    cx.update_window(window, |_, window, cx| window.simulate_next_frame(cx))
        .unwrap()
}

fn images(cx: &TestAppContext, canvas: &Entity<SlideCanvas>) -> Vec<Arc<RenderImage>> {
    cx.read(|cx| canvas.read(cx).images())
}

fn looks(cx: &TestAppContext, canvas: &Entity<SlideCanvas>) -> Vec<LayerLook> {
    cx.read(|cx| canvas.read(cx).looks().to_vec())
}

fn shown(cx: &TestAppContext, canvas: &Entity<SlideCanvas>) -> Vec<bool> {
    looks(cx, canvas).iter().map(|look| look.visible).collect()
}

fn slide_look(cx: &TestAppContext, canvas: &Entity<SlideCanvas>) -> SlideLook {
    cx.read(|cx| canvas.read(cx).slide_look())
}

fn outgoing(cx: &TestAppContext, view: &Entity<AudienceView>) -> Option<Entity<SlideCanvas>> {
    cx.read(|cx| view.read(cx).outgoing(cx).cloned())
}

fn in_atlas(cx: &mut TestAppContext, handle: AnyWindowHandle, image: &RenderImage) -> bool {
    cx.update_window(handle, |_, window, _| window.has_image_atlas_entry(image))
        .unwrap()
}

fn window_count(cx: &TestAppContext) -> usize {
    cx.read(|cx| cx.windows().len())
}

fn is_open(cx: &TestAppContext, window: AnyWindowHandle) -> bool {
    cx.read(|cx| {
        cx.windows()
            .iter()
            .any(|open| open.window_id() == window.window_id())
    })
}

fn reload(cx: &mut TestAppContext, show: &Rehearsal, source: &str) {
    cx.update(|cx| {
        show.document.update(cx, |document, cx| {
            document.reload(Some(source.to_string()));
            cx.notify();
        })
    });
    cx.run_until_parked();
    show.settle(cx);
}

#[gpui_kit::test]
fn right_reveals_then_moves_on(cx: &mut TestAppContext) {
    let show = rehearse(cx, TWO_CLICKS);
    assert_eq!(counter(cx, &show), "1 / 2");
    press(cx, show.presenter, "right");
    assert_eq!(counter(cx, &show), "1 / 2", "the first click");
    press(cx, show.presenter, "right");
    assert_eq!(counter(cx, &show), "1 / 2", "the second click");
    press(cx, show.presenter, "right");
    assert_eq!(counter(cx, &show), "2 / 2");
}

#[gpui_kit::test]
fn every_next_key_works(cx: &mut TestAppContext) {
    let show = rehearse(cx, &["# A\n"; 9].join("\n---\n\n"));
    let mut slide = 1;
    for window in [show.audience, show.presenter] {
        for key in ["right", "pagedown", "space", "enter"] {
            press(cx, window, key);
            slide += 1;
            assert_eq!(counter(cx, &show), format!("{slide} / 9"), "{key}");
        }
    }
}

#[gpui_kit::test]
fn every_previous_key_works(cx: &mut TestAppContext) {
    let show = rehearse(cx, &["# A\n"; 7].join("\n---\n\n"));
    for _ in 0..6 {
        press(cx, show.presenter, "right");
    }
    let mut slide = 7;
    for window in [show.audience, show.presenter] {
        for key in ["left", "pageup", "backspace"] {
            press(cx, window, key);
            slide -= 1;
            assert_eq!(counter(cx, &show), format!("{slide} / 7"), "{key}");
        }
    }
}

#[gpui_kit::test]
fn a_click_on_the_audience_window_means_next(cx: &mut TestAppContext) {
    let show = rehearse(cx, NO_TRANSITION);
    cx.update_window(show.audience, |_, window, cx| {
        window.render_frame(cx);
        window.click(AUDIENCE_ID, cx);
    })
    .unwrap();
    assert_eq!(counter(cx, &show), "2 / 2");
}

#[gpui_kit::test]
fn the_presenter_view_shows_notes_and_next(cx: &mut TestAppContext) {
    let show = rehearse(
        cx,
        "# One\n\n<Notes>\nSay this\n</Notes>\n\n---\n\n# Two\n\n<Notes>\nThen that\n</Notes>\n",
    );
    assert_eq!(label(cx, show.presenter, NOTES_ID), "Say this");
    cx.update_window(show.presenter, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find(CURRENT_ID).visible());
        let next = window.find(NEXT_ID);
        assert!(next.visible(), "{next:?}");
        assert_ne!(next.label(), Some(END_OF_THE_TALK));
    })
    .unwrap();

    press(cx, show.presenter, "right");
    assert_eq!(label(cx, show.presenter, NOTES_ID), "Then that");
}

#[gpui_kit::test]
fn the_presenter_view_holds_this_step_and_the_next(cx: &mut TestAppContext) {
    let show = rehearse(cx, TWO_CLICKS);
    let view = show.presenter_view(cx);
    let (current, upcoming) = cx.read(|cx| {
        let view = view.read(cx);
        (view.current().clone(), view.upcoming().clone())
    });
    assert_eq!(
        shown(cx, &current),
        [true, false, false],
        "before any click"
    );
    assert_eq!(shown(cx, &upcoming), [true, true, false], "one click on");

    press(cx, show.presenter, "right");
    show.settle(cx);
    assert_eq!(shown(cx, &current), [true, true, false]);
    assert_eq!(shown(cx, &upcoming), [true, true, true]);

    press(cx, show.presenter, "right");
    show.settle(cx);
    assert_eq!(shown(cx, &current), [true, true, true]);
    assert_eq!(
        images(cx, &upcoming).len(),
        2,
        "the next slide's own layers"
    );
}

#[gpui_kit::test]
fn the_slide_on_show_is_wider_than_the_next(cx: &mut TestAppContext) {
    let show = rehearse(cx, NO_TRANSITION);
    cx.update_window(show.presenter, |_, window, cx| {
        window.render_frame(cx);
        let (current, next) = (window.find(CURRENT_ID), window.find(NEXT_ID));
        assert!(
            current.bounds().size.width > next.bounds().size.width,
            "{current:?} {next:?}"
        );
        let shape = next.bounds().size.width / next.bounds().size.height;
        assert!((shape - 16. / 9.).abs() < 0.01, "{next:?}");
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_last_slide_says_the_talk_is_over(cx: &mut TestAppContext) {
    let show = rehearse(cx, NO_TRANSITION);
    assert_ne!(label_of_next(cx, &show), Some(END_OF_THE_TALK.to_string()));
    press(cx, show.presenter, "right");
    assert_eq!(label_of_next(cx, &show), Some(END_OF_THE_TALK.to_string()));
    show.settle(cx);
    let view = show.presenter_view(cx);
    let upcoming = cx.read(|cx| view.read(cx).upcoming().clone());
    assert!(images(cx, &upcoming).is_empty(), "there's nothing to show");
}

fn label_of_next(cx: &mut TestAppContext, show: &Rehearsal) -> Option<String> {
    cx.update_window(show.presenter, |_, window, cx| {
        window.render_frame(cx);
        window.find(NEXT_ID).label().map(str::to_string)
    })
    .unwrap()
}

#[gpui_kit::test]
fn the_timer_counts_from_the_start(cx: &mut TestAppContext) {
    let show = rehearse(cx, NO_TRANSITION);
    assert_eq!(label(cx, show.presenter, TIMER_ID), "00:00");
    cx.executor().advance_clock(Duration::from_secs(65));
    cx.run_until_parked();
    // Not drawn here: the tick has to have brought the window up to date on its own.
    let reads = cx
        .update_window(show.presenter, |_, window, _| {
            window.find(TIMER_ID).label().map(str::to_string)
        })
        .unwrap();
    assert_eq!(reads.as_deref(), Some("01:05"));
}

#[gpui_kit::test]
fn an_animation_stops_asking_for_frames_when_it_ends(cx: &mut TestAppContext) {
    let show = rehearse(cx, FADES_IN);
    let canvas = show.canvas(cx);
    frames_asked_for(cx, show.audience);

    press(cx, show.audience, "right");
    settle(cx, show.audience);
    assert!(
        frames_asked_for(cx, show.audience) > 0,
        "the fade needs frames"
    );
    let fading = *looks(cx, &canvas).last().expect("the shape has a look");
    assert!(fading.visible && fading.opacity < 1.0, "{fading:?}");

    wait(cx, &show, 200);
    let halfway = *looks(cx, &canvas).last().expect("the shape has a look");
    assert!(
        halfway.opacity > fading.opacity && halfway.opacity < 1.0,
        "{halfway:?}"
    );

    wait(cx, &show, 200);
    assert_eq!(
        *looks(cx, &canvas).last().expect("the shape has a look"),
        LayerLook::default(),
        "it has landed"
    );
    frames_asked_for(cx, show.audience);
    settle(cx, show.audience);
    assert_eq!(frames_asked_for(cx, show.audience), 0, "it's over");
}

#[gpui_kit::test]
fn a_still_slide_asks_for_no_frames(cx: &mut TestAppContext) {
    let show = rehearse(cx, NO_TRANSITION);
    frames_asked_for(cx, show.audience);
    press(cx, show.audience, "right");
    show.settle(cx);
    assert_eq!(frames_asked_for(cx, show.audience), 0);
    assert_eq!(frames_asked_for(cx, show.presenter), 0);
}

#[gpui_kit::test]
fn a_layer_that_only_appears_asks_for_no_frames(cx: &mut TestAppContext) {
    let show = rehearse(cx, TWO_CLICKS);
    frames_asked_for(cx, show.audience);
    press(cx, show.audience, "right");
    settle(cx, show.audience);
    assert_eq!(frames_asked_for(cx, show.audience), 0);
}

#[gpui_kit::test]
fn the_presenter_view_does_not_animate(cx: &mut TestAppContext) {
    let show = rehearse(cx, FADES_IN);
    frames_asked_for(cx, show.presenter);
    press(cx, show.presenter, "right");
    settle(cx, show.presenter);
    assert_eq!(frames_asked_for(cx, show.presenter), 0);
}

#[gpui_kit::test]
fn a_transition_releases_the_old_slide(cx: &mut TestAppContext) {
    let show = rehearse(cx, FADES);
    let view = show.audience_view(cx);
    assert!(outgoing(cx, &view).is_none(), "nothing is leaving yet");
    let first = images(cx, &show.canvas(cx));

    press(cx, show.audience, "right");
    show.settle(cx);
    let leaving = outgoing(cx, &view).expect("the first slide stays while the second comes");
    assert!(Arc::ptr_eq(&first[1], &images(cx, &leaving)[1]));
    assert!(
        !Arc::ptr_eq(&first[1], &images(cx, &show.canvas(cx))[1]),
        "the new slide is on a canvas of its own"
    );
    assert!(frames_asked_for(cx, show.audience) > 0);

    wait(cx, &show, 499);
    assert!(outgoing(cx, &view).is_some(), "too soon");
    assert!(in_atlas(cx, show.audience, &first[1]));

    wait(cx, &show, 1);
    assert!(outgoing(cx, &view).is_none());
    assert!(
        !in_atlas(cx, show.audience, &first[1]),
        "its images were released"
    );
    frames_asked_for(cx, show.audience);
    settle(cx, show.audience);
    assert_eq!(frames_asked_for(cx, show.audience), 0, "it's over");
}

#[gpui_kit::test]
fn a_transition_starts_on_its_first_frame(cx: &mut TestAppContext) {
    let show = rehearse(cx, FADES);
    let executor = cx.executor();
    // Inside one update, so that the time passes before anything is drawn.
    cx.update(|cx| {
        show.show.update(cx, |show, cx| show.next(cx));
        // However long getting the new slide ready takes, none of the fade has been seen yet.
        executor.advance_clock(Duration::from_millis(100));
    });
    assert_eq!(slide_look(cx, &show.canvas(cx)).opacity, 0.0);

    wait(cx, &show, 250);
    let halfway = slide_look(cx, &show.canvas(cx)).opacity;
    assert!((halfway - ease(0.5)).abs() < 1e-4, "{halfway}");
}

#[gpui_kit::test]
fn a_slide_with_no_transition_replaces_the_last_at_once(cx: &mut TestAppContext) {
    let show = rehearse(cx, NO_TRANSITION);
    let view = show.audience_view(cx);
    let first = images(cx, &show.canvas(cx));
    press(cx, show.audience, "right");
    show.settle(cx);
    assert!(outgoing(cx, &view).is_none());
    assert!(!in_atlas(cx, show.audience, &first[1]));
}

#[gpui_kit::test]
fn a_push_has_its_layers_before_its_first_frame(cx: &mut TestAppContext) {
    let show = rehearse(cx, PUSHES);
    let before = images(cx, &show.canvas(cx));
    press(cx, show.audience, "right");
    let incoming = images(cx, &show.canvas(cx));
    assert_eq!(incoming.len(), 2);
    assert!(!Arc::ptr_eq(&before[1], &incoming[1]));
}

#[gpui_kit::test]
fn going_back_ends_a_transition_at_once(cx: &mut TestAppContext) {
    let show = rehearse(cx, FADES);
    let view = show.audience_view(cx);
    press(cx, show.audience, "right");
    show.settle(cx);
    let second = images(cx, &show.canvas(cx));
    press(cx, show.audience, "left");
    show.settle(cx);
    assert_eq!(counter(cx, &show), "1 / 3");
    assert!(outgoing(cx, &view).is_none());
    assert!(!in_atlas(cx, show.audience, &second[1]));
    assert_eq!(frames_asked_for_after_the_last(cx, show.audience), 0);
}

/// Delivers the frames already asked for, draws, and says how many that one drawing asked for.
fn frames_asked_for_after_the_last(cx: &mut TestAppContext, window: AnyWindowHandle) -> usize {
    frames_asked_for(cx, window);
    settle(cx, window);
    frames_asked_for(cx, window)
}

#[gpui_kit::test]
fn moving_on_during_a_transition_starts_the_next_one(cx: &mut TestAppContext) {
    let show = rehearse(cx, FADES);
    let view = show.audience_view(cx);
    let first = images(cx, &show.canvas(cx));
    press(cx, show.audience, "right");
    show.settle(cx);
    let second = images(cx, &show.canvas(cx));
    press(cx, show.audience, "right");
    show.settle(cx);

    assert_eq!(counter(cx, &show), "3 / 3");
    let leaving = outgoing(cx, &view).expect("the second slide is on its way out");
    assert!(Arc::ptr_eq(&second[1], &images(cx, &leaving)[1]));
    assert!(!in_atlas(cx, show.audience, &first[1]));
}

#[gpui_kit::test]
fn a_save_during_the_show_updates_it(cx: &mut TestAppContext) {
    let show = rehearse(cx, "# One\n\n<Notes>\nOld words\n</Notes>\n");
    assert_eq!(label(cx, show.presenter, NOTES_ID), "Old words");
    let before = images(cx, &show.canvas(cx));

    reload(cx, &show, "# Changed\n\n<Notes>\nNew words\n</Notes>\n");

    assert_eq!(label(cx, show.presenter, NOTES_ID), "New words");
    assert!(
        !Arc::ptr_eq(&before[1], &images(cx, &show.canvas(cx))[1]),
        "the slide was drawn again"
    );
}

#[gpui_kit::test]
fn a_save_that_removes_slides_keeps_the_show_on_a_real_slide(cx: &mut TestAppContext) {
    let show = rehearse(cx, THREE_SLIDES);
    press(cx, show.presenter, "right");
    press(cx, show.presenter, "right");
    assert_eq!(counter(cx, &show), "3 / 3");

    reload(cx, &show, "# One\n");

    assert_eq!(counter(cx, &show), "1 / 1");
    assert_eq!(label(cx, show.presenter, NEXT_ID), END_OF_THE_TALK);
    assert_eq!(images(cx, &show.canvas(cx)).len(), 2);
}

#[gpui_kit::test]
fn a_save_that_removes_clicks_takes_the_show_back_to_a_real_step(cx: &mut TestAppContext) {
    let show = rehearse(cx, TWO_CLICKS);
    press(cx, show.presenter, "right");
    press(cx, show.presenter, "right");

    reload(
        cx,
        &show,
        "<Box x=\"0\" y=\"0\" w=\"99\" h=\"99\" fill=\"red\" click=\"1\" />\n\n---\n\n# Two\n",
    );

    assert_eq!(shown(cx, &show.canvas(cx)), [true, true]);
    press(cx, show.presenter, "right");
    assert_eq!(
        counter(cx, &show),
        "2 / 2",
        "no click is left on this slide"
    );
}

#[gpui_kit::test]
fn a_save_during_a_transition_ends_it(cx: &mut TestAppContext) {
    let show = rehearse(cx, FADES);
    let view = show.audience_view(cx);
    press(cx, show.audience, "right");
    show.settle(cx);
    assert!(outgoing(cx, &view).is_some());

    reload(cx, &show, "# One\n");

    assert!(outgoing(cx, &view).is_none());
    assert_eq!(counter(cx, &show), "1 / 1");
}

#[gpui_kit::test]
fn the_show_can_finish_a_talk_that_grew(cx: &mut TestAppContext) {
    let show = rehearse(cx, NO_TRANSITION);
    press(cx, show.presenter, "right");
    assert_eq!(label(cx, show.presenter, NEXT_ID), END_OF_THE_TALK);
    reload(cx, &show, THREE_SLIDES);
    assert_eq!(counter(cx, &show), "2 / 3");
    press(cx, show.presenter, "right");
    assert_eq!(counter(cx, &show), "3 / 3");
}

struct Editor {
    _folder: tempfile::TempDir,
    file: PathBuf,
    handle: AnyWindowHandle,
    view: Entity<WorkspaceView>,
}

/// An editor window on a talk whose file stays on disk.
fn open_editor(cx: &mut TestAppContext, source: &str) -> Editor {
    set_up(cx);
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("slides.md");
    std::fs::write(&file, source).unwrap();
    let opened = Document::open(folder.path(), Fonts::bundled_only()).unwrap();
    let (handle, view) = cx.update(|cx: &mut App| {
        let bounds = Bounds {
            origin: Point::default(),
            size: size(px(1280.), px(800.)),
        };
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| WorkspaceView::open_with(opened, window, cx)),
        )
        .expect("open test window")
    });
    settle(cx, handle);
    Editor {
        _folder: folder,
        file,
        handle,
        view,
    }
}

/// The windows that aren't the editor's.
fn show_windows(cx: &TestAppContext, editor: &Editor) -> Vec<AnyWindowHandle> {
    cx.read(|cx| cx.windows())
        .into_iter()
        .filter(|window| window.window_id() != editor.handle.window_id())
        .collect()
}

/// The show's window with `id` in it.
fn window_with(
    cx: &mut TestAppContext,
    editor: &Editor,
    id: &'static str,
) -> Option<AnyWindowHandle> {
    show_windows(cx, editor).into_iter().find(|window| {
        cx.update_window(*window, |_, window, cx| {
            window.render_frame(cx);
            window.try_find(id).is_some()
        })
        .unwrap()
    })
}

#[gpui_kit::test]
fn a_save_in_the_editor_reaches_the_show(cx: &mut TestAppContext) {
    let editor = open_editor(cx, "# One\n\n<Notes>\nOld words\n</Notes>\n");
    press(cx, editor.handle, "shift-f5");
    let presenter = window_with(cx, &editor, NOTES_ID).expect("the presenter window is open");
    assert_eq!(label(cx, presenter, NOTES_ID), "Old words");

    std::fs::write(&editor.file, "# One\n\n<Notes>\nNew words\n</Notes>\n").unwrap();
    cx.update(|cx| {
        editor
            .view
            .update(cx, |view, cx| view.reload_soon(editor.file.clone(), cx))
    });
    cx.executor().advance_clock(Duration::from_millis(100));
    cx.run_until_parked();

    assert_eq!(label(cx, presenter, NOTES_ID), "New words");
}

#[gpui_kit::test]
fn a_save_in_the_editor_that_removes_slides_reaches_the_show(cx: &mut TestAppContext) {
    let editor = open_editor(cx, THREE_SLIDES);
    press(cx, editor.handle, "shift-f5");
    let presenter = window_with(cx, &editor, NOTES_ID).expect("the presenter window is open");
    press(cx, presenter, "right");
    press(cx, presenter, "right");
    assert_eq!(label(cx, presenter, COUNTER_ID), "3 / 3");

    std::fs::write(&editor.file, "# One\n\n---\n\n# Two\n").unwrap();
    cx.update(|cx| {
        editor
            .view
            .update(cx, |view, cx| view.reload_soon(editor.file.clone(), cx))
    });
    cx.executor().advance_clock(Duration::from_millis(100));
    cx.run_until_parked();

    assert_eq!(label(cx, presenter, COUNTER_ID), "2 / 2");
    press(cx, presenter, "left");
    assert_eq!(label(cx, presenter, COUNTER_ID), "1 / 2");
}

#[gpui_kit::test]
fn escape_closes_the_show_and_leaves_the_editor(cx: &mut TestAppContext) {
    let editor = open_editor(cx, THREE_SLIDES);
    press(cx, editor.handle, "shift-f5");
    assert_eq!(window_count(cx), 3);
    let presenter = window_with(cx, &editor, NOTES_ID).expect("the presenter window is open");

    press(cx, presenter, "escape");

    assert_eq!(window_count(cx), 1);
    assert!(is_open(cx, editor.handle));
}

#[gpui_kit::test]
fn escape_works_from_the_audience_window_too(cx: &mut TestAppContext) {
    let editor = open_editor(cx, THREE_SLIDES);
    press(cx, editor.handle, "shift-f5");
    let audience = window_with(cx, &editor, AUDIENCE_ID).expect("the audience window is open");

    press(cx, audience, "escape");

    assert_eq!(window_count(cx), 1);
    assert!(is_open(cx, editor.handle));
}

#[gpui_kit::test]
fn closing_the_audience_window_ends_the_show(cx: &mut TestAppContext) {
    let editor = open_editor(cx, THREE_SLIDES);
    press(cx, editor.handle, "shift-f5");
    let audience = window_with(cx, &editor, AUDIENCE_ID).expect("the audience window is open");

    cx.update_window(audience, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();

    assert_eq!(window_count(cx), 1, "the presenter window went with it");
    assert!(is_open(cx, editor.handle));
}

#[gpui_kit::test]
fn closing_the_presenter_window_ends_the_show(cx: &mut TestAppContext) {
    let editor = open_editor(cx, THREE_SLIDES);
    press(cx, editor.handle, "shift-f5");
    let presenter = window_with(cx, &editor, NOTES_ID).expect("the presenter window is open");

    cx.update_window(presenter, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();

    assert_eq!(window_count(cx), 1, "the audience window went with it");
    assert!(is_open(cx, editor.handle));
}

#[gpui_kit::test]
fn closing_the_editor_leaves_the_show_running(cx: &mut TestAppContext) {
    let editor = open_editor(cx, THREE_SLIDES);
    press(cx, editor.handle, "shift-f5");
    let presenter = window_with(cx, &editor, NOTES_ID).expect("the presenter window is open");

    cx.update_window(editor.handle, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();

    assert_eq!(window_count(cx), 2);
    press(cx, presenter, "right");
    assert_eq!(label(cx, presenter, COUNTER_ID), "2 / 3");
}

#[gpui_kit::test]
fn f5_starts_a_show(cx: &mut TestAppContext) {
    let editor = open_editor(cx, THREE_SLIDES);
    assert_eq!(window_count(cx), 1);
    press(cx, editor.handle, "f5");
    let opened = show_windows(cx, &editor);
    assert_eq!(opened.len(), 1, "one screen, so only the audience's window");
    cx.update_window(opened[0], |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find(AUDIENCE_ID).visible());
        assert!(window.is_fullscreen());
    })
    .unwrap();
}

#[gpui_kit::test]
fn shift_f5_opens_both_as_ordinary_windows(cx: &mut TestAppContext) {
    let editor = open_editor(cx, THREE_SLIDES);
    press(cx, editor.handle, "shift-f5");
    let opened = show_windows(cx, &editor);
    assert_eq!(opened.len(), 2);
    assert!(window_with(cx, &editor, AUDIENCE_ID).is_some());
    assert!(window_with(cx, &editor, NOTES_ID).is_some());
    for window in opened {
        cx.update_window(window, |_, window, _| assert!(!window.is_fullscreen()))
            .unwrap();
    }
}

#[gpui_kit::test]
fn the_present_button_starts_a_show(cx: &mut TestAppContext) {
    let editor = open_editor(cx, THREE_SLIDES);
    cx.update_window(editor.handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(PRESENT_ID, cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(show_windows(cx, &editor).len(), 1);
}

#[gpui_kit::test]
fn only_one_show_runs_at_a_time(cx: &mut TestAppContext) {
    let editor = open_editor(cx, THREE_SLIDES);
    press(cx, editor.handle, "f5");
    press(cx, editor.handle, "f5");
    press(cx, editor.handle, "shift-f5");
    assert_eq!(show_windows(cx, &editor).len(), 1);
}

#[gpui_kit::test]
fn a_show_can_start_again_once_it_has_ended(cx: &mut TestAppContext) {
    let editor = open_editor(cx, THREE_SLIDES);
    press(cx, editor.handle, "f5");
    let audience = show_windows(cx, &editor)[0];
    press(cx, audience, "escape");
    assert_eq!(window_count(cx), 1);

    press(cx, editor.handle, "f5");
    assert_eq!(show_windows(cx, &editor).len(), 1);
}

#[gpui_kit::test]
fn with_no_talk_f5_does_nothing(cx: &mut TestAppContext) {
    set_up(cx);
    let (handle, _view) = cx.update(|cx: &mut App| {
        gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| WorkspaceView::for_test(window, cx))
        })
        .expect("open test window")
    });
    settle(cx, handle);
    press(cx, handle, "f5");
    assert_eq!(window_count(cx), 1);
}

#[gpui_kit::test]
fn the_show_leaves_the_editor_on_its_own_slide(cx: &mut TestAppContext) {
    let editor = open_editor(cx, "---\ntitle: T\n---\n\n# One\n\n---\n\n# Two\n");
    press(cx, editor.handle, "shift-f5");
    let presenter = window_with(cx, &editor, NOTES_ID).expect("the presenter window is open");
    press(cx, presenter, "right");
    assert_eq!(label(cx, presenter, COUNTER_ID), "2 / 2");
    assert_eq!(label(cx, editor.handle, TITLE_ID), "T · 1 / 2");
}
