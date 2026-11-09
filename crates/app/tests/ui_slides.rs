use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::test::{TestQueryExt, TestWindowExt};
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Context, Entity, Pixels, Point, RenderImage,
    TestAppContext, Window, WindowBounds, WindowOptions, point, px, size,
};
use slides_engine::Fonts;
use stagepack_studio::document::Document;
use stagepack_studio::slide_canvas::{SlideCanvas, letterbox};
use stagepack_studio::workspace_view::{
    self, CANVAS_ID, MARKDOWN_ID, NOTICES_ID, PROBLEM_ID, TITLE_ID, WorkspaceView,
};

fn document(source: &str) -> Document {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), source).unwrap();
    Document::open(dir.path(), Fonts::bundled_only()).unwrap()
}

fn open_view(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut Window, &mut Context<WorkspaceView>) -> WorkspaceView + 'static,
) -> (AnyWindowHandle, Entity<WorkspaceView>) {
    cx.update(|cx: &mut App| {
        gpui_kit::init(cx);
        workspace_view::bind_keys(cx);
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
            |window, cx| cx.new(|cx| build(window, cx)),
        )
        .expect("open test window")
    })
}

fn open_with_view(
    cx: &mut TestAppContext,
    source: &str,
) -> (AnyWindowHandle, Entity<WorkspaceView>) {
    let talk = document(source);
    open_view(cx, |window, cx| WorkspaceView::open_with(talk, window, cx))
}

fn open_talk(cx: &mut TestAppContext, source: &str) -> AnyWindowHandle {
    open_with_view(cx, source).0
}

/// Draws until what one frame asked for has reached the screen.
fn settle(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    for _ in 0..2 {
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        cx.run_until_parked();
    }
}

fn slide_canvas(cx: &TestAppContext, view: &Entity<WorkspaceView>) -> Entity<SlideCanvas> {
    cx.read(|cx| view.read(cx).slide_canvas().clone())
}

fn images(cx: &TestAppContext, canvas: &Entity<SlideCanvas>) -> Vec<Arc<RenderImage>> {
    cx.read(|cx| canvas.read(cx).images())
}

fn scale(cx: &TestAppContext, canvas: &Entity<SlideCanvas>) -> f32 {
    cx.read(|cx| canvas.read(cx).scale())
        .expect("the canvas has painted")
}

fn title(cx: &mut TestAppContext, handle: AnyWindowHandle) -> String {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window
            .find(TITLE_ID)
            .label()
            .map(str::to_string)
            .expect("the title has a label")
    })
    .unwrap()
}

#[gpui_kit::test]
fn right_arrow_moves_to_the_next_slide(cx: &mut TestAppContext) {
    let handle = open_talk(cx, "---\ntitle: T\n---\n\n# One\n\n---\n\n# Two\n");
    cx.simulate_keystrokes(handle, "right");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find(TITLE_ID).label(), Some("T · 2 / 2"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn left_and_the_page_keys_move_too(cx: &mut TestAppContext) {
    let handle = open_talk(
        cx,
        "---\ntitle: T\n---\n\n# One\n\n---\n\n# Two\n\n---\n\n# Three\n",
    );
    assert_eq!(title(cx, handle), "T · 1 / 3");
    cx.simulate_keystrokes(handle, "pagedown");
    assert_eq!(title(cx, handle), "T · 2 / 3");
    cx.simulate_keystrokes(handle, "right");
    assert_eq!(title(cx, handle), "T · 3 / 3");
    cx.simulate_keystrokes(handle, "pageup");
    assert_eq!(title(cx, handle), "T · 2 / 3");
    cx.simulate_keystrokes(handle, "left");
    assert_eq!(title(cx, handle), "T · 1 / 3");
}

#[gpui_kit::test]
fn a_talk_is_drawn_at_the_device_width_of_its_slide(cx: &mut TestAppContext) {
    let (handle, view) = open_with_view(cx, "# One\n");
    settle(cx, handle);
    let canvas = slide_canvas(cx, &view);
    // The window is 1280 wide, with room to spare in height, at a scale factor of 2.
    assert_eq!(scale(cx, &canvas), 2560. / 1920.);
    let drawn = images(cx, &canvas);
    assert_eq!(drawn.len(), 2, "background and title");
    cx.update_window(handle, |_, window, _| {
        assert!(
            drawn
                .iter()
                .all(|image| window.has_image_atlas_entry(image))
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_next_slide_replaces_the_images_and_releases_the_old_ones(cx: &mut TestAppContext) {
    let (handle, view) = open_with_view(cx, "# One\n\n---\n\n# Two\n");
    settle(cx, handle);
    let canvas = slide_canvas(cx, &view);
    let old = images(cx, &canvas);
    cx.simulate_keystrokes(handle, "right");
    settle(cx, handle);
    let new = images(cx, &canvas);
    assert_eq!(new.len(), 2);
    cx.update_window(handle, |_, window, _| {
        assert!(
            old.iter().all(|image| !window.has_image_atlas_entry(image)),
            "the first slide's images were released"
        );
        assert!(
            new.iter().all(|image| window.has_image_atlas_entry(image)),
            "the second slide's images are in the atlas"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_layer_that_stays_keeps_its_image(cx: &mut TestAppContext) {
    let (handle, view) = open_with_view(cx, "# One\n");
    settle(cx, handle);
    let canvas = slide_canvas(cx, &view);
    let mut other = document("# Other\n");
    let (first, second) = (other.layers(1.0), other.layers(1.0));
    assert!(
        Arc::ptr_eq(&first[0], &second[0]),
        "the document hands back the same layers"
    );

    let set = |cx: &mut TestAppContext, layers| {
        cx.update(|cx| canvas.update(cx, |canvas, cx| canvas.set_layers(layers, cx)));
        settle(cx, handle);
    };
    set(cx, first.clone());
    let before = images(cx, &canvas);
    set(cx, vec![second[0].clone()]);
    let after = images(cx, &canvas);

    assert_eq!(after.len(), 1);
    assert!(
        Arc::ptr_eq(&before[0], &after[0]),
        "the same layer keeps its image"
    );
    cx.update_window(handle, |_, window, _| {
        assert!(window.has_image_atlas_entry(&after[0]));
        assert!(
            !window.has_image_atlas_entry(&before[1]),
            "the layer that fell out was released"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_same_layer_twice_in_a_slide_is_one_image(cx: &mut TestAppContext) {
    let (handle, view) = open_with_view(cx, "# One\n");
    settle(cx, handle);
    let canvas = slide_canvas(cx, &view);
    let layers = document("# Other\n").layers(1.0);
    cx.update(|cx| {
        canvas.update(cx, |canvas, cx| {
            canvas.set_layers(vec![layers[0].clone(), layers[0].clone()], cx)
        });
    });
    settle(cx, handle);
    let drawn = images(cx, &canvas);
    assert_eq!(drawn.len(), 2);
    assert!(Arc::ptr_eq(&drawn[0], &drawn[1]));

    cx.update(|cx| {
        canvas.update(cx, |canvas, cx| {
            canvas.set_layers(vec![layers[1].clone()], cx)
        })
    });
    settle(cx, handle);
    cx.update_window(handle, |_, window, _| {
        assert!(!window.has_image_atlas_entry(&drawn[0]))
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_small_change_of_width_keeps_the_layers(cx: &mut TestAppContext) {
    let (handle, view) = open_with_view(cx, "# One\n");
    settle(cx, handle);
    let canvas = slide_canvas(cx, &view);
    let (scale_before, before) = (scale(cx, &canvas), images(cx, &canvas));

    cx.update_window(handle, |_, window, _| window.set_scale_factor(2.0 * 1.005))
        .unwrap();
    settle(cx, handle);

    assert_eq!(scale(cx, &canvas), scale_before);
    assert!(Arc::ptr_eq(&before[0], &images(cx, &canvas)[0]));
}

#[gpui_kit::test]
fn a_larger_change_of_width_draws_the_layers_again(cx: &mut TestAppContext) {
    let (handle, view) = open_with_view(cx, "# One\n");
    settle(cx, handle);
    let canvas = slide_canvas(cx, &view);
    let (scale_before, before) = (scale(cx, &canvas), images(cx, &canvas));

    cx.update_window(handle, |_, window, _| window.set_scale_factor(2.0 * 1.02))
        .unwrap();
    settle(cx, handle);

    assert!(scale(cx, &canvas) > scale_before * 1.01);
    let after = images(cx, &canvas);
    assert!(!Arc::ptr_eq(&before[0], &after[0]));
    cx.update_window(handle, |_, window, _| {
        assert!(
            !window.has_image_atlas_entry(&before[0]),
            "the old image was released"
        );
        assert!(window.has_image_atlas_entry(&after[0]));
    })
    .unwrap();
}

#[gpui_kit::test]
fn notices_show_under_the_slide(cx: &mut TestAppContext) {
    let handle = open_talk(cx, "# One\n\n---\nlayout: nope\n---\n\n# Two\n");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find(NOTICES_ID).is_none(),
            "a slide with no notices has no line for them"
        );
    })
    .unwrap();
    cx.simulate_keystrokes(handle, "right");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let notices = window.find(NOTICES_ID);
        assert!(
            notices.label().is_some_and(|label| label.contains("nope")),
            "{notices:?}"
        );
        assert!(
            notices.bounds().top() >= window.find(workspace_view::CANVAS_ID).bounds().bottom(),
            "{notices:?}"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_folder_that_cannot_be_opened_says_why(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().to_path_buf();
    let (handle, _view) = open_view(cx, move |window, cx| {
        WorkspaceView::open(folder, window, cx)
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let problem = window.find(PROBLEM_ID);
        assert!(
            problem
                .label()
                .is_some_and(|label| label.contains("slides.md")),
            "{problem:?}"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn ctrl_m_shows_the_markdown(cx: &mut TestAppContext) {
    let handle = open_talk(cx, "# One\n");
    cx.simulate_keystrokes(handle, "ctrl-m");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find(MARKDOWN_ID).visible());
    })
    .unwrap();
}

#[gpui_kit::test]
fn ctrl_m_hides_the_markdown_again(cx: &mut TestAppContext) {
    let handle = open_talk(cx, "# One\n");
    cx.simulate_keystrokes(handle, "ctrl-m");
    cx.simulate_keystrokes(handle, "ctrl-m");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(MARKDOWN_ID).is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_markdown_is_to_the_right_of_the_slide(cx: &mut TestAppContext) {
    let handle = open_talk(cx, "# One\n");
    cx.simulate_keystrokes(handle, "ctrl-m");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let (canvas, panel) = (window.find(CANVAS_ID), window.find(MARKDOWN_ID));
        assert_eq!(panel.bounds().left(), canvas.bounds().right());
        assert_eq!(panel.bounds().top(), canvas.bounds().top());
        assert_eq!(panel.bounds().bottom(), canvas.bounds().bottom());
        assert_eq!(panel.bounds().right(), px(1280.));
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_markdown_shows_the_lines_of_the_file(cx: &mut TestAppContext) {
    let handle = open_talk(cx, "\n# One\n\nSome words\n");
    cx.simulate_keystrokes(handle, "ctrl-m");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let (heading, words) = (
            window.find_by_label("# One"),
            window.find_by_label("Some words"),
        );
        assert!(heading.visible() && words.visible());
        // The list takes its row height from the first row, and here that's a blank line.
        let row = heading.bounds().size.height;
        assert!(row > px(0.), "{heading:?}");
        assert_eq!(words.bounds().top() - heading.bounds().top(), row * 2.);
    })
    .unwrap();
}

#[gpui_kit::test]
fn an_empty_talk_can_show_its_markdown(cx: &mut TestAppContext) {
    let handle = open_talk(cx, "");
    cx.simulate_keystrokes(handle, "ctrl-m");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find(MARKDOWN_ID).visible());
    })
    .unwrap();
}

struct SavedTalk {
    _folder: tempfile::TempDir,
    file: PathBuf,
    handle: AnyWindowHandle,
    view: Entity<WorkspaceView>,
}

/// A talk whose file stays on disk, with the Markdown showing.
fn open_saved_talk(cx: &mut TestAppContext, source: &str) -> SavedTalk {
    let folder = tempfile::tempdir().unwrap();
    let file = folder.path().join("slides.md");
    std::fs::write(&file, source).unwrap();
    let talk = Document::open(folder.path(), Fonts::bundled_only()).unwrap();
    let (handle, view) = open_view(cx, |window, cx| WorkspaceView::open_with(talk, window, cx));
    settle(cx, handle);
    cx.simulate_keystrokes(handle, "ctrl-m");
    settle(cx, handle);
    SavedTalk {
        _folder: folder,
        file,
        handle,
        view,
    }
}

fn save(cx: &mut TestAppContext, talk: &SavedTalk, contents: &str) {
    std::fs::write(&talk.file, contents).unwrap();
    cx.update(|cx| {
        talk.view
            .update(cx, |view, cx| view.reload_soon(talk.file.clone(), cx))
    });
}

/// Lets `millis` pass, and everything waiting on them run and draw.
fn wait(cx: &mut TestAppContext, talk: &SavedTalk, millis: u64) {
    cx.executor().advance_clock(Duration::from_millis(millis));
    cx.run_until_parked();
    settle(cx, talk.handle);
}

fn shows_line(cx: &mut TestAppContext, talk: &SavedTalk, line: &str) -> bool {
    cx.update_window(talk.handle, |_, window, cx| {
        window.render_frame(cx);
        window.try_find_by_label(line).is_some()
    })
    .unwrap()
}

#[gpui_kit::test]
fn a_save_shows_once_the_file_has_been_quiet(cx: &mut TestAppContext) {
    let talk = open_saved_talk(cx, "---\ntitle: T\n---\n\n# One\n");
    let canvas = slide_canvas(cx, &talk.view);
    let before = images(cx, &canvas);
    assert_eq!(before.len(), 2, "background and title");

    save(cx, &talk, "---\ntitle: T\n---\n\n# Changed\n");
    wait(cx, &talk, 99);
    assert!(shows_line(cx, &talk, "# One"), "too soon");
    assert!(Arc::ptr_eq(&before[1], &images(cx, &canvas)[1]), "too soon");

    wait(cx, &talk, 1);
    assert!(shows_line(cx, &talk, "# Changed"));
    assert!(!shows_line(cx, &talk, "# One"));
    assert!(
        !Arc::ptr_eq(&before[1], &images(cx, &canvas)[1]),
        "the heading was drawn again"
    );
}

#[gpui_kit::test]
fn each_change_starts_the_wait_again(cx: &mut TestAppContext) {
    let talk = open_saved_talk(cx, "# One\n");
    save(cx, &talk, "# Half saved\n");
    wait(cx, &talk, 60);
    save(cx, &talk, "# Fully saved\n");
    wait(cx, &talk, 60);
    assert!(shows_line(cx, &talk, "# One"), "the file isn't quiet yet");

    wait(cx, &talk, 40);
    assert!(shows_line(cx, &talk, "# Fully saved"));
}

#[gpui_kit::test]
fn a_save_to_another_slide_leaves_this_one_and_updates_the_count(cx: &mut TestAppContext) {
    let talk = open_saved_talk(cx, "---\ntitle: T\n---\n\n# One\n\n---\n\n# Two\n");
    let canvas = slide_canvas(cx, &talk.view);
    let before = images(cx, &canvas);

    save(
        cx,
        &talk,
        "---\ntitle: T\n---\n\n# One\n\n---\n\n# Two\n\n---\n\n# Three\n",
    );
    wait(cx, &talk, 100);

    assert_eq!(title(cx, talk.handle), "T · 1 / 3");
    assert!(shows_line(cx, &talk, "# Three"));
    let after = images(cx, &canvas);
    assert_eq!(before.len(), after.len());
    assert!(
        before
            .iter()
            .zip(&after)
            .all(|(before, after)| Arc::ptr_eq(before, after))
    );
}

#[gpui_kit::test]
fn being_moved_to_an_unchanged_slide_shows_it(cx: &mut TestAppContext) {
    let talk = open_saved_talk(cx, "# One\n\n---\n\n# Two\n");
    cx.simulate_keystrokes(talk.handle, "right");
    settle(cx, talk.handle);
    let canvas = slide_canvas(cx, &talk.view);
    let on_second = images(cx, &canvas);

    save(cx, &talk, "# One\n\n");
    wait(cx, &talk, 100);

    assert!(title(cx, talk.handle).ends_with("1 / 1"));
    assert!(
        !Arc::ptr_eq(&on_second[1], &images(cx, &canvas)[1]),
        "the canvas moved to the slide that's left"
    );
}

#[gpui_kit::test]
fn a_file_that_cannot_be_read_keeps_what_is_showing(cx: &mut TestAppContext) {
    let talk = open_saved_talk(cx, "# One\n");
    let canvas = slide_canvas(cx, &talk.view);
    let before = images(cx, &canvas);

    save(cx, &talk, "");
    wait(cx, &talk, 100);
    std::fs::remove_file(&talk.file).unwrap();
    cx.update(|cx| {
        talk.view
            .update(cx, |view, cx| view.reload_soon(talk.file.clone(), cx))
    });
    wait(cx, &talk, 100);

    assert!(shows_line(cx, &talk, "# One"));
    assert!(Arc::ptr_eq(&before[1], &images(cx, &canvas)[1]));
}

#[gpui_kit::test]
fn a_folder_that_cannot_be_watched_says_so(cx: &mut TestAppContext) {
    let (handle, view) = open_with_view(cx, "# One\n");
    let gone = tempfile::tempdir().unwrap().path().to_path_buf();
    cx.update(|cx| view.update(cx, |view, cx| view.follow_saves(gone, cx)));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let notices = window.find(NOTICES_ID);
        assert!(
            notices.label().is_some_and(|label| label.contains("watch")),
            "{notices:?}"
        );
    })
    .unwrap();
}

fn area(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
    Bounds {
        origin: point(px(x), px(y)),
        size: size(px(width), px(height)),
    }
}

#[test]
fn a_wide_area_gets_bars_at_the_sides() {
    assert_eq!(
        letterbox(area(10., 20., 2000., 720.), 1.0),
        area(370., 20., 1280., 720.)
    );
}

#[test]
fn a_tall_area_gets_bars_at_the_top_and_bottom() {
    assert_eq!(
        letterbox(area(10., 20., 1280., 1000.), 1.0),
        area(10., 160., 1280., 720.)
    );
}

#[test]
fn the_slide_sits_on_whole_device_pixels() {
    // Centring leaves half a logical pixel over. At a scale factor of 2 that's a whole device
    // pixel, and at a scale factor of 1 it has to go.
    let single = letterbox(area(0., 0., 1501., 720.), 1.0);
    assert_eq!(single.size.width, px(1280.));
    assert_eq!(f32::from(single.origin.x).fract(), 0.0, "{single:?}");
    assert_eq!(
        letterbox(area(0., 0., 1501., 720.), 2.0).origin.x,
        px(110.5)
    );
}
