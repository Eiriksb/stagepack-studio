//! A live terminal on the slide shown: its picture over its layer, *Start*, *Stop* and *Keep as
//! recording* on one selected, and the keys going to it once it's clicked.

use super::*;

use gpui_kit::component::WindowExt as _;

use slides_engine::TerminalLayer;

use crate::live_terminal::{LiveId, gives_keys_back};
use crate::slide_canvas::LivePlace;

/// What a terminal selected that runs a command has: the one that starts it, the one that ends it
/// and the one that keeps what it showed as a recording.
pub const TERMINAL_START_ID: &str = "terminal-start";
pub const TERMINAL_END_ID: &str = "terminal-end";
pub const TERMINAL_KEEP_ID: &str = "terminal-keep";

/// The key context while a live terminal has the keys: none of the editor's keys apply.
pub const LIVE_CONTEXT: &str = "SlideLive";

/// A terminal of the slide shown that runs a command, with who it is.
pub(super) struct Runs {
    pub terminal: TerminalLayer,
    pub id: LiveId,
}

impl SlideEditor {
    /// The terminals on the slide shown that run a command.
    fn running_here(&self, cx: &mut Context<Self>) -> Vec<Runs> {
        let Some(document) = self.document.clone() else { return Vec::new() };
        document.update(cx, |document, _| {
            document
                .terminals_here()
                .into_iter()
                .filter_map(|terminal| {
                    let id = LiveId::new(terminal.source.clone()?, terminal.on.run.clone()?);
                    Some(Runs { terminal, id })
                })
                .collect()
        })
    }

    /// The terminal selected, if it runs a command.
    pub(super) fn selected_runs(&self, cx: &mut Context<Self>) -> Option<Runs> {
        let document = self.document.clone()?;
        let selected = document.update(cx, |document, _| document.selected_terminal())?;
        let id = LiveId::new(selected.source.clone()?, selected.on.run.clone()?);
        Some(Runs { terminal: selected, id })
    }

    /// Lays the running terminals of the slide shown over their layers, unless a show has them.
    pub(super) fn place_live_terminals(&mut self, cx: &mut Context<Self>) {
        let live = self.live.clone();
        // With nothing started, the slide has nothing to lay over.
        let here = if live.show_on() || live.is_idle() { Vec::new() } else { self.running_here(cx) };
        let keys = live.keys();
        let places: Vec<LivePlace> = here
            .iter()
            .filter_map(|runs| {
                Some(LivePlace {
                    layer: runs.terminal.layer,
                    session: live.weak(&runs.id)?,
                    rect: runs.terminal.on.rect,
                    rotate: runs.terminal.on.rotate,
                    keys: keys.as_ref() == Some(&runs.id),
                })
            })
            .collect();
        if places.is_empty() {
            self.canvas.update(cx, |canvas, cx| canvas.clear_live(cx));
            return;
        }
        let Some(document) = self.document.clone() else { return };
        let fonts = document.update(cx, |document, _| document.fonts_seed());
        self.canvas.update(cx, |canvas, cx| canvas.place_live(places, fonts, cx));
    }

    /// Whether a live terminal has the keys, here.
    /// A terminal that isn't drawn here, on the slide shown, has no keys: they go back.
    pub(super) fn live_has_keys(&mut self, cx: &mut Context<Self>) -> bool {
        if self.live.show_on() {
            return false;
        }
        let Some(held) = self.live.keys() else { return false };
        if self.running_here(cx).iter().any(|runs| runs.id == held) {
            return true;
        }
        self.live.release_keys();
        false
    }

    /// *Start*: the window starts the terminal selected, asking first if it has to.
    pub(super) fn start_terminal(&mut self, cx: &mut Context<Self>) {
        cx.emit(EditorEvent::StartTerminal);
    }

    /// *Stop*: ends the program of the terminal selected.
    pub(super) fn end_terminal(&mut self, cx: &mut Context<Self>) {
        if let Some(runs) = self.selected_runs(cx) {
            self.live.end(&runs.id);
            cx.notify();
        }
    }

    /// A press on the slide: on a terminal that runs it gives the terminal the keys, and anywhere
    /// else takes them back. Whatever else the press does it does as before.
    pub(super) fn press_for_keys(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.live.show_on() || event.button != MouseButton::Left {
            return;
        }
        let Some(slide) = self.canvas.read(cx).slide_bounds() else { return };
        let unit = f32::from(slide.size.width) / SLIDE_WIDTH;
        if unit <= 0.0 {
            return;
        }
        let at = (f32::from(event.position.x - slide.origin.x) / unit, f32::from(event.position.y - slide.origin.y) / unit);
        let live = self.live.clone();
        let hit = self
            .running_here(cx)
            .into_iter()
            .rev()
            .find(|runs| live.is_running(&runs.id) && crate::model_view::inside_turned(runs.terminal.on.rect, runs.terminal.on.rotate, at));
        match hit {
            Some(runs) => {
                live.give_keys(&runs.id);
                self.focus_handle.focus(window, cx);
            }
            None => {
                live.release_keys();
            }
        }
        cx.notify();
    }

    /// A key while a live terminal has the keys: it goes to the program, except the one that gives
    /// the keys back, and nothing else, the window's bindings included, takes it.
    fn key_for_terminal(&mut self, key: &Keystroke, window: &mut Window, cx: &mut Context<Self>) {
        // A question open: the keys are its, and the terminal has none from now on.
        if self.live.keys().is_some() && window.has_active_dialog(cx) {
            self.live.release_keys();
            return;
        }
        if !self.live_has_keys(cx) {
            return;
        }
        if gives_keys_back(key) {
            self.live.release_keys();
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // Whatever it sends, nothing else, the window's bindings included, takes the key.
        self.live.send(key);
        cx.stop_propagation();
    }

    /// Has the editor take the keys before any binding of the window does, while a terminal has
    /// them and the editor is where the keyboard is.
    pub(super) fn watch_keys_for_terminal(&mut self, cx: &mut Context<Self>) {
        let editor = cx.entity().downgrade();
        let focus = self.focus_handle.clone();
        let watching = cx.intercept_keystrokes(move |event, window, cx| {
            if !focus.is_focused(window) {
                return;
            }
            editor.update(cx, |editor, cx| editor.key_for_terminal(&event.keystroke, window, cx)).ok();
        });
        self._subscriptions.push(watching);
    }
}
