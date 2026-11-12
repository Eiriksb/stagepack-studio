//! Editing the talk from the window: the slide commands the tools and keys share, moving
//! between slides, and the panels around the slide.

use super::*;

impl WorkspaceView {
    /// Moves to another slide, unless the notes field has the keyboard: its keys move the caret.
    pub(super) fn go(
        &mut self,
        step: impl FnOnce(&mut Document),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.notes_panel.read(cx).is_focused(window, cx) {
            return;
        }
        self.select(step, window, cx);
    }

    /// Makes another slide the current one. Nothing about the talk changes, so its show isn't
    /// told: the panels that follow the current slide are.
    pub(super) fn select(
        &mut self,
        step: impl FnOnce(&mut Document),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(document) = self.document.clone() else {
            return;
        };
        let before = document.read(cx).current();
        document.update(cx, |document, _| step(document));
        if document.read(cx).current() == before {
            return;
        }
        self.notes_panel
            .update(cx, |panel, cx| panel.follow_slide(window, cx));
        self.slide_strip.update(cx, |_, cx| cx.notify());
        self.ribbon.update(cx, |_, cx| cx.notify());
        self.show_slide(cx);
    }

    /// Makes an edit to the talk with `change`, or says why it couldn't be made. Every edit made
    /// in the window comes this way: the talk's observers, the working copy among them, hear of
    /// it as they hear of any other change.
    pub(super) fn edit(
        &mut self,
        change: impl FnOnce(&mut Document) -> Result<Vec<usize>, EditRefused>,
        cx: &mut Context<Self>,
    ) {
        let Some(document) = self.document.clone() else {
            return;
        };
        let (made, changed) = document.update(cx, |document, cx| {
            let before = talk_state(document);
            let made = change(document);
            // Whether the talk changed, as its text or its history did: what the edit says
            // to draw again is about pictures, and can be empty for a change all the same.
            let changed = talk_state(document) != before;
            if changed {
                cx.notify();
            }
            (made, changed)
        });
        if changed {
            self.edited(cx);
        }
        if let Err(refused) = made {
            self.say(refused.to_string(), cx);
        }
    }

    /// Takes back the last edit, or makes the last one undone again.
    pub(super) fn step_history(
        &mut self,
        step: fn(&mut Document) -> Result<Option<Vec<usize>>, EditRefused>,
        cx: &mut Context<Self>,
    ) {
        self.edit(|document| step(document).map(Option::unwrap_or_default), cx);
    }

    /// Brings what shows the talk up to date after an edit. A "Saved" on the status line no
    /// longer holds, so it goes.
    fn edited(&mut self, cx: &mut Context<Self>) {
        self.clear_fading_status(cx);
        let Some(document) = &self.document else {
            return;
        };
        let source = document.read(cx).source().to_string();
        self.markdown_panel
            .update(cx, |panel, cx| panel.set_source(&source, cx));
        self.show_slide(cx);
    }

    pub(super) fn add_slide(&mut self, layout: &str, cx: &mut Context<Self>) {
        self.edit(|document| document.add_slide(layout), cx);
    }

    pub(super) fn delete_slide(&mut self, cx: &mut Context<Self>) {
        self.edit(
            |document| {
                if document.slide_count() > 1 {
                    document.delete_current()
                } else {
                    Ok(Vec::new())
                }
            },
            cx,
        );
    }

    pub(super) fn toggle_hidden(&mut self, cx: &mut Context<Self>) {
        self.edit(
            |document| {
                let hidden = document.is_hidden(document.current());
                document.set_current_hidden(!hidden)
            },
            cx,
        );
    }

    /// Moves the current slide one place earlier, or later with `later`.
    pub(super) fn move_slide(&mut self, later: bool, cx: &mut Context<Self>) {
        self.edit(
            |document| {
                let at = document.current();
                let to = if later { at + 1 } else { at.wrapping_sub(1) };
                if to < document.slide_count() {
                    document.move_slide(at, to)
                } else {
                    Ok(Vec::new())
                }
            },
            cx,
        );
    }

    /// Moves slide `from` so that it's slide `to` afterwards, if both are slides of the talk.
    pub(super) fn move_slide_to(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        self.edit(
            |document| {
                let count = document.slide_count();
                if from != to && from < count && to < count {
                    document.move_slide(from, to)
                } else {
                    Ok(Vec::new())
                }
            },
            cx,
        );
    }

    fn panels(&self) -> Panels {
        Panels {
            notes: self.show_notes,
            slide_list: self.show_slide_list,
            markdown: self.show_markdown,
        }
    }

    /// Shows or hides one of the panels.
    pub(super) fn toggle_panel(
        &mut self,
        panel: fn(&mut Self) -> &mut bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let shown = panel(self);
        *shown = !*shown;
        let panels = self.panels();
        self.ribbon
            .update(cx, |ribbon, cx| ribbon.set_panels(panels, cx));
        // A panel put away can't keep the keyboard.
        let notes_away = !self.show_notes && self.notes_panel.read(cx).is_focused(window, cx);
        let list_away = !self.show_slide_list
            && self
                .slide_strip
                .read(cx)
                .focus_handle()
                .contains_focused(window, cx);
        if notes_away || list_away {
            self.focus_handle.focus(window, cx);
        }
        cx.notify();
    }

    /// The list of slides, the slide with its notes under it, and the Markdown, side by side.
    pub(super) fn editor_area(
        &self,
        canvas: impl IntoElement,
        bars: Vec<AnyElement>,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let has_talk = self.document.is_some();
        let slide_and_notes = v_resizable("slide-and-notes")
            .child(
                resizable_panel().child(
                    // Room around the slide, so that it sits on the desk rather than against
                    // the tools. The bars that need an answer lie over the top of the desk, so
                    // that the slide keeps its size as they come and go.
                    div()
                        .relative()
                        .size_full()
                        .p_4()
                        .bg(cx.theme().muted)
                        .child(canvas)
                        .child(
                            v_flex()
                                .absolute()
                                .top_0()
                                .left_0()
                                .right_0()
                                .children(bars),
                        ),
                ),
            )
            .child(
                resizable_panel()
                    .size(NOTES_HEIGHT)
                    .size_range(NOTES_MIN_HEIGHT..NOTES_MAX_HEIGHT)
                    .fixed()
                    .visible(has_talk && self.show_notes)
                    .child(self.notes_panel.clone()),
            );
        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(
                h_resizable("editor-columns")
                    .child(
                        resizable_panel()
                            .size(STRIP_WIDTH)
                            .size_range(STRIP_MIN_WIDTH..STRIP_MAX_WIDTH)
                            .fixed()
                            .visible(has_talk && self.show_slide_list)
                            .child(self.slide_strip.clone()),
                    )
                    .child(resizable_panel().child(slide_and_notes))
                    .child(
                        resizable_panel()
                            .size(MARKDOWN_WIDTH)
                            .size_range(MARKDOWN_MIN_WIDTH..MARKDOWN_MAX_WIDTH)
                            .fixed()
                            .visible(self.show_markdown)
                            .child(
                                div()
                                    .id(MARKDOWN_ID)
                                    .test_support()
                                    .size_full()
                                    .child(self.markdown_panel.clone()),
                            ),
                    ),
            )
            .bg(cx.theme().background)
    }
}

/// What tells one state of the talk from another: its text, and where its history stands.
fn talk_state(
    document: &Document,
) -> (u64, bool, bool, Option<&'static str>, Option<&'static str>) {
    let mut hasher = std::hash::DefaultHasher::new();
    std::hash::Hash::hash(document.source(), &mut hasher);
    (
        std::hash::Hasher::finish(&hasher),
        document.can_undo(),
        document.can_redo(),
        document.undo_label(),
        document.redo_label(),
    )
}
