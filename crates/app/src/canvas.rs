//! The Slidev page in a web view, and the bridge to Studio inside it.
//!
//! Studio's bridge posts JSON through `window.ipc.postMessage`; commands go
//! back through `window.__studioBridge.dispatch`. The page says `ready` on
//! every load, and nothing is sent to it before that.

use std::sync::LazyLock;

use gpui_kit::*;
use gpui_webview::WebView;
use studio_protocol::{Command, CommandBody, PageMessage, command_script, parse_page_message};

pub enum CanvasEvent {
    Page(PageMessage),
    ProtocolError(String),
}

/// What the web view hands over from outside GPUI, in the order it happened.
enum Incoming {
    Message(String),
    LoadStarted,
}

/// `STUDIO_DEBUG_IPC=1` logs the type of each page message to stderr.
static DEBUG_IPC: LazyLock<bool> =
    LazyLock::new(|| std::env::var("STUDIO_DEBUG_IPC").is_ok_and(|v| v == "1"));

/// The one web view of a window. It is built once and lives as long as the
/// window: gpui-webview 0.9.0 aborts on X11 when a web view is dropped.
pub struct Canvas {
    webview: Entity<WebView>,
    next_id: i64,
    ready: bool,
    pending: Vec<CommandBody>,
}

impl EventEmitter<CanvasEvent> for Canvas {}

impl Canvas {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (tx, rx) = async_channel::unbounded::<Incoming>();
        let webview = cx.new(|cx| {
            // Both handlers run on the UI thread but outside GPUI, so they only
            // pass what happened on to the loop below.
            let on_message = tx.clone();
            let builder = wry::WebViewBuilder::new()
                .with_ipc_handler(move |request| {
                    let _ = on_message.try_send(Incoming::Message(request.into_body()));
                })
                .with_on_page_load_handler(move |event, _url| {
                    if matches!(event, wry::PageLoadEvent::Started) {
                        let _ = tx.try_send(Incoming::LoadStarted);
                    }
                });
            #[cfg(debug_assertions)]
            let builder = builder.with_devtools(true);
            WebView::build(builder, window, cx).expect("Failed to create the web view")
        });

        cx.spawn(async move |this, cx| {
            while let Ok(incoming) = rx.recv().await {
                if this
                    .update(cx, |this, cx| this.receive(incoming, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        Self {
            webview,
            next_id: 1,
            ready: false,
            pending: Vec::new(),
        }
    }

    /// Loads `<base_url>/<slide>`.
    pub fn load(&mut self, base_url: &str, slide: u32, cx: &mut Context<Self>) {
        let url = format!("{}/{slide}", base_url.trim_end_matches('/'));
        self.load_url(&url, cx);
    }

    /// Loads `url` as it is. Commands sent from now on wait for its `ready`.
    pub fn load_url(&mut self, url: &str, cx: &mut Context<Self>) {
        self.ready = false;
        self.webview.update(cx, |webview, _| webview.load_url(url));
    }

    /// Sends a command to the page, or keeps it until the page is ready.
    /// Returns the command's id, which the page's `result` refers to.
    pub fn send(&mut self, body: CommandBody, cx: &mut Context<Self>) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        if self.ready {
            self.run(&Command { id, body }, cx);
        } else {
            self.pending.push(body);
        }
        id
    }

    fn receive(&mut self, incoming: Incoming, cx: &mut Context<Self>) {
        let json = match incoming {
            Incoming::Message(json) => json,
            // A reload or navigation: the old page is going away.
            Incoming::LoadStarted => {
                self.ready = false;
                return;
            }
        };
        match parse_page_message(&json) {
            Ok(msg) => {
                if *DEBUG_IPC {
                    eprintln!("[studio ipc] {}", message_type(&msg));
                }
                if matches!(msg, PageMessage::Ready { .. }) {
                    self.ready = true;
                    let pending = std::mem::take(&mut self.pending);
                    for cmd in queued_commands(self.next_id, pending) {
                        self.run(&cmd, cx);
                    }
                }
                cx.emit(CanvasEvent::Page(msg));
            }
            Err(err) => {
                if *DEBUG_IPC {
                    eprintln!("[studio ipc] bad message ({err}): {json}");
                }
                cx.emit(CanvasEvent::ProtocolError(err.to_string()));
            }
        }
    }

    fn run(&self, cmd: &Command, cx: &mut Context<Self>) {
        let script = command_script(cmd);
        if let Err(err) = self.webview.read(cx).raw().evaluate_script(&script) {
            eprintln!("stagepack-studio: could not send command {}: {err}", cmd.id);
        }
    }
}

impl Render for Canvas {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.webview.clone())
    }
}

/// The queued commands with their ids. While the page isn't ready every send
/// is queued, so the queue holds the most recent ids, oldest first.
fn queued_commands(next_id: i64, pending: Vec<CommandBody>) -> Vec<Command> {
    let first = next_id - pending.len() as i64;
    pending
        .into_iter()
        .zip(first..)
        .map(|(body, id)| Command { id, body })
        .collect()
}

fn message_type(msg: &PageMessage) -> &'static str {
    match msg {
        PageMessage::Ready { .. } => "ready",
        PageMessage::Selection { .. } => "selection",
        PageMessage::History { .. } => "history",
        PageMessage::Edited { .. } => "edited",
        PageMessage::Slide { .. } => "slide",
        PageMessage::Error { .. } => "error",
        PageMessage::Result { .. } => "result",
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: with test-support, `gpui_kit::*` brings a `test` macro
    // that shadows the built-in one.
    use super::queued_commands;
    use studio_protocol::CommandBody;

    #[test]
    fn queued_commands_keep_their_ids_in_order() {
        // Ids 1 and 2 went out; 3, 4 and 5 were queued; 6 is next.
        let pending = vec![
            CommandBody::Deselect,
            CommandBody::Go { slide: 2 },
            CommandBody::Undo,
        ];
        let ids: Vec<(i64, CommandBody)> = queued_commands(6, pending)
            .into_iter()
            .map(|c| (c.id, c.body))
            .collect();
        assert_eq!(
            ids,
            vec![
                (3, CommandBody::Deselect),
                (4, CommandBody::Go { slide: 2 }),
                (5, CommandBody::Undo),
            ]
        );
    }

    #[test]
    fn an_empty_queue_sends_nothing() {
        assert!(queued_commands(1, Vec::new()).is_empty());
    }
}
