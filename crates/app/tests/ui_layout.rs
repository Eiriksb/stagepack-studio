use gpui_kit::test::TestWindowExt;
use gpui_kit::{AppContext, Bounds, Point, TestAppContext, WindowBounds, WindowOptions, px, size};
use stagepack_studio::workspace_view::{CANVAS_ID, INSPECTOR_ID, TITLE_ID, WorkspaceView};

fn open_workspace(cx: &mut TestAppContext) -> gpui_kit::AnyWindowHandle {
    cx.update(gpui_kit::init);
    cx.update(|cx| {
        let bounds = Bounds {
            origin: Point::default(),
            size: size(px(1280.), px(800.)),
        };
        let (window, _view) = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            cx,
            |_, cx| cx.new(WorkspaceView::for_test),
        )
        .expect("open test window");
        window
    })
}

#[gpui_kit::test]
fn layout_has_canvas_inspector_and_title(cx: &mut TestAppContext) {
    let handle = open_workspace(cx);

    cx.update_window(handle, |_, window, cx| {
        // The resizable group settles its panel sizes after the first frame.
        window.render_frame(cx);
        window.render_frame(cx);

        let title = window.find(TITLE_ID);
        let canvas = window.find(CANVAS_ID);
        let inspector = window.find(INSPECTOR_ID);

        assert!(title.visible(), "title: {title:?}");
        assert!(canvas.visible(), "canvas: {canvas:?}");
        assert!(inspector.visible(), "inspector: {inspector:?}");

        assert!(
            title.bounds().bottom() <= canvas.bounds().top(),
            "the title bar is above the canvas: {title:?} {canvas:?}"
        );
        assert!(
            inspector.bounds().left() >= canvas.bounds().right(),
            "the inspector is right of the canvas: {canvas:?} {inspector:?}"
        );
        let width = inspector.bounds().size.width.as_f32();
        assert!(
            (width - 280.).abs() <= 1.,
            "the inspector is 280 px wide, not {width}"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn title_bar_actions_start_disabled_and_clean(cx: &mut TestAppContext) {
    let handle = open_workspace(cx);

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);

        for id in ["undo", "redo", "save"] {
            let button = window.find(id);
            assert!(button.visible(), "{id}: {button:?}");
            assert_eq!(button.disabled(), Some(true), "{id}: {button:?}");
        }
        let dirty = window.find("dirty");
        assert!(!dirty.visible(), "the dirty marker is hidden: {dirty:?}");
        assert_eq!(window.find(TITLE_ID).label(), Some("Untitled"));
    })
    .unwrap();
}
