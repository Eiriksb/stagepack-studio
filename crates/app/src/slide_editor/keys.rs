//! The editor's keys: its actions, what they're bound to, and moving the caret and editing
//! with them.

use super::*;

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
        SelectAllThings,
        PlayOrStopVideo,
        TurnOrStopModel,
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
        KeyBinding::new("ctrl-a", SelectAllThings, selecting),
        // Arrow keys move what's selected, a unit at a time, or ten with Shift.
        KeyBinding::new("left", Nudge { dx: -1.0, dy: 0.0 }, selecting),
        KeyBinding::new("right", Nudge { dx: 1.0, dy: 0.0 }, selecting),
        KeyBinding::new("up", Nudge { dx: 0.0, dy: -1.0 }, selecting),
        KeyBinding::new("down", Nudge { dx: 0.0, dy: 1.0 }, selecting),
        KeyBinding::new("shift-left", Nudge { dx: -10.0, dy: 0.0 }, selecting),
        KeyBinding::new("shift-right", Nudge { dx: 10.0, dy: 0.0 }, selecting),
        KeyBinding::new("shift-up", Nudge { dx: 0.0, dy: -10.0 }, selecting),
        KeyBinding::new("shift-down", Nudge { dx: 0.0, dy: 10.0 }, selecting),
        // A video takes no typing: Space plays it, or stops it.
        KeyBinding::new("space", PlayOrStopVideo, Some(VIDEO_CONTEXT)),
        // Nor does a 3D model: Space turns it by hand, or goes back to its still.
        KeyBinding::new("space", TurnOrStopModel, Some(MODEL_CONTEXT)),
    ]);
    crate::text_tools::bind_keys(cx);
}

impl SlideEditor {
    pub(super) fn go(&mut self, go: Go, extend: bool, cx: &mut Context<Self>) {
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

    pub(super) fn enter(&mut self, cx: &mut Context<Self>) {
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

    pub(super) fn edit_with(&mut self, op: fn(&TextEdit) -> TextEdit, cx: &mut Context<Self>) {
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
    pub(super) fn tab(&mut self, back: bool, cx: &mut Context<Self>) {
        if self.in_cell(cx) {
            self.move_in_table(if back { CellMove::Previous } else { CellMove::Next }, cx);
        } else {
            self.edit_with(if back { TextEdit::outdent } else { TextEdit::indent }, cx);
        }
    }

    /// Shift+Enter: a line break, which a cell, one line, has no place for.
    pub(super) fn line_break(&mut self, cx: &mut Context<Self>) {
        if !self.in_cell(cx) {
            self.edit_with(TextEdit::soft_break, cx);
        }
    }

    /// Escape: typing ends. In a cell, its table is selected then, as a box typed in stays
    /// selected.
    pub(super) fn stop_editing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
    pub(super) fn arrow(&mut self, go: Go, beside: Beside, cx: &mut Context<Self>) {
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
}

/// Whether `keystroke` is a shortcut rather than text: with Ctrl, Alt, the platform key or Fn
/// held, apart from Ctrl and Alt together giving a character, which is AltGr typing.
pub(super) fn is_shortcut(keystroke: &Keystroke) -> bool {
    let modifiers = keystroke.modifiers;
    let altgr = modifiers.control && modifiers.alt && keystroke.key_char.is_some();
    (modifiers.control || modifiers.alt || modifiers.platform || modifiers.function) && !altgr
}
