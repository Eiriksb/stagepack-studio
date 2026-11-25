//! The mouse on the slide: where a press lands, and what it picks, selects or types in. A press
//! on a thing selects it, and what the press goes on to do comes after.

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

    /// Where `position`, in the window, is in slide units from the slide's corner, on the slide
    /// or off it.
    pub(super) fn slide_units(&self, position: Point<Pixels>, cx: &App) -> Option<(f32, f32)> {
        let slide = self.canvas.read(cx).slide_bounds()?;
        let unit = f32::from(slide.size.width) / SLIDE_WIDTH;
        (unit > 0.0).then(|| (f32::from(position.x - slide.origin.x) / unit, f32::from(position.y - slide.origin.y) / unit))
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
        // On a model being turned, a press turns it, and moves nothing.
        if self.press_turning(event.position, cx) {
            cx.stop_propagation();
            self.focus_handle.focus(window, cx);
            return;
        }
        // On a thing being tried, a press is that thing's, and selects nothing.
        if self.press_trying(event.position, cx) {
            cx.stop_propagation();
            self.focus_handle.focus(window, cx);
            return;
        }
        // A colour held for the selection goes to it before the click moves it.
        self.give_held_color(cx);
        self.end_drag(cx);
        self.dragging = false;
        self.pressed = None;
        // A handle of the thing selected, on the slide or just off it, before what's under it;
        // but with Ctrl the press adds or takes away what's under it, and so with Shift, unless
        // it's right on the handle, which Shift then resizes or turns as it does. A second click
        // is the handle's only right on it, so a double click by an edge still types.
        let at = self.slide_units(event.position, cx);
        let grabbed = (!event.modifiers.control)
            .then(|| self.handle_under(event.position, cx))
            .flatten()
            .filter(|(handles, handle)| {
                let right_on = at.is_some_and(|at| handles.right_on(*handle, at));
                right_on || (event.click_count <= 1 && !event.modifiers.shift)
            });
        if let Some((handles, handle)) = grabbed {
            cx.stop_propagation();
            self.focus_handle.focus(window, cx);
            self.ready_handle_drag(handles, handle, event.position, cx);
            return;
        }
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
        // Shift or Ctrl adds a thing to what's selected, or takes it away.
        let adding = event.modifiers.shift || event.modifiers.control;
        if let Some((slide, region)) = self.frame_under(point, cx) {
            // On the frame of the area being typed in, or of one selected: the area.
            cx.stop_propagation();
            self.press_thing(slide, Thing::Region(region), adding, event.position, window, cx);
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
        if adding {
            let thing = match &hit {
                Hit::Element(element) => Some(self.picked(slide, element.source.clone(), cx)),
                // Not in the text being typed, where Shift extends what's selected of it.
                Hit::Text { flow, pos } if elsewhere => self.box_of_text(slide, *flow, pos.para, point, cx),
                _ => None,
            };
            if let Some(source) = thing {
                cx.stop_propagation();
                self.press_thing(slide, Thing::Block(source), true, event.position, window, cx);
                return;
            }
        }
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
                    self.press_thing(slide, Thing::Block(source), false, event.position, window, cx);
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
                let source = self.picked(slide, element.source.clone(), cx);
                // On what the box selected alone holds: that, once let go of without a drag.
                let inner = (source != element.source && self.is_selected(slide, &source, cx)).then_some(element.source);
                self.press_thing(slide, Thing::Block(source), false, event.position, window, cx);
                if let Some(inner) = inner {
                    self.pressed = Some((slide, Thing::Block(inner)));
                }
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

    /// A right press. On a thing, or on an area's frame, it's selected, unless it's one of the
    /// things selected already, and the slide takes the keyboard, before the thing's menu opens.
    /// Elsewhere, on the empty slide, the desk, or a layout area's text that isn't being typed,
    /// what's selected is let go of, and the menu offers only Paste; so too where picking the
    /// thing left nothing selected. On the text being typed, anywhere but on a thing while text
    /// is typed, or with no talk, the press is left be, and no menu opens.
    pub(super) fn right_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // A colour held for the selection goes to it before the press moves it.
        self.give_held_color(cx);
        self.end_drag(cx);
        self.dragging = false;
        self.pressed = None;
        self.menu_on = None;
        let under = self.slide_point(event.position, cx).and_then(|point| self.thing_under(point, cx));
        match under {
            Some((slide, thing)) => {
                let selected = self.document.as_ref().is_some_and(|document| {
                    document.read(cx).selection().is_some_and(|selection| selection.slide == slide && selection.things.contains(&thing))
                });
                if selected {
                    self.focus_handle.focus(window, cx);
                } else {
                    self.select_one(slide, thing, window, cx);
                }
                // Picking it can leave nothing selected, as when ending the typing takes it away.
                self.menu_on = Some(if self.selection_here(cx).is_some() { MenuOn::Thing } else { MenuOn::Slide });
            }
            None if self.is_editing(cx) || self.document.is_none() => {
                cx.stop_propagation();
                return;
            }
            None => {
                self.deselect(cx);
                self.menu_on = Some(MenuOn::Slide);
            }
        }
        self.tell_selection(cx);
    }

    /// The thing at `point` of the slide shown that a right press picks, with its slide: a thing,
    /// a box by its text, a table by a cell, or an area by its frame; but not the text being
    /// typed, nor the new box waiting for its first letter.
    fn thing_under(&self, point: (f32, f32), cx: &mut Context<Self>) -> Option<(usize, Thing)> {
        if self.new_box_here(cx).is_some_and(|(x, y, w, h)| x <= point.0 && point.0 <= x + w && y <= point.1 && point.1 <= y + h) {
            return None;
        }
        if let Some((slide, region)) = self.frame_under(point, cx) {
            return Some((slide, Thing::Region(region)));
        }
        let (slide, hit) = self.hit(point, cx)?;
        let source = match hit {
            Hit::Element(element) => self.picked(slide, element.source, cx),
            Hit::Text { flow, pos } if !self.editing_flow(slide, flow, cx) => self
                .box_of_text(slide, flow, pos.para, point, cx)
                .or_else(|| Some(self.layout(slide, cx)?.element_at(point.0, point.1)?.source))?,
            _ => return None,
        };
        Some((slide, Thing::Block(source)))
    }

    /// What a press on the thing at `source` of slide `slide` picks: the box with a place of its
    /// own that holds it and nothing else, as a table moved out of the text is held, unless the
    /// thing is selected already; or else the thing.
    fn picked(&self, slide: usize, source: Range<usize>, cx: &App) -> Range<usize> {
        if self.is_selected(slide, &source, cx) {
            return source;
        }
        let document = self.document.as_ref().map(|document| document.read(cx));
        document.and_then(|document| selection::box_holding(document.talk(), slide, &source)).unwrap_or(source)
    }

    /// A press at `from` on `thing` of slide `slide`. With `adding`, it's added to what's
    /// selected, or taken away; on one of several things selected, they stay selected, as a drag
    /// takes them all, and it's selected alone once the button is let go of without a drag;
    /// otherwise it's selected alone at once. Left selected, it's ready to be dragged.
    fn press_thing(&mut self, slide: usize, thing: Thing, adding: bool, from: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let pressed = thing.clone();
        if adding {
            self.select_also(slide, thing, window, cx);
        } else if self.is_among_selected(slide, &thing, cx) {
            self.focus_handle.focus(window, cx);
            self.pressed = Some((slide, thing));
        } else {
            self.select_one(slide, thing, window, cx);
        }
        self.ready_drag(&pressed, from, cx);
    }

    /// The button is let go of, anywhere: a drag ends, or a press on one of several things
    /// selected selects it alone. Another button let go of, as the right one during a drag,
    /// does nothing.
    pub(super) fn mouse_up(&mut self, event: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.button != MouseButton::Left {
            return;
        }
        self.dragging = false;
        if self.release_turning(cx) {
            return;
        }
        if self.release_trying(event.position, cx) {
            return;
        }
        if self.drop_drag(event, window, cx) {
            return;
        }
        if let Some((slide, thing)) = self.pressed.take() {
            self.select_one(slide, thing, window, cx);
        }
    }

    /// The area being typed in, or an area selected on the slide shown, with its slide, if
    /// `point` of the slide is on its frame.
    fn frame_under(&self, point: (f32, f32), cx: &mut Context<Self>) -> Option<(usize, String)> {
        let selected = self.selection_here(cx).into_iter().flat_map(|selection| {
            let slide = selection.slide;
            selection.things.into_iter().filter_map(move |thing| match thing {
                Thing::Region(name) => Some((slide, name)),
                Thing::Block(_) => None,
            })
        });
        let areas: Vec<(usize, String)> = self.typed_region(cx).into_iter().chain(selected).collect();
        areas.into_iter().find(|(slide, region)| self.on_frame(*slide, region, point, cx).is_some())
    }

    /// Something if (`x`, `y`) of slide `slide` is within [`FRAME_REACH`] window pixels of the
    /// frame of area `region`, and on no text: neither its own nor another thing's.
    fn on_frame(&self, slide: usize, region: &str, (x, y): (f32, f32), cx: &mut Context<Self>) -> Option<()> {
        let layout = self.layout(slide, cx)?;
        let area = layout.region(region)?;
        let unit = f32::from(self.canvas.read(cx).slide_bounds()?.size.width) / SLIDE_WIDTH;
        if unit <= 0.0 {
            return None;
        }
        let Rect { x: left, y: top, w, h } = area.rect;
        let (px, py) = turned((x, y), (left + w / 2., top + h / 2.), -area.rotate);
        let (right, bottom) = (left + w, top + h);
        let inside = left <= px && px <= right && top <= py && py <= bottom;
        let away = if inside {
            (px - left).min(right - px).min(py - top).min(bottom - py)
        } else {
            let (dx, dy) = ((left - px).max(px - right).max(0.), (top - py).max(py - bottom).max(0.));
            (dx * dx + dy * dy).sqrt()
        };
        if away * unit > FRAME_REACH {
            return None;
        }
        let other = match layout.hit(x, y) {
            Some(Hit::Text { flow, .. }) => layout.region_of_flow(flow) != Some(region),
            Some(Hit::Element(_)) => true,
            // Inside another area's offer to start text, the press is that area's.
            Some(Hit::Placeholder(other)) => other != region,
            _ => false,
        };
        // On a line of the area's own text, as at the start of a line that touches the frame,
        // the press types.
        let on_text = layout.paras().iter().filter(|placed| layout.region_of_flow(placed.flow) == Some(region)).any(|placed| {
            let Some(caret) = layout.caret(placed.flow, Pos { para: 0, offset: 0 }) else {
                return false;
            };
            let (px, py) = turned((x, y), caret.centre, -caret.rotate);
            let Rect { x: left, y: top, w, h } = placed.rect;
            left <= px && px <= left + w && top <= py && py <= top + h
        });
        (!other && !on_text).then_some(())
    }

    /// The area of the slide shown whose text is being typed, if it's in one.
    pub(super) fn typed_region(&self, cx: &App) -> Option<(usize, String)> {
        let document = self.document.as_ref()?.read(cx);
        let editing = document.text()?;
        (editing.slide == document.current()).then(|| Some((editing.slide, editing.flow.region()?.to_string()))).flatten()
    }

    pub(super) fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.drag_turning(event, cx) {
            return;
        }
        if self.move_trying(event.position, cx) {
            return;
        }
        if self.drag.is_some() {
            self.drag_to(event, cx);
            return;
        }
        if event.pressed_button.is_none() {
            self.hover(event.position, cx);
        }
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
