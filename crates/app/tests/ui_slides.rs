use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use gpui_kit::test::{TestQueryExt, TestWindowExt};
use gpui_kit::{
    Action as _, AnyWindowHandle, App, AppContext, Bounds, Context, Entity, Pixels, Point, RenderImage,
    TestAppContext, VisualTestContext, Window, WindowBounds, WindowOptions, point, px, size,
};
use slides_components::{Bundle, Diagnostic, component_tag};
use slides_engine::{ComponentState, Fonts};
use stagepack_studio::builds::Compile;
use stagepack_studio::document::Document;
use stagepack_studio::permissions::BuildPermissions;
use stagepack_studio::present::{LayerLook, SlideLook};
use stagepack_studio::ribbon::{NEW_SLIDE_ID, RIBBON_ID, SAVE_ID};
use stagepack_studio::slide_canvas::{SlideCanvas, letterbox};
use stagepack_studio::workspace_view::{
    self, BUILD_ALWAYS_ID, BUILD_ONCE_ID, CANVAS_ID, CLOSE_CANCEL_ID, CLOSE_DONT_SAVE_ID,
    CLOSE_SAVE_ID, COMPONENT_PROBLEMS_ID, EXPORT_MENU_ID, ExportPdf, ExportPowerPoint,
    FILE_MENU_ID, KEEP_MINE_ID, LOAD_THEIRS_ID, MARKDOWN_ID, NOTICES_ID, OPEN_IN_STAGEPACK_ID,
    OUTSIDE_CHANGE, OUTSIDE_CHANGE_ID, OpenFolder, OpenPacked, PROBLEM_ID, SAVE_AS_FOLDER_ID,
    SAVE_AS_PACKED_ID, STATUS_ID, Save, SaveAs, SetUpForCodeEditor, TITLE_ID, WorkspaceView,
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
    // The slide fills its place as a 16:9 box can, at a scale factor of 2.
    let area = cx
        .update_window(handle, |_, window, _| window.find(CANVAS_ID).bounds())
        .unwrap();
    let slide = letterbox(area, 2.0);
    assert_eq!(scale(cx, &canvas), f32::from(slide.size.width) * 2.0 / 1920.);
    let drawn = images(cx, &canvas);
    assert_eq!(drawn.len(), 3, "background, title and the offer to add text");
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
    assert_eq!(new.len(), 3, "background, title and the offer to add text");
    assert!(
        Arc::ptr_eq(&old[0], &new[0]),
        "the slides share one background, which keeps its image"
    );
    cx.update_window(handle, |_, window, _| {
        assert!(
            old[1..]
                .iter()
                .all(|image| !window.has_image_atlas_entry(image)),
            "the first slide's own images were released"
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

const SHOWN: LayerLook = LayerLook {
    visible: true,
    opacity: 1.0,
    offset_y: 0.0,
};

const HIDDEN: LayerLook = LayerLook {
    visible: false,
    opacity: 0.0,
    offset_y: 0.0,
};

fn in_atlas(cx: &mut TestAppContext, handle: AnyWindowHandle, image: &RenderImage) -> bool {
    cx.update_window(handle, |_, window, _| window.has_image_atlas_entry(image))
        .unwrap()
}

fn set_looks(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    canvas: &Entity<SlideCanvas>,
    looks: Vec<LayerLook>,
) {
    cx.update(|cx| canvas.update(cx, |canvas, cx| canvas.set_looks(looks, cx)));
    settle(cx, handle);
}

fn set_slide_look(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    canvas: &Entity<SlideCanvas>,
    look: SlideLook,
) {
    cx.update(|cx| canvas.update(cx, |canvas, cx| canvas.set_slide_look(look, cx)));
    settle(cx, handle);
}

/// Puts up a background and a heading that the window has never painted, with `looks`.
fn show_new_layers(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    canvas: &Entity<SlideCanvas>,
    looks: Vec<LayerLook>,
) -> Vec<Arc<RenderImage>> {
    let layers = document("# Other\n").layers(scale(cx, canvas));
    cx.update(|cx| {
        canvas.update(cx, |canvas, cx| {
            canvas.set_layers(layers, cx);
            canvas.set_looks(looks, cx);
        })
    });
    settle(cx, handle);
    images(cx, canvas)
}

#[gpui_kit::test]
fn hidden_layers_are_not_painted_but_keep_their_images(cx: &mut TestAppContext) {
    let (handle, view) = open_with_view(cx, "# One\n");
    settle(cx, handle);
    let canvas = slide_canvas(cx, &view);

    let drawn = show_new_layers(cx, handle, &canvas, vec![SHOWN, HIDDEN]);
    assert_eq!(drawn.len(), 2, "the hidden layer is still listed");
    assert!(in_atlas(cx, handle, &drawn[0]));
    assert!(
        !in_atlas(cx, handle, &drawn[1]),
        "a hidden layer isn't painted"
    );

    set_looks(cx, handle, &canvas, vec![SHOWN, SHOWN]);
    assert!(
        Arc::ptr_eq(&drawn[1], &images(cx, &canvas)[1]),
        "showing it uses the image it already had"
    );
    assert!(in_atlas(cx, handle, &drawn[1]), "it's painted once shown");

    set_looks(cx, handle, &canvas, vec![SHOWN, HIDDEN]);
    assert!(
        in_atlas(cx, handle, &drawn[1]),
        "hiding it again leaves it in the atlas, ready to come back"
    );
}

#[gpui_kit::test]
fn a_layer_with_no_look_is_fully_visible(cx: &mut TestAppContext) {
    let (handle, view) = open_with_view(cx, "# One\n");
    settle(cx, handle);
    let canvas = slide_canvas(cx, &view);

    let drawn = show_new_layers(cx, handle, &canvas, vec![HIDDEN]);
    assert!(!in_atlas(cx, handle, &drawn[0]));
    assert!(in_atlas(cx, handle, &drawn[1]), "it has no look of its own");
}

#[gpui_kit::test]
fn looks_beyond_the_last_layer_are_ignored(cx: &mut TestAppContext) {
    let (handle, view) = open_with_view(cx, "# One\n");
    settle(cx, handle);
    let canvas = slide_canvas(cx, &view);

    let drawn = show_new_layers(cx, handle, &canvas, vec![SHOWN, SHOWN, HIDDEN, HIDDEN]);
    assert_eq!(drawn.len(), 2);
    assert!(in_atlas(cx, handle, &drawn[0]));
    assert!(in_atlas(cx, handle, &drawn[1]));
}

#[gpui_kit::test]
fn layers_at_different_opacities_are_all_painted(cx: &mut TestAppContext) {
    let (handle, view) = open_with_view(cx, "# One\n");
    settle(cx, handle);
    let canvas = slide_canvas(cx, &view);
    let old = images(cx, &canvas);

    let fading = LayerLook {
        opacity: 0.5,
        ..SHOWN
    };
    let drawn = show_new_layers(cx, handle, &canvas, vec![SHOWN, fading]);
    assert!(in_atlas(cx, handle, &drawn[0]));
    assert!(in_atlas(cx, handle, &drawn[1]));
    assert!(
        !in_atlas(cx, handle, &old[0]) && !in_atlas(cx, handle, &old[1]),
        "the images no layer uses any more were released"
    );
}

#[gpui_kit::test]
fn a_layer_just_starting_to_fade_is_painted_already(cx: &mut TestAppContext) {
    let (handle, view) = open_with_view(cx, "# One\n");
    settle(cx, handle);
    let canvas = slide_canvas(cx, &view);

    let starting = LayerLook {
        opacity: 0.0,
        ..SHOWN
    };
    let drawn = show_new_layers(cx, handle, &canvas, vec![SHOWN, starting]);
    assert!(in_atlas(cx, handle, &drawn[1]));
}

#[gpui_kit::test]
fn with_every_layer_hidden_the_canvas_still_does_its_housekeeping(cx: &mut TestAppContext) {
    let (handle, view) = open_with_view(cx, "# One\n");
    settle(cx, handle);
    let canvas = slide_canvas(cx, &view);
    let (scale_before, old) = (scale(cx, &canvas), images(cx, &canvas));

    show_new_layers(cx, handle, &canvas, vec![HIDDEN, HIDDEN]);
    assert!(
        !in_atlas(cx, handle, &old[0]) && !in_atlas(cx, handle, &old[1]),
        "the images no layer uses any more were released"
    );

    cx.update_window(handle, |_, window, _| window.set_scale_factor(2.0 * 1.02))
        .unwrap();
    settle(cx, handle);
    assert!(
        scale(cx, &canvas) > scale_before * 1.01,
        "the new size of the slide was measured"
    );
}

#[gpui_kit::test]
fn a_layer_moves_by_slide_units(cx: &mut TestAppContext) {
    let (handle, view) = open_with_view(cx, "# One\n");
    settle(cx, handle);
    let canvas = slide_canvas(cx, &view);

    // The background fills the slide, which is 1080 units high.
    let moved_by = |offset_y| LayerLook { offset_y, ..SHOWN };
    let drawn = show_new_layers(cx, handle, &canvas, vec![moved_by(1081.0), SHOWN]);
    assert!(!in_atlas(cx, handle, &drawn[0]), "it has left the slide");

    set_looks(cx, handle, &canvas, vec![moved_by(1075.0), SHOWN]);
    assert!(in_atlas(cx, handle, &drawn[0]), "a strip of it is still on");
}

#[gpui_kit::test]
fn a_pushed_slide_stays_inside_its_letterbox(cx: &mut TestAppContext) {
    let (handle, view) = open_with_view(cx, "# One\n");
    // Far wider than 16:9, so there's a bar at each side for the slide to be pushed into.
    cx.simulate_window_resize(handle, size(px(1800.), px(500.)));
    settle(cx, handle);
    let canvas = slide_canvas(cx, &view);

    let look = |offset_x| SlideLook {
        opacity: 1.0,
        offset_x,
    };
    set_slide_look(cx, handle, &canvas, look(1.0));
    let drawn = show_new_layers(cx, handle, &canvas, vec![SHOWN, SHOWN]);
    assert!(
        !in_atlas(cx, handle, &drawn[0]),
        "a whole slide along, the background has left the slide, though there's room in the bar"
    );

    set_slide_look(cx, handle, &canvas, look(0.99));
    assert!(in_atlas(cx, handle, &drawn[0]), "a strip of it is still on");
}

#[gpui_kit::test]
fn a_faded_slide_still_paints(cx: &mut TestAppContext) {
    let (handle, view) = open_with_view(cx, "# One\n");
    settle(cx, handle);
    let canvas = slide_canvas(cx, &view);

    set_slide_look(
        cx,
        handle,
        &canvas,
        SlideLook {
            opacity: 0.25,
            offset_x: 0.0,
        },
    );
    let drawn = show_new_layers(cx, handle, &canvas, vec![SHOWN, SHOWN]);
    assert!(in_atlas(cx, handle, &drawn[0]));
    assert!(in_atlas(cx, handle, &drawn[1]));
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
fn ctrl_shift_m_shows_the_markdown(cx: &mut TestAppContext) {
    let handle = open_talk(cx, "# One\n");
    cx.simulate_keystrokes(handle, "ctrl-shift-m");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find(MARKDOWN_ID).visible());
    })
    .unwrap();
}

#[gpui_kit::test]
fn ctrl_shift_m_hides_the_markdown_again(cx: &mut TestAppContext) {
    let handle = open_talk(cx, "# One\n");
    cx.simulate_keystrokes(handle, "ctrl-shift-m");
    cx.simulate_keystrokes(handle, "ctrl-shift-m");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(MARKDOWN_ID).is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_markdown_is_to_the_right_of_the_slide(cx: &mut TestAppContext) {
    let handle = open_talk(cx, "# One\n");
    cx.simulate_keystrokes(handle, "ctrl-shift-m");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let (canvas, panel) = (window.find(CANVAS_ID), window.find(MARKDOWN_ID));
        assert!(panel.bounds().left() >= canvas.bounds().right());
        assert!(panel.bounds().top() <= canvas.bounds().top());
        assert!(
            panel.bounds().bottom() >= canvas.bounds().bottom(),
            "beside the notes too"
        );
        assert_eq!(panel.bounds().right(), px(1280.));
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_markdown_shows_the_lines_of_the_file(cx: &mut TestAppContext) {
    let handle = open_talk(cx, "\n# One\n\nSome words\n");
    cx.simulate_keystrokes(handle, "ctrl-shift-m");
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
    cx.simulate_keystrokes(handle, "ctrl-shift-m");
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
    cx.simulate_keystrokes(handle, "ctrl-shift-m");
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
    assert_eq!(before.len(), 3, "background, title and the offer to add text");

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

#[gpui_kit::test]
fn the_title_bar_has_file_and_export(cx: &mut TestAppContext) {
    let handle = open_talk(cx, "# One\n");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let (file, export) = (window.find(FILE_MENU_ID), window.find(EXPORT_MENU_ID));
        assert!(file.visible(), "{file:?}");
        assert!(export.visible(), "{export:?}");
        let title = window.find(TITLE_ID);
        assert!(
            file.bounds().right() <= export.bounds().left()
                && export.bounds().right() <= title.bounds().left(),
            "{file:?} {export:?} {title:?}"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_slide_still_changes_with_the_document_shared(cx: &mut TestAppContext) {
    let handle = open_talk(cx, "---\ntitle: T\n---\n\n# One\n\n---\n\n# Two\n");
    assert_eq!(title(cx, handle), "T · 1 / 2");
    cx.simulate_keystrokes(handle, "right");
    assert_eq!(title(cx, handle), "T · 2 / 2");
}

#[gpui_kit::test]
fn the_arrow_keys_still_work_after_using_a_menu(cx: &mut TestAppContext) {
    let handle = open_talk(cx, "---\ntitle: T\n---\n\n# One\n\n---\n\n# Two\n");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(EXPORT_MENU_ID, cx);
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.simulate_keystrokes(handle, "right");
    assert_eq!(title(cx, handle), "T · 2 / 2");
}

/// What the status line says, if it says anything.
fn status(cx: &mut TestAppContext, handle: AnyWindowHandle) -> Option<String> {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window
            .try_find(STATUS_ID)
            .map(|status| status.label().expect("the status has a label").to_string())
    })
    .unwrap()
}

/// Answers the Save dialog that has just been asked for, and lets what comes of it finish.
fn choose(cx: &mut TestAppContext, chosen: Option<PathBuf>) {
    cx.run_until_parked();
    assert!(cx.did_prompt_for_new_path(), "a Save dialog is showing");
    cx.simulate_new_path_selection(move |_| chosen);
    cx.run_until_parked();
}

#[gpui_kit::test]
fn an_export_writes_the_file_and_says_so(cx: &mut TestAppContext) {
    let handle = open_talk(cx, "# One\n");
    assert_eq!(status(cx, handle), None);
    let elsewhere = tempfile::tempdir().unwrap();
    let out = elsewhere.path().join("exported.pdf");

    cx.dispatch_action(handle, ExportPdf);
    choose(cx, Some(out.clone()));

    assert!(std::fs::read(&out).unwrap().starts_with(b"%PDF"));
    assert_eq!(
        status(cx, handle).as_deref(),
        Some("Exported exported.pdf.")
    );
}

#[gpui_kit::test]
fn a_powerpoint_export_reports_what_the_slides_said(cx: &mut TestAppContext) {
    let handle = open_talk(cx, "# One\n\n---\nlayout: nope\n---\n\n# Two\n");
    let elsewhere = tempfile::tempdir().unwrap();
    let out = elsewhere.path().join("exported.pptx");

    cx.dispatch_action(handle, ExportPowerPoint);
    choose(cx, Some(out.clone()));

    assert!(std::fs::read(&out).unwrap().starts_with(b"PK"));
    let said = status(cx, handle).expect("it says what happened");
    assert!(
        said.starts_with("Exported exported.pptx.") && said.contains("nope"),
        "{said}"
    );
}

#[gpui_kit::test]
fn cancelling_the_save_dialog_does_nothing(cx: &mut TestAppContext) {
    let handle = open_talk(cx, "# One\n");
    cx.dispatch_action(handle, ExportPdf);
    choose(cx, None);
    assert_eq!(status(cx, handle), None);
}

#[gpui_kit::test]
fn an_export_that_fails_says_why(cx: &mut TestAppContext) {
    let handle = open_talk(cx, "# One\n");
    let elsewhere = tempfile::tempdir().unwrap();
    let out = elsewhere.path().join("missing/exported.pdf");

    cx.dispatch_action(handle, ExportPdf);
    choose(cx, Some(out));

    let said = status(cx, handle).expect("it says what happened");
    assert!(said.contains("exported.pdf"), "{said}");
    assert!(!said.starts_with("Exported"), "{said}");
}

#[gpui_kit::test]
fn an_export_is_asked_for_beside_the_talk(cx: &mut TestAppContext) {
    let talk = open_saved_talk(cx, "# One\n");
    let beside = talk.file.parent().unwrap().parent().unwrap().to_path_buf();
    cx.dispatch_action(talk.handle, ExportPdf);
    cx.run_until_parked();
    cx.simulate_new_path_selection(move |directory| {
        assert_eq!(directory, beside);
        None
    });
}

/// How many windows are open.
fn window_count(cx: &TestAppContext) -> usize {
    cx.read(|cx| cx.windows().len())
}

/// The one window that isn't `first`, once a second one has opened.
fn newest_window(cx: &TestAppContext, first: AnyWindowHandle) -> AnyWindowHandle {
    assert_eq!(window_count(cx), 2, "a window of its own");
    cx.read(|cx| cx.windows())
        .into_iter()
        .find(|window| window.window_id() != first.window_id())
        .expect("there are two windows")
}

#[gpui_kit::test]
fn opening_a_folder_gives_it_a_window_of_its_own(cx: &mut TestAppContext) {
    let handle = open_talk(cx, "# One\n");
    // With no slides.md, so that the new window has no watcher: a test's scheduler can't have a
    // watcher's thread waking its tasks.
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().to_path_buf();

    cx.dispatch_action(handle, OpenFolder);
    cx.run_until_parked();
    cx.simulate_path_prompt_response(move |options| {
        assert!(options.directories && !options.files);
        Some(vec![folder])
    });
    cx.run_until_parked();

    let opened = newest_window(cx, handle);
    cx.update_window(opened, |_, window, cx| {
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
fn opening_a_packed_talk_gives_it_a_window_of_its_own(cx: &mut TestAppContext) {
    let handle = open_talk(cx, "# One\n");
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("talk");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(
        folder.join("slides.md"),
        "---\ntitle: Packed\n---\n\n# One\n",
    )
    .unwrap();
    let file = dir.path().join("talk.sptx");
    slides_engine::pack_folder(&folder, &file, "test").unwrap();

    cx.dispatch_action(handle, OpenPacked);
    cx.run_until_parked();
    cx.simulate_path_prompt_response(move |options| {
        assert!(options.files && !options.directories);
        Some(vec![file])
    });
    cx.run_until_parked();

    let opened = newest_window(cx, handle);
    assert_eq!(title(cx, opened), "Packed · 1 / 1");
}

#[gpui_kit::test]
fn cancelling_the_open_dialog_does_nothing(cx: &mut TestAppContext) {
    let handle = open_talk(cx, "# One\n");
    cx.dispatch_action(handle, OpenFolder);
    cx.run_until_parked();
    cx.simulate_path_prompt_response(|_| None);
    cx.run_until_parked();
    assert_eq!(window_count(cx), 1);
    assert_eq!(status(cx, handle), None);
}

/// A window on a packed copy of `source`, opened the way the app opens one.
fn open_packed_talk(
    cx: &mut TestAppContext,
    source: &str,
) -> (tempfile::TempDir, AnyWindowHandle, Entity<WorkspaceView>) {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("talk");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("slides.md"), source).unwrap();
    let file = dir.path().join("talk.sptx");
    slides_engine::pack_folder(&folder, &file, "test").unwrap();
    let (handle, view) = open_view(cx, move |window, cx| WorkspaceView::open(file, window, cx));
    (dir, handle, view)
}

/// Whether the File menu, opened and closed again, holds `item`.
fn file_menu_has(cx: &mut TestAppContext, handle: AnyWindowHandle, item: &str) -> bool {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(FILE_MENU_ID, cx);
        let found = window.within("popup-menu").try_find_by_label(item).is_some();
        window.click(FILE_MENU_ID, cx);
        found
    })
    .unwrap()
}

#[gpui_kit::test]
fn the_file_menu_offers_save_and_save_as(cx: &mut TestAppContext) {
    let handle = open_talk(cx, "# One\n");
    assert!(file_menu_has(cx, handle, "Save"));
    assert!(file_menu_has(cx, handle, "Save As…"));
    assert!(!file_menu_has(cx, handle, "Save as packed talk…"));
    let (_dir, handle, _view) = open_packed_talk(cx, "# One\n");
    assert!(file_menu_has(cx, handle, "Save As…"));
    assert!(!file_menu_has(cx, handle, "Save as folder…"));
}

/// Asks to Save As, and answers which form with the button `kind`.
fn save_as(cx: &mut TestAppContext, handle: AnyWindowHandle, kind: &'static str) {
    cx.dispatch_action(handle, SaveAs);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(kind, cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn an_item_of_the_export_menu_runs_the_export(cx: &mut TestAppContext) {
    let handle = open_talk(cx, "# One\n");
    let elsewhere = tempfile::tempdir().unwrap();
    let out = elsewhere.path().join("from-the-menu.pdf");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(EXPORT_MENU_ID, cx);
        // The menu's rows are numbered: PowerPoint, then PDF.
        window.within("popup-menu").click(1usize, cx);
    })
    .unwrap();
    choose(cx, Some(out.clone()));
    assert!(std::fs::read(&out).unwrap().starts_with(b"%PDF"));
}

#[gpui_kit::test]
fn a_packed_talk_shows_and_is_not_watched(cx: &mut TestAppContext) {
    let (_dir, handle, view) = open_packed_talk(cx, "---\ntitle: T\n---\n\n# One\n");
    assert_eq!(title(cx, handle), "T · 1 / 1");
    cx.read(|cx| assert!(!view.read(cx).follows_saves()));
}

#[gpui_kit::test]
fn a_packed_talk_can_be_saved_as_a_folder_from_the_window(cx: &mut TestAppContext) {
    let (dir, handle, view) = open_packed_talk(cx, "# One\n");
    cx.update(|cx| view.update(cx, |view, _| view.set_no_watching_for_test()));
    let folder = dir.path().join("unpacked");

    save_as(cx, handle, SAVE_AS_FOLDER_ID);
    choose(cx, Some(folder.clone()));

    assert_eq!(
        std::fs::read_to_string(folder.join("slides.md")).unwrap(),
        "# One\n"
    );
    assert_eq!(status(cx, handle).as_deref(), Some("Saved unpacked."));
    // It's kept there from now on, and its saves are followed as a folder's are.
    cx.read(|cx| assert!(!view.read(cx).document().unwrap().read(cx).is_packed()));
    cx.read(|cx| assert_eq!(view.read(cx).followed_folder(), Some(folder.as_path())));
}

#[gpui_kit::test]
fn a_talk_in_a_folder_can_be_packed_from_the_window(cx: &mut TestAppContext) {
    let talk = open_saved_talk(cx, "# One\n");
    let elsewhere = tempfile::tempdir().unwrap();
    let out = elsewhere.path().join("copy.sptx");

    save_as(cx, talk.handle, SAVE_AS_PACKED_ID);
    choose(cx, Some(out.clone()));

    let copy = Document::open_packed(&out, Fonts::bundled_only()).unwrap();
    assert_eq!(copy.source(), "# One\n");
    assert_eq!(status(cx, talk.handle).as_deref(), Some("Saved copy.sptx."));
}

const SET_UP_FOR_CODE_EDITOR: &str = "Set up for a code editor";

/// A bundle holding only the toolkit's source, which is all a Cargo file needs.
fn bundle_with_toolkit_source() -> (tempfile::TempDir, Bundle) {
    let dir = tempfile::tempdir().unwrap();
    let toolkit = dir.path().join("toolkit-src").join("toolkit");
    std::fs::create_dir_all(&toolkit).unwrap();
    std::fs::write(toolkit.join("Cargo.toml"), "[package]\n").unwrap();
    let bundle = Bundle {
        root: dir.path().to_path_buf(),
    };
    (dir, bundle)
}

#[gpui_kit::test]
fn a_talk_in_a_folder_can_be_set_up_for_a_code_editor(cx: &mut TestAppContext) {
    let talk = open_saved_talk(cx, "# One\n");
    let (_bundle_dir, bundle) = bundle_with_toolkit_source();
    let projects = tempfile::tempdir().unwrap();
    let root = projects.path().to_path_buf();
    cx.update(|cx| {
        talk.view.update(cx, |view, _| {
            view.set_bundle_for_test(Some(bundle));
            view.set_editor_projects_for_test(root);
        })
    });
    assert!(file_menu_has(cx, talk.handle, SET_UP_FOR_CODE_EDITOR));

    cx.dispatch_action(talk.handle, SetUpForCodeEditor);
    cx.run_until_parked();

    assert!(!talk.file.with_file_name("Cargo.toml").exists(), "nothing is written in the talk");
    let made: Vec<PathBuf> = std::fs::read_dir(projects.path()).unwrap().map(|entry| entry.unwrap().path()).collect();
    assert_eq!(made.len(), 1, "{made:?}");
    assert!(made[0].join("Cargo.toml").is_file());
    let project = made[0].canonicalize().unwrap();
    assert_eq!(
        status(cx, talk.handle),
        Some(format!("Open {} in your code editor to get completions for this talk's components.", project.display()))
    );
}

#[gpui_kit::test]
fn only_a_folder_talk_with_a_bundle_offers_a_code_editor(cx: &mut TestAppContext) {
    let talk = open_saved_talk(cx, "# One\n");
    cx.update(|cx| {
        talk.view
            .update(cx, |view, _| view.set_bundle_for_test(None))
    });
    assert!(!file_menu_has(cx, talk.handle, SET_UP_FOR_CODE_EDITOR));

    let (_dir, handle, view) = open_packed_talk(cx, "# One\n");
    let (_bundle_dir, bundle) = bundle_with_toolkit_source();
    cx.update(|cx| view.update(cx, |view, _| view.set_bundle_for_test(Some(bundle))));
    assert!(!file_menu_has(cx, handle, SET_UP_FOR_CODE_EDITOR));
}

const SLIDEV_TALK: &str =
    "This is a Slidev talk made for Stagepack, so this app can't open it yet.";

/// A file holding Stagepack's manifest.
fn stagepack_file() -> (tempfile::TempDir, PathBuf) {
    use std::io::Write as _;

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("slidev.sptx");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&file).unwrap());
    zip.start_file("stagepack.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"{}").unwrap();
    zip.finish().unwrap();
    (dir, file)
}

#[gpui_kit::test]
fn a_stagepack_file_says_what_it_is(cx: &mut TestAppContext) {
    let (_dir, file) = stagepack_file();
    let (handle, _view) = open_view(cx, move |window, cx| WorkspaceView::open(file, window, cx));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find(PROBLEM_ID).label(), Some(SLIDEV_TALK));
    })
    .unwrap();
}

#[gpui_kit::test]
fn with_no_stagepack_there_is_nothing_to_offer(cx: &mut TestAppContext) {
    let (_dir, file) = stagepack_file();
    let (handle, _view) = open_view(cx, move |window, cx| {
        WorkspaceView::for_slidev_talk(file, None, window, cx)
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find(PROBLEM_ID).label(), Some(SLIDEV_TALK));
        assert!(window.try_find(OPEN_IN_STAGEPACK_ID).is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_stagepack_that_will_not_start_says_so(cx: &mut TestAppContext) {
    let (dir, file) = stagepack_file();
    let gone = dir.path().join("no-such-program");
    let (handle, _view) = open_view(cx, move |window, cx| {
        WorkspaceView::for_slidev_talk(file, Some(gone), window, cx)
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(OPEN_IN_STAGEPACK_ID, cx);
    })
    .unwrap();
    let said = status(cx, handle).expect("it says what happened");
    assert!(said.contains("Stagepack"), "{said}");
}

#[cfg(unix)]
#[gpui_kit::test]
fn the_button_opens_the_file_in_stagepack(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt as _;

    let (dir, file) = stagepack_file();
    let said = dir.path().join("said");
    let stagepack = dir.path().join("stagepack");
    std::fs::write(
        &stagepack,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{0}.tmp'\nmv '{0}.tmp' '{0}'\n",
            said.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&stagepack, std::fs::Permissions::from_mode(0o755)).unwrap();
    let expected = format!("open\n{}\n", file.display());
    let (handle, _view) = open_view(cx, move |window, cx| {
        WorkspaceView::for_slidev_talk(file, Some(stagepack), window, cx)
    });

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(OPEN_IN_STAGEPACK_ID, cx);
    })
    .unwrap();

    let start = std::time::Instant::now();
    while !said.exists() {
        assert!(start.elapsed() < Duration::from_secs(10), "it never ran");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(std::fs::read_to_string(&said).unwrap(), expected);
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

// Building a talk's own components.

const NO_COMPILER: &str = "This copy of the app has no compiler, so components can't be built.";

const BUILD_BAR: &str = "Some components in this talk need building to show as they're written.";

/// A slide with the talk's own `Score` on it.
const SCORE_SLIDE: &str = "# One\n\n<Score />\n";

/// The source of a component called `tag`.
fn component_source(tag: &str) -> String {
    format!(
        "use toolkit::prelude::*;\n\n#[component]\nfn {tag}() -> Element {{\n    text(\"{tag}\")\n}}\n"
    )
}

/// A module that calls itself `tag`, and renders a line of text.
fn component_module(tag: &str) -> Vec<u8> {
    let schema = format!(r#"{{"name":"{tag}","props":[],"children":false}}"#);
    let answer = r#"{"element":{"Text":{"text":"built","size":null,"bold":false,"color":null}},"animated":false}"#;
    let packed = |at: u64, text: &str| ((at << 32) | text.len() as u64) as i64;
    let data = |at: u32, text: &str| {
        let escaped: String = text.bytes().map(|byte| format!("\\{byte:02x}")).collect();
        format!(r#"(data (i32.const {at}) "{escaped}")"#)
    };
    let text = format!(
        r#"(module
  (memory (export "memory") 8)
  (global $heap (mut i32) (i32.const 262144))
  {}
  {}
  (func (export "abi_version") (result i32) (i32.const 1))
  (func (export "alloc") (param $len i32) (result i32)
    (local $at i32)
    (local.set $at (global.get $heap))
    (global.set $heap (i32.add (global.get $heap) (local.get $len)))
    (local.get $at))
  (func (export "schema") (result i64) (i64.const {}))
  (func (export "render") (param $ptr i32) (param $len i32) (result i64) (i64.const {})))"#,
        data(1024, &schema),
        data(8192, answer),
        packed(1024, &schema),
        packed(8192, answer),
    );
    wat::parse_str(&text).unwrap_or_else(|error| panic!("{error}\n{text}"))
}

/// A compiler that counts its calls, refuses a source marked broken at line 12, and otherwise
/// gives a module named for the source's component.
fn counting_compiler() -> (Compile, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let compile: Compile = Arc::new(move |file: &str, source: &str| {
        counted.fetch_add(1, Ordering::SeqCst);
        if source.contains("// broken") {
            return Err(vec![Diagnostic {
                file: file.to_string(),
                line: 12,
                message: "expected `;`".to_string(),
            }]);
        }
        Ok(component_module(
            &component_tag(source).expect("a component"),
        ))
    });
    (compile, calls)
}

/// A talk folder whose slide shows `Score`, with `components/score.rs` holding `score`.
fn component_talk_folder(score: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), SCORE_SLIDE).unwrap();
    std::fs::create_dir(dir.path().join("components")).unwrap();
    std::fs::write(dir.path().join("components/score.rs"), score).unwrap();
    dir
}

struct ComponentTalk {
    folder: tempfile::TempDir,
    /// Where the app keeps its permissions, for this test.
    permissions: PathBuf,
    _app_data: tempfile::TempDir,
    calls: Arc<AtomicUsize>,
    handle: AnyWindowHandle,
    view: Entity<WorkspaceView>,
}

impl ComponentTalk {
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn click(&self, cx: &mut TestAppContext, id: &'static str) {
        cx.update_window(self.handle, |_, window, cx| {
            window.render_frame(cx);
            window.click(id, cx);
        })
        .unwrap();
        cx.run_until_parked();
        settle(cx, self.handle);
    }

    fn shows(&self, cx: &mut TestAppContext, id: &'static str) -> bool {
        cx.update_window(self.handle, |_, window, cx| {
            window.render_frame(cx);
            window.try_find(id).is_some_and(|found| found.visible())
        })
        .unwrap()
    }

    fn says(&self, cx: &mut TestAppContext, line: &str) -> bool {
        cx.update_window(self.handle, |_, window, cx| {
            window.render_frame(cx);
            window.try_find_by_label(line).is_some()
        })
        .unwrap()
    }

    fn score_state(&self, cx: &TestAppContext) -> ComponentState {
        let document = cx
            .read(|cx| self.view.read(cx).document().cloned())
            .unwrap();
        cx.read(|cx| {
            document
                .read(cx)
                .components()
                .iter()
                .find(|info| info.tag == "Score")
                .map(|info| info.state)
                .expect("Score is one of the talk's components")
        })
    }

    /// Saves a new source for `Score`, as the watcher would see it, and lets the wait pass.
    fn change_score(&self, cx: &mut TestAppContext, source: &str) {
        std::fs::write(self.folder.path().join("components/score.rs"), source).unwrap();
        cx.update(|cx| {
            self.view
                .update(cx, |view, cx| view.components_changed_soon(cx))
        });
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        settle(cx, self.handle);
    }

    fn always_allowed(&self) -> bool {
        BuildPermissions::load(self.permissions.clone()).always(self.folder.path())
    }
}

/// Opens the talk in `folder` with a counting compiler, and permissions of its own, already
/// allowing the folder if `allowed`.
fn open_component_talk_in(
    cx: &mut TestAppContext,
    folder: tempfile::TempDir,
    allowed: bool,
) -> ComponentTalk {
    let app_data = tempfile::tempdir().unwrap();
    let permissions = app_data.path().join("permissions.json");
    if allowed {
        BuildPermissions::load(permissions.clone())
            .set_always(folder.path())
            .unwrap();
    }
    let (compile, calls) = counting_compiler();
    let talk = Document::open(folder.path(), Fonts::bundled_only()).unwrap();
    let file = permissions.clone();
    let (handle, view) = open_view(cx, move |window, cx| {
        let mut view = WorkspaceView::open_with(talk, window, cx);
        view.set_compile_for_test(compile);
        view.set_permissions_for_test(BuildPermissions::load(file));
        view
    });
    settle(cx, handle);
    ComponentTalk {
        folder,
        permissions,
        _app_data: app_data,
        calls,
        handle,
        view,
    }
}

fn open_component_talk(cx: &mut TestAppContext, score: &str) -> ComponentTalk {
    open_component_talk_in(cx, component_talk_folder(score), false)
}

#[gpui_kit::test]
fn opening_a_talk_starts_no_build(cx: &mut TestAppContext) {
    // Even in a folder that's always built.
    let talk = open_component_talk_in(cx, component_talk_folder(&component_source("Score")), true);
    cx.executor().advance_clock(Duration::from_secs(5));
    cx.run_until_parked();
    settle(cx, talk.handle);
    assert_eq!(talk.calls(), 0);
    assert_eq!(talk.score_state(cx), ComponentState::NeedsBuilding);
    assert!(!talk.folder.path().join("build").exists());
}

#[gpui_kit::test]
fn the_bar_offers_both_choices(cx: &mut TestAppContext) {
    let talk = open_component_talk(cx, &component_source("Score"));
    assert!(talk.shows(cx, BUILD_ONCE_ID));
    assert!(talk.shows(cx, BUILD_ALWAYS_ID));
    assert!(talk.says(cx, BUILD_BAR));
    assert_eq!(talk.calls(), 0);
}

#[gpui_kit::test]
fn a_talk_with_everything_built_shows_no_bar(cx: &mut TestAppContext) {
    let (handle, _view) = open_with_view(cx, "# One\n");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(BUILD_ONCE_ID).is_none());
        assert!(window.try_find(BUILD_ALWAYS_ID).is_none());
        assert!(window.try_find(COMPONENT_PROBLEMS_ID).is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn build_once_builds_and_shows_the_component(cx: &mut TestAppContext) {
    let talk = open_component_talk(cx, &component_source("Score"));
    let canvas = slide_canvas(cx, &talk.view);
    let before = images(cx, &canvas);

    talk.click(cx, BUILD_ONCE_ID);

    assert_eq!(talk.calls(), 1);
    assert_eq!(talk.score_state(cx), ComponentState::UpToDate);
    assert!(!talk.shows(cx, BUILD_ONCE_ID), "nothing is left to build");
    let after = images(cx, &canvas);
    assert!(
        before.len() != after.len() || before.iter().zip(&after).any(|(a, b)| !Arc::ptr_eq(a, b)),
        "the slide was drawn again"
    );
    assert!(!talk.always_allowed());

    let mut changed = component_source("Score");
    changed.push_str("// changed\n");
    talk.change_score(cx, &changed);
    assert_eq!(talk.calls(), 1, "once was only once");
    assert_eq!(talk.score_state(cx), ComponentState::OutOfDate);
    assert!(talk.shows(cx, BUILD_ONCE_ID));
}

#[gpui_kit::test]
fn build_always_is_remembered_and_builds_on_change(cx: &mut TestAppContext) {
    let talk = open_component_talk(cx, &component_source("Score"));

    talk.click(cx, BUILD_ALWAYS_ID);

    assert_eq!(talk.calls(), 1);
    assert!(talk.always_allowed());
    assert_eq!(talk.score_state(cx), ComponentState::UpToDate);
    assert!(!talk.shows(cx, BUILD_ALWAYS_ID));

    let mut changed = component_source("Score");
    changed.push_str("// changed\n");
    talk.change_score(cx, &changed);
    assert_eq!(talk.calls(), 2);
    assert_eq!(talk.score_state(cx), ComponentState::UpToDate);
}

#[gpui_kit::test]
fn changes_close_together_build_once(cx: &mut TestAppContext) {
    let talk = open_component_talk_in(cx, component_talk_folder(&component_source("Score")), true);
    for edit in ["// one\n", "// two\n"] {
        let mut changed = component_source("Score");
        changed.push_str(edit);
        std::fs::write(talk.folder.path().join("components/score.rs"), changed).unwrap();
        cx.update(|cx| {
            talk.view
                .update(cx, |view, cx| view.components_changed_soon(cx))
        });
        cx.executor().advance_clock(Duration::from_millis(60));
        cx.run_until_parked();
    }
    assert_eq!(talk.calls(), 0, "the folder isn't quiet yet");
    cx.executor().advance_clock(Duration::from_millis(40));
    cx.run_until_parked();
    assert_eq!(talk.calls(), 1);
}

#[gpui_kit::test]
fn a_change_in_a_folder_not_allowed_starts_no_build(cx: &mut TestAppContext) {
    let talk = open_component_talk(cx, &component_source("Score"));
    let mut changed = component_source("Score");
    changed.push_str("// changed\n");
    talk.change_score(cx, &changed);
    cx.executor().advance_clock(Duration::from_secs(5));
    cx.run_until_parked();
    assert_eq!(talk.calls(), 0);
    assert!(talk.shows(cx, BUILD_ONCE_ID));
}

#[gpui_kit::test]
fn a_packed_talk_offers_no_build(cx: &mut TestAppContext) {
    let folder = component_talk_folder(&component_source("Score"));
    let elsewhere = tempfile::tempdir().unwrap();
    let file = elsewhere.path().join("talk.sptx");
    slides_engine::pack_folder(folder.path(), &file, "test").unwrap();
    let (compile, calls) = counting_compiler();
    let (handle, view) = open_view(cx, move |window, cx| {
        let mut view = WorkspaceView::open(file, window, cx);
        view.set_compile_for_test(compile);
        view
    });
    settle(cx, handle);
    let document = cx.read(|cx| view.read(cx).document().cloned()).unwrap();
    cx.read(|cx| {
        let document = document.read(cx);
        assert!(document.is_packed());
        assert_eq!(document.components().len(), 1, "it does have a component");
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(BUILD_ONCE_ID).is_none());
        assert!(window.try_find(BUILD_ALWAYS_ID).is_none());
        assert!(window.try_find_by_label(NO_COMPILER).is_none());
    })
    .unwrap();
    cx.update(|cx| view.update(cx, |view, cx| view.components_changed_soon(cx)));
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[gpui_kit::test]
fn a_problem_shows_its_file_and_line(cx: &mut TestAppContext) {
    let broken = format!("{}// broken\n", component_source("Score"));
    let talk = open_component_talk(cx, &broken);

    talk.click(cx, BUILD_ONCE_ID);

    assert!(talk.shows(cx, COMPONENT_PROBLEMS_ID));
    assert!(talk.says(cx, "components/score.rs:12: expected `;`"));
    assert_eq!(talk.score_state(cx), ComponentState::NeedsBuilding);

    talk.change_score(cx, &component_source("Score"));
    talk.click(cx, BUILD_ONCE_ID);
    assert!(
        !talk.says(cx, "components/score.rs:12: expected `;`"),
        "a successful build clears its own"
    );
    assert!(!talk.shows(cx, COMPONENT_PROBLEMS_ID));
}

#[gpui_kit::test]
fn a_problem_the_scan_finds_shows_too(cx: &mut TestAppContext) {
    let folder = component_talk_folder(&component_source("Score"));
    std::fs::write(folder.path().join("components/bad name.rs"), "").unwrap();
    let talk = open_component_talk_in(cx, folder, false);
    assert!(talk.shows(cx, COMPONENT_PROBLEMS_ID));
    assert!(talk.says(
        cx,
        "components/bad name.rs: A component's file name can have only letters, digits, _ and -, and at most 64 of them"
    ));
}

#[gpui_kit::test]
fn the_problems_panel_shows_at_most_twenty_lines(cx: &mut TestAppContext) {
    let folder = component_talk_folder(&component_source("Score"));
    for n in 0..25 {
        std::fs::write(folder.path().join(format!("components/bad name {n:02}.rs")), "").unwrap();
    }
    let talk = open_component_talk_in(cx, folder, false);
    assert!(talk.says(
        cx,
        "components/bad name 19.rs: A component's file name can have only letters, digits, _ and -, and at most 64 of them"
    ));
    assert!(!talk.says(
        cx,
        "components/bad name 20.rs: A component's file name can have only letters, digits, _ and -, and at most 64 of them"
    ));
    assert!(talk.says(cx, "and 5 more"));
}

#[gpui_kit::test]
fn no_compiler_says_so(cx: &mut TestAppContext) {
    // No bundle is set up for the tests, so this window finds no compiler.
    let folder = component_talk_folder(&component_source("Score"));
    let talk = Document::open(folder.path(), Fonts::bundled_only()).unwrap();
    let (handle, _view) = open_view(cx, |window, cx| WorkspaceView::open_with(talk, window, cx));
    settle(cx, handle);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find_by_label(NO_COMPILER).is_some());
        assert!(window.try_find(BUILD_ONCE_ID).is_none());
        assert!(window.try_find(BUILD_ALWAYS_ID).is_none());
    })
    .unwrap();
}

// ---- Saving ----

/// Adds a slide to the talk in the window, as the slide commands do.
fn add_slide(cx: &mut TestAppContext, view: &Entity<WorkspaceView>) {
    cx.update(|cx| {
        let document = view.read(cx).document().unwrap().clone();
        document.update(cx, |document, cx| {
            document.add_slide("content").unwrap();
            cx.notify();
        });
    });
}

fn source_of(cx: &TestAppContext, view: &Entity<WorkspaceView>) -> String {
    cx.read(|cx| view.read(cx).document().unwrap().read(cx).source().to_string())
}

#[gpui_kit::test]
fn the_title_shows_unsaved_changes(cx: &mut TestAppContext) {
    let talk = open_saved_talk(cx, "---\ntitle: T\n---\n\n# One\n");
    assert_eq!(title(cx, talk.handle), "T · 1 / 1");
    add_slide(cx, &talk.view);
    assert_eq!(title(cx, talk.handle), "• T · 2 / 2");
}

#[gpui_kit::test]
fn ctrl_s_saves(cx: &mut TestAppContext) {
    let talk = open_saved_talk(cx, "---\ntitle: T\n---\n\n# One\n");
    add_slide(cx, &talk.view);

    cx.simulate_keystrokes(talk.handle, "ctrl-s");
    cx.run_until_parked();

    assert_eq!(std::fs::read_to_string(&talk.file).unwrap(), source_of(cx, &talk.view));
    assert_eq!(title(cx, talk.handle), "T · 2 / 2");
    let folder = talk.file.parent().unwrap().file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(status(cx, talk.handle), Some(format!("Saved {folder}.")));
}

#[gpui_kit::test]
fn a_failed_save_says_why_and_stays_unsaved(cx: &mut TestAppContext) {
    let talk = open_saved_talk(cx, "---\ntitle: T\n---\n\n# One\n");
    add_slide(cx, &talk.view);
    // A folder where the file was: it can't be replaced.
    std::fs::remove_file(&talk.file).unwrap();
    std::fs::create_dir(&talk.file).unwrap();

    cx.simulate_keystrokes(talk.handle, "ctrl-s");
    cx.run_until_parked();

    let said = status(cx, talk.handle).expect("it says what happened");
    assert!(said.starts_with("Couldn't save"), "{said}");
    assert_eq!(title(cx, talk.handle), "• T · 2 / 2");
}

/// The talk in the window with an edit of its own, and its file changed from outside.
fn changed_underneath(cx: &mut TestAppContext) -> (SavedTalk, String, String) {
    let talk = open_saved_talk(cx, "---\ntitle: T\n---\n\n# One\n");
    add_slide(cx, &talk.view);
    let mine = source_of(cx, &talk.view);
    let theirs = "---\ntitle: T\n---\n\n# Theirs\n".to_string();
    save(cx, &talk, &theirs);
    wait(cx, &talk, 200);
    (talk, mine, theirs)
}

fn shows_the_bar(cx: &mut TestAppContext, handle: AnyWindowHandle) -> bool {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.try_find(OUTSIDE_CHANGE_ID).is_some()
    })
    .unwrap()
}

#[gpui_kit::test]
fn the_outside_change_bar_offers_both(cx: &mut TestAppContext) {
    let (talk, mine, theirs) = changed_underneath(cx);
    cx.update_window(talk.handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find(OUTSIDE_CHANGE_ID).label(), Some(OUTSIDE_CHANGE));
        assert!(window.find(KEEP_MINE_ID).visible());
        assert!(window.find(LOAD_THEIRS_ID).visible());
    })
    .unwrap();
    assert_eq!(source_of(cx, &talk.view), mine, "nothing is taken until asked");

    cx.update_window(talk.handle, |_, window, cx| window.click(LOAD_THEIRS_ID, cx))
        .unwrap();
    settle(cx, talk.handle);
    assert_eq!(source_of(cx, &talk.view), theirs);
    assert!(!shows_the_bar(cx, talk.handle));
    assert!(shows_line(cx, &talk, "# Theirs"));
    assert_eq!(title(cx, talk.handle), "T · 1 / 1");
}

#[gpui_kit::test]
fn keeping_my_changes_puts_the_bar_away(cx: &mut TestAppContext) {
    let (talk, mine, _theirs) = changed_underneath(cx);
    cx.update_window(talk.handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(KEEP_MINE_ID, cx);
    })
    .unwrap();
    settle(cx, talk.handle);
    assert!(!shows_the_bar(cx, talk.handle));
    assert_eq!(source_of(cx, &talk.view), mine);
    assert_eq!(title(cx, talk.handle), "• T · 2 / 2");

    cx.simulate_keystrokes(talk.handle, "ctrl-s");
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&talk.file).unwrap(), mine);
}

#[gpui_kit::test]
fn the_apps_own_save_brings_no_bar(cx: &mut TestAppContext) {
    let talk = open_saved_talk(cx, "---\ntitle: T\n---\n\n# One\n");
    add_slide(cx, &talk.view);
    cx.simulate_keystrokes(talk.handle, "ctrl-s");
    cx.run_until_parked();
    // The watcher sees the app's own write, after another edit.
    add_slide(cx, &talk.view);
    cx.update(|cx| talk.view.update(cx, |view, cx| view.reload_soon(talk.file.clone(), cx)));
    wait(cx, &talk, 200);
    assert!(!shows_the_bar(cx, talk.handle));
    assert_eq!(title(cx, talk.handle), "• T · 3 / 3");
}

/// Tries to close the window, as its close button does, and says whether it closed at once.
fn try_to_close(cx: &mut TestAppContext, handle: AnyWindowHandle) -> bool {
    let closed = VisualTestContext::from_window(handle, cx).simulate_close();
    cx.run_until_parked();
    closed
}

fn is_open(cx: &TestAppContext, handle: AnyWindowHandle) -> bool {
    cx.read(|cx| cx.windows().iter().any(|window| window.window_id() == handle.window_id()))
}

fn click_in(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
fn closing_with_unsaved_changes_asks(cx: &mut TestAppContext) {
    let talk = open_saved_talk(cx, "---\ntitle: T\n---\n\n# One\n");
    assert!(try_to_close(cx, talk.handle), "nothing to lose");

    add_slide(cx, &talk.view);
    assert!(!try_to_close(cx, talk.handle));
    cx.update_window(talk.handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find(CLOSE_SAVE_ID).visible());
        assert!(window.find(CLOSE_DONT_SAVE_ID).visible());
        assert!(window.find(CLOSE_CANCEL_ID).visible());
    })
    .unwrap();

    // Cancel keeps the window, and its changes.
    click_in(cx, talk.handle, CLOSE_CANCEL_ID);
    assert!(is_open(cx, talk.handle));
    assert_eq!(title(cx, talk.handle), "• T · 2 / 2");

    // Save writes them, then closes.
    assert!(!try_to_close(cx, talk.handle));
    let mine = source_of(cx, &talk.view);
    click_in(cx, talk.handle, CLOSE_SAVE_ID);
    assert_eq!(std::fs::read_to_string(&talk.file).unwrap(), mine);
    assert!(!is_open(cx, talk.handle));
}

#[gpui_kit::test]
fn dont_save_closes_without_writing(cx: &mut TestAppContext) {
    let talk = open_saved_talk(cx, "---\ntitle: T\n---\n\n# One\n");
    add_slide(cx, &talk.view);
    assert!(!try_to_close(cx, talk.handle));

    click_in(cx, talk.handle, CLOSE_DONT_SAVE_ID);

    assert!(!is_open(cx, talk.handle));
    assert_eq!(
        std::fs::read_to_string(&talk.file).unwrap(),
        "---\ntitle: T\n---\n\n# One\n"
    );
}

#[gpui_kit::test]
fn a_save_that_fails_on_closing_keeps_the_window(cx: &mut TestAppContext) {
    let talk = open_saved_talk(cx, "---\ntitle: T\n---\n\n# One\n");
    add_slide(cx, &talk.view);
    std::fs::remove_file(&talk.file).unwrap();
    std::fs::create_dir(&talk.file).unwrap();
    assert!(!try_to_close(cx, talk.handle));

    click_in(cx, talk.handle, CLOSE_SAVE_ID);

    assert!(is_open(cx, talk.handle));
    let said = status(cx, talk.handle).expect("it says what happened");
    assert!(said.starts_with("Couldn't save"), "{said}");
}

#[gpui_kit::test]
fn save_as_a_packed_talk_keeps_it_there(cx: &mut TestAppContext) {
    let talk = open_saved_talk(cx, "---\ntitle: T\n---\n\n# One\n");
    add_slide(cx, &talk.view);
    let elsewhere = tempfile::tempdir().unwrap();
    let out = elsewhere.path().join("copy.sptx");

    cx.simulate_keystrokes(talk.handle, "ctrl-shift-s");
    cx.update_window(talk.handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(SAVE_AS_PACKED_ID, cx);
    })
    .unwrap();
    choose(cx, Some(out.clone()));

    assert_eq!(title(cx, talk.handle), "T · 2 / 2");
    assert_eq!(status(cx, talk.handle).as_deref(), Some("Saved copy.sptx."));
    // The next save goes to the pack, and the folder is left as it was.
    add_slide(cx, &talk.view);
    cx.simulate_keystrokes(talk.handle, "ctrl-s");
    cx.run_until_parked();
    let packed = slides_engine::open_packed(&out).unwrap();
    assert_eq!(packed.slides().unwrap(), source_of(cx, &talk.view));
    assert_eq!(
        std::fs::read_to_string(&talk.file).unwrap(),
        "---\ntitle: T\n---\n\n# One\n"
    );
    cx.read(|cx| assert!(cx.windows().len() == 1 && talk.view.read(cx).document().unwrap().read(cx).is_packed()));
}

/// Asks for a save, and leaves it running: nothing waiting is run yet.
fn start_saving(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    cx.update_window(handle, |_, window, cx| window.dispatch_action(Save.boxed_clone(), cx))
        .unwrap();
}

#[gpui_kit::test]
fn closing_with_save_while_a_save_runs_writes_the_last_edits(cx: &mut TestAppContext) {
    let talk = open_saved_talk(cx, "---\ntitle: T\n---\n\n# One\n");
    add_slide(cx, &talk.view);
    start_saving(cx, talk.handle);
    add_slide(cx, &talk.view);
    // Closing while that save runs, before anything waiting has run.
    assert!(!VisualTestContext::from_window(talk.handle, cx).simulate_close());
    cx.update_window(talk.handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(CLOSE_SAVE_ID, cx);
    })
    .unwrap();
    cx.run_until_parked();

    assert_eq!(std::fs::read_to_string(&talk.file).unwrap(), source_of(cx, &talk.view));
    assert!(source_of(cx, &talk.view).matches("# ").count() >= 3, "both slides were added");
    assert!(!is_open(cx, talk.handle));
}

#[gpui_kit::test]
fn a_read_held_back_by_a_save_is_read_again_after_it(cx: &mut TestAppContext) {
    let first = "---\ntitle: T\n---\n\n# One\n";
    let talk = open_saved_talk(cx, first);
    add_slide(cx, &talk.view);
    let mine = source_of(cx, &talk.view);

    start_saving(cx, talk.handle);
    // A read from before the save comes in while it runs.
    cx.update(|cx| {
        talk.view
            .update(cx, |view, cx| view.reload_for_test(Some(first.to_string()), cx))
    });
    wait(cx, &talk, 200);

    assert_eq!(std::fs::read_to_string(&talk.file).unwrap(), mine);
    assert_eq!(source_of(cx, &talk.view), mine, "the old read isn't taken");
    assert!(!shows_the_bar(cx, talk.handle));
    assert_eq!(title(cx, talk.handle), "T · 2 / 2");
}

#[gpui_kit::test]
fn saving_over_an_outside_change_asks_first(cx: &mut TestAppContext) {
    let talk = open_saved_talk(cx, "---\ntitle: T\n---\n\n# One\n");
    add_slide(cx, &talk.view);
    let mine = source_of(cx, &talk.view);
    let theirs = "---\ntitle: T\n---\n\n# Theirs\n";
    // Changed outside, and the save asked for before the watcher has said so.
    std::fs::write(&talk.file, theirs).unwrap();

    cx.simulate_keystrokes(talk.handle, "ctrl-s");
    settle(cx, talk.handle);

    assert_eq!(std::fs::read_to_string(&talk.file).unwrap(), theirs, "nothing was written");
    assert!(shows_the_bar(cx, talk.handle));
    assert_eq!(source_of(cx, &talk.view), mine);

    cx.update_window(talk.handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(KEEP_MINE_ID, cx);
    })
    .unwrap();
    cx.simulate_keystrokes(talk.handle, "ctrl-s");
    assert_eq!(std::fs::read_to_string(&talk.file).unwrap(), mine);
    assert!(!shows_the_bar(cx, talk.handle));
}

#[gpui_kit::test]
fn the_save_tool_comes_first_on_home_and_works_when_there_is_something_to_save(
    cx: &mut TestAppContext,
) {
    let talk = open_saved_talk(cx, "# One\n");
    cx.update_window(talk.handle, |_, window, cx| {
        window.render_frame(cx);
        let save = window.find(SAVE_ID);
        assert!(save.visible(), "{save:?}");
        assert_eq!(save.disabled(), Some(true), "nothing to save");
        let file = window.find("group-File");
        assert_eq!(file.label(), Some("File"));
        let new_slide = window.find(NEW_SLIDE_ID);
        assert!(save.bounds().right() <= new_slide.bounds().left(), "Save comes first");
        assert!(file.bounds().right() <= window.find("group-Slides").bounds().left());
    })
    .unwrap();

    click_and_settle(cx, talk.handle, NEW_SLIDE_ID);
    cx.update_window(talk.handle, |_, window, cx| {
        window.render_frame(cx);
        assert_ne!(window.find(SAVE_ID).disabled(), Some(true), "an edit to save");
    })
    .unwrap();

    click_and_settle(cx, talk.handle, SAVE_ID);
    assert_eq!(std::fs::read_to_string(&talk.file).unwrap(), source_of(cx, &talk.view));
    cx.update_window(talk.handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find(SAVE_ID).disabled(), Some(true), "saved");
    })
    .unwrap();
}

fn click_and_settle(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    })
    .unwrap();
    cx.run_until_parked();
    settle(cx, handle);
}

fn bounds_of(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) -> Bounds<Pixels> {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.find(id).bounds()
    })
    .unwrap()
}

#[gpui_kit::test]
fn the_outside_change_bar_sits_under_the_ribbon_and_leaves_the_slide_its_size(
    cx: &mut TestAppContext,
) {
    let talk = open_saved_talk(cx, "---\ntitle: T\n---\n\n# One\n");
    let canvas = bounds_of(cx, talk.handle, CANVAS_ID);
    add_slide(cx, &talk.view);
    save(cx, &talk, "---\ntitle: T\n---\n\n# Theirs\n");
    wait(cx, &talk, 200);
    assert!(shows_the_bar(cx, talk.handle));

    let bar = bounds_of(cx, talk.handle, OUTSIDE_CHANGE_ID);
    let ribbon = bounds_of(cx, talk.handle, RIBBON_ID);
    assert!(bar.top() >= ribbon.bottom(), "under the ribbon: {bar:?} {ribbon:?}");
    assert!(bar.bottom() <= canvas.bottom(), "over the top of the desk: {bar:?} {canvas:?}");
    assert_eq!(bounds_of(cx, talk.handle, CANVAS_ID), canvas, "the slide keeps its size");

    click_and_settle(cx, talk.handle, KEEP_MINE_ID);
    assert!(!shows_the_bar(cx, talk.handle));
    assert_eq!(bounds_of(cx, talk.handle, CANVAS_ID), canvas);
}

#[gpui_kit::test]
fn saved_goes_from_the_status_line_after_five_seconds_or_at_the_next_edit(
    cx: &mut TestAppContext,
) {
    let talk = open_saved_talk(cx, "# One\n");
    click_and_settle(cx, talk.handle, NEW_SLIDE_ID);
    cx.simulate_keystrokes(talk.handle, "ctrl-s");
    settle(cx, talk.handle);
    let said = status(cx, talk.handle).expect("it says so");
    assert!(said.starts_with("Saved "), "{said}");
    wait(cx, &talk, 4_999);
    assert_eq!(status(cx, talk.handle), Some(said.clone()));
    wait(cx, &talk, 1);
    assert_eq!(status(cx, talk.handle), None);

    click_and_settle(cx, talk.handle, NEW_SLIDE_ID);
    cx.simulate_keystrokes(talk.handle, "ctrl-s");
    settle(cx, talk.handle);
    assert!(status(cx, talk.handle).is_some());
    click_and_settle(cx, talk.handle, NEW_SLIDE_ID);
    assert_eq!(status(cx, talk.handle), None, "the next edit takes it away");
}

#[gpui_kit::test]
fn the_file_menu_names_both_ways_to_open_a_talk(cx: &mut TestAppContext) {
    let handle = open_talk(cx, "# One\n");
    assert!(file_menu_has(cx, handle, "Open talk folder…"));
    assert!(file_menu_has(cx, handle, "Open talk file…"));
    assert!(!file_menu_has(cx, handle, "Open folder…"));
}

#[gpui_kit::test]
fn the_title_has_room_after_the_export_menu(cx: &mut TestAppContext) {
    let handle = open_talk(cx, "# One\n");
    let export = bounds_of(cx, handle, EXPORT_MENU_ID);
    let title = bounds_of(cx, handle, TITLE_ID);
    assert!(title.left() - export.right() >= px(12.), "{export:?} {title:?}");
}
