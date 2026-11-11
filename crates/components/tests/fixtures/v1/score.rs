use toolkit::prelude::*;

/// The points, on a rounded badge.
fn badge(points: u32) -> Element {
    let mut badge = shape(ShapeKind::Rounded, 120.0, 80.0);
    if let Element::Shape { child, padding, .. } = &mut badge {
        *padding = 8.0;
        *child = Some(Box::new(text(points.to_string()).size(40.0).bold()));
    }
    badge
}

#[component]
fn Score(team: String, points: u32, colour: Color) -> Element {
    row![text(team).size(48.0), badge(points).fill(colour)].gap(24.0)
}
