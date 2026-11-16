//! The Clipboard group on the Home tab, Paste, Cut and Copy, and the window's side of the
//! system clipboard. The tools act on what has the keyboard: the text being typed, or else
//! the current slide. Using one leaves the keyboard where it was. What goes on the clipboard,
//! and what's taken from it, is [`crate::clipboard`]'s.

use std::path::PathBuf;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::assets::IconName;
use gpui_kit::component::{Disableable as _, Sizable as _};
use gpui_kit::*;

use crate::clipboard::Entry;
use crate::document::Document;
use crate::ribbon::group;
use crate::slide_editor::{EDITOR_CONTEXT, SlideEditor};
use crate::slide_strip::STRIP_CONTEXT;
use crate::workspace_view::{CopyToClipboard, CutToClipboard, PasteFromClipboard};

pub const CLIPBOARD_TOOLS_ID: &str = "clipboard-tools";
pub const PASTE_ID: &str = "paste";
pub const CUT_ID: &str = "cut";
pub const COPY_ID: &str = "copy";

/// Puts `entry` on the system clipboard: its text, with the app's metadata if it has any.
pub(crate) fn write_clipboard(entry: Entry, cx: &mut App) {
    let item = match entry.metadata {
        Some(metadata) => ClipboardItem::new_string_with_metadata(entry.text, metadata),
        None => ClipboardItem::new_string(entry.text),
    };
    cx.write_to_clipboard(item);
}

/// What the system clipboard holds: its text, and its metadata, if it's one piece of text
/// that has some.
pub(crate) fn read_clipboard(cx: &App) -> (Option<String>, Option<String>) {
    cx.read_from_clipboard()
        .map(|item| (item.text(), item.metadata().cloned()))
        .unwrap_or_default()
}

/// What on the clipboard can be put on the slide, besides text.
pub(crate) enum ClipboardThing {
    Picture(Image),
    Paths(Vec<PathBuf>),
}

/// The picture on the clipboard `item`, or else the files it names, if it holds either.
pub(crate) fn picture_or_files(item: &ClipboardItem) -> Option<ClipboardThing> {
    let entries = item.entries();
    let picture = entries.iter().find_map(|entry| match entry {
        ClipboardEntry::Image(image) => Some(ClipboardThing::Picture(image.clone())),
        _ => None,
    });
    picture.or_else(|| {
        entries.iter().find_map(|entry| match entry {
            ClipboardEntry::ExternalPaths(paths) if !paths.paths().is_empty() => {
                Some(ClipboardThing::Paths(paths.paths().to_vec()))
            }
            _ => None,
        })
    })
}

/// Whether the clipboard `item` holds text.
pub(crate) fn holds_text(item: &ClipboardItem) -> bool {
    item.entries().iter().any(|entry| matches!(entry, ClipboardEntry::String(_)))
}

/// What the tools last saw of the text being typed: whether there is any, and whether any of
/// it is selected.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Seen {
    typing: bool,
    selected: bool,
}

pub struct ClipboardTools {
    document: Option<Entity<Document>>,
    seen: Seen,
    /// The notes field's keyboard: while it has it, the tools act on its text.
    notes: Option<FocusHandle>,
    _subscriptions: Vec<Subscription>,
}

impl ClipboardTools {
    /// The tools for the talk `document`, whose text `editor` types.
    pub fn new(
        document: Option<Entity<Document>>,
        editor: &Entity<SlideEditor>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut subscriptions = vec![
            // The selection moves with the editor, which draws again as its caret blinks: that
            // changes nothing here.
            cx.observe(editor, |this, _, cx| {
                let seen = this.seen(cx);
                if seen != this.seen {
                    this.seen = seen;
                    cx.notify();
                }
            }),
        ];
        if let Some(document) = &document {
            subscriptions.push(cx.observe(document, |_, _, cx| cx.notify()));
        }
        ClipboardTools {
            document,
            seen: Seen::default(),
            notes: None,
            _subscriptions: subscriptions,
        }
    }

    /// Has the tools act on the notes field's text while `notes`, its keyboard, is with it, and
    /// show so as it comes and goes.
    pub fn follow_notes(&mut self, notes: FocusHandle, window: &mut Window, cx: &mut Context<Self>) {
        self._subscriptions.push(cx.on_focus_in(&notes, window, |_, _, cx| cx.notify()));
        self._subscriptions.push(cx.on_focus_out(&notes, window, |_, _, _, cx| cx.notify()));
        self.notes = Some(notes);
    }

    fn seen(&self, cx: &App) -> Seen {
        let editing = self.document.as_ref().and_then(|document| document.read(cx).text());
        Seen {
            typing: editing.is_some(),
            selected: editing.is_some_and(|editing| !editing.edit.selection.is_caret()),
        }
    }
}

/// A tool, `id` with `icon`, named `label`, which it shows if `shows_name`, that does what
/// `action`'s key does, there.
fn tool(
    (id, icon, label, shows_name): (&'static str, IconName, &'static str, bool),
    tip: &'static str,
    action: Box<dyn Action>,
    context: &'static str,
    enabled: bool,
) -> AnyElement {
    let dispatched = action.boxed_clone();
    let button = Button::new(id).ghost().small().icon(icon);
    let button = if shows_name { button.label(label) } else { button.accessibility_label(label) };
    button
        .tooltip_with_action(tip, action.as_ref(), Some(context))
        .disabled(!enabled)
        .on_click(move |_, window, cx| window.dispatch_action(dispatched.boxed_clone(), cx))
        .into_any_element()
}

impl Render for ClipboardTools {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let seen = self.seen(cx);
        self.seen = seen;
        let slides = self.document.as_ref().map(|document| document.read(cx).slide_count());
        let has_talk = slides.is_some();
        // Only Paste shows its name, as there's room on the tab for no more.
        // With text being typed, they're for its selection; Cut and Copy need some selected.
        // Otherwise they're for the current slide, which can't be cut if it's the only one.
        // In the notes field, they do what its own keys do, to its selection.
        let in_notes = self.notes.as_ref().is_some_and(|notes| notes.contains_focused(window, cx));
        let (copy, cut, context) = if in_notes {
            (has_talk, has_talk, STRIP_CONTEXT)
        } else if seen.typing {
            (seen.selected, seen.selected, EDITOR_CONTEXT)
        } else {
            (has_talk, slides.is_some_and(|count| count > 1), STRIP_CONTEXT)
        };
        div().id(CLIPBOARD_TOOLS_ID).test_support().h_full().child(group(
            "Clipboard",
            vec![
                tool(
                    (PASTE_ID, IconName::ClipboardPaste, "Paste", true),
                    "Paste",
                    Box::new(PasteFromClipboard),
                    context,
                    has_talk,
                ),
                tool(
                    (CUT_ID, IconName::Scissors, "Cut", false),
                    "Cut",
                    Box::new(CutToClipboard),
                    context,
                    cut,
                ),
                tool(
                    (COPY_ID, IconName::Copy, "Copy", false),
                    "Copy",
                    Box::new(CopyToClipboard),
                    context,
                    copy,
                ),
            ],
            cx,
        ))
    }
}
