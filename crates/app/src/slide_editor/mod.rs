//! The slide in the editor, as it's typed on: its layers, with the caret and the selection
//! over them, taking the mouse and the keys. What's typed goes straight into the talk, through
//! [`Document::apply_text`]; this only says what to do and shows where it happened.
//!
//! While text is being typed the editor has the keyboard, in a key context of its own, so the
//! keys that change slides or edit the list of slides don't reach the window.
//!
//! What's typed comes from the system's input method, through [`EntityInputHandler`]: a letter,
//! a dead key's accented letter, a composed character or an emoji from the picker all arrive
//! there, and nothing else types. The keys themselves only move the caret and edit.

use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use slides_engine::{Align, CaretRect, ElementBox, Hit, LineMove, Rect, SelectionRects, TextLayout};
use slides_format::{Block, FlowPlace, Motion, Para, ParaKind, Pos, Selection, Span, StyleBit, TextEdit, uneditable_reason};

use crate::arrange::Handle;
use crate::clipboard::{self, TextPaste};
use crate::clipboard_tools::write_clipboard;
use crate::document::{Beside, CellMove, Document, EditRefused};
use crate::selection::{self, Thing};
use crate::slide_canvas::{SLIDE_WIDTH, SlideCanvas, letterbox};
use crate::insert::TEXT_BOX_SIZE;
use crate::text_editing::{Composing, EditingText, clean_typed, one_line, utf16_slice};
use crate::text_tools::{
    SetTextSize, ToggleBold, ToggleBullets, ToggleItalic, ToggleNumbering, ToggleUnderline,
};
use crate::workspace_view::{
    CARET_BLINKS_FOR, CopyToClipboard, CutToClipboard, PasteFromClipboard,
};
mod arranging;
mod ime;
mod keys;
mod marks;
mod mouse;
mod render;
mod selecting;
mod typing;

pub use arranging::Nudge;
pub use keys::*;
use marks::{caret_box, caret_corners, corners, corners_box, guide_bounds, outline_corners, paint_dashed, paint_outline, paint_polygon, turned};

pub const SLIDE_EDITOR_ID: &str = "slide-editor";
pub const CARET_ID: &str = "caret";
/// What the system's input method is composing, shown at the caret.
pub const COMPOSITION_ID: &str = "composition";
/// The dashed line around a new text box that isn't written yet.
pub const NEW_BOX_ID: &str = "new-text-box";
/// The outline around the thing selected on the slide, or of several, the one picked last.
pub const SELECTION_OUTLINE_ID: &str = "selection-outline";

/// The outline around the `n`th thing picked of several selected, but the one picked last.
pub fn selection_outline_id(n: usize) -> String {
    format!("selection-outline-{n}")
}

/// The handle `handle` on the one thing or area selected.
pub fn handle_id(handle: Handle) -> String {
    let name = match handle {
        Handle::TopLeft => "top-left",
        Handle::Top => "top",
        Handle::TopRight => "top-right",
        Handle::Right => "right",
        Handle::BottomRight => "bottom-right",
        Handle::Bottom => "bottom",
        Handle::BottomLeft => "bottom-left",
        Handle::Left => "left",
        Handle::Turn => "turn",
    };
    format!("handle-{name}")
}

/// The dashed outline of a thing being dragged out of the slide's text, where it'll go.
pub const LIFT_OUTLINE_ID: &str = "lift-outline";

/// The guide line drawn while a thing dragged snaps to a line, or of several, the first.
pub const SNAP_GUIDE_ID: &str = "snap-guide";

/// The `n`th of several guide lines, but the first.
pub fn snap_guide_id(n: usize) -> String {
    format!("snap-guide-{n}")
}

/// The dashed frame around the area `name` of the slide's layout, while its text is typed.
pub fn region_frame_id(name: &str) -> String {
    format!("region-frame-{name}")
}

/// The badge at the top-left corner of the `n`th thing or area animated, in the order the
/// document gives them, shown while the Animate tab is open.
pub fn step_badge_id(n: usize) -> String {
    format!("step-badge-{n}")
}

/// The key context the editor's keys apply in, while text is being typed.
pub const EDITOR_CONTEXT: &str = "SlideEditor";
/// The key context the editor's keys apply in while a thing on the slide is selected and no
/// text is being typed.
pub const SELECTION_CONTEXT: &str = "SlideSelection";

/// How long the caret shows, and then doesn't, as it blinks.
const BLINK: Duration = Duration::from_millis(500);
/// How long typing can stop for and still be one step to undo.
const TYPING_PAUSE: Duration = Duration::from_secs(2);
/// How wide the caret is drawn, in the window's pixels.
const CARET_WIDTH: f32 = 2.0;
/// How wide the outline around the thing selected is drawn, in the window's pixels.
const OUTLINE_WIDTH: f32 = 2.0;
/// How near the frame of the area being typed in, in the window's pixels, a press picks the area.
const FRAME_REACH: f32 = 5.0;
/// How wide a handle is drawn, in the window's pixels.
const HANDLE_SIZE: f32 = 9.0;
/// How near a handle's middle, in the window's pixels, a press takes it.
const HANDLE_REACH: f32 = 6.0;
/// How far above a thing's top edge the turn handle is, in the window's pixels.
const TURN_GAP: f32 = 24.0;
/// How far apart, in the window's pixels, a thing's corner handles are at least drawn.
const HANDLES_APART: f32 = 24.0;
/// How near, in the window's pixels, a thing dragged snaps to a line of the slide or of
/// another thing.
pub const SNAP_PIXELS: f32 = 6.0;
/// How wide a guide line is drawn, in the window's pixels.
const GUIDE_WIDTH: f32 = 1.0;
/// How big the text being composed is drawn, to the height of the caret.
const COMPOSITION_TEXT: f32 = 0.7;

/// What the editor tells the window.
pub enum EditorEvent {
    /// The talk's text changed.
    Edited,
    /// Something to put on the status line.
    Said(SharedString),
    /// What was put on the status line no longer holds, and goes if it's still there.
    Unsaid(SharedString),
}

/// What a right-click opened its menu on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MenuOn {
    /// A thing, or an area, which is selected.
    Thing,
    /// The empty slide, or the desk around it.
    Slide,
}

/// Where the caret goes.
#[derive(Clone, Copy)]
enum Go {
    By(Motion),
    Line(LineMove),
}

pub struct SlideEditor {
    document: Option<Entity<Document>>,
    canvas: Entity<SlideCanvas>,
    focus_handle: FocusHandle,
    /// Where the keyboard goes once typing ends: the window's own.
    away: FocusHandle,
    /// The keyboard of the list of slides, which the editor takes the keyboard from once a thing
    /// on the slide shown is selected while the list has it.
    panels: Vec<FocusHandle>,
    /// The keyboard of the notes, which the editor never takes: while the notes have it, what's
    /// selected on the slide is let go of instead.
    notes: Option<FocusHandle>,
    /// What holds the text tools: the keyboard can go there and typing goes on.
    tools: FocusHandle,
    /// What else the keyboard can go to while typing goes on, as the icon picker's search.
    more_tools: Vec<FocusHandle>,
    /// Whether the caret shows, as it blinks.
    caret_on: bool,
    /// How many more times it blinks before it holds still.
    blinks_left: u32,
    /// Whether it's blinking, and so has a timer running.
    blinking: bool,
    blink: Option<Task<()>>,
    /// The wait after typing, once over which the next typing is a step of its own.
    pause: Option<Task<()>>,
    /// Where across the caret keeps to as it goes up and down, in slide units.
    wanted_x: Option<f32>,
    /// Whether a press on the text is being dragged, selecting.
    dragging: bool,
    /// One of several things selected that a press landed on, which is selected alone once the
    /// button is let go of.
    pressed: Option<(usize, Thing)>,
    /// What the status line was told of the thing selected on the slide shown, if anything is.
    told: Option<SharedString>,
    /// A press on things selected, readied to drag them, or dragging them.
    drag: Option<arranging::Drag>,
    /// The run of arrow-key moves going on, if one is.
    nudging: Option<arranging::Nudging>,
    /// What the pointer shows, over a handle or dragging one, if not the usual.
    pointer: Option<CursorStyle>,
    /// The handles on the thing selected, as last worked out.
    handles_seen: Option<arranging::HandlesSeen>,
    /// What the menu a right-click opens is for, once one has.
    menu_on: Option<MenuOn>,
    /// Whether the step each thing animated comes in on is shown, as it is on the Animate tab.
    show_steps: bool,

    /// Where the caret and any composition were last drawn, so that the system's input method
    /// is told when they move, and only then.
    ime_place: Option<ImePlace>,
    _subscriptions: Vec<Subscription>,
}

/// What the system's input method places its window by: the selection, and where the caret
/// and the composition are drawn.
type ImePlace = (
    Selection,
    Option<Bounds<Pixels>>,
    Option<(String, Bounds<Pixels>)>,
);

impl EventEmitter<EditorEvent> for SlideEditor {}

impl SlideEditor {
    /// An editor over `canvas`, typing into `document`. The keyboard goes back to `away`
    /// once typing ends. The keyboard can go to what `tools` holds, the text tools, while
    /// typing goes on.
    pub fn new(
        document: Option<Entity<Document>>,
        canvas: Entity<SlideCanvas>,
        away: FocusHandle,
        tools: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        let mut subscriptions = vec![
            cx.on_focus_out(&focus_handle, window, |this, _, window, cx| {
                // A drag ends, writing nothing.
                this.end_drag(cx);
                // The keyboard went elsewhere: what was typed is in the talk, and typing ends,
                // unless it went to the text tools.
                if !this.tools_have_keyboard(window, cx) {
                    this.end_editing(false, window, cx)
                }
            }),
            cx.on_focus_out(&tools, window, |this, _, window, cx| {
                // From the text tools the keyboard went on elsewhere than the text.
                if !this.focus_handle.contains_focused(window, cx)
                    && !this.tools_have_keyboard(window, cx)
                {
                    this.end_editing(false, window, cx)
                }
            }),
            // The window losing the keyboard ends a drag, writing nothing.
            cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() {
                    this.end_drag(cx);
                }
            }),
        ];
        if let Some(document) = &document {
            subscriptions.push(cx.observe_in(document, window, |this, _, window, cx| {
                this.document_changed(window, cx)
            }));
        }
        // A shortcut pressed while the system composes ends the composition, before anything
        // the shortcut does: a platform that drops its own composition then, as Linux does on
        // Ctrl and a letter, doesn't say so, and what it was composing mustn't stay shown.
        let editor = cx.entity().downgrade();
        let focus = focus_handle.clone();
        subscriptions.push(cx.intercept_keystrokes(move |event, window, cx| {
            if !focus.is_focused(window) || !is_shortcut(&event.keystroke) {
                return;
            }
            editor
                .update(cx, |editor, cx| {
                    let composing = editor
                        .editing(cx)
                        .is_some_and(|editing| editing.composing.is_some());
                    if composing {
                        editor.set_composing(None, cx);
                    }
                })
                .ok();
        }));
        SlideEditor {
            document,
            canvas,
            focus_handle,
            away,
            panels: Vec::new(),
            notes: None,
            tools,
            more_tools: Vec::new(),
            caret_on: true,
            blinks_left: 0,
            blinking: false,
            blink: None,
            pause: None,
            wanted_x: None,
            dragging: false,
            pressed: None,
            told: None,
            drag: None,
            nudging: None,
            pointer: None,
            handles_seen: None,
            menu_on: None,
            show_steps: false,
            ime_place: None,
            _subscriptions: subscriptions,
        }
    }

    /// What the pointer shows over the slide, if not the usual: which way a handle under it
    /// resizes, or that it turns.
    pub fn pointer(&self) -> Option<CursorStyle> {
        self.pointer
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    /// Shows the step each thing and area animated comes in on, with `show`, or stops.
    pub fn show_steps(&mut self, show: bool, cx: &mut Context<Self>) {
        if self.show_steps != show {
            self.show_steps = show;
            cx.notify();
        }
    }

    /// Lets what holds `tools` have the keyboard while typing goes on, as the text tools can:
    /// typing ends once the keyboard goes on from there to anywhere but the slide.
    pub fn keep_typing_with(&mut self, tools: FocusHandle, window: &mut Window, cx: &mut Context<Self>) {
        let subscription = cx.on_focus_out(&tools, window, |this, _, window, cx| {
            if !this.focus_handle.contains_focused(window, cx) && !this.tools_have_keyboard(window, cx) {
                this.end_editing(false, window, cx)
            }
        });
        self._subscriptions.push(subscription);
        self.more_tools.push(tools);
    }

    /// Has the editor take the keyboard from a panel, as the list of slides, which `panel` is
    /// the keyboard of, once a thing on the slide shown is selected while it has it.
    pub fn take_keyboard_from(&mut self, panel: FocusHandle) {
        self.panels.push(panel);
    }

    /// Has the editor let go of what's selected on the slide whenever it hears of the talk while
    /// the notes, which `notes` is the keyboard of, have the keyboard: however they got it, a
    /// letter typed there never has the slide take the keyboard back, nor Delete act on a thing.
    pub fn give_way_to_notes(&mut self, notes: FocusHandle) {
        self.notes = Some(notes);
    }

    /// Whether the text tools, or something else that can have the keyboard while typing goes
    /// on, has it.
    fn tools_have_keyboard(&self, window: &Window, cx: &App) -> bool {
        self.tools.contains_focused(window, cx) || self.more_tools.iter().any(|tools| tools.contains_focused(window, cx))
    }

    fn is_editing(&self, cx: &App) -> bool {
        self.document
            .as_ref()
            .is_some_and(|document| document.read(cx).text().is_some())
    }

    /// Follows the talk: once text is being typed, as after an undo of typing, the editor has
    /// the keyboard, and once it isn't, it gives the keyboard back and stops its timers. A thing
    /// selected on the slide shown keeps the keyboard, and takes it from the window, though not
    /// from a panel that has it.
    fn document_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Any edit but the drag's own ends it.
        self.follow_drag(cx);
        let focused = self.focus_handle.is_focused(window);
        if self.is_editing(cx) {
            // A text tool keeps the keyboard until it's done with it.
            if !focused && !self.tools_have_keyboard(window, cx) {
                self.focus_handle.focus(window, cx);
                self.wake(cx);
            }
        } else {
            self.stop(cx);
            let notes_have_it = self.notes.as_ref().is_some_and(|notes| notes.contains_focused(window, cx));
            if notes_have_it {
                self.deselect(cx);
            } else if self.selection_here(cx).is_some() {
                // From the list of slides too, as by an Undo pressed there: a thing shown as
                // selected while the list has the keyboard would have Delete take its slide.
                let panel_has_it = self.panels.iter().any(|panel| panel.contains_focused(window, cx));
                if !focused && (self.away.is_focused(window) || panel_has_it) {
                    self.focus_handle.focus(window, cx);
                }
            } else if focused {
                self.away.focus(window, cx);
            }
        }
        self.tell_selection(cx);
        cx.notify();
    }

    /// Follows another slide being shown: a thing selected on the one left isn't shown, and
    /// one selected on the one come to is.
    pub fn follow_slide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.document_changed(window, cx);
    }
}

/// Whether `paras` are all list items: text that stays at the left, however it's aligned.
pub(crate) fn only_lists(paras: &[Para]) -> bool {
    !paras.is_empty()
        && paras
            .iter()
            .all(|para| matches!(para.kind, ParaKind::Bullet { .. } | ParaKind::Numbered { .. }))
}
