//! Where the caret, the selection of text and the outline of the thing selected are drawn,
//! and the drawing of them.

use super::*;

/// What's drawn over the slide for the text being typed, in slide units.
pub(super) struct Marks {
    pub(super) caret: Option<CaretRect>,
    pub(super) selection: SelectionRects,
}

impl SlideEditor {
    /// Where the caret goes at `pos` of the text being typed, in slide units.
    fn caret_at(&self, pos: Pos, cx: &mut Context<Self>) -> Option<CaretRect> {
        let (_, flow, layout) = self.editing_place(cx)?;
        if let Some(flow) = flow {
            return layout.caret(flow, pos);
        }
        // An empty region, before anything is typed: the caret is where its text would start.
        let key = &self.editing(cx)?.flow;
        if let Some((x, y)) = key.new_box() {
            // A new box, not written yet: at its corner.
            let (w, h) = TEXT_BOX_SIZE;
            let element = ElementBox { source: 0..0, rect: Rect { x: x as f32, y: y as f32, w, h }, rotate: 0.0, holds_text: true };
            return Some(empty_box_caret(&element, false));
        }
        if let Some(start) = key.tag_start() {
            // An empty box: at its corner, or a shape's middle.
            let element = layout.elements().iter().rev().find(|element| element.source.start == start)?.clone();
            let shape = self.document.as_ref()?.read(cx).talk().source.get(start..)?.starts_with("<Shape");
            return Some(empty_box_caret(&element, shape));
        }
        layout.placeholder_caret(key.region()?)
    }

    /// Where the caret at `pos` of the text being typed is in the window.
    pub(super) fn caret_in_window(&self, pos: Pos, cx: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let caret = self.caret_at(pos, cx)?;
        let slide = self.canvas.read(cx).slide_bounds()?;
        Some(caret_box(&caret, slide))
    }

    /// The caret and the selection of the text being typed, in slide units.
    pub(super) fn marks(&self, cx: &mut Context<Self>) -> Option<Marks> {
        let (_, flow, layout) = self.editing_place(cx)?;
        let selection = self.editing(cx)?.edit.selection;
        let Some(flow) = flow else {
            return Some(Marks {
                caret: self.caret_at(selection.head, cx),
                selection: SelectionRects::default(),
            });
        };
        let caret = if selection.is_caret() {
            layout.caret(flow, selection.head)
        } else {
            None
        };
        let selection = if selection.is_caret() {
            SelectionRects::default()
        } else {
            layout.selection_rects(flow, selection.start(), selection.end())
        };
        Some(Marks { caret, selection })
    }
}

/// `(x, y)` turned by `degrees` around `centre`, as the engine turns a layer.
pub(super) fn turned((x, y): (f32, f32), centre: (f32, f32), degrees: f32) -> (f32, f32) {
    if degrees == 0.0 {
        return (x, y);
    }
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (dx, dy) = (x - centre.0, y - centre.1);
    (
        centre.0 + dx * cos - dy * sin,
        centre.1 + dx * sin + dy * cos,
    )
}

/// The corners of the rectangle at `x`, `top`, `w` wide and `h` tall, turned, in slide units.
pub(super) fn corners(x: f32, top: f32, w: f32, h: f32, centre: (f32, f32), degrees: f32) -> [(f32, f32); 4] {
    [(x, top), (x + w, top), (x + w, top + h), (x, top + h)]
        .map(|corner| turned(corner, centre, degrees))
}

/// The caret's corners, `unit` window pixels to a slide unit.
pub(super) fn caret_corners(caret: &CaretRect, unit: f32) -> [(f32, f32); 4] {
    let width = CARET_WIDTH / unit;
    corners(
        caret.x - width / 2.,
        caret.top,
        width,
        caret.height,
        caret.centre,
        caret.rotate,
    )
}

/// The box the caret's corners fit in, in the window, on a slide drawn at `slide` there.
pub(super) fn caret_box(caret: &CaretRect, slide: Bounds<Pixels>) -> Bounds<Pixels> {
    let unit = f32::from(slide.size.width) / SLIDE_WIDTH;
    if unit <= 0.0 {
        return Bounds {
            origin: slide.origin,
            size: Size::default(),
        };
    }
    corners_box(&caret_corners(caret, unit), slide, unit)
}

/// How tall the caret in a box with nothing in it is drawn, in slide units, at most.
const EMPTY_BOX_CARET: f32 = 56.0;

/// Where the caret goes in `element`, a box with nothing typed in it yet: at its top-left
/// corner, or, in a shape, which centres its text, in its middle.
fn empty_box_caret(element: &ElementBox, shape: bool) -> CaretRect {
    let Rect { x, y, w, h } = element.rect;
    let height = EMPTY_BOX_CARET.min(h.max(1.0));
    let (x, top) = if shape { (x + w / 2., y + (h - height) / 2.) } else { (x, y) };
    CaretRect {
        x,
        top,
        height,
        rotate: element.rotate,
        centre: (element.rect.x + w / 2., element.rect.y + h / 2.),
    }
}

/// The corners of the thing selected, `element`, turned with it, in slide units.
pub(super) fn outline_corners(element: &ElementBox) -> [(f32, f32); 4] {
    let Rect { x, y, w, h } = element.rect;
    corners(x, y, w, h, (x + w / 2., y + h / 2.), element.rotate)
}

/// The box `corners`, in slide units, fit in, in the window, on a slide drawn at `slide`
/// there, `unit` window pixels to a slide unit.
pub(super) fn corners_box(corners: &[(f32, f32); 4], slide: Bounds<Pixels>, unit: f32) -> Bounds<Pixels> {
    let (xs, ys): (Vec<f32>, Vec<f32>) = corners.iter().copied().unzip();
    let fold = |values: &[f32], pick: fn(f32, f32) -> f32| {
        values.iter().copied().reduce(pick).unwrap_or(0.)
    };
    let (left, right) = (fold(&xs, f32::min), fold(&xs, f32::max));
    let (top, bottom) = (fold(&ys, f32::min), fold(&ys, f32::max));
    Bounds {
        origin: slide.origin + point(px(left * unit), px(top * unit)),
        size: size(px((right - left) * unit), px((bottom - top) * unit)),
    }
}

/// How long each dash of a new box's line is, and the gap after it, in the window's pixels.
const DASH: f32 = 6.0;

/// Draws a dashed line around `(x, y, w, h)`, in slide units, on a slide drawn at `slide`.
pub(super) fn paint_dashed((x, y, w, h): (f32, f32, f32, f32), slide: Bounds<Pixels>, unit: f32, color: Hsla, window: &mut Window) {
    let (dash, thick) = (DASH / unit, OUTLINE_WIDTH / 2. / unit);
    let edges = [((x, y), (x + w, y)), ((x + w, y), (x + w, y + h)), ((x + w, y + h), (x, y + h)), ((x, y + h), (x, y))];
    for ((x0, y0), (x1, y1)) in edges {
        let length = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
        if length <= 0. {
            continue;
        }
        let (dx, dy) = ((x1 - x0) / length, (y1 - y0) / length);
        let mut at = 0.;
        while at < length {
            let end = (at + dash).min(length);
            let (ax, ay, bx, by) = (x0 + dx * at, y0 + dy * at, x0 + dx * end, y0 + dy * end);
            // Across the line, half its width each side.
            let (nx, ny) = (-dy * thick, dx * thick);
            let points = [(ax + nx, ay + ny), (bx + nx, by + ny), (bx - nx, by - ny), (ax - nx, ay - ny)];
            paint_polygon(&points, slide, unit, color, window);
            at += dash * 2.;
        }
    }
}

pub(super) fn paint_polygon(
    points: &[(f32, f32)],
    slide: Bounds<Pixels>,
    unit: f32,
    color: Hsla,
    window: &mut Window,
) {
    paint_path(PathBuilder::fill(), points, slide, unit, color, window);
}

/// Draws the line around `points`, in slide units, on a slide drawn at `slide`.
pub(super) fn paint_outline(
    points: &[(f32, f32)],
    slide: Bounds<Pixels>,
    unit: f32,
    color: Hsla,
    window: &mut Window,
) {
    paint_path(PathBuilder::stroke(px(OUTLINE_WIDTH)), points, slide, unit, color, window);
}

fn paint_path(
    mut path: PathBuilder,
    points: &[(f32, f32)],
    slide: Bounds<Pixels>,
    unit: f32,
    color: Hsla,
    window: &mut Window,
) {
    let points: Vec<Point<Pixels>> = points
        .iter()
        .map(|&(x, y)| slide.origin + point(px(x * unit), px(y * unit)))
        .collect();
    path.add_polygon(&points, true);
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}
