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

use crate::clipboard::{self, TextPaste};
use crate::clipboard_tools::write_clipboard;
use crate::document::{Beside, CellMove, Document, EditRefused};
use crate::selection::{self, Selected};
use crate::slide_canvas::{SLIDE_WIDTH, SlideCanvas, letterbox};
use crate::insert::TEXT_BOX_SIZE;
use crate::text_editing::{Composing, EditingText, clean_typed, one_line, utf16_slice};
use crate::text_tools::{
    SetTextSize, ToggleBold, ToggleBullets, ToggleItalic, ToggleNumbering, ToggleUnderline,
};
use crate::workspace_view::{
    CARET_BLINKS_FOR, CopyToClipboard, CutToClipboard, PasteFromClipboard,
};

pub const SLIDE_EDITOR_ID: &str = "slide-editor";
pub const CARET_ID: &str = "caret";
/// What the system's input method is composing, shown at the caret.
pub const COMPOSITION_ID: &str = "composition";
/// The dashed line around a new text box that isn't written yet.
pub const NEW_BOX_ID: &str = "new-text-box";
/// The outline around the thing selected on the slide.
pub const SELECTION_OUTLINE_ID: &str = "selection-outline";

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
/// How big the text being composed is drawn, to the height of the caret.
const COMPOSITION_TEXT: f32 = 0.7;

actions!(
    slide_editor,
    [
        MoveLeft,
        MoveRight,
        MoveUp,
        MoveDown,
        MoveWordLeft,
        MoveWordRight,
        MoveLineStart,
        MoveLineEnd,
        MoveFlowStart,
        MoveFlowEnd,
        SelectLeft,
        SelectRight,
        SelectUp,
        SelectDown,
        SelectWordLeft,
        SelectWordRight,
        SelectLineStart,
        SelectLineEnd,
        SelectFlowStart,
        SelectFlowEnd,
        SelectAll,
        Backspace,
        Delete,
        Enter,
        LineBreak,
        Indent,
        Outdent,
        StopEditing,
        TakeAwaySelected,
        SelectNextThing,
        SelectPreviousThing,
        LetGoOfSelected,
        DuplicateSelected,
    ]
);

/// The editor's keys. The window's keys that change slides or the list of slides are held
/// back in the editor, as the keys that move the caret are its own.
pub fn bind_keys(cx: &mut App) {
    let context = Some(EDITOR_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("left", MoveLeft, context),
        KeyBinding::new("right", MoveRight, context),
        KeyBinding::new("up", MoveUp, context),
        KeyBinding::new("down", MoveDown, context),
        KeyBinding::new("ctrl-left", MoveWordLeft, context),
        KeyBinding::new("ctrl-right", MoveWordRight, context),
        KeyBinding::new("home", MoveLineStart, context),
        KeyBinding::new("end", MoveLineEnd, context),
        KeyBinding::new("ctrl-home", MoveFlowStart, context),
        KeyBinding::new("ctrl-end", MoveFlowEnd, context),
        KeyBinding::new("shift-left", SelectLeft, context),
        KeyBinding::new("shift-right", SelectRight, context),
        KeyBinding::new("shift-up", SelectUp, context),
        KeyBinding::new("shift-down", SelectDown, context),
        KeyBinding::new("ctrl-shift-left", SelectWordLeft, context),
        KeyBinding::new("ctrl-shift-right", SelectWordRight, context),
        KeyBinding::new("shift-home", SelectLineStart, context),
        KeyBinding::new("shift-end", SelectLineEnd, context),
        KeyBinding::new("ctrl-shift-home", SelectFlowStart, context),
        KeyBinding::new("ctrl-shift-end", SelectFlowEnd, context),
        KeyBinding::new("ctrl-a", SelectAll, context),
        KeyBinding::new("backspace", Backspace, context),
        KeyBinding::new("shift-backspace", Backspace, context),
        KeyBinding::new("delete", Delete, context),
        KeyBinding::new("enter", Enter, context),
        KeyBinding::new("shift-enter", LineBreak, context),
        KeyBinding::new("tab", Indent, context),
        KeyBinding::new("shift-tab", Outdent, context),
        KeyBinding::new("escape", StopEditing, context),
        KeyBinding::new("pagedown", NoAction, context),
        KeyBinding::new("pageup", NoAction, context),
        KeyBinding::new("ctrl-m", NoAction, context),
        KeyBinding::new("ctrl-d", NoAction, context),
        KeyBinding::new("ctrl-c", CopyToClipboard, context),
        KeyBinding::new("ctrl-x", CutToClipboard, context),
        // The window's: it reads the clipboard, and pastes text here or puts a picture on the slide.
        KeyBinding::new("ctrl-v", PasteFromClipboard, context),
    ]);
    let selecting = Some(SELECTION_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("delete", TakeAwaySelected, selecting),
        KeyBinding::new("backspace", TakeAwaySelected, selecting),
        KeyBinding::new("tab", SelectNextThing, selecting),
        KeyBinding::new("shift-tab", SelectPreviousThing, selecting),
        KeyBinding::new("escape", LetGoOfSelected, selecting),
        // The window's copy, cut and paste, which act on the thing selected.
        KeyBinding::new("ctrl-c", CopyToClipboard, selecting),
        KeyBinding::new("ctrl-x", CutToClipboard, selecting),
        KeyBinding::new("ctrl-v", PasteFromClipboard, selecting),
        KeyBinding::new("ctrl-d", DuplicateSelected, selecting),
    ]);
    crate::text_tools::bind_keys(cx);
}

/// What the editor tells the window.
pub enum EditorEvent {
    /// The talk's text changed.
    Edited,
    /// Something to put on the status line.
    Said(SharedString),
    /// What was put on the status line no longer holds, and goes if it's still there.
    Unsaid(SharedString),
}

/// Where the caret goes.
#[derive(Clone, Copy)]
enum Go {
    By(Motion),
    Line(LineMove),
}

/// What's drawn over the slide for the text being typed, in slide units.
struct Marks {
    caret: Option<CaretRect>,
    selection: SelectionRects,
}

pub struct SlideEditor {
    document: Option<Entity<Document>>,
    canvas: Entity<SlideCanvas>,
    focus_handle: FocusHandle,
    /// Where the keyboard goes once typing ends: the window's own.
    away: FocusHandle,
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
    /// What the status line was told of the thing selected on the slide shown, if anything is.
    told: Option<SharedString>,

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
            tools,
            more_tools: Vec::new(),
            caret_on: true,
            blinks_left: 0,
            blinking: false,
            blink: None,
            pause: None,
            wanted_x: None,
            dragging: false,
            told: None,
            ime_place: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
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
        let focused = self.focus_handle.is_focused(window);
        if self.is_editing(cx) {
            // A text tool keeps the keyboard until it's done with it.
            if !focused && !self.tools_have_keyboard(window, cx) {
                self.focus_handle.focus(window, cx);
                self.wake(cx);
            }
        } else {
            self.stop(cx);
            if self.selection_here(cx).is_some() {
                if !focused && self.away.is_focused(window) {
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

    /// The thing selected, if it's on the slide shown.
    fn selection_here(&self, cx: &App) -> Option<Selected> {
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
    fn tell_selection(&mut self, cx: &mut Context<Self>) {
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
    fn select_thing(&mut self, slide: usize, source: Range<usize>, window: &mut Window, cx: &mut Context<Self>) {
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
    fn deselect(&mut self, cx: &mut Context<Self>) {
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
    fn let_go(&mut self, cx: &mut Context<Self>) {
        if self.selection_here(cx).is_some() {
            self.deselect(cx);
        } else {
            cx.propagate();
        }
    }

    /// Copies or cuts the selection of the text being typed, as [`Self::copy_or_cut`] does; with
    /// none being typed, the window copies or cuts the thing selected.
    fn copy_or_cut_typed(&mut self, cut: bool, cx: &mut Context<Self>) {
        if self.editing(cx).is_none() {
            cx.propagate();
            return;
        }
        self.copy_or_cut(cut, cx);
    }

    /// Puts a copy of the thing selected on the slide shown beside it, and selects the copy.
    fn duplicate(&mut self, cx: &mut Context<Self>) {
        if self.selection_here(cx).is_none() {
            cx.propagate();
            return;
        }
        self.run(Document::duplicate_selected, cx);
    }

    /// Takes the thing selected on the slide shown away.
    fn take_away(&mut self, cx: &mut Context<Self>) {
        if self.selection_here(cx).is_none() {
            cx.propagate();
            return;
        }
        self.run(Document::delete_selected, cx);
    }

    /// Selects the next thing on the slide after the one selected, in the order they're
    /// written, or with `back` the one before; past the last comes the first again.
    fn select_along(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) {
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
    fn box_of_text(&self, slide: usize, flow: usize, para: usize, (x, y): (f32, f32), cx: &mut Context<Self>) -> Option<Range<usize>> {
        let element = self.layout(slide, cx)?.element_at(x, y)?;
        let document = self.document.as_ref()?.read(cx);
        let flows = document.text_flows(document.talk(), slide);
        let in_cell = flows.get(flow).is_some_and(|flow| matches!(flow.place, FlowPlace::Cell { .. }));
        (!in_cell && element.holds_text && selection::holds_para(&flows, flow, para, &element.source)).then_some(element.source)
    }

    /// Why the cell of the table `table` on slide `slide` that (`x`, `y`) is well inside can't be
    /// typed in, if it's one that holds more than text.
    fn cell_reason(&self, slide: usize, table: &Range<usize>, (x, y): (f32, f32), cx: &mut Context<Self>) -> Option<&'static str> {
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
    fn is_selected(&self, slide: usize, source: &Range<usize>, cx: &App) -> bool {
        self.document.as_ref().is_some_and(|document| {
            document
                .read(cx)
                .selected()
                .is_some_and(|selected| selected.slide == slide && selected.source == *source)
        })
    }

    /// Does `work` to the talk, and tells the window what came of it: the slides it changed,
    /// or `None` if it was refused, which is said, as is anything else that went wrong.
    fn run(
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
    fn apply(&mut self, op: impl FnOnce(&TextEdit) -> TextEdit, cx: &mut Context<Self>) {
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

    /// Drops what the system was composing, as before a cut or a paste.
    fn drop_composing(&mut self, cx: &mut Context<Self>) {
        if self.editing(cx).is_some_and(|editing| editing.composing.is_some()) {
            self.set_composing(None, cx);
        }
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
    fn type_over(&mut self, over: Option<(Pos, Pos)>, text: &str, cx: &mut Context<Self>) {
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
    fn editing<'a>(&self, cx: &'a App) -> Option<&'a EditingText> {
        self.document.as_ref()?.read(cx).text()
    }

    /// Shows `composing` at the caret, or drops what was being composed. The talk isn't
    /// touched.
    fn set_composing(&mut self, composing: Option<Composing>, cx: &mut Context<Self>) {
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

    /// After any key or click: the caret shows and blinks again, and a run of typing goes on
    /// while it's not paused for long.
    fn typed(&mut self, cx: &mut Context<Self>) {
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
    fn wake(&mut self, cx: &mut Context<Self>) {
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
    fn stop(&mut self, _cx: &mut Context<Self>) {
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
    fn end_editing(&mut self, give_back: bool, window: &mut Window, cx: &mut Context<Self>) {
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
    fn editing_place(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<(usize, Option<usize>, Arc<TextLayout>)> {
        let document = self.document.clone()?;
        let slide = document.read(cx).text()?.slide;
        let flow = document.read(cx).text_flow();
        Some((slide, flow, self.layout(slide, cx)?))
    }

    /// Where slide `slide`'s text is. If it couldn't be laid out, that's said.
    fn layout(&self, slide: usize, cx: &mut Context<Self>) -> Option<Arc<TextLayout>> {
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

    fn go(&mut self, go: Go, extend: bool, cx: &mut Context<Self>) {
        match go {
            Go::By(motion) => {
                self.wanted_x = None;
                self.apply(|edit| edit.move_by(motion, extend), cx);
            }
            Go::Line(direction) => {
                let Some((_, Some(flow), layout)) = self.editing_place(cx) else {
                    self.typed(cx);
                    return;
                };
                let Some(head) = self.head(cx) else { return };
                let keeps_x = matches!(direction, LineMove::Up | LineMove::Down);
                let x = keeps_x
                    .then(|| {
                        self.wanted_x
                            .or_else(|| layout.caret(flow, head).map(|caret| caret.x))
                    })
                    .flatten();
                let to = layout.line_move(flow, head, direction, x);
                self.apply(|edit| edit.move_to(to, extend), cx);
                self.wanted_x = x;
            }
        }
    }

    fn head(&self, cx: &App) -> Option<Pos> {
        let document = self.document.as_ref()?.read(cx);
        Some(document.text()?.edit.selection.head)
    }

    fn enter(&mut self, cx: &mut Context<Self>) {
        self.wanted_x = None;
        if self.in_cell(cx) {
            self.move_in_table(CellMove::Below, cx);
            return;
        }
        // A title is one paragraph, so Enter in it goes on to a new line.
        let title = self.document.as_ref().is_some_and(|document| {
            document
                .read(cx)
                .text()
                .is_some_and(|editing| editing.is_title())
        });
        if title {
            self.apply(TextEdit::soft_break, cx);
        } else {
            self.apply(TextEdit::enter, cx);
        }
    }

    /// Makes the paragraphs a list, numbered with `numbered`, or not, as the Bullets and
    /// Numbering tools do. A title can't be a list.
    pub fn toggle_list(&mut self, numbered: bool, cx: &mut Context<Self>) {
        if self.editing(cx).is_some_and(|editing| editing.is_title() || editing.is_cell()) {
            return;
        }
        self.format(|edit| edit.toggle_list(numbered), cx);
    }

    fn edit_with(&mut self, op: fn(&TextEdit) -> TextEdit, cx: &mut Context<Self>) {
        self.wanted_x = None;
        self.apply(op, cx);
    }

    /// Whether the text being typed is a cell of a table.
    fn in_cell(&self, cx: &App) -> bool {
        self.editing(cx).is_some_and(EditingText::is_cell)
    }

    /// Takes the caret from the cell it's in to another, as Tab, Shift+Tab and Enter do.
    fn move_in_table(&mut self, step: CellMove, cx: &mut Context<Self>) {
        self.wanted_x = None;
        self.run(|document| document.move_in_table(step), cx);
        self.typed(cx);
    }

    /// Tab and Shift+Tab: in a cell, on to the next one or back; elsewhere, a list item in or out.
    fn tab(&mut self, back: bool, cx: &mut Context<Self>) {
        if self.in_cell(cx) {
            self.move_in_table(if back { CellMove::Previous } else { CellMove::Next }, cx);
        } else {
            self.edit_with(if back { TextEdit::outdent } else { TextEdit::indent }, cx);
        }
    }

    /// Shift+Enter: a line break, which a cell, one line, has no place for.
    fn line_break(&mut self, cx: &mut Context<Self>) {
        if !self.in_cell(cx) {
            self.edit_with(TextEdit::soft_break, cx);
        }
    }

    /// Escape: typing ends. In a cell, its table is selected then, as a box typed in stays
    /// selected.
    fn stop_editing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let table = self.document.as_ref().and_then(|document| {
            let document = document.read(cx);
            Some((document.text()?.slide, document.table_at_caret()?.0))
        });
        match table {
            Some((slide, table)) => self.select_thing(slide, table, window, cx),
            None => self.end_editing(true, window, cx),
        }
    }

    /// An arrow key, which moves the caret `go`, but at a cell's edge that way goes on into the
    /// cell `beside` it: Left at its start, Right at its end, Up on its first line and Down on its
    /// last.
    fn arrow(&mut self, go: Go, beside: Beside, cx: &mut Context<Self>) {
        if !self.across_cells(beside, cx) {
            self.go(go, false, cx);
        }
    }

    /// Moves the caret into the cell `beside` the one it's in, if it's at the edge of that one that
    /// way and there's a cell there to type in. Says whether it did.
    fn across_cells(&mut self, beside: Beside, cx: &mut Context<Self>) -> bool {
        let Some(editing) = self.editing(cx).filter(|editing| editing.is_cell() && editing.edit.selection.is_caret()) else {
            return false;
        };
        let (slide, head) = (editing.slide, editing.edit.selection.head);
        let end = TextEdit::new(editing.edit.paras.clone()).select_all().selection.end();
        let Some((_, Some(flow), layout)) = self.editing_place(cx) else {
            return false;
        };
        let top = |pos: Pos| layout.caret(flow, pos).map(|caret| caret.top);
        let at_edge = match beside {
            Beside::Before => head == Pos::default(),
            Beside::After => head == end,
            // On the first line, or the last: where the caret's top is that of the start, or the end.
            Beside::Above => top(head).zip(top(Pos::default())).is_some_and(|(a, b)| (a - b).abs() < 0.5),
            Beside::Below => top(head).zip(top(end)).is_some_and(|(a, b)| (a - b).abs() < 0.5),
        };
        let Some(document) = self.document.clone() else {
            return false;
        };
        let Some(target) = at_edge.then(|| document.read(cx).cell_beside(beside)).flatten() else {
            return false;
        };
        let document = document.read(cx);
        let paras = document.text_flows(document.talk(), slide).get(target).map(|flow| flow.paras.clone()).unwrap_or_default();
        let target_end = TextEdit::new(paras).select_all().selection.end();
        let x = self.wanted_x.or_else(|| layout.caret(flow, head).map(|caret| caret.x));
        let pos = match beside {
            Beside::Before => target_end,
            Beside::After => Pos::default(),
            // As near as can be across from where the caret was, on the line next to it.
            Beside::Above | Beside::Below => {
                let line = if beside == Beside::Above { target_end } else { Pos::default() };
                layout
                    .caret(target, line)
                    .zip(x)
                    .and_then(|(caret, x)| {
                        let (px, py) = caret.on_slide(x, caret.top + caret.height / 2.);
                        match layout.hit(px, py) {
                            Some(Hit::Text { flow, pos }) if flow == target => Some(pos),
                            _ => None,
                        }
                    })
                    .unwrap_or(line)
            }
        };
        self.run(
            |document| {
                document.begin_text(slide, target, pos);
                Ok(Vec::new())
            },
            cx,
        );
        self.wanted_x = if matches!(beside, Beside::Above | Beside::Below) { x } else { None };
        self.typed(cx);
        true
    }

    /// Where `position`, in the window, is on the slide, in slide units, if it's on it.
    fn slide_point(&self, position: Point<Pixels>, cx: &App) -> Option<(f32, f32)> {
        let slide = self.canvas.read(cx).slide_bounds()?;
        slide
            .contains(&position)
            .then(|| self.nearest_slide_point(position, cx))
            .flatten()
    }

    /// The point of the slide nearest `position`, in the window, in slide units: `position`
    /// itself if it's on the slide, or else the nearest point of its edge.
    fn nearest_slide_point(&self, position: Point<Pixels>, cx: &App) -> Option<(f32, f32)> {
        let slide = self.canvas.read(cx).slide_bounds()?;
        let unit = f32::from(slide.size.width) / SLIDE_WIDTH;
        if unit <= 0.0 {
            return None;
        }
        let along = |at: Pixels, start: Pixels, length: Pixels| {
            f32::from(at - start).clamp(0.0, f32::from(length)) / unit
        };
        Some((
            along(position.x, slide.origin.x, slide.size.width),
            along(position.y, slide.origin.y, slide.size.height),
        ))
    }

    /// What's at (`x`, `y`) of the current slide.
    fn hit(&self, (x, y): (f32, f32), cx: &mut Context<Self>) -> Option<(usize, Hit)> {
        let slide = self.document.as_ref()?.read(cx).current();
        let layout = self.layout(slide, cx)?;
        Some((slide, layout.hit(x, y)?))
    }

    /// Whether typing is going on in flow `flow` of slide `slide`.
    fn editing_flow(&self, slide: usize, flow: usize, cx: &App) -> bool {
        self.document.as_ref().is_some_and(|document| {
            let document = document.read(cx);
            document
                .text()
                .is_some_and(|editing| editing.slide == slide)
                && document.text_flow() == Some(flow)
        })
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // A colour held for the selection goes to it before the click moves it.
        self.give_held_color(cx);
        self.dragging = false;
        let Some(point) = self.slide_point(event.position, cx) else {
            // The desk around the slide.
            self.stop_typing(window, cx);
            return;
        };
        if self.new_box_here(cx).is_some_and(|(x, y, w, h)| x <= point.0 && point.0 <= x + w && y <= point.1 && point.1 <= y + h) {
            // In the new box being typed in: typing goes on there.
            cx.stop_propagation();
            self.focus_handle.focus(window, cx);
            self.typed(cx);
            return;
        }
        let Some((slide, mut hit)) = self.hit(point, cx) else {
            self.stop_typing(window, cx);
            return;
        };
        let elsewhere = match &hit {
            Hit::Text { flow, .. } => !self.editing_flow(slide, *flow, cx),
            _ => true,
        };
        if elsewhere && self.is_editing(cx) {
            // What was typed there is left tidy first, which can move the text clicked on.
            self.stop(cx);
            let changed = self.run(Document::end_text, cx);
            if changed.is_some_and(|changed| !changed.is_empty()) {
                match self.hit(point, cx) {
                    Some((_, again)) => hit = again,
                    None => return,
                }
            }
        }
        match hit {
            Hit::Text { flow, pos } => {
                cx.stop_propagation();
                let starts = !self.editing_flow(slide, flow, cx);
                // The first click on a box's text picks the box; the next types in it.
                let pick = self
                    .box_of_text(slide, flow, pos.para, point, cx)
                    .filter(|source| starts && event.click_count == 1 && !self.is_selected(slide, source, cx));
                if let Some(source) = pick {
                    self.select_thing(slide, source, window, cx);
                    return;
                }
                self.wanted_x = None;
                if starts {
                    self.run(
                        |document| {
                            document.begin_text(slide, flow, pos);
                            Ok(Vec::new())
                        },
                        cx,
                    );
                }
                let extend = event.modifiers.shift && !elsewhere;
                // A double click that starts the typing puts the caret where it was clicked.
                let count = if starts { 1 } else { event.click_count };
                match count {
                    2 => self.apply(|edit| edit.select_word_at(pos), cx),
                    count if count >= 3 => self.apply(
                        |edit| {
                            edit.move_to(
                                Pos {
                                    para: pos.para,
                                    offset: 0,
                                },
                                false,
                            )
                            .move_by(Motion::ParaEnd, true)
                        },
                        cx,
                    ),
                    _ => {
                        self.apply(|edit| edit.move_to(pos, extend), cx);
                        self.dragging = true;
                    }
                }
                self.focus_handle.focus(window, cx);
            }
            Hit::Element(element) => {
                cx.stop_propagation();
                let reason = self.cell_reason(slide, &element.source, point, cx);
                self.select_thing(slide, element.source, window, cx);
                if let Some(reason) = reason {
                    cx.emit(EditorEvent::Said(reason.into()));
                }
            }
            Hit::Placeholder(region) => {
                cx.stop_propagation();
                self.wanted_x = None;
                self.deselect(cx);
                self.run(
                    |document| {
                        document.begin_text_in_region(slide, &region);
                        Ok(Vec::new())
                    },
                    cx,
                );
                self.focus_handle.focus(window, cx);
                self.typed(cx);
            }
            Hit::Uneditable(reason) => {
                self.stop_typing(window, cx);
                cx.emit(EditorEvent::Said(reason.into()));
            }
            Hit::Nothing => self.stop_typing(window, cx),
        }
        self.tell_selection(cx);
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.dragging || event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        // Off the slide, or away from the text, the selection goes on to the nearest place.
        let Some(point) = self.nearest_slide_point(event.position, cx) else {
            return;
        };
        let Some((_, Some(flow), layout)) = self.editing_place(cx) else {
            return;
        };
        let in_flow = |point: (f32, f32)| match layout.hit(point.0, point.1) {
            Some(Hit::Text { flow: hit, pos }) if hit == flow => Some(pos),
            _ => None,
        };
        let pos = in_flow(point).or_else(|| in_flow(nearest_in_flow(&layout, flow, point)?));
        if let Some(pos) = pos {
            self.apply(|edit| edit.move_to(pos, true), cx);
        }
    }

    /// Where the caret goes at `pos` of the text being typed, in slide units.
    fn caret_at(&self, pos: Pos, cx: &mut Context<Self>) -> Option<CaretRect> {
        let (_, flow, layout) = self.editing_place(cx)?;
        if let Some(flow) = flow {
            return layout.caret(flow, pos);
        }
        // An empty region, before anything is typed: the caret is where its text would start.
        let key = &self.editing(cx)?.flow;
        if let Some((x, y)) = key.new_box() {
            // A new box, not written yet: at its corner.
            let (w, h) = TEXT_BOX_SIZE;
            let element = ElementBox { source: 0..0, rect: Rect { x: x as f32, y: y as f32, w, h }, rotate: 0.0, holds_text: true };
            return Some(empty_box_caret(&element, false));
        }
        if let Some(start) = key.tag_start() {
            // An empty box: at its corner, or a shape's middle.
            let element = layout.elements().iter().rev().find(|element| element.source.start == start)?.clone();
            let shape = self.document.as_ref()?.read(cx).talk().source.get(start..)?.starts_with("<Shape");
            return Some(empty_box_caret(&element, shape));
        }
        layout.placeholder_caret(key.region()?)
    }

    /// The new text box typed in that isn't written yet, if it's on the slide shown: where it is,
    /// `(x, y, w, h)`, in slide units.
    fn new_box_here(&self, cx: &App) -> Option<(f32, f32, f32, f32)> {
        let document = self.document.as_ref()?.read(cx);
        document.new_box().filter(|(slide, _)| *slide == document.current()).map(|(_, rect)| rect)
    }

    /// Whether the thing selected on the slide shown takes typing, with nothing being typed: a
    /// shape, a box, or one of the talk's own components that takes content, which typing puts
    /// words in.
    fn takes_typing(&self, cx: &App) -> bool {
        !self.is_editing(cx)
            && self.selection_here(cx).is_some()
            && self.document.as_ref().is_some_and(|document| document.read(cx).selected_takes_text())
    }

    /// Starts typing in the thing selected, if it takes typing, as the first thing typed on it
    /// does. Says whether text is being typed now.
    fn start_typing_in_selected(&mut self, cx: &mut Context<Self>) -> bool {
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

    /// Where the caret at `pos` of the text being typed is in the window.
    fn caret_in_window(&self, pos: Pos, cx: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let caret = self.caret_at(pos, cx)?;
        let slide = self.canvas.read(cx).slide_bounds()?;
        Some(caret_box(&caret, slide))
    }

    /// The caret and the selection of the text being typed, in slide units.
    fn marks(&self, cx: &mut Context<Self>) -> Option<Marks> {
        let (_, flow, layout) = self.editing_place(cx)?;
        let selection = self.editing(cx)?.edit.selection;
        let Some(flow) = flow else {
            return Some(Marks {
                caret: self.caret_at(selection.head, cx),
                selection: SelectionRects::default(),
            });
        };
        let caret = if selection.is_caret() {
            layout.caret(flow, selection.head)
        } else {
            None
        };
        let selection = if selection.is_caret() {
            SelectionRects::default()
        } else {
            layout.selection_rects(flow, selection.start(), selection.end())
        };
        Some(Marks { caret, selection })
    }
}

/// The point of flow `flow`'s paragraphs, taken together, nearest `point` of the slide, in
/// slide units: where a drag that leaves the text keeps selecting.
fn nearest_in_flow(layout: &TextLayout, flow: usize, point: (f32, f32)) -> Option<(f32, f32)> {
    let (left, top, right, bottom) = layout
        .paras()
        .iter()
        .filter(|placed| placed.flow == flow)
        .map(|placed| placed.rect)
        .fold(None, |area: Option<(f32, f32, f32, f32)>, rect| {
            let (l, t, r, b) = (rect.x, rect.y, rect.x + rect.w, rect.y + rect.h);
            Some(area.map_or((l, t, r, b), |(al, at, ar, ab)| {
                (al.min(l), at.min(t), ar.max(r), ab.max(b))
            }))
        })?;
    // The paragraphs lie in their layer, which may be turned.
    let caret = layout.caret(flow, Pos { para: 0, offset: 0 })?;
    let (x, y) = turned(point, caret.centre, -caret.rotate);
    // Just inside, as a point on the very edge can be taken for the next line.
    let inside = |at: f32, low: f32, high: f32| at.clamp(low, (high - 0.5).max(low));
    Some(turned(
        (inside(x, left, right), inside(y, top, bottom)),
        caret.centre,
        caret.rotate,
    ))
}

/// `(x, y)` turned by `degrees` around `centre`, as the engine turns a layer.
fn turned((x, y): (f32, f32), centre: (f32, f32), degrees: f32) -> (f32, f32) {
    if degrees == 0.0 {
        return (x, y);
    }
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (dx, dy) = (x - centre.0, y - centre.1);
    (
        centre.0 + dx * cos - dy * sin,
        centre.1 + dx * sin + dy * cos,
    )
}

/// The corners of the rectangle at `x`, `top`, `w` wide and `h` tall, turned, in slide units.
fn corners(x: f32, top: f32, w: f32, h: f32, centre: (f32, f32), degrees: f32) -> [(f32, f32); 4] {
    [(x, top), (x + w, top), (x + w, top + h), (x, top + h)]
        .map(|corner| turned(corner, centre, degrees))
}

/// The caret's corners, `unit` window pixels to a slide unit.
fn caret_corners(caret: &CaretRect, unit: f32) -> [(f32, f32); 4] {
    let width = CARET_WIDTH / unit;
    corners(
        caret.x - width / 2.,
        caret.top,
        width,
        caret.height,
        caret.centre,
        caret.rotate,
    )
}

/// Whether `keystroke` is a shortcut rather than text: with Ctrl, Alt, the platform key or Fn
/// held, apart from Ctrl and Alt together giving a character, which is AltGr typing.
fn is_shortcut(keystroke: &Keystroke) -> bool {
    let modifiers = keystroke.modifiers;
    let altgr = modifiers.control && modifiers.alt && keystroke.key_char.is_some();
    (modifiers.control || modifiers.alt || modifiers.platform || modifiers.function) && !altgr
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

/// How far inside a cell of a table, in slide units, a click is on the cell rather than on the
/// lines around it, which pick the table.
const EDGE: f32 = 4.5;

/// `range` from its lower end to its higher, as the system may give it either way round.
fn ordered(range: Range<usize>) -> Range<usize> {
    range.start.min(range.end)..range.start.max(range.end)
}

/// The box the caret's corners fit in, in the window, on a slide drawn at `slide` there.
fn caret_box(caret: &CaretRect, slide: Bounds<Pixels>) -> Bounds<Pixels> {
    let unit = f32::from(slide.size.width) / SLIDE_WIDTH;
    if unit <= 0.0 {
        return Bounds {
            origin: slide.origin,
            size: Size::default(),
        };
    }
    corners_box(&caret_corners(caret, unit), slide, unit)
}

/// How tall the caret in a box with nothing in it is drawn, in slide units, at most.
const EMPTY_BOX_CARET: f32 = 56.0;

/// Where the caret goes in `element`, a box with nothing typed in it yet: at its top-left
/// corner, or, in a shape, which centres its text, in its middle.
fn empty_box_caret(element: &ElementBox, shape: bool) -> CaretRect {
    let Rect { x, y, w, h } = element.rect;
    let height = EMPTY_BOX_CARET.min(h.max(1.0));
    let (x, top) = if shape { (x + w / 2., y + (h - height) / 2.) } else { (x, y) };
    CaretRect {
        x,
        top,
        height,
        rotate: element.rotate,
        centre: (element.rect.x + w / 2., element.rect.y + h / 2.),
    }
}

/// The corners of the thing selected, `element`, turned with it, in slide units.
fn outline_corners(element: &ElementBox) -> [(f32, f32); 4] {
    let Rect { x, y, w, h } = element.rect;
    corners(x, y, w, h, (x + w / 2., y + h / 2.), element.rotate)
}

/// The box `corners`, in slide units, fit in, in the window, on a slide drawn at `slide`
/// there, `unit` window pixels to a slide unit.
fn corners_box(corners: &[(f32, f32); 4], slide: Bounds<Pixels>, unit: f32) -> Bounds<Pixels> {
    let (xs, ys): (Vec<f32>, Vec<f32>) = corners.iter().copied().unzip();
    let fold = |values: &[f32], pick: fn(f32, f32) -> f32| {
        values.iter().copied().reduce(pick).unwrap_or(0.)
    };
    let (left, right) = (fold(&xs, f32::min), fold(&xs, f32::max));
    let (top, bottom) = (fold(&ys, f32::min), fold(&ys, f32::max));
    Bounds {
        origin: slide.origin + point(px(left * unit), px(top * unit)),
        size: size(px((right - left) * unit), px((bottom - top) * unit)),
    }
}

/// How long each dash of a new box's line is, and the gap after it, in the window's pixels.
const DASH: f32 = 6.0;

/// Draws a dashed line around `(x, y, w, h)`, in slide units, on a slide drawn at `slide`.
fn paint_dashed((x, y, w, h): (f32, f32, f32, f32), slide: Bounds<Pixels>, unit: f32, color: Hsla, window: &mut Window) {
    let (dash, thick) = (DASH / unit, OUTLINE_WIDTH / 2. / unit);
    let edges = [((x, y), (x + w, y)), ((x + w, y), (x + w, y + h)), ((x + w, y + h), (x, y + h)), ((x, y + h), (x, y))];
    for ((x0, y0), (x1, y1)) in edges {
        let length = ((x1 - x0).powi(2) + (y1 - y0).powi(2)).sqrt();
        if length <= 0. {
            continue;
        }
        let (dx, dy) = ((x1 - x0) / length, (y1 - y0) / length);
        let mut at = 0.;
        while at < length {
            let end = (at + dash).min(length);
            let (ax, ay, bx, by) = (x0 + dx * at, y0 + dy * at, x0 + dx * end, y0 + dy * end);
            // Across the line, half its width each side.
            let (nx, ny) = (-dy * thick, dx * thick);
            let points = [(ax + nx, ay + ny), (bx + nx, by + ny), (bx - nx, by - ny), (ax - nx, ay - ny)];
            paint_polygon(&points, slide, unit, color, window);
            at += dash * 2.;
        }
    }
}

fn paint_polygon(
    points: &[(f32, f32)],
    slide: Bounds<Pixels>,
    unit: f32,
    color: Hsla,
    window: &mut Window,
) {
    paint_path(PathBuilder::fill(), points, slide, unit, color, window);
}

/// Draws the line around `points`, in slide units, on a slide drawn at `slide`.
fn paint_outline(
    points: &[(f32, f32)],
    slide: Bounds<Pixels>,
    unit: f32,
    color: Hsla,
    window: &mut Window,
) {
    paint_path(PathBuilder::stroke(px(OUTLINE_WIDTH)), points, slide, unit, color, window);
}

fn paint_path(
    mut path: PathBuilder,
    points: &[(f32, f32)],
    slide: Bounds<Pixels>,
    unit: f32,
    color: Hsla,
    window: &mut Window,
) {
    let points: Vec<Point<Pixels>> = points
        .iter()
        .map(|&(x, y)| slide.origin + point(px(x * unit), px(y * unit)))
        .collect();
    path.add_polygon(&points, true);
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

impl Render for SlideEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = cx.entity().downgrade();
        let marks = self.marks(cx);
        let caret_color = cx.theme().caret;
        let selection_color = cx.theme().selection.opacity(0.6);
        let (composition_color, composition_background) =
            (cx.theme().foreground, cx.theme().background);
        let caret_on = self.caret_on;
        let outline_color = cx.theme().ring;
        // The thing selected on the slide shown, drawn around.
        let outline = self.selection_here(cx).and_then(|_| {
            let document = self.document.clone()?;
            document.update(cx, |document, _| document.selected_box())
        });
        let outline = outline.as_ref().map(outline_corners);
        // A new box waiting for its first character: a dashed line around where it'll be.
        let new_box = self.new_box_here(cx);
        let new_box_color = cx.theme().muted_foreground;
        let selecting = outline.is_some() && !self.is_editing(cx);
        let placed = {
            let canvas = self.canvas.read(cx);
            canvas.slide_bounds().zip(canvas.area())
        };
        let in_editor = |bounds: Bounds<Pixels>, area: Bounds<Pixels>| Bounds {
            origin: bounds.origin - area.origin,
            size: bounds.size,
        };
        // Where the caret is in the editor, for whoever looks for it.
        let caret_box = marks
            .as_ref()
            .and_then(|marks| marks.caret)
            .zip(placed)
            .map(|(caret, (slide, area))| in_editor(caret_box(&caret, slide), area));
        // Where the outline is in the editor, for whoever looks for it.
        let outline_box = outline.zip(placed).and_then(|(corners, (slide, area))| {
            let unit = f32::from(slide.size.width) / SLIDE_WIDTH;
            (unit > 0.0).then(|| in_editor(corners_box(&corners, slide, unit), area))
        });
        // Where the new box is in the editor, for whoever looks for it.
        let new_box_bounds = new_box.zip(placed).and_then(|((x, y, w, h), (slide, area))| {
            let unit = f32::from(slide.size.width) / SLIDE_WIDTH;
            let corners = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
            (unit > 0.0).then(|| in_editor(corners_box(&corners, slide, unit), area))
        });
        // What the system is composing, at the caret, where the selection it stands for starts.
        let composing = self.editing(cx).and_then(|editing| {
            let composing = editing.composing.as_ref()?;
            Some((composing.text.clone(), editing.edit.selection.start()))
        });
        let composition = composing.and_then(|(text, at)| {
            let caret = self.caret_in_window(at, cx)?;
            Some((text, in_editor(caret, placed?.1)))
        });
        // The system's input method places its window by the caret: it's told when that moves.
        let place = self
            .editing(cx)
            .map(|editing| (editing.edit.selection, caret_box, composition.clone()));
        if place != self.ime_place {
            if place.is_some() {
                window.invalidate_character_coordinates();
            }
            self.ime_place = place;
        }
        // While text is typed, the system's input method types it, and on a shape or a box
        // selected, where it starts the typing.
        let input = (self.is_editing(cx) || self.takes_typing(cx))
            .then(|| (self.focus_handle.clone(), cx.entity()));
        let overlay = canvas(
            |_, _, _| {},
            move |area, _, window, cx| {
                if let Some((focus, editor)) = &input {
                    window.handle_input(focus, ElementInputHandler::new(area, editor.clone()), cx);
                }
                // A drag goes on selecting wherever the mouse goes, off the slide and off the
                // editor too, until it's let go of anywhere.
                let moved = editor.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble {
                        moved
                            .update(cx, |editor, cx| editor.mouse_move(event, window, cx))
                            .ok();
                    }
                });
                window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        editor.update(cx, |editor, _| editor.dragging = false).ok();
                    }
                });
                let slide = letterbox(area, window.scale_factor());
                let unit = f32::from(slide.size.width) / SLIDE_WIDTH;
                if unit <= 0.0 {
                    return;
                }
                window.with_content_mask(Some(ContentMask { bounds: slide }), |window| {
                    if let Some(corners) = &outline {
                        paint_outline(corners, slide, unit, outline_color, window);
                    }
                    if let Some(rect) = new_box {
                        paint_dashed(rect, slide, unit, new_box_color, window);
                    }
                    let Some(marks) = marks else { return };
                    let SelectionRects {
                        rects,
                        rotate,
                        centre,
                    } = &marks.selection;
                    for rect in rects {
                        let corners = corners(rect.x, rect.y, rect.w, rect.h, *centre, *rotate);
                        paint_polygon(&corners, slide, unit, selection_color, window);
                    }
                    if let Some(caret) = marks.caret.filter(|_| caret_on) {
                        paint_polygon(
                            &caret_corners(&caret, unit),
                            slide,
                            unit,
                            caret_color,
                            window,
                        );
                    }
                });
            },
        )
        .absolute()
        .size_full();
        div()
            .id(SLIDE_EDITOR_ID)
            .test_support()
            .key_context(if selecting { SELECTION_CONTEXT } else { EDITOR_CONTEXT })
            .track_focus(&self.focus_handle)
            .absolute()
            .size_full()
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_action(cx.listener(|this, _: &MoveLeft, _, cx| {
                this.arrow(Go::By(Motion::Left), Beside::Before, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveRight, _, cx| {
                this.arrow(Go::By(Motion::Right), Beside::After, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveUp, _, cx| {
                this.arrow(Go::Line(LineMove::Up), Beside::Above, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveDown, _, cx| {
                this.arrow(Go::Line(LineMove::Down), Beside::Below, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveWordLeft, _, cx| {
                this.go(Go::By(Motion::WordLeft), false, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveWordRight, _, cx| {
                this.go(Go::By(Motion::WordRight), false, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveLineStart, _, cx| {
                this.go(Go::Line(LineMove::LineStart), false, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveLineEnd, _, cx| {
                this.go(Go::Line(LineMove::LineEnd), false, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveFlowStart, _, cx| {
                this.go(Go::By(Motion::FlowStart), false, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveFlowEnd, _, cx| {
                this.go(Go::By(Motion::FlowEnd), false, cx)
            }))
            .on_action(
                cx.listener(|this, _: &SelectLeft, _, cx| this.go(Go::By(Motion::Left), true, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SelectRight, _, cx| {
                    this.go(Go::By(Motion::Right), true, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &SelectUp, _, cx| this.go(Go::Line(LineMove::Up), true, cx)),
            )
            .on_action(cx.listener(|this, _: &SelectDown, _, cx| {
                this.go(Go::Line(LineMove::Down), true, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectWordLeft, _, cx| {
                this.go(Go::By(Motion::WordLeft), true, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectWordRight, _, cx| {
                this.go(Go::By(Motion::WordRight), true, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectLineStart, _, cx| {
                this.go(Go::Line(LineMove::LineStart), true, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectLineEnd, _, cx| {
                this.go(Go::Line(LineMove::LineEnd), true, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectFlowStart, _, cx| {
                this.go(Go::By(Motion::FlowStart), true, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectFlowEnd, _, cx| {
                this.go(Go::By(Motion::FlowEnd), true, cx)
            }))
            .on_action(
                cx.listener(|this, _: &SelectAll, _, cx| this.edit_with(TextEdit::select_all, cx)),
            )
            .on_action(
                cx.listener(|this, _: &Backspace, _, cx| this.edit_with(TextEdit::backspace, cx)),
            )
            .on_action(cx.listener(|this, _: &Delete, _, cx| this.edit_with(TextEdit::delete, cx)))
            .on_action(cx.listener(|this, _: &Enter, _, cx| this.enter(cx)))
            .on_action(cx.listener(|this, _: &LineBreak, _, cx| this.line_break(cx)))
            .on_action(cx.listener(|this, _: &Indent, _, cx| this.tab(false, cx)))
            .on_action(cx.listener(|this, _: &Outdent, _, cx| this.tab(true, cx)))
            .on_action(cx.listener(|this, _: &StopEditing, window, cx| this.stop_editing(window, cx)))
            .on_action(cx.listener(|this, _: &TakeAwaySelected, _, cx| this.take_away(cx)))
            .on_action(cx.listener(|this, _: &SelectNextThing, window, cx| {
                this.select_along(false, window, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectPreviousThing, window, cx| {
                this.select_along(true, window, cx)
            }))
            .on_action(cx.listener(|this, _: &LetGoOfSelected, _, cx| this.let_go(cx)))
            .on_action(cx.listener(|this, _: &CopyToClipboard, _, cx| {
                this.copy_or_cut_typed(false, cx)
            }))
            .on_action(cx.listener(|this, _: &CutToClipboard, _, cx| {
                this.copy_or_cut_typed(true, cx)
            }))
            .on_action(cx.listener(|this, _: &DuplicateSelected, _, cx| this.duplicate(cx)))
            .on_action(cx.listener(|this, _: &ToggleBold, _, cx| {
                this.format(|edit| edit.toggle(StyleBit::Bold), cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleItalic, _, cx| {
                this.format(|edit| edit.toggle(StyleBit::Italic), cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleUnderline, _, cx| {
                this.format(|edit| edit.toggle(StyleBit::Underline), cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleBullets, _, cx| this.toggle_list(false, cx)))
            .on_action(cx.listener(|this, _: &ToggleNumbering, _, cx| this.toggle_list(true, cx)))
            .on_action(cx.listener(|this, action: &SetTextSize, _, cx| {
                this.set_text_size(action.size, cx)
            }))
            .child(self.canvas.clone())
            .child(overlay)
            .when_some(new_box_bounds, |this, bounds| {
                this.child(
                    div()
                        .id(NEW_BOX_ID)
                        .test_support()
                        .absolute()
                        .left(bounds.origin.x)
                        .top(bounds.origin.y)
                        .w(bounds.size.width)
                        .h(bounds.size.height),
                )
            })
            .when_some(outline_box, |this, outline| {
                this.child(
                    div()
                        .id(SELECTION_OUTLINE_ID)
                        .test_support()
                        .absolute()
                        .left(outline.origin.x)
                        .top(outline.origin.y)
                        .w(outline.size.width)
                        .h(outline.size.height),
                )
            })
            .when_some(caret_box, |this, caret| {
                this.child(
                    div()
                        .id(CARET_ID)
                        .test_support()
                        .absolute()
                        .left(caret.origin.x)
                        .top(caret.origin.y)
                        .w(caret.size.width)
                        .h(caret.size.height),
                )
            })
            .when_some(composition, |this, (text, at)| {
                let height = at.size.height;
                this.child(
                    div()
                        .id(COMPOSITION_ID)
                        .test_support()
                        .aria_label(text.clone())
                        .absolute()
                        .left(at.origin.x + at.size.width)
                        .top(at.origin.y)
                        .h(height)
                        .flex()
                        .items_center()
                        .bg(composition_background)
                        .text_color(composition_color)
                        .text_size(height * COMPOSITION_TEXT)
                        .line_height(height)
                        .underline()
                        .whitespace_nowrap()
                        .child(text),
                )
            })
    }
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

/// Whether `paras` are all list items: text that stays at the left, however it's aligned.
pub(crate) fn only_lists(paras: &[Para]) -> bool {
    !paras.is_empty()
        && paras
            .iter()
            .all(|para| matches!(para.kind, ParaKind::Bullet { .. } | ParaKind::Numbered { .. }))
}
