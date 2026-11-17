use gpui_kit::test::TestWindowExt;
use gpui_kit::{AppContext, Bounds, Point, TestAppContext, WindowBounds, WindowOptions, px, size};
use stagepack_studio::workspace_view::{CANVAS_ID, TITLE_ID, WorkspaceView};

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
            |window, cx| cx.new(|cx| WorkspaceView::for_test(window, cx)),
        )
        .expect("open test window");
        window
    })
}

#[gpui_kit::test]
fn layout_has_a_canvas_under_the_title(cx: &mut TestAppContext) {
    let handle = open_workspace(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let title = window.find(TITLE_ID);
        let canvas = window.find(CANVAS_ID);
        assert!(title.visible(), "title: {title:?}");
        assert!(canvas.visible(), "canvas: {canvas:?}");
        assert!(
            title.bounds().bottom() <= canvas.bounds().top(),
            "the title bar is above the canvas: {title:?} {canvas:?}"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn undo_and_redo_show_at_1280(cx: &mut TestAppContext) {
    use stagepack_studio::ribbon::{MORE_RIGHT_ID, REDO_ID, RIBBON_ID, UNDO_ID};
    let handle = open_workspace(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let ribbon = window.find(RIBBON_ID).bounds();
        // What's under the arrow at the row's right end is behind it.
        let right = window.try_find(MORE_RIGHT_ID).map_or(ribbon.right(), |arrow| arrow.bounds().left());
        for tool in [UNDO_ID, REDO_ID] {
            let bounds = window.find(tool).bounds();
            assert!(ribbon.left() <= bounds.left() && bounds.right() <= right, "{tool} is cut off: {bounds:?} in {ribbon:?}");
        }
    })
    .unwrap();
}
