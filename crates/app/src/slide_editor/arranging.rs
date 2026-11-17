//! Moving the things selected on the slide shown: dragging them with the mouse, and the arrow
//! keys. While they're dragged the slide shows them where the pointer has them, and nothing is
//! written until the button is let go of; the geometry is [`crate::arrange`]'s.

use super::*;
use crate::arrange::{self, DRAG_OUT_FIRST, MOVE_STARTS, Placed};
use crate::document::{EditRun, MOVE, NUDGE};
use crate::history::RunId;

/// Moves the things selected by `dx` across and `dy` down, in slide units.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = slide_editor, no_json)]
pub struct Nudge {
    pub dx: f32,
    pub dy: f32,
}

/// How long the arrow keys can stop for and their moves still be one step to undo.
const NUDGE_PAUSE: Duration = Duration::from_secs(1);

/// A press on things selected, which becomes a drag of them once the pointer has gone far
/// enough with the button down.
pub(super) struct Drag {
    /// The slide pressed on, and the talk's text as it was then: once either changes, as by an
    /// undo, another slide or a reload, the drag is over.
    slide: usize,
    version: u64,
    /// What's dragged, each where it was when pressed: the things selected that can be moved.
    things: Vec<(Thing, Placed)>,
    /// Where the press was, in the window.
    from: Point<Pixels>,
    /// Whether the pointer has gone far enough for the press to be a drag.
    begun: bool,
}

/// The run of nudges going on: its run in the history, and the wait after the last nudge, once
/// over which the next is a step of its own.
pub(super) type Nudging = (RunId, Task<()>);

impl SlideEditor {
    /// Readies a drag of what's selected on the slide shown, from a press at `from` on `thing`,
    /// if the press left that selected.
    pub(super) fn ready_drag(&mut self, thing: &Thing, from: Point<Pixels>, cx: &mut Context<Self>) {
        self.drag = None;
        let Some(selection) = self.selection_here(cx).filter(|selection| selection.things.contains(thing)) else {
            return;
        };
        let Some(document) = self.document.clone() else {
            return;
        };
        let (things, version) = document.update(cx, |document, _| (document.movable_selected().0, document.text_version()));
        self.drag = Some(Drag { slide: selection.slide, version, things, from, begun: false });
    }

    /// Whether the talk is as it was when the drag was readied, on the same slide.
    fn drag_holds(&self, cx: &App) -> bool {
        let (Some(drag), Some(document)) = (&self.drag, &self.document) else {
            return false;
        };
        let document = document.read(cx);
        document.text_version() == drag.version && document.current() == drag.slide
    }

    /// Ends a drag readied or going on if the talk changed under it, by any edit but its own.
    pub(super) fn follow_drag(&mut self, cx: &mut Context<Self>) {
        if self.drag.is_some() && !self.drag_holds(cx) {
            self.end_drag(cx);
        }
    }

    /// The pointer moved with a drag readied: once it's gone more than [`MOVE_STARTS`] from the
    /// press, the slide shows what's dragged where the pointer has it. With Shift, the move
    /// keeps to one direction. With the button up, as once it was let go of where the window
    /// didn't hear, the drag ends and nothing is written.
    pub(super) fn drag_to(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if event.pressed_button != Some(MouseButton::Left) || !self.drag_holds(cx) {
            self.end_drag(cx);
            return;
        }
        let Some(drag) = &mut self.drag else {
            return;
        };
        if !drag.begun {
            let (dx, dy) = (f32::from(event.position.x - drag.from.x), f32::from(event.position.y - drag.from.y));
            if (dx * dx + dy * dy).sqrt() <= MOVE_STARTS {
                return;
            }
            drag.begun = true;
            // A drag of several takes them all, rather than selecting the one pressed alone.
            self.pressed = None;
            if drag.things.is_empty() {
                cx.emit(EditorEvent::Said(DRAG_OUT_FIRST.into()));
                return;
            }
        }
        let Some(places) = self.dragged_to(event.position, event.modifiers.shift, cx) else {
            return;
        };
        if let Some(document) = &self.document {
            document.update(cx, |document, _| document.preview_places(&places));
        }
        self.show_layers(cx);
        cx.notify();
    }

    /// Where what's dragged goes with the pointer at `position`, kept to one direction with
    /// `straight`, and on the slide.
    fn dragged_to(&self, position: Point<Pixels>, straight: bool, cx: &App) -> Option<Vec<(Thing, Placed)>> {
        let drag = self.drag.as_ref().filter(|drag| !drag.things.is_empty())?;
        let unit = f32::from(self.canvas.read(cx).slide_bounds()?.size.width) / SLIDE_WIDTH;
        if unit <= 0.0 {
            return None;
        }
        let by = (f32::from(position.x - drag.from.x) / unit, f32::from(position.y - drag.from.y) / unit);
        let by = if straight { arrange::straight(by) } else { by };
        let starts: Vec<Placed> = drag.things.iter().map(|(_, placed)| *placed).collect();
        let by = arrange::kept_move(&starts, by);
        Some(drag.things.iter().map(|(thing, placed)| (thing.clone(), arrange::moved(placed, by))).collect())
    }

    /// The button let go of with a drag readied: what was dragged is written where it was let
    /// go of, as one step, unless that's outside the window. Says whether a drag ended here,
    /// rather than a click.
    pub(super) fn drop_drag(&mut self, event: &MouseUpEvent, window: &Window, cx: &mut Context<Self>) -> bool {
        let Some(begun) = self.drag.as_ref().map(|drag| drag.begun) else {
            return false;
        };
        if !begun {
            self.drag = None;
            return false;
        }
        let inside = Bounds { origin: Point::default(), size: window.viewport_size() }.contains(&event.position);
        let places = (inside && self.drag_holds(cx)).then(|| self.dragged_to(event.position, event.modifiers.shift, cx)).flatten();
        self.drag = None;
        self.pressed = None;
        let changed = places.and_then(|places| self.run(|document| document.place_things(MOVE, &places, EditRun::New), cx));
        if changed.is_none_or(|changed| changed.is_empty()) {
            // Nothing written, so nothing else draws the slide again as the talk has it.
            self.drop_preview(cx);
        }
        cx.notify();
        true
    }

    /// Ends a drag readied or going on, writing nothing, and shows the slide as the talk has it.
    pub(super) fn end_drag(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        self.pressed = None;
        if drag.begun {
            self.drop_preview(cx);
            cx.notify();
        }
    }

    /// Shows the slide as the talk has it again.
    fn drop_preview(&mut self, cx: &mut Context<Self>) {
        let Some(document) = self.document.clone() else {
            return;
        };
        document.update(cx, |document, _| document.drop_preview());
        self.show_layers(cx);
    }

    /// Draws the slide's layers again as they're shown now, as a drag goes.
    fn show_layers(&mut self, cx: &mut Context<Self>) {
        let (Some(document), Some(scale)) = (self.document.clone(), self.canvas.read(cx).scale()) else {
            return;
        };
        let (layers, message) = document.update(cx, |document, _| (document.editor_layers(scale), document.take_message()));
        if let Some(message) = message {
            cx.emit(EditorEvent::Said(message.into()));
        }
        self.canvas.update(cx, |canvas, cx| canvas.set_layers(layers, cx));
    }

    /// An arrow key with things selected: they move by `by`, in slide units, as far as they stay
    /// on the slide. A run of them, each within [`NUDGE_PAUSE`] of the last, is one step to undo.
    /// A thing written in the slide's text isn't moved, and that's said.
    pub(super) fn nudge(&mut self, by: (f32, f32), cx: &mut Context<Self>) {
        if self.selection_here(cx).is_none() {
            cx.propagate();
            return;
        }
        let Some(document) = self.document.clone() else {
            return;
        };
        if self.drag.is_some() {
            return;
        }
        let (things, left_out) = document.update(cx, |document, _| document.movable_selected());
        if left_out || things.is_empty() {
            cx.emit(EditorEvent::Said(DRAG_OUT_FIRST.into()));
            return;
        }
        let starts: Vec<Placed> = things.iter().map(|(_, placed)| *placed).collect();
        let by = arrange::kept_move(&starts, by);
        let places: Vec<(Thing, Placed)> = things.iter().map(|(thing, placed)| (thing.clone(), arrange::moved(placed, by))).collect();
        let run = match &self.nudging {
            Some((run, _)) => EditRun::Continue(*run),
            None => EditRun::New,
        };
        let changed = self.run(|document| document.place_things(NUDGE, &places, run), cx);
        if changed.is_some_and(|changed| !changed.is_empty()) {
            let pause = cx.spawn(async move |this, cx| {
                cx.background_executor().timer(NUDGE_PAUSE).await;
                this.update(cx, |this, _| this.nudging = None).ok();
            });
            self.nudging = document.read(cx).open_run().map(|run| (run, pause));
        }
    }
}
