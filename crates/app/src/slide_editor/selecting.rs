//! The thing selected on the slide shown: picking it, letting go of it, and what's said of it.

use super::*;

impl SlideEditor {
    /// The thing selected, if it's on the slide shown.
    pub(super) fn selection_here(&self, cx: &App) -> Option<Selected> {
        let document = self.document.as_ref()?.read(cx);
        document
            .selected()
            .filter(|selected| selected.slide == document.current())
            .cloned()
    }

    /// What the status line says of the thing selected on the slide shown.
    fn said_of_selection(&self, cx: &App) -> Option<SharedString> {
        let selected = self.selection_here(cx)?;
        let talk = self.document.as_ref()?.read(cx).talk();
        selection::said_of(talk, selected.slide, &selected.source).map(SharedString::from)
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
        let Some(document) = self.document.clone() else {
            return;
        };
        self.stop(cx);
        let message = document.update(cx, |document, cx| {
            document.select(slide, source);
            cx.notify();
            document.take_message()
        });
        if let Some(message) = message {
            cx.emit(EditorEvent::Said(message.into()));
        }
        self.focus_handle.focus(window, cx);
        // Said even if it's said already, as the status line may say something else by now.
        self.told = self.said_of_selection(cx);
        if let Some(said) = &self.told {
            cx.emit(EditorEvent::Said(said.clone()));
        }
        cx.notify();
    }

    /// Lets go of the thing selected, if anything is.
    pub(super) fn deselect(&mut self, cx: &mut Context<Self>) {
        if let Some(document) = &self.document {
            document.update(cx, |document, cx| {
                if document.selected().is_some() {
                    document.deselect();
                    cx.notify();
                }
            });
        }
    }

    /// Lets go of the thing selected on the slide shown, as Escape does once nothing is typed.
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

    /// Puts a copy of the thing selected on the slide shown beside it, and selects the copy.
    pub(super) fn duplicate(&mut self, cx: &mut Context<Self>) {
        if self.selection_here(cx).is_none() {
            cx.propagate();
            return;
        }
        self.run(Document::duplicate_selected, cx);
    }

    /// Takes the thing selected on the slide shown away.
    pub(super) fn take_away(&mut self, cx: &mut Context<Self>) {
        if self.selection_here(cx).is_none() {
            cx.propagate();
            return;
        }
        self.run(Document::delete_selected, cx);
    }

    /// Selects the next thing on the slide after the one selected, in the order they're
    /// written, or with `back` the one before; past the last comes the first again.
    pub(super) fn select_along(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(selected) = self.selection_here(cx) else {
            cx.propagate();
            return;
        };
        let Some(layout) = self.layout(selected.slide, cx) else {
            return;
        };
        let mut things: Vec<Range<usize>> = layout.elements().iter().map(|element| element.source.clone()).collect();
        things.sort_by_key(|source| (source.start, std::cmp::Reverse(source.end)));
        things.dedup();
        let Some(at) = things.iter().position(|source| *source == selected.source) else {
            return;
        };
        let next = if back { (at + things.len() - 1) % things.len() } else { (at + 1) % things.len() };
        self.select_thing(selected.slide, things[next].clone(), window, cx);
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
}

/// How far inside a cell of a table, in slide units, a click is on the cell rather than on the
/// lines around it, which pick the table.
const EDGE: f32 = 4.5;
