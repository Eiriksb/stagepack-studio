//! Moving the things selected on the slide shown: dragging them with the mouse, and the arrow
//! keys; and resizing and turning one by its handles. While they're dragged the slide shows them
//! where the pointer has them, and nothing is written until the button is let go of; the
//! geometry is [`crate::arrange`]'s.

use super::*;
use crate::arrange::{self, Handle, MOVE_STARTS, Placed};
use crate::document::{DRAG_OUT_FIRST, EditRun, MOVE, NUDGE, RESIZE, TURN};
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

/// What a drag does to what it drags: moves it, or resizes or turns it by a handle.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Grip {
    Move,
    Handle(Handle),
}

/// A press on things selected, or on a handle of one, which becomes a drag of them once the
/// pointer has gone far enough with the button down.
pub(super) struct Drag {
    /// The slide pressed on, the talk's text as it was then, and the pick that selected what's
    /// dragged: once any of them changes, as by an undo, another slide, a reload or Tab, the
    /// drag is over.
    slide: usize,
    version: u64,
    pick: u64,
    /// What's dragged, each where it was when pressed: the things selected, or nothing if any
    /// of them can't be moved where it's written.
    things: Vec<(Thing, Placed)>,
    /// Where the press was, in the window, and on the slide, in slide units.
    from: Point<Pixels>,
    pressed_at: (f32, f32),
    /// Whether the pointer has gone far enough for the press to be a drag.
    begun: bool,
    /// What the drag does.
    grip: Grip,
    /// For a handle: the point on the slide the pointer has hold of, which goes as far as the
    /// pointer does, and whether the thing keeps its proportions as a corner is dragged.
    held_at: (f32, f32),
    keeps_shape: bool,
}

/// The handles on the one thing selected on the slide shown: what it is, where it is, whether
/// it keeps its shape, and where each handle is drawn, in slide units, `unit` window pixels to
/// a slide unit.
pub(super) struct Handles {
    thing: Thing,
    placed: Placed,
    keeps_shape: bool,
    pub(super) spots: [(Handle, (f32, f32)); 9],
    unit: f32,
}

impl Handles {
    /// The handle within [`HANDLE_REACH`] window pixels of `point` of the slide, the nearest if
    /// several are.
    fn under(&self, (x, y): (f32, f32)) -> Option<Handle> {
        self.spots
            .iter()
            .map(|&(handle, (hx, hy))| (handle, ((hx - x).powi(2) + (hy - y).powi(2)).sqrt() * self.unit))
            .filter(|&(_, away)| away <= HANDLE_REACH)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(handle, _)| handle)
    }

    /// How far the thing is turned, in degrees.
    pub(super) fn rotate(&self) -> f32 {
        self.placed.rotate
    }
}

/// What the pointer shows over `handle` of a thing turned by `rotate` degrees: which way it
/// resizes, or an open hand to turn it, closed while it's turning.
fn handle_cursor(handle: Handle, rotate: f32, turning: bool) -> CursorStyle {
    let (fx, fy) = match handle {
        Handle::Turn if turning => return CursorStyle::ClosedHand,
        Handle::Turn => return CursorStyle::OpenHand,
        Handle::TopLeft => (-1., -1.),
        Handle::Top => (0., -1.),
        Handle::TopRight => (1., -1.),
        Handle::Right => (1., 0.),
        Handle::BottomRight => (1., 1.),
        Handle::Bottom => (0., 1.),
        Handle::BottomLeft => (-1., 1.),
        Handle::Left => (-1., 0.),
    };
    // Which way the handle goes from the middle, on the slide, a half turn being the same way.
    let degrees = f32::atan2(fy, fx).to_degrees() + rotate;
    match ((degrees + 22.5).rem_euclid(180.) / 45.) as u32 {
        0 => CursorStyle::ResizeLeftRight,
        1 => CursorStyle::ResizeUpLeftDownRight,
        2 => CursorStyle::ResizeUpDown,
        _ => CursorStyle::ResizeUpRightDownLeft,
    }
}

/// `placed` grown about its middle, where it has to be, to at least `least` units each way.
fn spread(placed: &Placed, least: f32) -> Placed {
    let Rect { x, y, w, h } = placed.rect;
    let (wide, tall) = (w.max(least), h.max(least));
    Placed { rect: Rect { x: x - (wide - w) / 2., y: y - (tall - h) / 2., w: wide, h: tall }, rotate: placed.rotate }
}

/// The run of nudges going on: its run in the history, what it moved, as selected after the
/// last nudge, and the wait after that nudge, once over which the next is a step of its own.
pub(super) struct Nudging {
    run: RunId,
    moved: selection::Selection,
    _pause: Task<()>,
}

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
        let Some(pressed_at) = self.slide_units(from, cx) else {
            return;
        };
        let (things, version, pick) = document.update(cx, |document, _| {
            // A thing written in the text can't be moved yet, and nothing is moved without it.
            let (things, left_out) = document.movable_selected();
            (if left_out { Vec::new() } else { things }, document.text_version(), document.selection_pick())
        });
        let grip = Grip::Move;
        let (held_at, keeps_shape) = (pressed_at, false);
        self.drag = Some(Drag { slide: selection.slide, version, pick, things, from, pressed_at, begun: false, grip, held_at, keeps_shape });
    }

    /// The handles on what's selected on the slide shown, if they show: with one thing or area
    /// selected that has a place of its own, and not typed in. A thing too small to tell its
    /// handles apart has them drawn further apart than its corners.
    pub(super) fn handles(&self, cx: &mut Context<Self>) -> Option<Handles> {
        let selection = self.selection_here(cx).filter(|selection| selection.things.len() == 1)?;
        if self.is_editing(cx) {
            return None;
        }
        let unit = f32::from(self.canvas.read(cx).slide_bounds()?.size.width) / SLIDE_WIDTH;
        if unit <= 0.0 {
            return None;
        }
        let document = self.document.clone()?;
        let (things, left_out) = document.update(cx, |document, _| document.movable_selected());
        let (thing, placed) = things.into_iter().next().filter(|_| !left_out)?;
        let keeps_shape = match &thing {
            Thing::Block(source) => self.layout(selection.slide, cx)?.element(source)?.keeps_shape,
            Thing::Region(_) => false,
        };
        let shown = spread(&placed, HANDLES_APART / unit);
        let spots = Handle::ALL.map(|handle| (handle, arrange::handle_point(&shown, handle, TURN_GAP / unit)));
        Some(Handles { thing, placed, keeps_shape, spots, unit })
    }

    /// The handles showing and the one at `position` in the window, if the pointer is on one.
    pub(super) fn handle_under(&self, position: Point<Pixels>, cx: &mut Context<Self>) -> Option<(Handles, Handle)> {
        let handles = self.handles(cx)?;
        let handle = handles.under(self.slide_units(position, cx)?)?;
        Some((handles, handle))
    }

    /// Readies a resize or a turn of what `handles` are on, from a press at `from` on `handle`.
    pub(super) fn ready_handle_drag(&mut self, handles: Handles, handle: Handle, from: Point<Pixels>, cx: &mut Context<Self>) {
        self.drag = None;
        self.pressed = None;
        let (Some(selection), Some(document), Some(pressed_at)) = (self.selection_here(cx), self.document.clone(), self.slide_units(from, cx)) else {
            return;
        };
        let (version, pick) = document.read_with(cx, |document, _| (document.text_version(), document.selection_pick()));
        // The pointer has hold of the corner or side itself, and the turn handle where it's drawn.
        let held_at = match handle {
            Handle::Turn => handles.spots.iter().find(|(spot, _)| *spot == Handle::Turn).map_or(pressed_at, |(_, at)| *at),
            _ => arrange::handle_point(&handles.placed, handle, 0.),
        };
        self.pointer = Some(handle_cursor(handle, handles.placed.rotate, true));
        self.drag = Some(Drag {
            slide: selection.slide,
            version,
            pick,
            things: vec![(handles.thing, handles.placed)],
            from,
            pressed_at,
            begun: false,
            grip: Grip::Handle(handle),
            held_at,
            keeps_shape: handles.keeps_shape,
        });
    }

    /// Whether a drag by a handle is readied or going on.
    pub(super) fn handle_drag(&self) -> bool {
        self.drag.as_ref().is_some_and(|drag| matches!(drag.grip, Grip::Handle(_)))
    }

    /// The pointer moved with no button down: it shows what the handle under it would do.
    pub(super) fn hover(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let shown = self.handle_under(position, cx).map(|(handles, handle)| handle_cursor(handle, handles.placed.rotate, false));
        if shown != self.pointer {
            self.pointer = shown;
            cx.notify();
        }
    }

    /// Whether the talk is as it was when the drag was readied, on the same slide, with the same
    /// things selected.
    fn drag_holds(&self, cx: &App) -> bool {
        let (Some(drag), Some(document)) = (&self.drag, &self.document) else {
            return false;
        };
        let document = document.read(cx);
        document.text_version() == drag.version && document.current() == drag.slide && document.selection_pick() == drag.pick
    }

    /// Ends a drag readied but not begun, as a key does: a press whose release was never heard
    /// doesn't hold the keys back.
    pub(super) fn end_unbegun_drag(&mut self, cx: &mut Context<Self>) {
        if self.drag.as_ref().is_some_and(|drag| !drag.begun) {
            self.end_drag(cx);
        }
    }

    /// Ends a drag readied or going on if the talk or what's selected changed under it, by
    /// anything but the drag itself.
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
        let Some(places) = self.dragged_to(event.position, event.modifiers, cx) else {
            return;
        };
        if let Some(document) = &self.document {
            document.update(cx, |document, _| document.preview_places(&places));
        }
        self.show_layers(cx);
        cx.notify();
    }

    /// Where what's dragged goes with the pointer at `position`, held with `modifiers`.
    fn dragged_to(&self, position: Point<Pixels>, modifiers: Modifiers, cx: &App) -> Option<Vec<(Thing, Placed)>> {
        let drag = self.drag.as_ref().filter(|drag| !drag.things.is_empty())?;
        let pointer = self.slide_units(position, cx)?;
        Some(match drag.grip {
            Grip::Move => moved_to(drag, pointer, modifiers),
            Grip::Handle(Handle::Turn) => turned_to(drag, pointer, modifiers),
            Grip::Handle(handle) => resized_to(drag, handle, pointer, modifiers),
        })
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
        let places = (inside && self.drag_holds(cx)).then(|| self.dragged_to(event.position, event.modifiers, cx)).flatten();
        let label = match self.drag.take().map(|drag| drag.grip) {
            Some(Grip::Handle(Handle::Turn)) => TURN,
            Some(Grip::Handle(_)) => RESIZE,
            _ => MOVE,
        };
        self.pressed = None;
        self.pointer = None;
        let changed = places.and_then(|places| self.run(|document| document.place_things(label, &places, EditRun::New), cx));
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
        if matches!(drag.grip, Grip::Handle(_)) {
            self.pointer = None;
        }
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
        // A drag going on keeps the keys back; one readied whose release was never heard doesn't.
        self.end_unbegun_drag(cx);
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
        // The run goes on only for the same things: once others are selected, it's a new step.
        let selected = self.selection_here(cx);
        let run = match &self.nudging {
            Some(nudging) if selected.as_ref() == Some(&nudging.moved) => EditRun::Continue(nudging.run),
            _ => EditRun::New,
        };
        let changed = self.run(|document| document.place_things(NUDGE, &places, run), cx);
        if changed.is_some_and(|changed| !changed.is_empty()) {
            let pause = cx.spawn(async move |this, cx| {
                cx.background_executor().timer(NUDGE_PAUSE).await;
                this.update(cx, |this, _| this.nudging = None).ok();
            });
            let run = document.read(cx).open_run();
            self.nudging = run.zip(self.selection_here(cx)).map(|(run, moved)| Nudging { run, moved, _pause: pause });
        }
    }
}

/// Where a move takes what's dragged with the pointer at `pointer` of the slide: as far as the
/// pointer has gone since the press, kept to one direction with Shift, and on the slide.
fn moved_to(drag: &Drag, pointer: (f32, f32), modifiers: Modifiers) -> Vec<(Thing, Placed)> {
    let by = (pointer.0 - drag.pressed_at.0, pointer.1 - drag.pressed_at.1);
    let by = if modifiers.shift { arrange::straight(by) } else { by };
    let starts: Vec<Placed> = drag.things.iter().map(|(_, placed)| *placed).collect();
    let by = arrange::kept_move(&starts, by);
    drag.things.iter().map(|(thing, placed)| (thing.clone(), arrange::moved(placed, by))).collect()
}

/// Where `handle` dragged to `pointer` of the slide takes what's dragged: a corner keeps the
/// shape of a thing that keeps its shape, and Shift does the opposite.
fn resized_to(drag: &Drag, handle: Handle, pointer: (f32, f32), modifiers: Modifiers) -> Vec<(Thing, Placed)> {
    let to = (drag.held_at.0 + pointer.0 - drag.pressed_at.0, drag.held_at.1 + pointer.1 - drag.pressed_at.1);
    let keep_shape = drag.keeps_shape != modifiers.shift;
    drag.things.iter().map(|(thing, placed)| (thing.clone(), arrange::resized(placed, handle, to, keep_shape))).collect()
}

/// Where the turn handle dragged to `pointer` of the slide turns what's dragged: in whole
/// degrees, or with Shift in steps of fifteen.
fn turned_to(drag: &Drag, pointer: (f32, f32), modifiers: Modifiers) -> Vec<(Thing, Placed)> {
    let to = (drag.held_at.0 + pointer.0 - drag.pressed_at.0, drag.held_at.1 + pointer.1 - drag.pressed_at.1);
    drag.things.iter().map(|(thing, placed)| (thing.clone(), arrange::turned_to(placed, to, modifiers.shift))).collect()
}
