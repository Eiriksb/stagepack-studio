//! The mouse on the slide: where a press lands, and what it picks, selects or types in.

use super::*;

impl SlideEditor {
    /// Where `position`, in the window, is on the slide, in slide units, if it's on it.
    pub(super) fn slide_point(&self, position: Point<Pixels>, cx: &App) -> Option<(f32, f32)> {
        let slide = self.canvas.read(cx).slide_bounds()?;
        slide
            .contains(&position)
            .then(|| self.nearest_slide_point(position, cx))
            .flatten()
    }

    /// The point of the slide nearest `position`, in the window, in slide units: `position`
    /// itself if it's on the slide, or else the nearest point of its edge.
    fn nearest_slide_point(&self, position: Point<Pixels>, cx: &App) -> Option<(f32, f32)> {
        let slide = self.canvas.read(cx).slide_bounds()?;
        let unit = f32::from(slide.size.width) / SLIDE_WIDTH;
        if unit <= 0.0 {
            return None;
        }
        let along = |at: Pixels, start: Pixels, length: Pixels| {
            f32::from(at - start).clamp(0.0, f32::from(length)) / unit
        };
        Some((
            along(position.x, slide.origin.x, slide.size.width),
            along(position.y, slide.origin.y, slide.size.height),
        ))
    }

    /// What's at (`x`, `y`) of the current slide.
    pub(super) fn hit(&self, (x, y): (f32, f32), cx: &mut Context<Self>) -> Option<(usize, Hit)> {
        let slide = self.document.as_ref()?.read(cx).current();
        let layout = self.layout(slide, cx)?;
        Some((slide, layout.hit(x, y)?))
    }

    /// Whether typing is going on in flow `flow` of slide `slide`.
    pub(super) fn editing_flow(&self, slide: usize, flow: usize, cx: &App) -> bool {
        self.document.as_ref().is_some_and(|document| {
            let document = document.read(cx);
            document
                .text()
                .is_some_and(|editing| editing.slide == slide)
                && document.text_flow() == Some(flow)
        })
    }

    pub(super) fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // A colour held for the selection goes to it before the click moves it.
        self.give_held_color(cx);
        self.dragging = false;
        let Some(point) = self.slide_point(event.position, cx) else {
            // The desk around the slide.
            self.stop_typing(window, cx);
            return;
        };
        if self.new_box_here(cx).is_some_and(|(x, y, w, h)| x <= point.0 && point.0 <= x + w && y <= point.1 && point.1 <= y + h) {
            // In the new box being typed in: typing goes on there.
            cx.stop_propagation();
            self.focus_handle.focus(window, cx);
            self.typed(cx);
            return;
        }
        let Some((slide, mut hit)) = self.hit(point, cx) else {
            self.stop_typing(window, cx);
            return;
        };
        let elsewhere = match &hit {
            Hit::Text { flow, .. } => !self.editing_flow(slide, *flow, cx),
            _ => true,
        };
        if elsewhere && self.is_editing(cx) {
            // What was typed there is left tidy first, which can move the text clicked on.
            self.stop(cx);
            let changed = self.run(Document::end_text, cx);
            if changed.is_some_and(|changed| !changed.is_empty()) {
                match self.hit(point, cx) {
                    Some((_, again)) => hit = again,
                    None => return,
                }
            }
        }
        match hit {
            Hit::Text { flow, pos } => {
                cx.stop_propagation();
                let starts = !self.editing_flow(slide, flow, cx);
                // The first click on a box's text picks the box; the next types in it.
                let pick = self
                    .box_of_text(slide, flow, pos.para, point, cx)
                    .filter(|source| starts && event.click_count == 1 && !self.is_selected(slide, source, cx));
                if let Some(source) = pick {
                    self.select_thing(slide, source, window, cx);
                    return;
                }
                self.wanted_x = None;
                if starts {
                    self.run(
                        |document| {
                            document.begin_text(slide, flow, pos);
                            Ok(Vec::new())
                        },
                        cx,
                    );
                }
                let extend = event.modifiers.shift && !elsewhere;
                // A double click that starts the typing puts the caret where it was clicked.
                let count = if starts { 1 } else { event.click_count };
                match count {
                    2 => self.apply(|edit| edit.select_word_at(pos), cx),
                    count if count >= 3 => self.apply(
                        |edit| {
                            edit.move_to(
                                Pos {
                                    para: pos.para,
                                    offset: 0,
                                },
                                false,
                            )
                            .move_by(Motion::ParaEnd, true)
                        },
                        cx,
                    ),
                    _ => {
                        self.apply(|edit| edit.move_to(pos, extend), cx);
                        self.dragging = true;
                    }
                }
                self.focus_handle.focus(window, cx);
            }
            Hit::Element(element) => {
                cx.stop_propagation();
                let reason = self.cell_reason(slide, &element.source, point, cx);
                self.select_thing(slide, element.source, window, cx);
                if let Some(reason) = reason {
                    cx.emit(EditorEvent::Said(reason.into()));
                }
            }
            Hit::Placeholder(region) => {
                cx.stop_propagation();
                self.wanted_x = None;
                self.deselect(cx);
                self.run(
                    |document| {
                        document.begin_text_in_region(slide, &region);
                        Ok(Vec::new())
                    },
                    cx,
                );
                self.focus_handle.focus(window, cx);
                self.typed(cx);
            }
            Hit::Uneditable(reason) => {
                self.stop_typing(window, cx);
                cx.emit(EditorEvent::Said(reason.into()));
            }
            Hit::Nothing => self.stop_typing(window, cx),
        }
        self.tell_selection(cx);
    }

    pub(super) fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.dragging || event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        // Off the slide, or away from the text, the selection goes on to the nearest place.
        let Some(point) = self.nearest_slide_point(event.position, cx) else {
            return;
        };
        let Some((_, Some(flow), layout)) = self.editing_place(cx) else {
            return;
        };
        let in_flow = |point: (f32, f32)| match layout.hit(point.0, point.1) {
            Some(Hit::Text { flow: hit, pos }) if hit == flow => Some(pos),
            _ => None,
        };
        let pos = in_flow(point).or_else(|| in_flow(nearest_in_flow(&layout, flow, point)?));
        if let Some(pos) = pos {
            self.apply(|edit| edit.move_to(pos, true), cx);
        }
    }
}

/// The point of flow `flow`'s paragraphs, taken together, nearest `point` of the slide, in
/// slide units: where a drag that leaves the text keeps selecting.
fn nearest_in_flow(layout: &TextLayout, flow: usize, point: (f32, f32)) -> Option<(f32, f32)> {
    let (left, top, right, bottom) = layout
        .paras()
        .iter()
        .filter(|placed| placed.flow == flow)
        .map(|placed| placed.rect)
        .fold(None, |area: Option<(f32, f32, f32, f32)>, rect| {
            let (l, t, r, b) = (rect.x, rect.y, rect.x + rect.w, rect.y + rect.h);
            Some(area.map_or((l, t, r, b), |(al, at, ar, ab)| {
                (al.min(l), at.min(t), ar.max(r), ab.max(b))
            }))
        })?;
    // The paragraphs lie in their layer, which may be turned.
    let caret = layout.caret(flow, Pos { para: 0, offset: 0 })?;
    let (x, y) = turned(point, caret.centre, -caret.rotate);
    // Just inside, as a point on the very edge can be taken for the next line.
    let inside = |at: f32, low: f32, high: f32| at.clamp(low, (high - 0.5).max(low));
    Some(turned(
        (inside(x, left, right), inside(y, top, bottom)),
        caret.centre,
        caret.rotate,
    ))
}
