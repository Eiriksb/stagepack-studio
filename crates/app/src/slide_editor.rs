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
use slides_engine::{Align, CaretRect, Hit, LineMove, SelectionRects, TextLayout};
use slides_format::{Motion, Pos, StyleBit, TextEdit};

use crate::document::{Document, EditRefused};
use crate::slide_canvas::{SLIDE_WIDTH, SlideCanvas, letterbox};
use crate::text_editing::{Composing, EditingText, utf16_slice};
use crate::text_tools::{
    SetTextSize, ToggleBold, ToggleBullets, ToggleItalic, ToggleNumbering, ToggleUnderline,
};
use crate::workspace_view::CARET_BLINKS_FOR;

pub const SLIDE_EDITOR_ID: &str = "slide-editor";
pub const CARET_ID: &str = "caret";
/// What the system's input method is composing, shown at the caret.
pub const COMPOSITION_ID: &str = "composition";

/// The key context the editor's keys apply in, while text is being typed.
pub const EDITOR_CONTEXT: &str = "SlideEditor";

/// How long the caret shows, and then doesn't, as it blinks.
const BLINK: Duration = Duration::from_millis(500);
/// How long typing can stop for and still be one step to undo.
const TYPING_PAUSE: Duration = Duration::from_secs(2);
/// How wide the caret is drawn, in the window's pixels.
const CARET_WIDTH: f32 = 2.0;
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
    ]);
    crate::text_tools::bind_keys(cx);
}

/// What the editor tells the window.
pub enum EditorEvent {
    /// The talk's text changed.
    Edited,
    /// Something to put on the status line.
    Said(SharedString),
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
    _subscriptions: Vec<Subscription>,
}

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
                if !this.tools.contains_focused(window, cx) {
                    this.end_editing(false, window, cx)
                }
            }),
            cx.on_focus_out(&tools, window, |this, _, window, cx| {
                // From the text tools the keyboard went on elsewhere than the text.
                if !this.focus_handle.contains_focused(window, cx)
                    && !this.tools.contains_focused(window, cx)
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
        SlideEditor {
            document,
            canvas,
            focus_handle,
            away,
            tools,
            caret_on: true,
            blinks_left: 0,
            blinking: false,
            blink: None,
            pause: None,
            wanted_x: None,
            dragging: false,
            _subscriptions: subscriptions,
        }
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus_handle
    }

    fn is_editing(&self, cx: &App) -> bool {
        self.document
            .as_ref()
            .is_some_and(|document| document.read(cx).text().is_some())
    }

    /// Follows the talk: once text is being typed, as after an undo of typing, the editor has
    /// the keyboard, and once it isn't, it gives the keyboard back and stops its timers.
    fn document_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focused = self.focus_handle.is_focused(window);
        if self.is_editing(cx) {
            // A text tool keeps the keyboard until it's done with it.
            if !focused && !self.tools.contains_focused(window, cx) {
                self.focus_handle.focus(window, cx);
                self.wake(cx);
            }
        } else {
            self.stop(cx);
            if focused {
                self.away.focus(window, cx);
            }
        }
        cx.notify();
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
            let made = work(document);
            if made.as_ref().is_ok_and(|changed| !changed.is_empty()) {
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
    pub fn align_text(&mut self, align: Align, cx: &mut Context<Self>) {
        let align = match align {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
        };
        self.run(|document| document.align_text(align), cx);
        self.typed(cx);
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

    /// Ends typing on the slide, as a click on the desk around it does, and gives the keyboard
    /// back to the window.
    pub fn stop_typing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.end_editing(true, window, cx);
    }

    /// Ends typing on the slide. With `give_back`, the keyboard goes back to the window, if
    /// the editor has it.
    fn end_editing(&mut self, give_back: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.stop(cx);
        if self.is_editing(cx) {
            self.run(Document::end_text, cx);
        }
        if give_back && self.focus_handle.is_focused(window) {
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
        if self.editing(cx).is_some_and(EditingText::is_title) {
            return;
        }
        self.format(|edit| edit.toggle_list(numbered), cx);
    }

    fn edit_with(&mut self, op: fn(&TextEdit) -> TextEdit, cx: &mut Context<Self>) {
        self.wanted_x = None;
        self.apply(op, cx);
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
        self.dragging = false;
        let Some(point) = self.slide_point(event.position, cx) else {
            // The desk around the slide.
            self.end_editing(true, window, cx);
            return;
        };
        let Some((slide, mut hit)) = self.hit(point, cx) else {
            self.end_editing(true, window, cx);
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
                self.wanted_x = None;
                if !self.editing_flow(slide, flow, cx) {
                    self.run(
                        |document| {
                            document.begin_text(slide, flow, pos);
                            Ok(Vec::new())
                        },
                        cx,
                    );
                }
                let extend = event.modifiers.shift && !elsewhere;
                match event.click_count {
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
            Hit::Placeholder(region) => {
                cx.stop_propagation();
                self.wanted_x = None;
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
                self.end_editing(true, window, cx);
                cx.emit(EditorEvent::Said(reason.into()));
            }
            Hit::Nothing => self.end_editing(true, window, cx),
        }
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
        let region = self.editing(cx)?.flow.region()?;
        layout.placeholder_caret(region)
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

/// The box the caret's corners fit in, in the window, on a slide drawn at `slide` there.
fn caret_box(caret: &CaretRect, slide: Bounds<Pixels>) -> Bounds<Pixels> {
    let unit = f32::from(slide.size.width) / SLIDE_WIDTH;
    if unit <= 0.0 {
        return Bounds {
            origin: slide.origin,
            size: Size::default(),
        };
    }
    let corners = caret_corners(caret, unit);
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

fn paint_polygon(
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
    let mut path = PathBuilder::fill();
    path.add_polygon(&points, true);
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

impl Render for SlideEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = cx.entity().downgrade();
        let marks = self.marks(cx);
        let caret_color = cx.theme().caret;
        let selection_color = cx.theme().selection.opacity(0.6);
        let (composition_color, composition_background) =
            (cx.theme().foreground, cx.theme().background);
        let caret_on = self.caret_on;
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
        // What the system is composing, at the caret, where the selection it stands for starts.
        let composing = self.editing(cx).and_then(|editing| {
            let composing = editing.composing.as_ref()?;
            Some((composing.text.clone(), editing.edit.selection.start()))
        });
        let composition = composing.and_then(|(text, at)| {
            let caret = self.caret_in_window(at, cx)?;
            Some((text, in_editor(caret, placed?.1)))
        });
        // While text is typed, the system's input method types it.
        let input = self
            .is_editing(cx)
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
                let Some(marks) = marks else { return };
                let slide = letterbox(area, window.scale_factor());
                let unit = f32::from(slide.size.width) / SLIDE_WIDTH;
                if unit <= 0.0 {
                    return;
                }
                window.with_content_mask(Some(ContentMask { bounds: slide }), |window| {
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
            .key_context(EDITOR_CONTEXT)
            .track_focus(&self.focus_handle)
            .absolute()
            .size_full()
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_action(
                cx.listener(|this, _: &MoveLeft, _, cx| this.go(Go::By(Motion::Left), false, cx)),
            )
            .on_action(
                cx.listener(|this, _: &MoveRight, _, cx| this.go(Go::By(Motion::Right), false, cx)),
            )
            .on_action(
                cx.listener(|this, _: &MoveUp, _, cx| this.go(Go::Line(LineMove::Up), false, cx)),
            )
            .on_action(
                cx.listener(|this, _: &MoveDown, _, cx| {
                    this.go(Go::Line(LineMove::Down), false, cx)
                }),
            )
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
            .on_action(
                cx.listener(|this, _: &LineBreak, _, cx| this.edit_with(TextEdit::soft_break, cx)),
            )
            .on_action(cx.listener(|this, _: &Indent, _, cx| this.edit_with(TextEdit::indent, cx)))
            .on_action(
                cx.listener(|this, _: &Outdent, _, cx| this.edit_with(TextEdit::outdent, cx)),
            )
            .on_action(
                cx.listener(|this, _: &StopEditing, window, cx| this.end_editing(true, window, cx)),
            )
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
                let size = action.size;
                this.format(|edit| edit.set_size(Some(size)), cx)
            }))
            .child(self.canvas.clone())
            .child(overlay)
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
        let (text, range) = utf16_slice(&self.editing(cx)?.system_text(), range);
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
        let Some(editing) = self.editing(cx) else {
            return;
        };
        let composing = editing.composing.is_some();
        // The composition's own range is the selection it stands in for.
        let range = range.filter(|range| Some(range) != editing.marked_range().as_ref());
        if text.is_empty() && range.is_none() {
            // A composition cancelled, or nothing typed: nothing changes.
            if composing {
                self.set_composing(None, cx);
            }
            return;
        }
        let over = range.and_then(|range| self.places(&range, cx));
        self.type_over(over, text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editing) = self.editing(cx) else {
            return;
        };
        let range = range.filter(|range| Some(range) != editing.marked_range().as_ref());
        if let Some((from, to)) = range.and_then(|range| self.places(&range, cx)) {
            // The system composes over text of its own choosing: that's what's selected.
            self.wanted_x = None;
            self.apply(|edit| edit.move_to(from, false).move_to(to, true), cx);
        }
        self.set_composing(Composing::new(text, selected), cx);
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let at = self.editing(cx)?.system_pos(range.start, false);
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
        self.is_editing(cx)
    }
}
