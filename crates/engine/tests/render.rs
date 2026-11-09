mod common;
use slides_engine::*;
use tiny_skia::PathBuilder;
use toolkit::Color;

const RED: Color = Color { r: 255, g: 0, b: 0, a: 255 };

fn square_layer(rotate: f32) -> LayerSpec {
    let path = PathBuilder::from_rect(tiny_skia::Rect::from_xywh(0.0, 0.0, 100.0, 100.0).unwrap());
    LayerSpec { source: None, rect: Rect { x: 200.0, y: 100.0, w: 100.0, h: 100.0 }, rotate, items: vec![DisplayItem::Fill { path, color: RED }] }
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
