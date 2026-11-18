//! The system's input method: what it composes, and the text and places it's told of.

use super::*;

impl SlideEditor {
    /// Drops what the system was composing, as before a cut or a paste.
    pub(super) fn drop_composing(&mut self, cx: &mut Context<Self>) {
        if self.editing(cx).is_some_and(|editing| editing.composing.is_some()) {
            self.set_composing(None, cx);
        }
    }

    /// Shows `composing` at the caret, or drops what was being composed. The talk isn't
    /// touched.
    pub(super) fn set_composing(&mut self, composing: Option<Composing>, cx: &mut Context<Self>) {
        let Some(document) = self.document.clone() else {
            return;
        };
        document.update(cx, |document, _| document.compose(composing));
        self.wake(cx);
        cx.notify();
    }

    /// Types what was being composed, as it is.
    fn commit_composing(&mut self, cx: &mut Context<Self>) {
        let text = self
            .editing(cx)
            .and_then(|editing| editing.composing.as_ref())
            .map(|composing| composing.text.clone());
        if let Some(text) = text {
            self.insert_text(&text, cx);
        }
    }

    /// The places in the text being typed from UTF-16 offset `range.start` to `range.end` of
    /// what the system sees, on the edges of characters, taking in any character they cut. An
    /// empty range is a caret, before the character it's in.
    fn places(&self, range: &Range<usize>, cx: &App) -> Option<(Pos, Pos)> {
        let editing = self.editing(cx)?;
        let (start, end) = (range.start.min(range.end), range.start.max(range.end));
        let from = editing.system_pos(start, false);
        let to = if end == start {
            from
        } else {
            editing.system_pos(end, true)
        };
        Some((from, to))
    }
}

/// `range` from its lower end to its higher, as the system may give it either way round.
fn ordered(range: Range<usize>) -> Range<usize> {
    range.start.min(range.end)..range.start.max(range.end)
}

/// The system's input method types through this: what it sees is the text being typed, with
/// what it's composing in place of the selection, and its offsets are UTF-16 (see
/// [`EditingText::system_text`]). What it composes isn't in the talk until it's committed, which
/// types it, as a key would; a composition that's cancelled leaves nothing.
impl EntityInputHandler for SlideEditor {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let (text, range) = utf16_slice(&self.editing(cx)?.system_text(), ordered(range));
        *adjusted_range = Some(range);
        Some(text)
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let (range, reversed) = self.editing(cx)?.system_selection();
        Some(UTF16Selection { range, reversed })
    }

    fn marked_text_range(&self, _: &mut Window, cx: &mut Context<Self>) -> Option<Range<usize>> {
        self.editing(cx)?.marked_range()
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.commit_composing(cx);
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !clean_typed(text).is_empty() {
            self.start_typing_in_selected(cx);
        }
        let Some(editing) = self.editing(cx) else {
            return;
        };
        let text = clean_typed(text);
        let composing = editing.composing.is_some();
        // The composition's own range is the selection it stands in for.
        let range = range
            .map(ordered)
            .filter(|range| Some(range) != editing.marked_range().as_ref());
        if text.is_empty() && range.is_none() {
            // A composition cancelled, or nothing typed: nothing changes.
            if composing {
                self.set_composing(None, cx);
            }
            return;
        }
        let over = range.and_then(|range| self.places(&range, cx));
        self.type_over(over, &text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !text.is_empty() {
            self.start_typing_in_selected(cx);
        }
        let Some(editing) = self.editing(cx) else {
            return;
        };
        let range = range
            .map(ordered)
            .filter(|range| Some(range) != editing.marked_range().as_ref());
        if let Some((from, to)) = range.and_then(|range| self.places(&range, cx)) {
            // The system composes over text of its own choosing: that's what's selected.
            self.wanted_x = None;
            self.apply(|edit| edit.move_to(from, false).move_to(to, true), cx);
        }
        // Shown as it'll be typed. A selection inside text that cleaning changed is its end.
        let cleaned = clean_typed(text);
        let selected = selected.map(ordered).filter(|_| cleaned == text);
        self.set_composing(Composing::new(&cleaned, selected), cx);
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let at = self.editing(cx)?.system_pos(ordered(range).start, false);
        self.caret_in_window(at, cx)
    }

    fn character_index_for_point(
        &mut self,
        position: Point<Pixels>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        let point = self.slide_point(position, cx)?;
        let (slide, Hit::Text { flow, pos }) = self.hit(point, cx)? else {
            return None;
        };
        if !self.editing_flow(slide, flow, cx) {
            return None;
        }
        Some(self.editing(cx)?.system_offset(pos))
    }

    fn set_selected_text_range(
        &mut self,
        range: Range<usize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = ordered(range);
        let Some(editing) = self.editing(cx) else {
            return;
        };
        if let (Some(composing), Some(marked)) = (&editing.composing, editing.marked_range())
            && marked.start <= range.start
            && range.end <= marked.end
        {
            // The system's caret moves inside what it's composing.
            let selected = range.start - marked.start..range.end - marked.start;
            let composing = Composing::new(&composing.text, Some(selected));
            self.set_composing(composing, cx);
            return;
        }
        if let Some((from, to)) = self.places(&range, cx) {
            self.wanted_x = None;
            self.apply(|edit| edit.move_to(from, false).move_to(to, true), cx);
        }
    }

    fn accepts_text_input(&self, _: &mut Window, cx: &mut Context<Self>) -> bool {
        self.is_editing(cx) || self.takes_typing(cx)
    }
}
