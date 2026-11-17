//! The things selected on the slide shown: picking them, letting go of them, and what's said
//! of them.

use super::*;

impl SlideEditor {
    /// What's selected, if it's on the slide shown.
    pub(super) fn selection_here(&self, cx: &App) -> Option<selection::Selection> {
        let document = self.document.as_ref()?.read(cx);
        document
            .selection()
            .filter(|selection| selection.slide == document.current())
            .cloned()
    }

    /// What the status line says of what's selected on the slide shown.
    fn said_of_selection(&self, cx: &App) -> Option<SharedString> {
        let selection = self.selection_here(cx)?;
        let talk = self.document.as_ref()?.read(cx).talk();
        selection::said_of_selection(talk, &selection).map(SharedString::from)
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
        if self.selection_here(cx).is_some() {
            self.deselect(cx);
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

/// How far inside a cell of a table, in slide units, a click is on the cell rather than on the
/// lines around it, which pick the table.
const EDGE: f32 = 4.5;
