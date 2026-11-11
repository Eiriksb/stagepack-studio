use toolkit::prelude::*;

/// A team's name, and its points on a badge of its colour.
#[component]
fn Score(team: String, points: u32, colour: Color) -> Element {
    row![text(team).size(48.0), badge(points, colour)].gap(24.0)
}

/// The points in white, on a rounded box of `colour`.
fn badge(points: u32, colour: Color) -> Element {
    let white = Color::rgb(255, 255, 255);
    canvas(
        120.0,
        80.0,
        vec![
            at(0.0, 0.0, shape(ShapeKind::Rounded, 120.0, 80.0).fill(colour)),
            at(0.0, 10.0, text(points.to_string()).size(48.0).bold().fill(white)).centered_in(120.0),
        ],
    )
}
