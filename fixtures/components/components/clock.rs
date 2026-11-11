use toolkit::prelude::*;

/// A clock face whose hand goes round once a minute, from when the slide appears.
#[component]
fn Clock(size: f32, ctx: &Ctx) -> Element {
    // A missing or bad number is 0, so this is the size to use then.
    let size = if size > 0.0 { size } else { 200.0 };
    let middle = size / 2.0;
    let turned = ctx.time() / 60.0 * std::f32::consts::TAU;
    let tip_x = middle + turned.sin() * middle * 0.8;
    let tip_y = middle - turned.cos() * middle * 0.8;
    // A line from the middle to the tip, in the words of an SVG path.
    let hand = format!("M {middle} {middle} L {tip_x} {tip_y}");
    let white = Color::rgb(255, 255, 255);
    canvas(
        size,
        size,
        vec![
            at(0.0, 0.0, shape(ShapeKind::Ellipse, size, size).fill(ctx.theme().accent)),
            at(0.0, 0.0, path(hand, size, size).stroke(white, size / 25.0)),
        ],
    )
}
