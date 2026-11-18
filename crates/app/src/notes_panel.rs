//! The current slide's notes, under the slide, to read and type into.

use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::*;

use crate::document::Document;

pub const NOTES_ID: &str = "slide-notes";
pub const NOTES_CAPTION_ID: &str = "slide-notes-caption";

/// The caption above the field.
const NOTES_CAPTION: &str = "Notes";

/// What the field says while the slide has no notes.
pub const NOTES_PLACEHOLDER: &str = "Notes for this slide";

/// How tall the panel starts, and how short and tall it can be made.
pub const NOTES_HEIGHT: Pixels = px(160.);
pub const NOTES_MIN_HEIGHT: Pixels = px(80.);
pub const NOTES_MAX_HEIGHT: Pixels = px(420.);

/// Sent when the notes typed for slide `index` are now `text`.
pub struct NotesEdited {
    pub index: usize,
    pub text: String,
}

pub struct NotesPanel {
    document: Option<Entity<Document>>,
    field: Entity<TextareaState>,
    /// The slide whose notes the field holds.
    showing: Option<usize>,
    _subscriptions: Vec<Subscription>,
}

/// Sent when the field takes the keyboard, by a press on it or otherwise: nothing on the slide
/// stays selected or typed in then.
pub struct NotesFocused;

impl EventEmitter<NotesEdited> for NotesPanel {}
impl EventEmitter<NotesFocused> for NotesPanel {}

impl NotesPanel {
    pub fn new(
        document: Option<Entity<Document>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let field = cx.new(|cx| TextareaState::new(window, cx).placeholder(NOTES_PLACEHOLDER));
        let mut subscriptions = vec![cx.subscribe(&field, |this: &mut Self, field, event, cx| {
            if matches!(event, InputEvent::Focus) {
                cx.emit(NotesFocused);
            }
            if matches!(event, InputEvent::Change)
                && let Some(index) = this.showing
            {
                let text = field.read(cx).value().to_string();
                cx.emit(NotesEdited { index, text });
            }
        })];
        if let Some(document) = &document {
            subscriptions.push(cx.observe_in(document, window, |this, _, window, cx| {
                this.follow_slide(window, cx)
            }));
        }
        let mut panel = NotesPanel {
            document,
            field,
            showing: None,
            _subscriptions: subscriptions,
        };
        panel.follow_slide(window, cx);
        panel
    }

    /// The field the notes are typed into.
    pub fn field(&self) -> &Entity<TextareaState> {
        &self.field
    }

    /// What the field holds.
    pub fn text(&self, cx: &App) -> String {
        self.field.read(cx).value().to_string()
    }

    /// Whether the field has the keyboard.
    pub fn is_focused(&self, window: &Window, cx: &App) -> bool {
        self.field.focus_handle(cx).contains_focused(window, cx)
    }

    /// Shows the current slide's notes, unless the field holds them already: the typing that
    /// made them stays as it was, with the caret where it is.
    pub fn follow_slide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(document) = &self.document else {
            return;
        };
        let document = document.read(cx);
        let current = document.current();
        let notes = document.notes_of(current).unwrap_or_default();
        let typed = self.text(cx);
        if self.showing != Some(current) || typed.trim() != notes.trim() {
            self.showing = Some(current);
            self.field
                .update(cx, |field, cx| field.set_value(notes, window, cx));
            cx.notify();
        }
    }
}

impl Render for NotesPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id(NOTES_ID)
            .test_support()
            .size_full()
            .p_2()
            .gap_1()
            .bg(cx.theme().background)
            .child(
                div()
                    .id(NOTES_CAPTION_ID)
                    .test_support()
                    .aria_label(NOTES_CAPTION)
                    .flex_none()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(NOTES_CAPTION),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    // A press on the field gives it the keyboard: what's on the slide is let go
                    // of at once, before anything the press goes on to do.
                    .capture_any_mouse_down(cx.listener(|_, _, _, cx| cx.emit(NotesFocused)))
                    .child(Textarea::new(&self.field).aria_label("Notes").size_full()),
            )
    }
}
