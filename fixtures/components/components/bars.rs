use toolkit::prelude::*;

const WIDTH: f32 = 1200.0;
const HEIGHT: f32 = 600.0;
/// The room below the bars, for their labels.
const LABEL_ROOM: f32 = 48.0;

/// A bar for each row of a CSV file after the first: a label, a comma, then a number.
#[component]
fn Bars(data: DataFile, ctx: &Ctx) -> Element {
    let Some(table) = ctx.asset_text(&data.0) else {
        return problem(format!("Can't read {}", data.0));
    };
    let mut rows = Vec::new();
    for line in table.lines().skip(1) {
        let Some((label, number)) = line.split_once(',') else { continue };
        let Ok(value) = number.trim().parse::<f32>() else {
            return problem(format!("\"{}\" isn't a number", number.trim()));
        };
        rows.push((label.trim().to_string(), value));
    }
    let largest = rows.iter().map(|(_, value)| *value).fold(0.0, f32::max);
    if largest <= 0.0 {
        return problem("There are no bars to draw");
    }
    let slot = WIDTH / rows.len() as f32;
    let floor = HEIGHT - LABEL_ROOM;
    let mut items = Vec::new();
    for (index, (label, value)) in rows.into_iter().enumerate() {
        let x = index as f32 * slot;
        let height = value / largest * floor;
        if height > 0.0 {
            let bar = shape(ShapeKind::Rect, slot * 0.8, height).fill(ctx.theme().accent);
            items.push(at(x + slot * 0.1, floor - height, bar));
        }
        items.push(at(x, floor + 8.0, text(label).size(24.0)).centered_in(slot));
    }
    canvas(WIDTH, HEIGHT, items)
}
