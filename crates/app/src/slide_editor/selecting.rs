//! The things selected on the slide shown: picking them, letting go of them, and what's said
//! of them.

use super::*;
use crate::document::LargeOpen;
use crate::model_view::inside_turned;

impl SlideEditor {
    /// What's selected, if it's on the slide shown.
    pub(super) fn selection_here(&self, cx: &App) -> Option<selection::Selection> {
        let document = self.document.as_ref()?.read(cx);
        document
            .selection()
            .filter(|selection| selection.slide == document.current())
            .cloned()
    }

    /// What the status line says of what's selected on the slide shown: of a video that can't be
    /// played, why not, too.
    fn said_of_selection(&self, cx: &mut App) -> Option<SharedString> {
        let selection = self.selection_here(cx)?;
        let document = self.document.clone()?;
        let said = selection::said_of_selection(document.read(cx).talk(), &selection)?;
        let problem = document.update(cx, |document, _| document.selected_video_said().or_else(|| document.selected_model_said()));
        Some(match problem {
            Some(problem) => format!("{said}. {problem}").into(),
            None => said.into(),
        })
    }

    /// Plays the video selected, from its start, or stops it while it plays, as its button and
    /// Space do. With no video selected, the key goes on.
    pub(super) fn play_or_stop(&mut self, cx: &mut Context<Self>) {
        let Some(document) = self.document.clone() else {
            return;
        };
        let Some(video) = document.update(cx, |document, _| document.selected_video()) else {
            cx.propagate();
            return;
        };
        if self.video_playing(cx) {
            self.stop_video(cx);
            return;
        }
        self.stop_video(cx);
        self.stop_turning(cx);
        let Some(source) = video.source.clone() else { return };
        let opened = document.read(cx).open_large(&video.on.src);
        let Some(opened) = opened else {
            let said = document.update(cx, |document, _| document.selected_video_said());
            let said = said.unwrap_or_else(|| slides_video::CantPlay::CouldNotRead.to_string());
            cx.emit(EditorEvent::Said(said.into()));
            return;
        };
        let (slide, talk) = {
            let document = document.read(cx);
            (document.current(), talk_hash(document.source()))
        };
        self.canvas.update(cx, |canvas, cx| canvas.play(video.layer, video.on, opened, cx));
        self.playing = Some(PlayingHere { slide, source, talk });
        cx.notify();
    }

    /// Whether the video selected is playing, or being opened to.
    pub(super) fn video_playing(&self, cx: &App) -> bool {
        self.playing.is_some() && !self.canvas.read(cx).playing_videos().is_empty()
    }

    /// Stops the video played, if one was, showing its still again.
    pub fn stop_video(&mut self, cx: &mut Context<Self>) {
        if self.playing.take().is_some() {
            self.canvas.update(cx, |canvas, cx| canvas.stop_videos(cx));
            cx.notify();
        }
    }

    /// Has the 3D model selected drawn live, to be turned by hand, or goes back to its still
    /// while it's turned, as its buttons and Space do. With no model selected, the key goes on.
    pub(super) fn turn_or_stop(&mut self, cx: &mut Context<Self>) {
        let Some(document) = self.document.clone() else {
            return;
        };
        let Some(model) = document.update(cx, |document, _| document.selected_model()) else {
            cx.propagate();
            return;
        };
        if self.turning.is_some() {
            self.stop_turning(cx);
            return;
        }
        self.stop_video(cx);
        let Some(source) = model.source.clone() else { return };
        let opened = document.read(cx).open_large(&model.on.src);
        let Some(opened) = opened else {
            let said = document.update(cx, |document, _| document.selected_model_said());
            let said = said.unwrap_or_else(|| slides_model::CantDraw::CouldNotRead.to_string());
            cx.emit(EditorEvent::Said(said.into()));
            return;
        };
        let (slide, talk) = {
            let document = document.read(cx);
            (document.current(), talk_hash(document.source()))
        };
        let layer = model.layer;
        self.canvas.update(cx, |canvas, cx| canvas.turn_model(layer, model.on, opened, false, crate::model_view::Aim::Home, cx));
        self.turning = Some((PlayingHere { slide, source, talk }, layer));
        cx.notify();
    }

    /// Whether the model selected is being turned.
    pub(super) fn model_turning(&self) -> bool {
        self.turning.is_some()
    }

    /// Goes back to the still of the model turned, if one was.
    pub fn stop_turning(&mut self, cx: &mut Context<Self>) {
        self.turn_drag = None;
        if self.turning.take().is_some() {
            self.canvas.update(cx, |canvas, cx| canvas.stop_models(cx));
            cx.notify();
        }
    }

    /// Writes the view the model turned is shown from into the talk, as its starting view, as
    /// one step. It goes on being turned.
    pub(super) fn set_view(&mut self, cx: &mut Context<Self>) {
        let (Some((turning, layer)), Some(document)) = (self.turning.clone(), self.document.clone()) else {
            return;
        };
        let Some(view) = self.canvas.read(cx).model_view(layer) else {
            return;
        };
        let written = document.update(cx, |document, cx| {
            let written = document.set_model_view(turning.source.clone(), view);
            cx.notify();
            written.map(|_| (document.selected_model(), talk_hash(document.source())))
        });
        match written {
            // Written by this, the change is no reason to stop.
            Ok((Some(model), talk)) => {
                let Some(source) = model.source else { return };
                self.turning = Some((PlayingHere { slide: turning.slide, source, talk }, model.layer));
            }
            Ok((None, _)) => self.stop_turning(cx),
            Err(refused) => cx.emit(EditorEvent::Said(refused.to_string().into())),
        }
        cx.notify();
    }

    /// Starts a drag turning the model, if one is being turned and the press at `position` is on
    /// it: true if it did.
    pub(super) fn press_turning(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        let (Some((_, layer)), Some(document)) = (self.turning.clone(), self.document.clone()) else {
            return false;
        };
        let Some(model) = document.update(cx, |document, _| document.selected_model()) else {
            return false;
        };
        let Some((x, y)) = self.slide_units(position, cx) else {
            return false;
        };
        if !inside_turned(model.on.rect, model.on.rotate, (x, y)) {
            return false;
        }
        self.turn_drag = Some(position);
        self.canvas.update(cx, |canvas, cx| canvas.hold_model(layer, true, cx));
        true
    }

    /// Turns the model by as far as the pointer has moved, while a drag turns it: true if one does.
    pub(super) fn drag_turning(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        let (Some(from), Some((_, layer))) = (self.turn_drag, self.turning.clone()) else {
            return false;
        };
        let (dx, dy) = (f32::from(position.x - from.x), f32::from(position.y - from.y));
        self.turn_drag = Some(position);
        self.canvas.update(cx, |canvas, cx| canvas.turn_by(layer, dx, dy, cx));
        true
    }

    /// Ends a drag turning the model: true if one did.
    pub(super) fn release_turning(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(_) = self.turn_drag.take() else {
            return false;
        };
        if let Some((_, layer)) = self.turning.clone() {
            self.canvas.update(cx, |canvas, cx| canvas.hold_model(layer, false, cx));
        }
        true
    }

    /// Zooms the model turned by the wheel's `steps`, if the pointer at `position` is on it.
    pub(super) fn wheel_turning(&mut self, position: Point<Pixels>, steps: f32, cx: &mut Context<Self>) -> bool {
        let (Some((_, layer)), Some(document)) = (self.turning.clone(), self.document.clone()) else {
            return false;
        };
        let Some(model) = document.update(cx, |document, _| document.selected_model()) else {
            return false;
        };
        let on = self.slide_units(position, cx).is_some_and(|at| inside_turned(model.on.rect, model.on.rotate, at));
        if on {
            self.canvas.update(cx, |canvas, cx| canvas.zoom_by(layer, steps, cx));
        }
        on
    }

    /// Stops the model turned unless it's still the one thing selected, on the slide it was
    /// turned on, with the talk's text as it was; and reads it again once a save that let go of
    /// its file is done.
    fn keep_turning_only_what_holds(&mut self, cx: &mut Context<Self>) {
        let (Some((turning, _)), Some(document)) = (self.turning.clone(), self.document.clone()) else {
            return;
        };
        let holds = document.update(cx, |document, _| {
            document.current() == turning.slide
                && talk_hash(document.source()) == turning.talk
                && document.selected_model().is_some_and(|model| model.source.as_ref() == Some(&turning.source))
        });
        if !holds {
            self.stop_turning(cx);
            return;
        }
        let waiting = self.canvas.read(cx).models_let_go();
        if !waiting.is_empty() && !document.read(cx).saving_the_pack() {
            let mut opened: Vec<(String, LargeOpen)> =
                waiting.into_iter().filter_map(|src| Some((src.clone(), document.read(cx).open_large(&src)?))).collect();
            self.canvas.update(cx, |canvas, cx| {
                canvas.read_models_again(|src| opened.iter().position(|(at, _)| at == src).map(|at| opened.swap_remove(at).1), cx)
            });
        }
    }

    /// Stops the video played unless it's still the one thing selected, on the slide it was
    /// played on, with the talk's text as it was; and opens it again once a save that let go of
    /// its file is done. So for a model turned.
    pub(super) fn keep_playing_only_what_holds(&mut self, cx: &mut Context<Self>) {
        self.keep_turning_only_what_holds(cx);
        let (Some(playing), Some(document)) = (self.playing.clone(), self.document.clone()) else {
            return;
        };
        let holds = document.update(cx, |document, _| {
            document.current() == playing.slide
                && talk_hash(document.source()) == playing.talk
                && document.selected_video().is_some_and(|video| video.source.as_ref() == Some(&playing.source))
        });
        if !holds {
            self.stop_video(cx);
            return;
        }
        let waiting = self.canvas.read(cx).videos_let_go();
        if !waiting.is_empty() && !document.read(cx).saving_the_pack() {
            let mut opened: Vec<(String, LargeOpen)> =
                waiting.into_iter().filter_map(|src| Some((src.clone(), document.read(cx).open_large(&src)?))).collect();
            self.canvas.update(cx, |canvas, cx| {
                canvas.open_videos_again(|src| opened.iter().position(|(at, _)| at == src).map(|at| opened.swap_remove(at).1), cx)
            });
        }
    }

    /// Tells the status line what's selected, once that's changed, or that it's let go of.
    pub(super) fn tell_selection(&mut self, cx: &mut Context<Self>) {
        let said = self.said_of_selection(cx);
        if said == self.told {
            return;
        }
        match (&said, self.told.take()) {
            (Some(said), _) => cx.emit(EditorEvent::Said(said.clone())),
            (None, Some(told)) => cx.emit(EditorEvent::Unsaid(told)),
            (None, None) => {}
        }
        self.told = said;
    }

    /// Selects the thing on slide `slide` whose source is `source`, ending any typing, and
    /// takes the keyboard for it.
    pub(super) fn select_thing(&mut self, slide: usize, source: Range<usize>, window: &mut Window, cx: &mut Context<Self>) {
        self.select_with(|document| document.select(slide, source), window, cx);
    }

    /// Selects `thing` on slide `slide` alone, as [`Self::select_thing`] does a block.
    pub(super) fn select_one(&mut self, slide: usize, thing: Thing, window: &mut Window, cx: &mut Context<Self>) {
        match thing {
            Thing::Block(source) => self.select_thing(slide, source, window, cx),
            Thing::Region(name) => self.select_with(|document| document.select_region(slide, &name), window, cx),
        }
    }

    /// Adds `thing` on slide `slide` to what's selected, or takes it away if it's selected, as
    /// a click with Shift or Ctrl does.
    pub(super) fn select_also(&mut self, slide: usize, thing: Thing, window: &mut Window, cx: &mut Context<Self>) {
        self.select_with(|document| document.select_also(slide, thing), window, cx);
    }

    /// Selects every thing on the slide shown, as Ctrl+A does while a thing is selected.
    pub(super) fn select_all_things(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.select_with(Document::select_all_things, window, cx);
    }

    /// Selects what `pick` does, ending any typing, and takes the keyboard for it.
    fn select_with(&mut self, pick: impl FnOnce(&mut Document) -> bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(document) = self.document.clone() else {
            return;
        };
        self.stop(cx);
        self.pressed = None;
        let message = document.update(cx, |document, cx| {
            pick(document);
            cx.notify();
            document.take_message()
        });
        if let Some(message) = message {
            cx.emit(EditorEvent::Said(message.into()));
        }
        if self.selection_here(cx).is_some() {
            self.focus_handle.focus(window, cx);
        }
        // Said even if it's said already, as the status line may say something else by now.
        let said = self.said_of_selection(cx);
        match (&said, self.told.take()) {
            (Some(said), _) => cx.emit(EditorEvent::Said(said.clone())),
            (None, Some(told)) => cx.emit(EditorEvent::Unsaid(told)),
            (None, None) => {}
        }
        self.told = said;
        cx.notify();
    }

    /// Lets go of everything selected, if anything is.
    pub(super) fn deselect(&mut self, cx: &mut Context<Self>) {
        if let Some(document) = &self.document {
            document.update(cx, |document, cx| {
                if document.selection().is_some() {
                    document.deselect();
                    cx.notify();
                }
            });
        }
    }

    /// Lets go of what's selected on the slide shown, as Escape does once nothing is typed.
    pub(super) fn let_go(&mut self, cx: &mut Context<Self>) {
        // A drag readied whose release was never heard ends, and Escape goes on to let go.
        self.end_unbegun_drag(cx);
        if self.drag.is_some() {
            // A drag ends first, writing nothing, and what was dragged stays selected.
            self.end_drag(cx);
        } else if let Some(selection) = self.selection_here(cx) {
            // From what a box holds alone, back to the box.
            let held = self.document.clone().and_then(|document| document.update(cx, |document, _| document.holding_box_selected()));
            match (held, &self.document) {
                (Some((Thing::Block(held), _)), Some(document)) => {
                    document.update(cx, |document, cx| {
                        document.select(selection.slide, held);
                        cx.notify();
                    });
                    self.tell_selection(cx);
                }
                _ => self.deselect(cx),
            }
        } else {
            cx.propagate();
        }
    }

    /// Copies or cuts the selection of the text being typed, as [`Self::copy_or_cut`] does; with
    /// none being typed, the window copies or cuts the thing selected.
    pub(super) fn copy_or_cut_typed(&mut self, cut: bool, cx: &mut Context<Self>) {
        if self.editing(cx).is_none() {
            cx.propagate();
            return;
        }
        self.copy_or_cut(cut, cx);
    }

    /// Puts a copy of the things selected on the slide shown beside them, and selects the copies.
    pub(super) fn duplicate(&mut self, cx: &mut Context<Self>) {
        if self.selection_here(cx).is_none() {
            cx.propagate();
            return;
        }
        self.run(Document::duplicate_selected, cx);
    }

    /// Takes the things selected on the slide shown away.
    pub(super) fn take_away(&mut self, cx: &mut Context<Self>) {
        if self.selection_here(cx).is_none() {
            cx.propagate();
            return;
        }
        self.run(Document::delete_selected, cx);
    }

    /// Selects the next thing on the slide after the one picked last, in the order they're
    /// written and then the layout's areas, or with `back` the one before; past the last comes
    /// the first again.
    pub(super) fn select_along(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selection) = self.selection_here(cx) else {
            cx.propagate();
            return;
        };
        let Some(document) = self.document.clone() else {
            return;
        };
        let things = document.update(cx, |document, _| document.tab_order(selection.slide));
        let Some(at) = selection.things.last().and_then(|last| things.iter().position(|thing| thing == last)) else {
            return;
        };
        let next = if back { (at + things.len() - 1) % things.len() } else { (at + 1) % things.len() };
        self.select_one(selection.slide, things[next].clone(), window, cx);
    }

    /// The thing a click on paragraph `para` of flow `flow` of slide `slide`, at `point`, would
    /// select before typing in it: the box the text is written in, if it's one.
    /// A cell of a table is typed in at once.
    pub(super) fn box_of_text(&self, slide: usize, flow: usize, para: usize, (x, y): (f32, f32), cx: &mut Context<Self>) -> Option<Range<usize>> {
        let element = self.layout(slide, cx)?.element_at(x, y)?;
        let document = self.document.as_ref()?.read(cx);
        let flows = document.text_flows(document.talk(), slide);
        let in_cell = flows.get(flow).is_some_and(|flow| matches!(flow.place, FlowPlace::Cell { .. }));
        (!in_cell && element.holds_text && selection::holds_para(&flows, flow, para, &element.source)).then_some(element.source)
    }

    /// Why the cell of the table `table` on slide `slide` that (`x`, `y`) is well inside can't be
    /// typed in, if it's one that holds more than text.
    pub(super) fn cell_reason(&self, slide: usize, table: &Range<usize>, (x, y): (f32, f32), cx: &mut Context<Self>) -> Option<&'static str> {
        let layout = self.layout(slide, cx)?;
        let talk = self.document.as_ref()?.read(cx).talk();
        let Some(Block::Table { cells, .. }) = selection::block_at(&talk.slides.get(slide)?.blocks, table) else {
            return None;
        };
        // Away from its edges, which pick the table.
        let inside = |rect: Rect| rect.x + EDGE < x && x < rect.x + rect.w - EDGE && rect.y + EDGE < y && y < rect.y + rect.h - EDGE;
        let cell = cells.iter().enumerate().find_map(|(row, cells)| {
            cells.iter().enumerate().find(|(column, _)| layout.cell_rect(table, row, *column).is_some_and(inside)).map(|(_, cell)| cell)
        })?;
        if cell.only_text {
            return None;
        }
        uneditable_reason(talk, slide, cell.inline.start)
    }

    /// Whether the thing on slide `slide` at `source` is the one selected.
    pub(super) fn is_selected(&self, slide: usize, source: &Range<usize>, cx: &App) -> bool {
        self.document.as_ref().is_some_and(|document| {
            document
                .read(cx)
                .selected()
                .is_some_and(|selected| selected.slide == slide && selected.source == *source)
        })
    }

    /// Whether `thing` on slide `slide` is one of several selected.
    pub(super) fn is_among_selected(&self, slide: usize, thing: &Thing, cx: &App) -> bool {
        self.document.as_ref().is_some_and(|document| {
            document
                .read(cx)
                .selection()
                .is_some_and(|selection| selection.slide == slide && selection.things.len() > 1 && selection.things.contains(thing))
        })
    }
}

/// The video selected that was played: the slide it's on, where it's written, and the talk's
/// text then, by its hash.
#[derive(Clone)]
pub(super) struct PlayingHere {
    slide: usize,
    source: Range<usize>,
    talk: u64,
}

fn talk_hash(text: &str) -> u64 {
    use std::hash::{Hash as _, Hasher as _};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// How far inside a cell of a table, in slide units, a click is on the cell rather than on the
/// lines around it, which pick the table.
const EDGE: f32 = 4.5;
