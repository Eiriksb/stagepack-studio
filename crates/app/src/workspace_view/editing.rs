//! Editing the talk from the window: the slide commands the tools and keys share, moving
//! between slides, and the panels around the slide.

use super::*;
use crate::clipboard_tools;
use crate::clipboard::{self, SlidePaste, TooLarge};
use crate::clipboard_tools::{read_clipboard, write_clipboard};
use crate::document::Selected;
use crate::inspector::INSPECTOR_WIDTH;

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
        // What's typed on the slide is in the talk; only what typing left empty is tidied.
        if document.read(cx).text().is_some() {
            self.edit(Document::end_text, cx);
        }
        let before = document.read(cx).current();
        let message = document.update(cx, |document, _| {
            step(document);
            document.take_message()
        });
        if let Some(message) = message {
            self.say(message, cx);
        }
        if document.read(cx).current() == before {
            return;
        }
        self.notes_panel
            .update(cx, |panel, cx| panel.follow_slide(window, cx));
        self.slide_editor
            .update(cx, |editor, cx| editor.follow_slide(window, cx));
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
        let (made, changed, message) = document.update(cx, |document, cx| {
            let before = talk_state(document);
            let made = change(document);
            // Whether the talk changed, as its text or its history did: what the edit says
            // to draw again is about pictures, and can be empty for a change all the same.
            let changed = talk_state(document) != before;
            if changed {
                cx.notify();
            }
            (made, changed, document.take_message())
        });
        if changed {
            self.edited(cx);
        }
        if let Err(refused) = made {
            self.say(refused.to_string(), cx);
        }
        if let Some(message) = message {
            self.say(message, cx);
        }
    }

    /// Sets attribute `name` of the thing the inspector shows to `value`, or takes it away, as
    /// one edit, and says whether that was done. The inspector shows the thing picked `pick`,
    /// now at `target`: while it's still the one selected, the selection's is the one changed;
    /// once another is picked, as by the click that took the keyboard from a field, the change
    /// goes to the thing it was typed for, if that's still where it was.
    pub(crate) fn set_attr_from_inspector(
        &mut self,
        pick: u64,
        target: &Selected,
        name: &str,
        value: Option<&str>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(document) = self.document.clone() else {
            return false;
        };
        let picked = document.read(cx).selection_pick() == pick;
        let mut made = false;
        self.edit(
            |document| {
                let result = if picked {
                    document.set_selected_attr(name, value)
                } else {
                    document.set_attr_of(target, name, value)
                };
                made = result.is_ok();
                result
            },
            cx,
        );
        made
    }

    /// Whether the inspector shows: while a thing on the slide shown is selected.
    pub(super) fn shows_inspector(&self, cx: &App) -> bool {
        self.document.as_ref().is_some_and(|document| {
            let document = document.read(cx);
            document.selected().is_some_and(|selected| selected.slide == document.current())
        })
    }

    /// Leaves the talk tidy of what typing on the slide left empty, before it's saved, shown
    /// or exported. Typing goes on where it can.
    pub(super) fn settle_typing(&mut self, cx: &mut Context<Self>) {
        let typing = self.document.as_ref().and_then(|document| {
            document
                .read(cx)
                .text()
                .map(|text| text.composing.is_some())
        });
        if let Some(composing) = typing {
            self.edit(Document::settle_text, cx);
            // What the system was composing went with it, and isn't shown any more.
            if composing {
                self.slide_editor.update(cx, |_, cx| cx.notify());
            }
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
    pub(super) fn edited(&mut self, cx: &mut Context<Self>) {
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

    /// Copies, cuts or pastes what has the keyboard: the text being typed, or else the current
    /// slide. The notes field has its own.
    pub(super) fn clipboard(&mut self, op: ClipboardOp, window: &mut Window, cx: &mut Context<Self>) {
        let Some(document) = self.document.clone() else {
            return;
        };
        if self.notes_panel.read(cx).is_focused(window, cx) {
            // The notes field has the keyboard: the tool does what its own keys do there.
            let action: Box<dyn Action> = match op {
                ClipboardOp::Copy => Box::new(input::Copy),
                ClipboardOp::Cut => Box::new(input::Cut),
                ClipboardOp::Paste => Box::new(input::Paste),
            };
            window.dispatch_action(action, cx);
            return;
        }
        if document.read(cx).text().is_some() {
            // A picture, or files, with no text on the clipboard: the typing ends, and it goes on
            // the slide.
            let item = cx.read_from_clipboard();
            let thing = item
                .as_ref()
                .filter(|item| op == ClipboardOp::Paste && !clipboard_tools::holds_text(item))
                .and_then(clipboard_tools::picture_or_files);
            if let Some(thing) = thing {
                self.edit(Document::end_text, cx);
                self.paste_thing(thing, cx);
                return;
            }
            // From a text tool that has the keyboard: it goes back to the text, as after any tool.
            self.slide_editor.update(cx, |editor, cx| {
                editor.focus_text(window, cx);
                editor.clipboard(op, cx);
            });
            return;
        }
        match op {
            ClipboardOp::Copy => {
                self.copy_slide(cx);
            }
            ClipboardOp::Cut => self.cut_slide(cx),
            ClipboardOp::Paste => self.paste_slide(cx),
        }
    }

    /// Puts the current slide's own text on the clipboard, and says whether it did.
    fn copy_slide(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(document) = self.document.clone() else {
            return false;
        };
        let Some(text) = document.read(cx).current_for_copy() else {
            self.say(EditRefused::CouldNotBeMade.to_string(), cx);
            return false;
        };
        let Some(entry) = clipboard::slide_entry(&text) else {
            self.say(clipboard::SLIDE_TOO_BIG, cx);
            return false;
        };
        write_clipboard(entry, cx);
        true
    }

    /// Copies the current slide and takes it out, unless it's the only one, or it couldn't be
    /// copied.
    fn cut_slide(&mut self, cx: &mut Context<Self>) {
        let several = self
            .document
            .as_ref()
            .is_some_and(|document| document.read(cx).slide_count() > 1);
        if several && self.copy_slide(cx) {
            self.edit(Document::cut_current, cx);
        }
    }

    /// Pastes the slide on the clipboard after the current one. If it shows a picture or a
    /// component this talk lacks, that's said, once.
    fn paste_slide(&mut self, cx: &mut Context<Self>) {
        let Some(document) = self.document.clone() else {
            return;
        };
        let (plain, metadata) = read_clipboard(cx);
        // The app's own slide or text, as before; then a picture, then files, as a drop; then
        // plain text, as a slide.
        let app_data = metadata.as_deref().is_some_and(clipboard::is_app_data);
        if !app_data
            && let Some(thing) = cx.read_from_clipboard().as_ref().and_then(clipboard_tools::picture_or_files)
        {
            return self.paste_thing(thing, cx);
        }
        let markdown = match clipboard::slide_to_paste(plain.as_deref(), metadata.as_deref()) {
            Ok(SlidePaste::Slide(markdown)) => markdown,
            Ok(SlidePaste::NoSlide) => return self.say(clipboard::NO_SLIDE, cx),
            Ok(SlidePaste::NotOneSlide) => return self.say(EditRefused::WouldChangeTheSlides.to_string(), cx),
            Err(TooLarge) => return self.say(clipboard::TOO_LARGE, cx),
        };
        let before = document.read(cx).slide_count();
        self.edit(|document| document.paste_slide(&markdown), cx);
        if document.read(cx).slide_count() == before + 1 {
            let notices = document.update(cx, |document, _| document.notices());
            let document = document.read(cx);
            if clipboard::shows_something_missing(&notices, |path| document.has_file(path)) {
                self.say(clipboard::MISSING_PICTURES, cx);
            }
        }
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
                        // A click on the desk ends typing on the slide.
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, window, cx| {
                                this.slide_editor
                                    .update(cx, |editor, cx| editor.stop_typing(window, cx))
                            }),
                        )
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
        let inspector = self.shows_inspector(cx).then(|| self.inspector.clone());
        h_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .items_start()
            .child(
                div().flex_1().min_w_0().h_full().child(
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
                ),
            )
            // The slide makes room for it.
            .when_some(inspector, |this, inspector| {
                this.child(div().flex_none().w(INSPECTOR_WIDTH).h_full().child(inspector))
            })
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
