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
