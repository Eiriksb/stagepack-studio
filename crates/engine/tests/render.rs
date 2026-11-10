mod common;
use slides_engine::*;
use tiny_skia::PathBuilder;
use toolkit::Color;

const RED: Color = Color { r: 255, g: 0, b: 0, a: 255 };

fn square_layer(rotate: f32) -> LayerSpec {
    let path = PathBuilder::from_rect(tiny_skia::Rect::from_xywh(0.0, 0.0, 100.0, 100.0).unwrap());
    LayerSpec { source: None, rect: Rect { x: 200.0, y: 100.0, w: 100.0, h: 100.0 }, rotate, items: vec![DisplayItem::Fill { path, color: RED }], click: None, enter: Enter::Appear, native: Vec::new(), is_title: false }
}

#[test]
fn an_unrotated_layer_sits_at_its_rect() {
    let layer = render_layer(&square_layer(0.0), 1.0, &mut Fonts::bundled_only());
    assert_eq!((layer.x, layer.y), (200, 100));
}

#[test]
fn scale_multiplies_position_and_size() {
    let layer = render_layer(&square_layer(0.0), 2.0, &mut Fonts::bundled_only());
    assert_eq!((layer.x, layer.y), (400, 200));
    assert!(layer.pixmap.width() >= 200);
}

#[test]
fn rotation_turns_around_the_centre() {
    let layer = render_layer(&square_layer(45.0), 1.0, &mut Fonts::bundled_only());
    assert!(layer.pixmap.width() >= 141 && layer.pixmap.width() <= 144);
    assert!(layer.x < 200 && layer.x > 170);
}

const PLACE: Rect = Rect { x: 200.0, y: 100.0, w: 100.0, h: 100.0 };

fn square(x: f32, y: f32) -> tiny_skia::Path {
    PathBuilder::from_rect(tiny_skia::Rect::from_xywh(x, y, 100.0, 100.0).unwrap())
}

fn layer_of(rect: Rect, rotate: f32, item: DisplayItem) -> LayerSpec {
    LayerSpec { source: None, rect, rotate, items: vec![item], click: None, enter: Enter::Appear, native: Vec::new(), is_title: false }
}

fn is_red(layer: &Layer, x: u32, y: u32) -> bool {
    let pixel = layer.pixmap.pixel(x, y).unwrap().demultiply();
    (pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()) == (255, 0, 0, 255)
}

#[test]
fn an_item_past_the_rect_grows_the_layer_and_moves_its_origin() {
    let spec = layer_of(PLACE, 0.0, DisplayItem::Fill { path: square(-50.0, -20.0), color: RED });
    let mut fonts = Fonts::bundled_only();
    let layer = render_layer(&spec, 1.0, &mut fonts);
    assert_eq!((layer.x, layer.y), (150, 80));
    assert!(layer.pixmap.width() >= 150 && layer.pixmap.height() >= 120);
    assert!(is_red(&layer, 10, 10));
    let doubled = render_layer(&spec, 2.0, &mut fonts);
    assert_eq!((doubled.x, doubled.y), (300, 160));
}

#[test]
fn a_stroke_counts_half_its_width_past_its_path() {
    let spec = layer_of(PLACE, 0.0, DisplayItem::Stroke { path: square(0.0, 0.0), color: RED, width: 20.0 });
    let layer = render_layer(&spec, 1.0, &mut Fonts::bundled_only());
    assert_eq!((layer.x, layer.y), (190, 90));
    assert!(layer.pixmap.width() >= 120 && layer.pixmap.height() >= 120);
}

#[test]
fn a_picture_past_the_rect_grows_the_layer() {
    let pixmap = std::sync::Arc::new(tiny_skia::Pixmap::new(1, 1).unwrap());
    let spec = layer_of(PLACE, 0.0, DisplayItem::Picture { pixmap, rect: Rect { x: 0.0, y: 150.0, w: 100.0, h: 100.0 } });
    let layer = render_layer(&spec, 1.0, &mut Fonts::bundled_only());
    assert!(layer.pixmap.height() >= 250);
}

#[test]
fn a_rotated_layer_still_turns_around_its_rect() {
    // The item hangs below the rect, so a quarter turn about the rect's centre swings it left.
    let spec = layer_of(PLACE, 90.0, DisplayItem::Fill { path: square(0.0, 100.0), color: RED });
    let layer = render_layer(&spec, 1.0, &mut Fonts::bundled_only());
    assert!((99..=100).contains(&layer.x), "x is {}", layer.x);
    assert!((99..=100).contains(&layer.y), "y is {}", layer.y);
    assert!((200..=203).contains(&layer.pixmap.width()));
    assert!(is_red(&layer, 50, 50));
}

#[test]
fn absurd_rects_do_not_panic() {
    let mut fonts = Fonts::bundled_only();
    let mut rendered = |x: f32, w: f32, h: f32| {
        render_layer(&layer_of(Rect { x, y: x, w, h }, 0.0, DisplayItem::Fill { path: square(0.0, 0.0), color: RED }), 1.0, &mut fonts)
    };

    let wide = rendered(0.0, 1e9, 100.0);
    assert_eq!(wide.pixmap.width(), 16384);
    assert!(is_red(&wide, 50, 50), "what fits is still drawn");

    for w in [f32::NAN, f32::INFINITY] {
        let blank = rendered(0.0, w, 100.0);
        assert_eq!((blank.pixmap.width(), blank.pixmap.height()), (1, 1));
        assert_eq!(blank.pixmap.pixel(0, 0).unwrap().alpha(), 0);
    }

    assert!(is_red(&rendered(0.0, 0.0, 0.0), 50, 50));

    // compose adds a layer's size to its position, which a far-away layer must survive.
    let far: Vec<_> = [1e30, -1e30, 3e9, -3e9].map(|x| rendered(x, 100.0, 100.0)).into();
    compose(&mut tiny_skia::Pixmap::new(10, 10).unwrap(), &far);
}

#[test]
fn a_layer_is_limited_by_pixel_count() {
    let spec = layer_of(Rect { x: 0.0, y: 0.0, w: 16000.0, h: 16000.0 }, 0.0, DisplayItem::Fill { path: square(0.0, 0.0), color: RED });
    let layer = render_layer(&spec, 1.0, &mut Fonts::bundled_only());
    let (width, height) = (u64::from(layer.pixmap.width()), u64::from(layer.pixmap.height()));
    assert!(width * height <= 64_000_000, "{width} by {height}");
    assert!(width > 1 && height > 1 && width <= 16384 && height <= 16384, "{width} by {height}");
    assert!(is_red(&layer, 50, 50), "what fits is still drawn");
}

#[test]
fn frames_hold_straight_alpha_in_bgra_order() {
    let mut pixmap = tiny_skia::Pixmap::new(1, 1).unwrap();
    pixmap.pixels_mut()[0] = tiny_skia::ColorU8::from_rgba(255, 0, 0, 128).premultiply();
    assert_eq!(to_bgra_frame(&pixmap).buffer().as_raw(), &vec![0, 0, 255, 128]);
}

#[test]
fn golden_primitives() {
    let mut fonts = Fonts::bundled_only();
    let layers = vec![render_layer(&square_layer(0.0), 1.0, &mut fonts), render_layer(&square_layer(30.0), 1.0, &mut fonts)];
    let mut target = tiny_skia::Pixmap::new(480, 270).unwrap();
    target.fill(tiny_skia::Color::WHITE);
    compose(&mut target, &layers);
    common::assert_golden("primitives", &target);
}
