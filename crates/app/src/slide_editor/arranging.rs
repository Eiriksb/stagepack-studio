//! Moving the things selected on the slide shown: dragging them with the mouse, and the arrow
//! keys; and resizing and turning one by its handles. While they're dragged the slide shows them
//! where the pointer has them, and nothing is written until the button is let go of; the
//! geometry is [`crate::arrange`]'s. A thing moved or resized snaps to the slide and to the
//! things around it, as [`crate::snap`] has it, unless Alt is held; the arrow keys never snap.
//! One thing written in the slide's text is dragged out of it as a dashed outline of its size,
//! and given a place of its own where it's let go of.

use super::*;
use crate::arrange::{self, Handle, MOVE_STARTS, Placed};
use crate::document::{DRAG_OUT_FIRST, EditRun, MOVE, NUDGE, RESIZE, TURN};
use crate::history::RunId;
use crate::snap::{self, Guide};

/// Moves the things selected by `dx` across and `dy` down, in slide units.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = slide_editor, no_json)]
pub struct Nudge {
    pub dx: f32,
    pub dy: f32,
}

/// Where a drag has what it drags, each thing's place, and the lines it snapped to there.
type Dragged = (Vec<(Thing, Placed)>, Vec<Guide>);

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
    /// What's dragged, each where it was when pressed: the things selected, the one thing
    /// selected in the slide's text where it's drawn there, or nothing if any of several can't be
    /// moved where it's written.
    things: Vec<(Thing, Placed)>,
    /// Whether what's dragged is one thing written in the slide's text, which is shown as a
    /// dashed outline where the pointer has it and is given a place of its own once let go of.
    lifting: bool,
    /// Why nothing is dragged, said once the drag begins.
    refused: Option<EditRefused>,
    /// Where the outline of the thing lifted out of the text is shown, once the pointer moves.
    outline: Option<Placed>,
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
    /// The boxes on the slide that what's dragged snaps to, in slide units: every thing with a
    /// place of its own and every area with something in it, as they covered the slide when
    /// pressed, but what's dragged.
    others: Vec<Rect>,
    /// The lines what's dragged snapped to where the pointer last had it.
    guides: Vec<Guide>,
}

/// The handles on the one thing selected on the slide shown: what it is, where it is, whether
/// it keeps its shape, and where each handle is drawn, in slide units, `unit` window pixels to
/// a slide unit.
#[derive(Clone)]
pub(super) struct Handles {
    thing: Thing,
    placed: Placed,
    keeps_shape: bool,
    pub(super) spots: [(Handle, (f32, f32)); 9],
    unit: f32,
}

/// The handles last worked out, and what they were worked out for.
pub(super) struct HandlesSeen {
    layout: Arc<TextLayout>,
    pick: u64,
    editing: bool,
    unit: f32,
    handles: Option<Handles>,
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

    /// Whether `(x, y)` of the slide is on `handle` as it's drawn, not only near it.
    pub(super) fn right_on(&self, handle: Handle, (x, y): (f32, f32)) -> bool {
        self.spots.iter().any(|&(spot, (hx, hy))| {
            spot == handle && (hx - x).abs().max((hy - y).abs()) * self.unit <= HANDLE_SIZE / 2.
        })
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
        let (things, lifting, refused, version, pick) = document.update(cx, |document, _| {
            // One thing written in the text is lifted out of it; with others, nothing is moved.
            // What a box holds alone moves with the box.
            let (things, left_out) = document.movable_selected();
            let (things, lifting, refused) = if !left_out {
                (things, false, None)
            } else {
                match document.liftable_selected() {
                    Some(Ok(one)) => (vec![one], true, None),
                    Some(Err(why)) => (Vec::new(), false, Some(why)),
                    None => (Vec::new(), false, Some(EditRefused::InTheText)),
                }
            };
            (things, lifting, refused, document.text_version(), document.selection_pick())
        });
        let grip = Grip::Move;
        let (held_at, keeps_shape) = (pressed_at, false);
        let others = self.snap_targets(selection.slide, &things, cx);
        let guides = Vec::new();
        self.drag = Some(Drag {
            slide: selection.slide,
            version,
            pick,
            things,
            lifting,
            refused,
            outline: None,
            from,
            pressed_at,
            begun: false,
            grip,
            held_at,
            keeps_shape,
            others,
            guides,
        });
    }

    /// The boxes on slide `slide` that `things` dragged snap to: the box each thing with a place
    /// of its own covers, and each area with something in it, but those dragged. A thing in the
    /// slide's text, as a picture in an area, isn't one: it goes with what it's in.
    fn snap_targets(&self, slide: usize, things: &[(Thing, Placed)], cx: &mut Context<Self>) -> Vec<Rect> {
        let Some(layout) = self.layout(slide, cx).filter(|_| !things.is_empty()) else {
            return Vec::new();
        };
        let dragged = |thing: Thing| things.iter().any(|(dragged, _)| *dragged == thing);
        let elements = layout
            .elements()
            .iter()
            .filter(|element| element.placed && !dragged(Thing::Block(element.source.clone())))
            .filter_map(|element| arrange::holding([Placed { rect: element.rect, rotate: element.rotate }]));
        let areas = layout
            .regions()
            .iter()
            .filter(|region| !region.empty && !dragged(Thing::Region(region.name.clone())))
            .filter_map(|region| arrange::holding([Placed { rect: region.rect, rotate: region.rotate }]));
        elements.chain(areas).collect()
    }

    /// The lines what's dragged snaps to where the pointer has it, while the button is down.
    pub(super) fn guides(&self) -> &[Guide] {
        self.drag.as_ref().map_or(&[], |drag| &drag.guides)
    }

    /// Where the outline of a thing being dragged out of the slide's text is shown, if one is.
    pub(super) fn lift_outline(&self) -> Option<Placed> {
        self.drag.as_ref().filter(|drag| drag.begun).and_then(|drag| drag.outline)
    }

    /// The handles on what's selected on the slide shown, if they show: with one thing or area
    /// selected that has a place of its own, and not typed in. A thing too small to tell its
    /// handles apart has them drawn further apart than its corners. They're worked out again
    /// only once the slide's layout, what's selected, the typing or the slide's scale changes,
    /// and then the pointer no longer shows what a handle under it did.
    pub(super) fn handles(&mut self, cx: &mut Context<Self>) -> Option<Handles> {
        let selection = self.selection_here(cx).filter(|selection| selection.things.len() == 1)?;
        let unit = f32::from(self.canvas.read(cx).slide_bounds()?.size.width) / SLIDE_WIDTH;
        if unit <= 0.0 {
            return None;
        }
        let layout = self.layout(selection.slide, cx)?;
        let (pick, editing) = (self.document.as_ref()?.read(cx).selection_pick(), self.is_editing(cx));
        let seen = self
            .handles_seen
            .as_ref()
            .filter(|seen| Arc::ptr_eq(&seen.layout, &layout) && seen.pick == pick && seen.editing == editing && seen.unit == unit);
        if let Some(seen) = seen {
            return seen.handles.clone();
        }
        let handles = (!editing).then(|| self.work_out_handles(&layout, unit, cx)).flatten();
        if !self.handle_drag() {
            self.pointer = None;
        }
        self.handles_seen = Some(HandlesSeen { layout, pick, editing, unit, handles: handles.clone() });
        handles
    }

    /// The handles on the one thing selected, on a slide laid out as `layout`, `unit` window
    /// pixels to a slide unit.
    fn work_out_handles(&self, layout: &TextLayout, unit: f32, cx: &mut Context<Self>) -> Option<Handles> {
        let document = self.document.clone()?;
        let (things, left_out) = document.update(cx, |document, _| document.movable_selected());
        // What a box holds, selected alone, has no handles: the box's would show on another.
        let selection = self.selection_here(cx)?;
        let (thing, placed) = things.into_iter().next().filter(|(thing, _)| !left_out && selection.things.contains(thing))?;
        let keeps_shape = match &thing {
            Thing::Block(source) => layout.element(source)?.keeps_shape,
            Thing::Region(_) => false,
        };
        let shown = spread(&placed, HANDLES_APART / unit);
        let spots = Handle::ALL.map(|handle| (handle, arrange::handle_point(&shown, handle, TURN_GAP / unit)));
        Some(Handles { thing, placed, keeps_shape, spots, unit })
    }

    /// The handles showing and the one at `position` in the window, if the pointer is on one.
    pub(super) fn handle_under(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) -> Option<(Handles, Handle)> {
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
        let things = vec![(handles.thing, handles.placed)];
        let others = self.snap_targets(selection.slide, &things, cx);
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
            things,
            lifting: false,
            refused: None,
            outline: None,
            from,
            pressed_at,
            begun: false,
            grip: Grip::Handle(handle),
            held_at,
            keeps_shape: handles.keeps_shape,
            others,
            guides: Vec::new(),
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
                let why = drag.refused.map_or_else(|| DRAG_OUT_FIRST.to_string(), |why| why.to_string());
                cx.emit(EditorEvent::Said(why.into()));
                return;
            }
        }
        let Some((places, guides)) = self.dragged_to(event.position, event.modifiers, cx) else {
            // Nowhere to show it: no line is met either, nor an outline shown.
            if let Some(drag) = self.drag.as_mut().filter(|drag| !drag.guides.is_empty() || drag.outline.is_some()) {
                drag.guides.clear();
                drag.outline = None;
                cx.notify();
            }
            return;
        };
        let Some(drag) = &mut self.drag else {
            return;
        };
        drag.guides = guides;
        if drag.lifting {
            // The engine can't show a thing in the text elsewhere: its outline shows instead.
            drag.outline = places.first().map(|(_, placed)| *placed);
            cx.notify();
            return;
        }
        if let Some(document) = &self.document {
            document.update(cx, |document, _| document.preview_places(&places));
        }
        self.show_layers(cx);
        cx.notify();
    }

    /// Where what's dragged goes with the pointer at `position`, held with `modifiers`, and the
    /// lines it snaps to there: within [`SNAP_PIXELS`] of the window, unless Alt is held.
    fn dragged_to(&self, position: Point<Pixels>, modifiers: Modifiers, cx: &App) -> Option<Dragged> {
        let drag = self.drag.as_ref().filter(|drag| !drag.things.is_empty())?;
        let pointer = self.slide_units(position, cx)?;
        let unit = self.canvas.read(cx).slide_bounds().map(|slide| f32::from(slide.size.width) / SLIDE_WIDTH);
        let within = unit.filter(|unit| *unit > 0. && !modifiers.alt).map(|unit| SNAP_PIXELS / unit);
        Some(match drag.grip {
            Grip::Move => moved_to(drag, pointer, modifiers, within),
            Grip::Handle(Handle::Turn) => (turned_to(drag, pointer, modifiers), Vec::new()),
            Grip::Handle(handle) => resized_to(drag, handle, pointer, modifiers, within),
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
        let places = (inside && self.drag_holds(cx)).then(|| self.dragged_to(event.position, event.modifiers, cx)).flatten().map(|(places, _)| places);
        let drag = self.drag.take();
        let lifting = drag.as_ref().is_some_and(|drag| drag.lifting);
        let label = match drag.map(|drag| drag.grip) {
            Some(Grip::Handle(Handle::Turn)) => TURN,
            Some(Grip::Handle(_)) => RESIZE,
            _ => MOVE,
        };
        self.pressed = None;
        self.pointer = None;
        let changed = match places {
            Some(places) if lifting => {
                let to = places.first().map(|(_, placed)| *placed);
                to.and_then(|to| self.run(|document| document.lift_selected(to), cx))
            }
            Some(places) => self.run(|document| document.place_things(label, &places, EditRun::New), cx),
            None => None,
        };
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
        let (layers, message, videos, models, terminals) = document.update(cx, |document, _| {
            (document.editor_layers(scale), document.take_message(), document.videos_here(), document.models_here(), document.terminals_here())
        });
        if let Some(message) = message {
            cx.emit(EditorEvent::Said(message.into()));
        }
        self.canvas.update(cx, |canvas, cx| {
            canvas.set_layers(layers, cx);
            canvas.place_videos(&videos, cx);
            canvas.place_models(&models, cx);
            canvas.place_recordings(&terminals, cx);
        });
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

/// Where a move takes what's dragged with the pointer at `pointer` of the slide, and the lines
/// it snaps to there: as far as the pointer has gone since the press, kept to one direction with
/// Shift and kept on the slide; then the box that holds all of it, where that has it, snapped,
/// within `within` slide units, if given, in the directions it can still go; and kept on the
/// slide again, which a snap never undoes. A guide shows each line met, the way it snapped, where
/// it's shown. The slide shows it where the snap has it; it's written to whole units, as any
/// move is, so a thing of an odd size centred is written half a unit off.
fn moved_to(drag: &Drag, pointer: (f32, f32), modifiers: Modifiers, within: Option<f32>) -> Dragged {
    let pointed = (pointer.0 - drag.pressed_at.0, pointer.1 - drag.pressed_at.1);
    // Which ways it can go: kept straight, only the way the pointer goes further.
    let (across, down) = match modifiers.shift {
        true => (pointed.0.abs() >= pointed.1.abs(), pointed.0.abs() < pointed.1.abs()),
        false => (true, true),
    };
    let by = if modifiers.shift { arrange::straight(pointed) } else { pointed };
    let starts: Vec<Placed> = drag.things.iter().map(|(_, placed)| *placed).collect();
    let moved_by = |by: (f32, f32)| starts.iter().map(move |placed| arrange::moved(placed, by));
    let mut kept = arrange::kept_move(&starts, by);
    // Which ways it snapped to a line it's still on.
    let mut snapped = (false, false);
    if let Some((within, held)) = within.zip(arrange::holding(moved_by(kept))) {
        let snap = snap::snap_move(held, &drag.others, within);
        let met = |vertical: bool| snap.guides.iter().any(|guide| guide.vertical == vertical);
        let to = (kept.0 + if across { snap.dx } else { 0. }, kept.1 + if down { snap.dy } else { 0. });
        kept = arrange::kept_move(&starts, to);
        snapped = (across && met(true) && (kept.0 - to.0).abs() <= 0.001, down && met(false) && (kept.1 - to.1).abs() <= 0.001);
    }
    let guides = match (snapped, arrange::holding(moved_by(kept))) {
        ((false, false), _) | (_, None) => Vec::new(),
        ((across, down), Some(shown)) => {
            let mut guides = snap::guides_on(shown, &drag.others);
            guides.retain(|guide| if guide.vertical { across } else { down });
            guides
        }
    };
    (drag.things.iter().map(|(thing, placed)| (thing.clone(), arrange::moved(placed, kept))).collect(), guides)
}

/// Where `handle` dragged to `pointer` of the slide takes what's dragged, and the lines it snaps
/// to there: a corner keeps the shape of a thing that keeps its shape, and Shift does the
/// opposite. The sides the handle moves snap, within `within` slide units, if given, for a thing
/// that isn't turned; where a corner keeps the shape, only the side that goes the furthest
/// snaps, and the other follows it. Then the least and most it can be, and keeping it on the
/// slide, which a snap never undoes. It's shown as it'll be written, its sides and place to
/// whole units.
fn resized_to(drag: &Drag, handle: Handle, pointer: (f32, f32), modifiers: Modifiers, within: Option<f32>) -> Dragged {
    let mut to = held_to(drag, pointer);
    let keep_shape = drag.keeps_shape != modifiers.shift;
    let mut guides = Vec::new();
    let snapping = match drag.things.as_slice() {
        [(_, start)] if start.rotate == 0. => within.map(|within| (start, within)),
        _ => None,
    };
    // Which way the handle is, across and down, from the corner or side across from it.
    let mut ways = None;
    if let Some((start, within)) = snapping {
        // The corner or side across from the handle, which stays.
        let held = arrange::handle_point(start, handle, 0.);
        let Rect { x, y, w, h } = start.rect;
        let anchor = (2. * x + w - held.0, 2. * y + h - held.1);
        let (sideways, upright) = (!matches!(handle, Handle::Top | Handle::Bottom), !matches!(handle, Handle::Left | Handle::Right));
        let snap_x = sideways.then(|| snap::snap_edge(to.0, true, &drag.others, within)).flatten();
        let snap_y = upright.then(|| snap::snap_edge(to.1, false, &drag.others, within)).flatten();
        let (way_x, way_y) = (if held.0 >= anchor.0 { 1. } else { -1. }, if held.1 >= anchor.1 { 1. } else { -1. });
        if keep_shape && sideways && upright && w > 0. && h > 0. {
            // The size goes as far as the pointer goes furthest, so that way snaps.
            let (far_x, far_y) = ((to.0 - anchor.0) * way_x / w, (to.1 - anchor.1) * way_y / h);
            if far_x >= far_y {
                if let Some((shift, guide)) = snap_x {
                    to.0 += shift;
                    to.1 = anchor.1 + way_y * (to.0 - anchor.0) * way_x / w * h;
                    guides.push(guide);
                }
            } else if let Some((shift, guide)) = snap_y {
                to.1 += shift;
                to.0 = anchor.0 + way_x * (to.1 - anchor.1) * way_y / h * w;
                guides.push(guide);
            }
        } else {
            for (shift, guide) in snap_x.into_iter().chain(snap_y) {
                if guide.vertical {
                    to.0 += shift;
                } else {
                    to.1 += shift;
                }
                guides.push(guide);
            }
        }
        ways = Some((way_x, way_y));
    }
    let places: Vec<(Thing, Placed)> = drag
        .things
        .iter()
        .map(|(thing, placed)| (thing.clone(), arrange::whole(placed, &arrange::resized(placed, handle, to, keep_shape), handle)))
        .collect();
    // A guide stays only where the side it's for is on it as shown, to the whole unit it's
    // written to, and runs the length of the thing too.
    match (ways, places.first()) {
        (Some((way_x, way_y)), Some((_, shown))) => {
            let Rect { x, y, w, h } = shown.rect;
            guides.retain_mut(|guide| {
                let (side, start, length) = match guide.vertical {
                    true => (if way_x > 0. { x + w } else { x }, y, h),
                    false => (if way_y > 0. { y + h } else { y }, x, w),
                };
                (guide.from, guide.to) = (guide.from.min(start), guide.to.max(start + length));
                (side - guide.at).abs() <= 0.51
            });
        }
        _ => guides.clear(),
    }
    (places, guides)
}

/// Where the turn handle dragged to `pointer` of the slide turns what's dragged: in whole
/// degrees, or with Shift in steps of fifteen.
fn turned_to(drag: &Drag, pointer: (f32, f32), modifiers: Modifiers) -> Vec<(Thing, Placed)> {
    let to = held_to(drag, pointer);
    drag.things.iter().map(|(thing, placed)| (thing.clone(), arrange::turned_to(placed, to, modifiers.shift))).collect()
}

/// Where the point a handle drag has hold of is with the pointer at `pointer` of the slide: as
/// far from where it was as the pointer is from the press.
fn held_to(drag: &Drag, pointer: (f32, f32)) -> (f32, f32) {
    (drag.held_at.0 + pointer.0 - drag.pressed_at.0, drag.held_at.1 + pointer.1 - drag.pressed_at.1)
}
