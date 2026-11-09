//! A talk in a window: runs its `stagepack serve` sidecar, feeds what the
//! sidecar and the page say into [`Document`], applies the effects (the trust
//! question, loading the canvas, messages, closing), and draws the canvas area.

use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use async_channel::{Receiver, TryRecvError};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::{ActiveTheme as _, WindowExt as _, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use studio_protocol::{CommandBody, PageMessage};
use studio_sidecar::{Sidecar, SidecarEvent, SpawnOptions};

use crate::canvas::{Canvas, CanvasEvent, DEBUG_IPC};
use crate::document::{DocState, Document, Effect};

/// The folder `stagepack`, its home and the Studio addon come from, until a
/// release bundles them.
pub const DEV_HOME_VAR: &str = "STUDIO_DEV_HOME";
const DEV_HOME_UNSET: &str = "STUDIO_DEV_HOME is not set.";
const STUDIO_ADDON: &str = "runtimes/53/node_modules/slidev-addon-studio";

const TRUST_TITLE: &str = "Trust this presentation?";
const CLOSED_TEXT: &str = "The presentation was closed.";
const UNSAVED_HINT: &str = "Your unsaved changes are still here: use Save before closing.";
const LOST_HINT: &str = "Your unsaved changes could not be saved.";
const STILL_SAVING_TITLE: &str = "Still saving. Close anyway?";
/// How long a Save or close waits for the page to apply its inline editor.
const FINISH_EDIT_TIMEOUT: Duration = Duration::from_secs(2);

/// What the view asked of the sidecar. A document made with
/// [`DocumentView::for_test`] records these instead of sending them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidecarCommand {
    Answer(bool),
    Save,
    Stop,
}

enum Link {
    /// A running `stagepack serve`. Its event channel stays open until serve
    /// has exited: once nothing reads serve's stdout, its last write would
    /// kill it before it cleans up.
    Live {
        sidecar: Sidecar,
        events: Receiver<SidecarEvent>,
    },
    /// No sidecar, in tests: commands are recorded.
    Recorded(Vec<SidecarCommand>),
    /// Never started, or stopped.
    None,
}

pub struct DocumentView {
    document: Document,
    link: Link,
    /// `None` in headless tests, which can't host a web view.
    canvas: Option<Entity<Canvas>>,
    canvas_visible: bool,
    /// The window is going away; its close needs no more asking.
    closing: bool,
    /// What would have gone to the page, for a document with no canvas.
    page_sent: Vec<CommandBody>,
    _finish_timeout: Option<Task<()>>,
    _listen: Option<Task<()>>,
    _subscription: Option<Subscription>,
}

impl DocumentView {
    /// Opens the talk at `path`: spawns `stagepack serve` from
    /// `$STUDIO_DEV_HOME` and shows the page in `canvas` once serve is ready.
    pub fn open(
        path: PathBuf,
        canvas: Entity<Canvas>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut document = Document::new(path);
        canvas.update(cx, |canvas, cx| canvas.set_visible(false, cx));
        let subscription = cx.subscribe_in(&canvas, window, |this, _, event, window, cx| {
            this.on_canvas(event, window, cx)
        });

        let spawned = spawn_options(&document.path).and_then(|options| {
            Sidecar::spawn(options).map_err(|err| format!("Could not start Stagepack: {err}"))
        });
        let (link, listen) = match spawned {
            Ok((sidecar, events)) => {
                let listen = Self::listen(events.clone(), window, cx);
                (Link::Live { sidecar, events }, Some(listen))
            }
            Err(message) => {
                document.state = DocState::Failed {
                    message,
                    log: String::new(),
                };
                (Link::None, None)
            }
        };

        Self {
            document,
            link,
            canvas: Some(canvas),
            canvas_visible: false,
            closing: false,
            page_sent: Vec::new(),
            _finish_timeout: None,
            _listen: listen,
            _subscription: Some(subscription),
        }
    }

    /// A talk with no sidecar and no web view, for headless tests. Drive it
    /// with [`Self::on_sidecar`] and [`Self::on_page`], and read what it
    /// asked of the sidecar from [`Self::sent`].
    pub fn for_test(path: PathBuf, _cx: &mut Context<Self>) -> Self {
        Self {
            document: Document::new(path),
            link: Link::Recorded(Vec::new()),
            canvas: None,
            canvas_visible: false,
            closing: false,
            page_sent: Vec::new(),
            _finish_timeout: None,
            _listen: None,
            _subscription: None,
        }
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    /// The commands a canvas-less test document would have sent the page.
    pub fn page_sent(&self) -> &[CommandBody] {
        &self.page_sent
    }

    /// A command went to the page under `id`.
    pub fn command_sent(&mut self, id: i64) {
        self.document.command_sent(id);
    }

    /// The commands a test document recorded; empty for a real one.
    pub fn sent(&self) -> &[SidecarCommand] {
        match &self.link {
            Link::Recorded(sent) => sent,
            _ => &[],
        }
    }

    pub fn on_sidecar(&mut self, event: SidecarEvent, window: &mut Window, cx: &mut Context<Self>) {
        if *DEBUG_IPC {
            eprintln!("[studio sidecar] {event:?}");
        }
        let effects = self.document.on_sidecar(event);
        self.apply(effects, window, cx);
    }

    pub fn on_canvas(&mut self, event: &CanvasEvent, window: &mut Window, cx: &mut Context<Self>) {
        let effects = match event {
            CanvasEvent::Page(msg) => self.document.on_page(msg),
            CanvasEvent::ProtocolError(err) => self.document.on_protocol_error(err),
            CanvasEvent::SaveShortcut => return self.save(window, cx),
        };
        self.apply(effects, window, cx);
    }

    pub fn on_page(&mut self, msg: &PageMessage, window: &mut Window, cx: &mut Context<Self>) {
        let effects = self.document.on_page(msg);
        self.apply(effects, window, cx);
    }

    /// Ctrl+S and the Save button: gets what is being typed on the page into
    /// the slide, then saves when there is something to save.
    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let effects = self.document.request_save();
        self.apply(effects, window, cx);
    }

    /// Tells serve to save. A close prompt's Save comes here directly: its
    /// close already went through the page.
    fn send_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.document.save_sent();
        if let Err(err) = self.send(SidecarCommand::Save) {
            // As if serve had refused: the message shows, and a close waiting
            // for the save is called off.
            let effects = self.document.on_sidecar(SidecarEvent::SaveFailed {
                message: format!("Could not save: {err}"),
                names: Vec::new(),
            });
            self.apply(effects, window, cx);
        }
    }

    /// The window was asked to close. Returns whether it may close now; if
    /// not, a question is up: save the changes, or stop waiting for a save.
    pub fn request_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.closing {
            return true;
        }
        // Only set while serve runs: stopping or exiting clears it.
        if self.document.closing_after_save() {
            if !window.has_active_dialog(cx) {
                self.ask_still_saving(window, cx);
            }
            return false;
        }
        let effects = self.document.request_close();
        if effects == [Effect::CloseWindow] {
            self.closing = true;
            self.shutdown(cx);
            return true;
        }
        self.apply(effects, window, cx);
        false
    }

    fn listen(events: Receiver<SidecarEvent>, window: &Window, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn_in(window, async move |this, cx| {
            while let Ok(event) = events.recv().await {
                let delivered = cx.update(|window, cx| {
                    this.update(cx, |this, cx| this.on_sidecar(event, window, cx))
                });
                if !matches!(delivered, Ok(Ok(()))) {
                    break;
                }
            }
        })
    }

    fn apply(&mut self, effects: Vec<Effect>, window: &mut Window, cx: &mut Context<Self>) {
        for effect in effects {
            match effect {
                Effect::AskTrust(question) => self.ask_trust(question.into(), window, cx),
                Effect::LoadCanvas { base_url } => {
                    if let Some(canvas) = &self.canvas {
                        canvas.update(cx, |canvas, cx| canvas.load(&base_url, 1, cx));
                    }
                }
                Effect::ShowMessage(message) => show_message(message.into(), window, cx),
                Effect::CloseWindow => self.close_window(window, cx),
                Effect::FinishEdit => self.finish_edit(window, cx),
                Effect::SendSave => self.send_save(window, cx),
                Effect::AskSaveChanges => {
                    if !window.has_active_dialog(cx) {
                        self.ask_save_changes(window, cx);
                    }
                }
            }
        }
        self.sync_canvas(cx);
        cx.notify();
    }

    /// Asks the page to apply its inline editor, and gives it
    /// [`FINISH_EDIT_TIMEOUT`] to answer.
    fn finish_edit(&mut self, window: &Window, cx: &mut Context<Self>) {
        let id = match &self.canvas {
            Some(canvas) => {
                canvas.update(cx, |canvas, cx| canvas.send(CommandBody::FinishEdit, cx))
            }
            None => {
                self.page_sent.push(CommandBody::FinishEdit);
                self.page_sent.len() as i64
            }
        };
        self.document.finish_edit_sent(id);
        let timeout = cx.background_executor().timer(FINISH_EDIT_TIMEOUT);
        self._finish_timeout = Some(cx.spawn_in(window, async move |this, cx| {
            timeout.await;
            cx.update(|window, cx| {
                this.update(cx, |this, cx| {
                    let effects = this.document.finish_edit_timed_out(id);
                    this.apply(effects, window, cx);
                })
            })
            .ok();
        }));
    }

    /// The web view shows only while the talk is open. Otherwise the canvas
    /// area draws the state instead, and the view is hidden, never dropped.
    fn sync_canvas(&mut self, cx: &mut Context<Self>) {
        let visible = matches!(self.document.state, DocState::Ready { .. });
        if visible == self.canvas_visible {
            return;
        }
        self.canvas_visible = visible;
        if let Some(canvas) = &self.canvas {
            canvas.update(cx, |canvas, cx| canvas.set_visible(visible, cx));
        }
    }

    fn send(&mut self, command: SidecarCommand) -> io::Result<()> {
        match &mut self.link {
            Link::Live { sidecar, .. } => match command {
                SidecarCommand::Answer(yes) => sidecar.answer(yes),
                SidecarCommand::Save => sidecar.save(),
                SidecarCommand::Stop => sidecar.stop(),
            },
            Link::Recorded(sent) => {
                sent.push(command);
                Ok(())
            }
            Link::None => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "Stagepack is not running",
            )),
        }
    }

    fn answer_trust(&mut self, yes: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(err) = self.send(SidecarCommand::Answer(yes)) {
            // Serve is gone; its exit, already on its way, says what happened.
            eprintln!("stagepack-studio: could not answer Stagepack: {err}");
        }
        let effects = self.document.answered_trust();
        self.apply(effects, window, cx);
    }

    fn save_and_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.document.can_save() {
            self.document.close_after_save();
            self.send_save(window, cx);
        } else if !self.document.dirty {
            self.close_window(window, cx);
        }
        // Otherwise the talk failed while the question was up: nothing can be
        // saved, and the window stays to show why.
    }

    fn close_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.closing = true;
        self.shutdown(cx);
        window.remove_window();
    }

    /// Stops the sidecar. Serve needs a moment to stop Slidev and clean up,
    /// and `Sidecar`'s drop waits for that, so it happens off the UI thread.
    fn shutdown(&mut self, cx: &mut App) {
        match std::mem::replace(&mut self.link, Link::None) {
            Link::Live { sidecar, events } => {
                let stopped = stop_in_background(sidecar, events);
                cx.default_global::<Stopping>().0.push(stopped);
            }
            Link::Recorded(mut sent) => {
                sent.push(SidecarCommand::Stop);
                self.link = Link::Recorded(sent);
            }
            Link::None => {}
        }
    }

    fn ask_trust(&mut self, question: SharedString, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let answer = |yes: bool| {
                let view = view.clone();
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    window.close_dialog(cx);
                    view.update(cx, |this, cx| this.answer_trust(yes, window, cx))
                        .ok();
                }
            };
            // Serve waits for an answer, so the dialog only goes away with one.
            dialog
                .title(TRUST_TITLE)
                .w(px(560.))
                .keyboard(false)
                .overlay_closable(false)
                .close_button(false)
                .child(
                    div()
                        .id("trust-question")
                        .test_support()
                        .aria_label(question.clone())
                        .text_sm()
                        .child(question.clone()),
                )
                .footer(
                    DialogFooter::new()
                        .child(Button::new("trust-no").label("No").on_click(answer(false)))
                        .child(
                            Button::new("trust-yes")
                                .primary()
                                .label("Yes")
                                .on_click(answer(true)),
                        ),
                )
        });
    }

    fn ask_save_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.weak_entity();
        let title: SharedString = format!("Save changes to {}?", self.document.title).into();
        window.open_dialog(cx, move |dialog, _, _| {
            let choose = |choice: Choice| choose(view.clone(), choice);
            dialog
                .title(
                    div()
                        .id("save-changes-title")
                        .test_support()
                        .aria_label(title.clone())
                        .child(title.clone()),
                )
                .footer(
                    DialogFooter::new()
                        .child(
                            Button::new("save-changes-discard")
                                .label("Don't Save")
                                .on_click(choose(Self::close_window)),
                        )
                        .child(
                            Button::new("save-changes-cancel")
                                .label("Cancel")
                                .on_click(choose(|_, _, _| {})),
                        )
                        .child(
                            Button::new("save-changes-save")
                                .primary()
                                .label("Save")
                                .on_click(choose(Self::save_and_close)),
                        ),
                )
        });
    }

    fn ask_still_saving(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let choose = |choice: Choice| choose(view.clone(), choice);
            dialog
                .title(
                    div()
                        .id("still-saving-title")
                        .test_support()
                        .aria_label(STILL_SAVING_TITLE)
                        .child(STILL_SAVING_TITLE),
                )
                .footer(
                    DialogFooter::new()
                        .child(
                            Button::new("still-saving-close")
                                .label("Close Anyway")
                                .on_click(choose(Self::close_window)),
                        )
                        .child(
                            Button::new("still-saving-wait")
                                .primary()
                                .label("Keep Waiting")
                                .on_click(choose(|_, _, _| {})),
                        ),
                )
        });
    }
}

/// What a dialog's button does to the document, once the dialog has closed.
type Choice = fn(&mut DocumentView, &mut Window, &mut Context<DocumentView>);

fn choose(
    view: WeakEntity<DocumentView>,
    choice: Choice,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, window, cx| {
        window.close_dialog(cx);
        view.update(cx, |this, cx| choice(this, window, cx)).ok();
    }
}

impl Render for DocumentView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let centered = || {
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_3()
                .p_6()
        };
        let note = |id: &'static str, text: SharedString| {
            div()
                .id(id)
                .test_support()
                .aria_label(text.clone())
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(text)
        };

        match &self.document.state {
            DocState::Ready { .. } => div()
                .size_full()
                .when_some(self.canvas.clone(), |this, canvas| this.child(canvas))
                .into_any_element(),
            DocState::Opening | DocState::AwaitingTrust { .. } => {
                let text = format!("Opening {}…", self.document.file_name());
                centered()
                    .child(note("opening", text.into()))
                    .into_any_element()
            }
            DocState::Failed { message, log } => {
                let message: SharedString = message.clone().into();
                centered()
                    .child(
                        div()
                            .id("failed-message")
                            .test_support()
                            .aria_label(message.clone())
                            .max_w(px(640.))
                            .child(message),
                    )
                    .when(self.document.wants_close_prompt(), |this| {
                        this.child(note("failed-unsaved", UNSAVED_HINT.into()))
                    })
                    .when(self.document.changes_lost(), |this| {
                        this.child(note("failed-lost", LOST_HINT.into()))
                    })
                    .when(!log.is_empty(), |this| {
                        let log: SharedString = log.clone().into();
                        this.child(
                            div()
                                .id("failed-log")
                                .test_support()
                                .aria_label(log.clone())
                                .w_full()
                                .max_w(px(900.))
                                .max_h(px(360.))
                                .overflow_y_scroll()
                                .p_3()
                                .rounded_md()
                                .border_1()
                                .border_color(theme.border)
                                .bg(theme.background)
                                .font_family(theme.mono_font_family.clone())
                                .text_xs()
                                .child(log),
                        )
                    })
                    .into_any_element()
            }
            DocState::Closed => centered()
                .child(note("closed", CLOSED_TEXT.into()))
                .into_any_element(),
        }
    }
}

impl Drop for DocumentView {
    fn drop(&mut self) {
        // The window went away without asking (or the app is quitting):
        // still never let `Sidecar`'s drop block the UI thread.
        if let Link::Live { sidecar, events } = std::mem::replace(&mut self.link, Link::None) {
            stop_in_background(sidecar, events);
        }
    }
}

fn show_message(message: SharedString, window: &mut Window, cx: &mut App) {
    window.open_alert_dialog(cx, move |alert, _, _| {
        alert.description(
            div()
                .id("message")
                .test_support()
                .aria_label(message.clone())
                .child(message.clone()),
        )
    });
}

fn spawn_options(deck: &Path) -> Result<SpawnOptions, String> {
    let home = std::env::var_os(DEV_HOME_VAR)
        .filter(|home| !home.is_empty())
        .ok_or_else(|| DEV_HOME_UNSET.to_string())?;
    let home = std::path::absolute(PathBuf::from(home))
        .map_err(|err| format!("{DEV_HOME_VAR} is not usable: {err}"))?;
    Ok(SpawnOptions {
        stagepack: home.join("stagepack"),
        addons: vec![home.join(STUDIO_ADDON)],
        deck: deck.to_path_buf(),
        home,
    })
}

/// Sidecars that are stopping. The app quits only once they have exited, so
/// serve gets to stop Slidev and clean up.
#[derive(Default)]
struct Stopping(Vec<Receiver<()>>);

impl Global for Stopping {}

/// Quits the app once every stopping sidecar has exited. Call it when the
/// last window has closed.
pub fn quit_when_stopped(cx: &mut App) {
    let stopping = std::mem::take(&mut cx.default_global::<Stopping>().0);
    cx.spawn(async move |cx| {
        for stopped in stopping {
            // Closed, never sent to, once the sidecar is gone.
            let _ = stopped.recv().await;
        }
        cx.update(|cx| cx.quit());
    })
    .detach();
}

/// Stops serve on a thread of its own and drops the `Sidecar` there, which
/// waits for serve to exit (and kills it after 10 s). The returned channel
/// closes once that is done.
fn stop_in_background(mut sidecar: Sidecar, events: Receiver<SidecarEvent>) -> Receiver<()> {
    let (done, stopped) = async_channel::bounded::<()>(1);
    let spawned = thread::Builder::new()
        .name("stop-sidecar".into())
        .spawn(move || {
            let _done = done;
            // A sidecar that already exited has no stdin; that's fine.
            let _ = sidecar.stop();
            drop(sidecar);
            // Read to the end until here, so serve never writes into a closed pipe.
            if *DEBUG_IPC {
                log_last_events(&events);
            }
            drop(events);
        });
    if let Err(err) = spawned {
        eprintln!("stagepack-studio: could not stop Stagepack in the background: {err}");
    }
    stopped
}

/// Logs what serve said while it stopped, once nothing else reads its events.
/// Serve has exited by now, so the channel closes soon; the wait is capped
/// anyway, in case something else still holds serve's stdout open.
fn log_last_events(events: &Receiver<SidecarEvent>) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match events.try_recv() {
            Ok(event) => eprintln!("[studio sidecar] {event:?}"),
            Err(TryRecvError::Empty) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => return,
        }
    }
}
