//! Editing the talk from the window: the slide commands the tools and keys share, moving
//! between slides, and the panels around the slide.

use super::*;
use crate::clipboard_tools;
use crate::clipboard::{self, AppData, Carried, PastedFiles, SlidePaste, TooLarge};
use crate::clipboard_tools::write_clipboard;
use crate::document::{Brought, Selected};
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

    /// Sets each attribute of `values` on the thing the inspector shows, or takes it away, as
    /// one edit, or says why it wasn't made. The inspector shows the thing picked `pick`,
    /// now at `target`: while it's still the one selected, the selection's is the one changed;
    /// once another is picked, as by the click that took the keyboard from a field, the change
    /// goes to the thing it was typed for, if that's still where it was.
    pub(crate) fn set_attr_from_inspector(
        &mut self,
        pick: u64,
        target: &Selected,
        values: &[(&str, Option<&str>)],
        cx: &mut Context<Self>,
    ) -> Result<(), EditRefused> {
        let Some(document) = self.document.clone() else {
            return Err(EditRefused::CouldNotBeMade);
        };
        let picked = document.read(cx).selection_pick() == pick;
        let mut made = Err(EditRefused::CouldNotBeMade);
        self.edit(
            |document| {
                let result = if picked {
                    document.set_selected_attrs(values)
                } else {
                    document.set_attrs_of(target, values)
                };
                made = result.as_ref().map(|_| ()).map_err(|refused| *refused);
                result
            },
            cx,
        );
        made
    }

    /// Sets the word `name` of area `region` of slide `slide` to `value`, or takes it away, from
    /// the inspector, and says whether it was set.
    pub(crate) fn set_region_value_from_inspector(
        &mut self,
        slide: usize,
        region: &str,
        name: &str,
        value: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Result<(), EditRefused> {
        let mut made = Err(EditRefused::CouldNotBeMade);
        self.edit(
            |document| {
                let result = document.set_region_value(slide, region, name, value);
                made = result.as_ref().map(|_| ()).map_err(|refused| *refused);
                result
            },
            cx,
        );
        made
    }

    /// Puts area `region` of slide `slide` back where its layout has it, from the inspector.
    pub(crate) fn put_back_region_from_inspector(&mut self, slide: usize, region: &str, cx: &mut Context<Self>) {
        self.edit(|document| document.put_back_region(slide, region), cx);
    }

    /// Whether the inspector shows: whenever a talk is open, unless it's been hidden, so the
    /// slide's place never changes as things are selected and let go of.
    pub(super) fn shows_inspector(&self) -> bool {
        self.document.is_some() && self.show_inspector
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
            // A new text box with nothing in it is dropped, which changes nothing of the talk:
            // the editor hears of it all the same.
            if let Some(document) = &self.document {
                document.update(cx, |_, cx| cx.notify());
            }
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
        if op == ClipboardOp::Paste {
            return self.paste(window, cx);
        }
        let cut = op == ClipboardOp::Cut;
        if document.read(cx).text().is_some() {
            // From a text tool that has the keyboard: it goes back to the text, as after any tool.
            self.slide_editor.update(cx, |editor, cx| {
                editor.focus_text(window, cx);
                editor.copy_or_cut(cut, cx);
            });
            return;
        }
        // The thing selected on the slide shown, unless the list of slides has the keyboard.
        let in_list = self.slide_strip.read(cx).focus_handle().contains_focused(window, cx);
        let thing_here = document.read(cx).selection().is_some_and(|selection| selection.slide == document.read(cx).current());
        if thing_here && !in_list {
            if self.copy_thing(cx) && cut {
                self.edit(Document::cut_selected, cx);
            }
            return;
        }
        if cut {
            self.cut_slide(cx);
        } else {
            self.copy_slide(cx);
        }
    }

    /// Pastes what's on the clipboard, read once. With text being typed: a thing from a slide is
    /// refused, and said so; text, the app's or plain, goes into it; or else a picture, or files,
    /// once the typing ends, onto the slide. A new text box waiting for its first letter isn't
    /// typed in yet: a thing pasted drops it. With none: a thing from a slide, on the slide shown;
    /// then the app's own slide or text, as a slide; then a picture; then files, as a drop; then
    /// plain text, as a slide.
    fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(document) = self.document.clone() else {
            return;
        };
        let item = cx.read_from_clipboard();
        let (plain, metadata) = item.as_ref().map(|item| (item.text(), item.metadata().cloned())).unwrap_or_default();
        let typing = document.read(cx).text().is_some();
        let data = clipboard::read_app_data(metadata.as_deref());
        if let AppData::Element(carried) = data {
            if typing && document.read(cx).new_box().is_none() {
                return self.say(clipboard::NOT_INTO_TEXT, cx);
            }
            self.paste_element(carried, cx);
            return self.keyboard_to_the_slide(window, cx);
        }
        let thing_first = if typing {
            !item.as_ref().is_some_and(clipboard_tools::holds_text)
        } else {
            matches!(data, AppData::Not)
        };
        if thing_first && let Some(thing) = item.and_then(clipboard_tools::picture_or_files) {
            if typing {
                self.edit(Document::end_text, cx);
            }
            self.paste_thing(thing, cx);
            return self.keyboard_to_the_slide(window, cx);
        }
        if typing {
            // From a text tool that has the keyboard: it goes back to the text, as after any tool.
            self.slide_editor.update(cx, |editor, cx| {
                editor.focus_text(window, cx);
                editor.paste_text(plain.as_deref(), metadata.as_deref(), cx);
            });
            return;
        }
        self.paste_slide(plain, data, cx);
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
        let files = document.read(cx).files_for_copy(&text);
        let Some(entry) = clipboard::slide_entry_with(&text, &files) else {
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

    /// Puts the selected thing on the clipboard, with the files it uses, and says whether it did.
    fn copy_thing(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(document) = self.document.clone() else {
            return false;
        };
        let (text, files) = match document.read(cx).copy_selected() {
            Ok(copied) => copied,
            Err(refused) => {
                self.say(refused.to_string(), cx);
                return false;
            }
        };
        let Some(entry) = clipboard::element_entry(&text, &files) else {
            self.say(clipboard::THING_TOO_BIG, cx);
            return false;
        };
        write_clipboard(entry, cx);
        true
    }

    /// Puts the thing on the clipboard on the slide shown, with the files it brings, and says
    /// what there is to say of them.
    fn paste_element(&mut self, carried: Carried, cx: &mut Context<Self>) {
        let Some(document) = self.document.clone() else {
            return;
        };
        let mut brought = None;
        // The edit's notice is what has the working copy keep the files added.
        self.edit(
            |document| {
                let (changed, said) = document.paste_element(&carried.markdown, carried.files)?;
                brought = Some(said);
                Ok(changed)
            },
            cx,
        );
        // A new text box that was waiting is gone, which the editor hears of.
        document.update(cx, |_, cx| cx.notify());
        if let Some(brought) = brought {
            self.say_brought(brought, cx);
        }
    }

    /// Says what there is to say of the files a paste brought, or that something it shows is
    /// missing, once the status line has heard what's selected, so that this is what it says.
    fn say_brought(&mut self, brought: Brought, cx: &mut Context<Self>) {
        let Some(document) = self.document.clone() else {
            return;
        };
        let notices = document.update(cx, |document, _| document.notices());
        let missing = {
            let document = document.read(cx);
            clipboard::said_of_missing(&notices, |path| document.has_file(path))
        };
        let said = brought.said().or_else(|| missing.map(str::to_string));
        if let Some(said) = said {
            cx.spawn(async move |this, cx| {
                this.update(cx, |this, cx| this.say(said, cx)).ok();
            })
            .detach();
        }
    }

    /// Pastes the slide on the clipboard after the current one, with the files it brings. If it
    /// shows a picture or a component this talk lacks, that's said, once.
    fn paste_slide(&mut self, plain: Option<String>, data: AppData, cx: &mut Context<Self>) {
        let Some(document) = self.document.clone() else {
            return;
        };
        let (paste, files) = match data {
            AppData::Slide(carried) => (Ok(clipboard::carried_slide(&carried.markdown)), carried.files),
            _ => (clipboard::slide_to_paste(plain.as_deref(), None), PastedFiles::default()),
        };
        let markdown = match paste {
            Ok(SlidePaste::Slide(markdown)) => markdown,
            Ok(SlidePaste::NoSlide) => return self.say(clipboard::NO_SLIDE, cx),
            Ok(SlidePaste::NotOneSlide) => return self.say(EditRefused::WouldChangeTheSlides.to_string(), cx),
            Err(TooLarge) => return self.say(clipboard::TOO_LARGE, cx),
        };
        let before = document.read(cx).slide_count();
        let mut brought = None;
        self.edit(
            |document| {
                let (changed, said) = document.paste_slide_with_files(&markdown, files)?;
                brought = Some(said);
                Ok(changed)
            },
            cx,
        );
        if let Some(brought) = brought.filter(|_| document.read(cx).slide_count() == before + 1) {
            self.say_brought(brought, cx);
        }
    }

    fn panels(&self) -> Panels {
        Panels {
            notes: self.show_notes,
            slide_list: self.show_slide_list,
            markdown: self.show_markdown,
            inspector: self.show_inspector,
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
        let form_away = !self.show_inspector && self.inspector.read(cx).has_any_keyboard(window, cx);
        if notes_away || list_away {
            self.focus_handle.focus(window, cx);
        } else if form_away {
            // What was typed in the form is written, once this edit to the window is done, and
            // then the keyboard goes to the slide while a thing on it is selected, or else to
            // the window.
            let (inspector, document) = (self.inspector.clone(), self.document.clone());
            let (slide, away) = (self.slide_editor.read(cx).focus_handle().clone(), self.focus_handle.clone());
            window.defer(cx, move |window, cx| {
                inspector.update(cx, |inspector, cx| inspector.commit_typing(window, cx));
                let selected_here = document.is_some_and(|document| {
                    let document = document.read(cx);
                    document.selection().is_some_and(|selection| selection.slide == document.current())
                });
                if selected_here {
                    slide.focus(window, cx);
                } else {
                    away.focus(window, cx);
                }
            });
        }
        cx.notify();
    }

    /// The list of slides, the slide with its notes under it, and the Markdown, side by side.
    /// `width` is the window's. The panels beside the slide give way as it narrows, the
    /// Markdown first, so the slide's column is never narrower than [`SLIDE_COLUMN_MIN_WIDTH`]
    /// and nothing lies under the inspector.
    pub(super) fn editor_area(
        &self,
        canvas: impl IntoElement,
        bars: Vec<AnyElement>,
        width: Pixels,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let has_talk = self.document.is_some();
        let inspector = self.shows_inspector().then(|| self.inspector.clone());
        let strip_shown = has_talk && self.show_slide_list;
        let room = width - if inspector.is_some() { INSPECTOR_WIDTH } else { px(0.) };
        let strip_now = strip_shown.then(|| self.columns.read(cx).sizes().first().copied()).flatten();
        let (strip_widest, markdown_widest) = column_limits(room, strip_shown.then(|| strip_now.unwrap_or(STRIP_WIDTH)), self.show_markdown);
        let slide_and_notes = v_resizable("slide-and-notes")
            .child(
                resizable_panel().child(
                    // Room around the slide, so that it sits on the desk rather than against
                    // the tools. The bars that need an answer lie over the top of the desk, so
                    // that the slide keeps its size as they come and go.
                    div()
                        .id(SLIDE_COLUMN_ID)
                        .test_support()
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
        h_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .items_start()
            .child(
                div().flex_1().min_w_0().h_full().child(
                h_resizable("editor-columns")
                    .with_state(&self.columns)
                    .child(
                        resizable_panel()
                            .size(STRIP_WIDTH)
                            .size_range(STRIP_MIN_WIDTH..strip_widest)
                            .fixed()
                            .visible(has_talk && self.show_slide_list)
                            .child(self.slide_strip.clone()),
                    )
                    .child(resizable_panel().size_range(SLIDE_COLUMN_MIN_WIDTH..Pixels::MAX).child(slide_and_notes))
                    .child(
                        resizable_panel()
                            .size(MARKDOWN_WIDTH)
                            .size_range(MARKDOWN_MIN_WIDTH..markdown_widest)
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

/// How wide the slide list and the Markdown can be, `(list, markdown)`, beside a slide's column
/// of at least [`SLIDE_COLUMN_MIN_WIDTH`], in `room` across: the list, if it's shown, is
/// `strip` wide now. The Markdown gives way first, down to its narrowest, then the list.
pub(super) fn column_limits(room: Pixels, strip: Option<Pixels>, markdown: bool) -> (Pixels, Pixels) {
    let markdown_least = if markdown { MARKDOWN_MIN_WIDTH } else { px(0.) };
    let strip_widest = (room - SLIDE_COLUMN_MIN_WIDTH - markdown_least).clamp(STRIP_MIN_WIDTH, STRIP_MAX_WIDTH);
    let strip_now = strip.map_or(px(0.), |strip| strip.min(strip_widest));
    let markdown_widest = (room - SLIDE_COLUMN_MIN_WIDTH - strip_now).clamp(MARKDOWN_MIN_WIDTH, MARKDOWN_MAX_WIDTH);
    (strip_widest, markdown_widest)
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

#[cfg(test)]
mod tests {
    use gpui_kit::px;

    use super::{MARKDOWN_MAX_WIDTH, column_limits};
    use crate::slide_strip::{STRIP_MAX_WIDTH, STRIP_WIDTH};

    #[test]
    fn the_markdown_gives_way_first_then_the_list() {
        // 1024 across, less the inspector's 300.
        let room = px(724.);
        assert_eq!(column_limits(room, Some(STRIP_WIDTH), true), (px(244.), px(264.)));
        assert_eq!(column_limits(room, Some(STRIP_MAX_WIDTH), true), (px(244.), px(240.)), "a widened list gives way too");
        assert_eq!(column_limits(room, None, true).1, px(484.).min(MARKDOWN_MAX_WIDTH));
        assert_eq!(column_limits(room, Some(STRIP_WIDTH), false).0, STRIP_MAX_WIDTH);
        assert_eq!(column_limits(px(2000.), Some(STRIP_WIDTH), true), (STRIP_MAX_WIDTH, MARKDOWN_MAX_WIDTH));
    }
}
