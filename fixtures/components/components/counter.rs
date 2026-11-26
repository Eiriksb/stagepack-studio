use toolkit::prelude::*;

/// A count that goes up when its plus is pressed, or when a button on the slide says `add`, and
/// goes back to 0 when a button says `reset`.
#[component(interactive)]
fn Counter(ctx: &Ctx) -> Element {
    // What the count was after the last drawing: 0 when the slide has just appeared.
    let mut count: u32 = ctx.state();
    for event in ctx.events() {
        match event {
            // A press let go of on the plus.
            Event::Pointer { kind: PointerKind::Up, hit: Some(name), .. } if name == "plus" => count += 1,
            // A button's `do="add" value="1"` adds its value; a bad value adds nothing.
            Event::Action { name, value } if name == "add" => count += value.parse::<u32>().unwrap_or(0),
            Event::Action { name, .. } if name == "reset" => count = 0,
            _ => {}
        }
    }
    // Kept for the next drawing, for as long as the slide is shown.
    ctx.set_state(&count);
    let white = Color::rgb(255, 255, 255);
    let plus = canvas(
        160.0,
        160.0,
        vec![
            at(0.0, 0.0, shape(ShapeKind::Rounded, 160.0, 160.0).fill(ctx.theme().accent)),
            at(0.0, 30.0, text("+").size(80.0).bold().fill(white)).centered_in(160.0),
        ],
    );
    canvas(
        400.0,
        160.0,
        vec![at(0.0, 30.0, text(count.to_string()).size(80.0)), at(240.0, 0.0, hit("plus", plus))],
    )
}
