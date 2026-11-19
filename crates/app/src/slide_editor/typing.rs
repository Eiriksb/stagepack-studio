//! Typing on the slide: what's typed, formatted and pasted, the caret's blinking, and where
//! typing starts and ends.

use super::*;

impl SlideEditor {
    /// Does `work` to the talk, and tells the window what came of it: the slides it changed,
    /// or `None` if it was refused, which is said, as is anything else that went wrong.
    pub(super) fn run(
        &mut self,
        work: impl FnOnce(&mut Document) -> Result<Vec<usize>, EditRefused>,
        cx: &mut Context<Self>,
    ) -> Option<Vec<usize>> {
        let document = self.document.clone()?;
        let (made, message) = document.update(cx, |document, cx| {
            let cell = |document: &Document| document.text().filter(|editing| editing.is_cell()).map(|editing| (editing.slide, editing.flow.clone()));
            let was = cell(document);
            let made = work(document);
            // The table tools follow the caret from cell to cell, which changes nothing of the
            // talk: they're told all the same.
            if made.as_ref().is_ok_and(|changed| !changed.is_empty()) || cell(document) != was {
                cx.notify();
            }
            (made, document.take_message())
        });
        if let Some(message) = message {
            cx.emit(EditorEvent::Said(message.into()));
        }
        // Whatever the work changed: typing begun changes nothing of the talk, so isn't followed
        // as an edit is, but lets go of the video selected, which then stops.
        self.keep_playing_only_what_holds(cx);
        match made {
            Ok(changed) => {
                if !changed.is_empty() {
                    cx.emit(EditorEvent::Edited);
                }
                Some(changed)
            }
            Err(refused) => {
                cx.emit(EditorEvent::Said(refused.to_string().into()));
                None
            }
        }
    }

    /// Makes `op` to the text being typed.
    pub(super) fn apply(&mut self, op: impl FnOnce(&TextEdit) -> TextEdit, cx: &mut Context<Self>) {
        self.run(|document| document.apply_text(op), cx);
        self.typed(cx);
    }

    /// Makes `op`, one of the text tools', to the text being typed: a step of its own to undo,
    /// after which typing starts a new one.
    pub fn format(&mut self, op: impl FnOnce(&TextEdit) -> TextEdit, cx: &mut Context<Self>) {
        self.wanted_x = None;
        self.apply(op, cx);
    }

    /// Aligns the text being typed: all of its region, or its box.
    /// Text that's all list items isn't aligned: lists stay at the left.
    pub fn align_text(&mut self, align: Align, cx: &mut Context<Self>) {
        // A cell is aligned with its column, which isn't changed from here.
        if self.editing(cx).is_some_and(|editing| editing.is_cell() || only_lists(&editing.edit.paras)) {
            return;
        }
        self.run(|document| document.align_text(align), cx);
        self.typed(cx);
    }

    /// Gives the selection, or what's typed next, size `size`. Where that's the size its
    /// paragraphs are drawn at anyway, the text is given no size of its own instead.
    pub fn set_text_size(&mut self, size: u32, cx: &mut Context<Self>) {
        let base = self.base_size(cx);
        let size = (base != Some(size)).then_some(size);
        self.format(|edit| edit.set_size(size), cx);
    }

    /// The size the selected paragraphs are drawn at before their spans have their say, if
    /// it's one size for all of them.
    fn base_size(&self, cx: &mut Context<Self>) -> Option<u32> {
        let (_, flow, layout) = self.editing_place(cx)?;
        let editing = self.editing(cx)?;
        let selection = editing.edit.selection;
        let sizes: Vec<Option<f32>> = (selection.start().para..=selection.end().para)
            .map(|para| match flow {
                Some(flow) => layout.text_size(flow, para),
                None => editing
                    .flow
                    .region()
                    .and_then(|region| layout.placeholder_text(region))
                    .map(|(size, _)| size),
            })
            .collect();
        let first = (*sizes.first()?)?;
        sizes
            .iter()
            .all(|size| *size == Some(first))
            .then(|| first.round().max(0.0) as u32)
    }

    /// Copies the selection of the text being typed, or with `cut` cuts it. Pasting is the
    /// window's, which reads the clipboard once and hands text here, to [`Self::paste_text`].
    pub fn copy_or_cut(&mut self, cut: bool, cx: &mut Context<Self>) {
        if cut {
            self.cut_text(cx);
        } else {
            self.copy_text(cx);
        }
    }

    /// Puts the selected text on the clipboard, and says whether there was any to put there.
    /// With nothing selected, it does nothing.
    fn copy_text(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(paras) = self.editing(cx).map(|editing| editing.edit.copied()) else {
            return false;
        };
        if paras.is_empty() {
            return false;
        }
        write_clipboard(clipboard::text_entry(&paras), cx);
        true
    }

    /// Copies the selected text and deletes it, as one step to undo. Nothing is deleted that
    /// wasn't copied.
    fn cut_text(&mut self, cx: &mut Context<Self>) {
        self.drop_composing(cx);
        if !self.copy_text(cx) {
            return;
        }
        self.wanted_x = None;
        self.run(Document::cut_text, cx);
        self.typed(cx);
    }

    /// Puts what's on the clipboard in place of the selection, as one step to undo: the styled
    /// text the app copied, or else plain text, a paragraph to a line, in the style at the
    /// caret. In a title, which is one paragraph, the paragraphs are joined by line breaks;
    /// anywhere else, a heading takes the kind of the paragraph it's pasted into.
    pub fn paste_text(&mut self, plain: Option<&str>, metadata: Option<&str>, cx: &mut Context<Self>) {
        self.drop_composing(cx);
        let Some(editing) = self.editing(cx) else {
            return;
        };
        let title = editing.is_title();
        let in_cell = editing.is_cell();
        let edit = &editing.edit;
        let kind = edit.paras.get(edit.selection.start().para).map(|para| para.kind);
        let paste = match clipboard::text_to_paste(plain, metadata) {
            Ok(paste) => paste,
            Err(clipboard::TooLarge) => {
                cx.emit(EditorEvent::Said(clipboard::TOO_LARGE.into()));
                return;
            }
        };
        let bytes = match &paste {
            TextPaste::Nothing => 0,
            TextPaste::Styled(paras) => paras.iter().flat_map(|para| &para.spans).map(|span| span.text.len()).sum(),
            TextPaste::Plain(text) => text.len(),
        };
        // More than the slide can hold is refused before anything is written, which would take
        // seconds to find it can't be.
        if !self.document.as_ref().is_some_and(|document| document.read(cx).has_room_for(bytes)) {
            cx.emit(EditorEvent::Said(clipboard::TOO_LARGE.into()));
            return;
        }
        self.wanted_x = None;
        match paste {
            TextPaste::Nothing => return,
            TextPaste::Styled(paras) => {
                let paras = match kind {
                    // A cell holds one line: its paragraphs go in one after another, a space apart.
                    Some(kind) if in_cell => vec![one_line_para(paras, kind)],
                    Some(kind) if title => vec![one_paragraph(paras, kind)],
                    // Outside a title, a heading would be a second title, or a heading where
                    // there was none: it takes the kind of the paragraph it goes into.
                    _ => paras
                        .into_iter()
                        .map(|para| match para.kind {
                            ParaKind::Heading(_) => Para { kind: kind.unwrap_or(ParaKind::Body), ..para },
                            _ => para,
                        })
                        .collect(),
                };
                self.run(|document| document.paste_text(|edit| edit.paste(&paras)), cx);
            }
            TextPaste::Plain(text) => {
                // A line separator is a line break inside the paragraph; in a cell, a space.
                let text = if in_cell {
                    one_line(&text)
                } else if title {
                    text.replace(['\n', '\u{2029}'], "\u{2028}")
                } else {
                    text
                };
                self.run(|document| document.paste_text(|edit| edit.insert(&text)), cx);
            }
        }
        self.typed(cx);
    }

    /// Keeps `color` to give the selection of the text being typed once the colour picker
    /// closes, as its sliders choose one after another. The talk gives it to that selection
    /// before anything else is done to the text.
    pub fn hold_color(&mut self, color: String, cx: &mut Context<Self>) {
        if let Some(document) = &self.document {
            document.update(cx, |document, _| document.hold_color(color));
        }
    }

    /// Gives the colour held for the selection to it, if one is.
    pub fn give_held_color(&mut self, cx: &mut Context<Self>) {
        self.run(Document::give_held_color, cx);
    }

    /// Gives the keyboard back to the text being typed, if any is, as a text tool does once
    /// it's used.
    pub fn focus_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_editing(cx) && !self.focus_handle.is_focused(window) {
            self.focus_handle.focus(window, cx);
            self.wake(cx);
        }
    }

    /// Puts `text` in place of the selection, as typed. Every character typed comes this way,
    /// from the system's input method.
    pub fn insert_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.type_over(None, text, cx);
    }

    /// Puts `text` in place of what's from `over.0` to `over.1`, or the selection, as typed.
    pub(super) fn type_over(&mut self, over: Option<(Pos, Pos)>, text: &str, cx: &mut Context<Self>) {
        self.wanted_x = None;
        // A cell holds one line: a new line typed in one is a space.
        let lined;
        let text = if self.editing(cx).is_some_and(EditingText::is_cell) {
            lined = one_line(text);
            lined.as_str()
        } else {
            text
        };
        self.apply(
            |edit| match over {
                Some((from, to)) => edit.move_to(from, false).move_to(to, true).insert(text),
                None => edit.insert(text),
            },
            cx,
        );
    }

    /// The text being typed.
    pub(super) fn editing<'a>(&self, cx: &'a App) -> Option<&'a EditingText> {
        self.document.as_ref()?.read(cx).text()
    }

    /// After any key or click: the caret shows and blinks again, and a run of typing goes on
    /// while it's not paused for long.
    pub(super) fn typed(&mut self, cx: &mut Context<Self>) {
        self.wake(cx);
        let typing = self.document.as_ref().is_some_and(|document| {
            document
                .read(cx)
                .text()
                .is_some_and(|editing| editing.run.is_some())
        });
        self.pause = typing.then(|| {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(TYPING_PAUSE).await;
                this.update(cx, |this, cx| {
                    if let Some(document) = &this.document {
                        document.update(cx, |document, _| document.end_typing_run());
                    }
                })
                .ok();
            })
        });
        cx.notify();
    }

    /// Shows the caret, and has it blink for a while.
    pub(super) fn wake(&mut self, cx: &mut Context<Self>) {
        self.caret_on = true;
        self.blinks_left = (CARET_BLINKS_FOR.as_millis() / BLINK.as_millis()) as u32;
        if self.blinking {
            return;
        }
        self.blinking = true;
        self.blink = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(BLINK).await;
                let goes_on = this
                    .update(cx, |this, cx| this.blink_once(cx))
                    .unwrap_or(false);
                if !goes_on {
                    break;
                }
            }
        }));
    }

    /// One blink, and whether there's another to come.
    fn blink_once(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.blinking {
            return false;
        }
        if self.blinks_left == 0 {
            // Held still, showing.
            self.blinking = false;
            if !self.caret_on {
                self.caret_on = true;
                cx.notify();
            }
            return false;
        }
        self.blinks_left -= 1;
        self.caret_on = !self.caret_on;
        cx.notify();
        true
    }

    /// Stops the caret's timers, as nothing is being typed.
    pub(super) fn stop(&mut self, _cx: &mut Context<Self>) {
        self.blinking = false;
        self.blink = None;
        self.pause = None;
        self.dragging = false;
        self.wanted_x = None;
    }

    /// Ends typing on the slide and lets go of anything selected, as a click on the desk
    /// around it does, and gives the keyboard back to the window.
    pub fn stop_typing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.deselect(cx);
        self.end_editing(true, window, cx);
    }

    /// Ends typing on the slide. With `give_back`, the keyboard goes back to the window, if
    /// the editor has it, unless a thing on the slide is still selected.
    pub(super) fn end_editing(&mut self, give_back: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.stop(cx);
        if self.is_editing(cx) {
            self.run(Document::end_text, cx);
        }
        if give_back && self.focus_handle.is_focused(window) && self.selection_here(cx).is_none() {
            self.away.focus(window, cx);
        }
        cx.notify();
    }

    /// The text being typed: its slide, which flow it is if it's in the talk yet, and the
    /// layout of that slide.
    pub(super) fn editing_place(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<(usize, Option<usize>, Arc<TextLayout>)> {
        let document = self.document.clone()?;
        let slide = document.read(cx).text()?.slide;
        let flow = document.read(cx).text_flow();
        Some((slide, flow, self.layout(slide, cx)?))
    }

    /// Where slide `slide`'s text is. If it couldn't be laid out, that's said.
    pub(super) fn layout(&self, slide: usize, cx: &mut Context<Self>) -> Option<Arc<TextLayout>> {
        let document = self.document.clone()?;
        let (layout, message) = document.update(cx, |document, _| {
            (document.text_layout(slide), document.take_message())
        });
        if let Some(message) = message {
            cx.emit(EditorEvent::Said(message.into()));
            // Typing on it ended, so the keyboard goes back.
            document.update(cx, |_, cx| cx.notify());
        }
        Some(layout)
    }

    /// The new text box typed in that isn't written yet, if it's on the slide shown: where it is,
    /// `(x, y, w, h)`, in slide units.
    pub(super) fn new_box_here(&self, cx: &App) -> Option<(f32, f32, f32, f32)> {
        let document = self.document.as_ref()?.read(cx);
        document.new_box().filter(|(slide, _)| *slide == document.current()).map(|(_, rect)| rect)
    }

    /// Whether the thing selected on the slide shown takes typing, with nothing being typed: a
    /// shape, a box, or one of the talk's own components that takes content, which typing puts
    /// words in.
    pub(super) fn takes_typing(&self, cx: &App) -> bool {
        !self.is_editing(cx)
            && self.selection_here(cx).is_some()
            && self.document.as_ref().is_some_and(|document| document.read(cx).selected_takes_text())
    }

    /// Starts typing in the thing selected, if it takes typing, as the first thing typed on it
    /// does. Says whether text is being typed now.
    pub(super) fn start_typing_in_selected(&mut self, cx: &mut Context<Self>) -> bool {
        if self.is_editing(cx) {
            return true;
        }
        if !self.takes_typing(cx) {
            return false;
        }
        self.wanted_x = None;
        self.run(
            |document| {
                document.begin_text_in_selected();
                Ok(Vec::new())
            },
            cx,
        );
        if let Some(document) = &self.document {
            document.update(cx, |_, cx| cx.notify());
        }
        self.wake(cx);
        self.is_editing(cx)
    }
}

/// `paras` as one paragraph of `kind`, each joined to the next by a line break.
fn one_paragraph(paras: Vec<Para>, kind: slides_format::ParaKind) -> Para {
    let mut spans: Vec<Span> = Vec::new();
    for (index, para) in paras.into_iter().enumerate() {
        if index > 0 {
            let style = spans.last().map(|span| span.style.clone()).unwrap_or_default();
            spans.push(Span {
                text: "\n".to_string(),
                style,
            });
        }
        spans.extend(para.spans);
    }
    Para { kind, spans }
}

/// `paras` as one paragraph of `kind`, each joined to the next by a space of no style of its
/// own, and each new line in them a space too: what goes in a cell of a table, which holds one
/// line.
fn one_line_para(paras: Vec<Para>, kind: slides_format::ParaKind) -> Para {
    let mut spans: Vec<Span> = Vec::new();
    for (index, para) in paras.into_iter().enumerate() {
        if index > 0 {
            spans.push(Span { text: " ".to_string(), style: Default::default() });
        }
        spans.extend(para.spans.into_iter().map(|span| Span { text: one_line(&span.text), ..span }));
    }
    Para { kind, spans }
}
