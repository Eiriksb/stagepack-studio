//! The Slidev page in a web view, and the bridge to Studio inside it.
//!
//! Studio's bridge posts JSON through `window.ipc.postMessage`; commands go
//! back through `window.__studioBridge.dispatch`. The page says `ready` on
//! every load, and nothing is sent to it before that.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::LazyLock;

use gpui_kit::*;
use gpui_webview::WebView;
use studio_protocol::{Command, CommandBody, PageMessage, command_script, parse_page_message};

pub enum CanvasEvent {
    Page(PageMessage),
    ProtocolError(String),
    SaveShortcut,
}

/// What the web view hands over from outside GPUI, in the order it happened.
enum Incoming {
    Message(String),
    /// A new document committed. On Linux wry reports WebKit's commit as
    /// `PageLoadEvent::Started`; the old page lives until then.
    LoadCommitted,
    /// Ctrl+S, while the page had the keyboard.
    SaveShortcut,
}

/// `STUDIO_DEBUG_IPC=1` logs the type of each page message, and each sidecar
/// event, to stderr.
pub(crate) static DEBUG_IPC: LazyLock<bool> =
    LazyLock::new(|| std::env::var("STUDIO_DEBUG_IPC").is_ok_and(|v| v == "1"));

/// Call when a window has closed, before GPUI destroys its X11 window (an
/// `on_window_closed` observer runs just before that).
///
/// gpui-webview 0.9.0 on X11: destroying the window also destroys Wry's X11
/// windows inside it, over GPUI's own connection. GTK's later requests for
/// them (the web view's `hide` on drop, then Wry's destroy) fail with
/// `BadWindow`, and GDK exits the process on any X11 error. Upstream's fix
/// (dbb0b9a4, after 0.9.0) covers dropping a web view, not closing its window.
/// So GTK's pending requests go out first, while the windows still exist, and
/// GDK ignores X11 errors from then on. Nothing pops the trap: a closed window
/// leaves nothing GTK draws, and this app quits once its window has closed.
#[cfg(target_os = "linux")]
pub fn window_closing_on_x11() {
    if !gtk::is_initialized_main_thread() {
        return; // No web view was ever built.
    }
    if let Some(display) = gtk::gdk::Display::default() {
        display.sync();
    }
    gtk::gdk::error_trap_push();
}

/// The one web view of a window. It is built once and lives as long as the
/// window: gpui-webview 0.9.0 aborts on X11 when a web view is dropped (see
/// [`window_closing_on_x11`] for the window's own close).
pub struct Canvas {
    webview: Entity<WebView>,
    handshake: Handshake,
    /// Where the page may go: the talk's own server, once one is loaded, as
    /// the origin's serialization (`http://localhost:4000`).
    allowed_origin: Rc<RefCell<Option<String>>>,
}

impl EventEmitter<CanvasEvent> for Canvas {}

impl Canvas {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (tx, rx) = async_channel::unbounded::<Incoming>();
        #[cfg(target_os = "linux")]
        let on_save_key = tx.clone();
        let allowed_origin: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
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
                        let _ = tx.try_send(Incoming::LoadCommitted);
                    }
                });
            // Windows' handler sees the main frame only, so it can keep the
            // web view on the talk's origin as it is.
            #[cfg(not(target_os = "linux"))]
            let builder = {
                let origin = allowed_origin.clone();
                builder.with_navigation_handler(move |url| {
                    let allowed = origin_matches(&url, origin.borrow().as_deref());
                    if !allowed && *DEBUG_IPC {
                        eprintln!("[studio ipc] navigation to {url} blocked");
                    }
                    allowed
                })
            };
            #[cfg(debug_assertions)]
            let builder = builder.with_devtools(true);
            WebView::build(builder, window, cx).expect("Failed to create the web view")
        });
        #[cfg(target_os = "linux")]
        {
            let raw = webview.read(cx).raw();
            watch_save_key(raw, on_save_key);
            // The handler lives in WebKit's `decide-policy` signal, not
            // Wry's: Wry's sees iframes too, and has only the URL to go on.
            keep_on_talk(raw, allowed_origin.clone());
        }

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
            handshake: Handshake::new(),
            allowed_origin,
        }
    }

    /// Loads `<base_url>/<slide>`.
    pub fn load(&mut self, base_url: &str, slide: u32, cx: &mut Context<Self>) {
        let url = format!("{}/{slide}", base_url.trim_end_matches('/'));
        self.load_url(&url, cx);
    }

    /// Loads `url` as it is. Commands sent from now on wait for its `ready`.
    pub fn load_url(&mut self, url: &str, cx: &mut Context<Self>) {
        *self.allowed_origin.borrow_mut() = url::Url::parse(url)
            .ok()
            .map(|url| url.origin().ascii_serialization());
        self.handshake.load_requested();
        self.webview.update(cx, |webview, _| webview.load_url(url));
    }

    /// Shows or hides the native web view. A window that stops rendering the
    /// canvas must hide it, or the page stays on screen where it last was.
    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.webview.update(cx, |webview, _| {
            if visible {
                webview.show();
            } else {
                webview.hide();
            }
        });
    }

    /// Sends a command to the page, or keeps it until the page is ready.
    /// Returns the command's id, which the page's `result` refers to.
    pub fn send(&mut self, body: CommandBody, cx: &mut Context<Self>) -> i64 {
        let (id, now) = self.handshake.send(body);
        if let Some(cmd) = now {
            self.run(&cmd, cx);
        }
        id
    }

    fn receive(&mut self, incoming: Incoming, cx: &mut Context<Self>) {
        let json = match incoming {
            Incoming::Message(json) => json,
            Incoming::LoadCommitted => {
                self.handshake.load_committed();
                return;
            }
            Incoming::SaveShortcut => {
                if *DEBUG_IPC {
                    eprintln!("[studio ipc] save shortcut");
                }
                cx.emit(CanvasEvent::SaveShortcut);
                return;
            }
        };
        match parse_page_message(&json) {
            Ok(msg) => match self.handshake.on_page(&msg) {
                Some(run) => {
                    if *DEBUG_IPC {
                        eprintln!("[studio ipc] {}", message_type(&msg));
                    }
                    for cmd in run {
                        self.run(&cmd, cx);
                    }
                    cx.emit(CanvasEvent::Page(msg));
                }
                None => {
                    if *DEBUG_IPC {
                        eprintln!(
                            "[studio ipc] {} from the page being replaced, dropped",
                            message_type(&msg)
                        );
                    }
                }
            },
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

/// GTK gives keys to the focused web view, so GPUI never sees them while the
/// page has the keyboard. Takes Ctrl+S before WebKit does, and passes it on;
/// every other key, Ctrl+Z included, goes on to the page.
#[cfg(target_os = "linux")]
fn watch_save_key(webview: &wry::WebView, tx: async_channel::Sender<Incoming>) {
    use gtk::glib::Propagation;
    use gtk::prelude::WidgetExt as _;
    use wry::WebViewExtUnix as _;

    webview.webview().connect_key_press_event(move |_, event| {
        if !is_save_key(event.keyval(), event.state()) {
            return Propagation::Proceed;
        }
        // Runs inside GTK's main loop, so it only passes the key on.
        let _ = tx.try_send(Incoming::SaveShortcut);
        Propagation::Stop
    });
}

/// Keeps the web view on the talk: a link or form that leaves the allowed
/// origin is ignored (see [`blocks_navigation`]); frames, reloads and the rest
/// go ahead. Runs inside GTK's main loop and never touches GPUI.
#[cfg(target_os = "linux")]
fn keep_on_talk(webview: &wry::WebView, allowed_origin: Rc<RefCell<Option<String>>>) {
    use webkit2gtk::glib::Cast as _;
    use webkit2gtk::{
        NavigationPolicyDecision, NavigationPolicyDecisionExt as _, NavigationType, PolicyDecisionExt as _, PolicyDecisionType,
        URIRequestExt as _, WebViewExt as _,
    };
    use wry::WebViewExtUnix as _;

    webview
        .webview()
        .connect_decide_policy(move |_, decision, kind| {
            if kind != PolicyDecisionType::NavigationAction {
                return false; // New windows stay as they were.
            }
            let Some(nav) = decision
                .dynamic_cast_ref::<NavigationPolicyDecision>()
                .and_then(|d| d.navigation_action())
            else {
                return false;
            };
            let Some(url) = nav.request().and_then(|r| r.uri()) else {
                return false;
            };
            let kind = match nav.navigation_type() {
                NavigationType::LinkClicked => NavKind::LinkClick,
                NavigationType::FormSubmitted | NavigationType::FormResubmitted => {
                    NavKind::FormSubmit
                }
                _ => NavKind::Other,
            };
            if !blocks_navigation(kind, &url, allowed_origin.borrow().as_deref()) {
                return false; // WebKit's own decision: go ahead.
            }
            if *DEBUG_IPC {
                eprintln!("[studio ipc] navigation to {url} blocked");
            }
            decision.ignore();
            true
        });
}

/// Ctrl+S with no other modifier held. Caps Lock and Num Lock don't count.
#[cfg(target_os = "linux")]
fn is_save_key(key: gtk::gdk::keys::Key, state: gtk::gdk::ModifierType) -> bool {
    use gtk::gdk::ModifierType as Held;

    let held = state
        & (Held::CONTROL_MASK
            | Held::SHIFT_MASK
            | Held::MOD1_MASK
            | Held::SUPER_MASK
            | Held::META_MASK);
    held == Held::CONTROL_MASK && key.to_lower() == gtk::gdk::keys::constants::s
}

/// The `ready` handshake and the commands waiting for it, kept apart from
/// the web view so it can be tested headlessly.
struct Handshake {
    next_id: i64,
    /// The page has said `ready` since the last document committed.
    ready: bool,
    /// `load` was called and its document hasn't committed yet. Until it
    /// does, the old page is alive and may still say `ready`. A bool, not a
    /// count: a second `load` cancels the first, so only one commit comes.
    awaiting_commit: bool,
    pending: Vec<CommandBody>,
}

impl Handshake {
    fn new() -> Self {
        Self {
            next_id: 1,
            ready: false,
            awaiting_commit: false,
            pending: Vec::new(),
        }
    }

    /// Gives the command the next id. Returns that id, and the command to
    /// run now if the page is ready; otherwise the command is queued.
    fn send(&mut self, body: CommandBody) -> (i64, Option<Command>) {
        let id = self.next_id;
        self.next_id += 1;
        if self.ready {
            (id, Some(Command { id, body }))
        } else {
            self.pending.push(body);
            (id, None)
        }
    }

    /// `load` was called.
    fn load_requested(&mut self) {
        self.ready = false;
        self.awaiting_commit = true;
    }

    /// A document committed: the one `load` asked for, or a reload. The old
    /// page is gone and the new one hasn't said `ready` yet.
    fn load_committed(&mut self) {
        self.ready = false;
        self.awaiting_commit = false;
    }

    /// Takes a message from the page. Returns the commands to run before the
    /// message is passed on, or `None` to drop it: a `ready` from the page
    /// that a `load` is replacing. Other messages from that page still count
    /// (a late `edited` is a real edit).
    fn on_page(&mut self, msg: &PageMessage) -> Option<Vec<Command>> {
        if !matches!(msg, PageMessage::Ready { .. }) {
            return Some(Vec::new());
        }
        if self.awaiting_commit {
            return None;
        }
        self.ready = true;
        Some(queued_commands(
            self.next_id,
            std::mem::take(&mut self.pending),
        ))
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

/// What kind of navigation WebKit is deciding on. Only the person's own
/// clicks and submissions can take the web view off the talk; a frame loading
/// inside a slide (an iframe, a video embed) is the slide's own content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
enum NavKind {
    /// A link was clicked.
    LinkClick,
    /// A form was submitted or resubmitted.
    FormSubmit,
    /// Anything else: a frame loading, a reload, history, script.
    Other,
}

/// Whether `url` is on the talk's own origin (scheme, host and port, compared
/// parsed, so a look-alike host doesn't pass), or is the blank page the web
/// view starts on. `allowed` is the loaded URL or its origin, `None` until
/// something is loaded.
fn origin_matches(url: &str, allowed: Option<&str>) -> bool {
    if url == "about:blank" {
        return true;
    }
    let (Some(allowed), Ok(url)) = (allowed, url::Url::parse(url)) else {
        return false;
    };
    url::Url::parse(allowed).is_ok_and(|allowed| url.origin() == allowed.origin())
}

/// Whether to stop a navigation. A link or a form that leaves the talk's
/// origin is blocked: it would take the only web view off Slidev with no way
/// back, and hand that site `window.ipc`. Everything else goes ahead, frames
/// included: `layout: iframe`, `<Youtube>` and `<Tweet>` load other origins
/// in a frame, and `about:srcdoc` frames are the page's own. Nothing is
/// blocked before the first load.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn blocks_navigation(kind: NavKind, url: &str, allowed_origin: Option<&str>) -> bool {
    let clicked = matches!(kind, NavKind::LinkClick | NavKind::FormSubmit);
    clicked
        && allowed_origin.is_some()
        && url != "about:srcdoc"
        && !origin_matches(url, allowed_origin)
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
    use super::{Handshake, NavKind, blocks_navigation, origin_matches, queued_commands};
    use studio_protocol::{Command, CommandBody, PageMessage};

    fn ready() -> PageMessage {
        PageMessage::Ready {
            studio: "0.0.0".into(),
            slide: 2,
            total: 3,
        }
    }

    fn cmd(id: i64, body: CommandBody) -> Command {
        Command { id, body }
    }

    /// A handshake with a page up and ready, and no load in flight.
    fn page_up() -> Handshake {
        let mut h = Handshake::new();
        h.load_requested();
        h.load_committed();
        assert_eq!(h.on_page(&ready()), Some(Vec::new()));
        h
    }

    #[test]
    fn ready_while_awaiting_commit_is_dropped_and_flushes_nothing() {
        let mut h = page_up();
        h.load_requested();
        assert_eq!(h.send(CommandBody::Deselect), (1, None));
        // The old page is alive until the new document commits, and says ready.
        assert_eq!(h.on_page(&ready()), None);
        // Still not ready: the next command waits as well.
        assert_eq!(h.send(CommandBody::Undo), (2, None));

        h.load_committed();
        assert_eq!(
            h.on_page(&ready()),
            Some(vec![
                cmd(1, CommandBody::Deselect),
                cmd(2, CommandBody::Undo)
            ])
        );
    }

    #[test]
    fn commit_then_ready_flushes_in_order_then_sends_directly() {
        let mut h = Handshake::new();
        h.load_requested();
        assert_eq!(h.send(CommandBody::Go { slide: 2 }), (1, None));
        assert_eq!(h.send(CommandBody::Deselect), (2, None));
        h.load_committed();
        assert_eq!(h.send(CommandBody::Undo), (3, None));

        assert_eq!(
            h.on_page(&ready()),
            Some(vec![
                cmd(1, CommandBody::Go { slide: 2 }),
                cmd(2, CommandBody::Deselect),
                cmd(3, CommandBody::Undo),
            ])
        );
        assert_eq!(
            h.send(CommandBody::Redo),
            (4, Some(cmd(4, CommandBody::Redo)))
        );
    }

    #[test]
    fn two_loads_then_one_commit_then_ready_flushes_once() {
        let mut h = page_up();
        h.load_requested();
        assert_eq!(h.send(CommandBody::Deselect), (1, None));
        // The second load cancels the first; only its document commits.
        h.load_requested();
        assert_eq!(h.send(CommandBody::Undo), (2, None));
        assert_eq!(h.on_page(&ready()), None, "the old page's ready");

        h.load_committed();
        assert_eq!(
            h.on_page(&ready()),
            Some(vec![
                cmd(1, CommandBody::Deselect),
                cmd(2, CommandBody::Undo)
            ])
        );
        assert_eq!(h.on_page(&ready()), Some(Vec::new()), "nothing twice");
    }

    #[test]
    fn other_messages_while_awaiting_commit_are_passed_on() {
        let mut h = page_up();
        h.load_requested();
        let edited = PageMessage::Edited {
            slide: 2,
            label: "Move".into(),
        };
        let result = PageMessage::Result {
            id: 1,
            ok: true,
            error: None,
        };
        assert_eq!(h.on_page(&edited), Some(Vec::new()));
        assert_eq!(h.on_page(&result), Some(Vec::new()));
        assert_eq!(h.send(CommandBody::Undo), (1, None), "still not ready");
    }

    #[test]
    fn a_reload_waits_for_the_new_pages_ready() {
        let mut h = page_up();
        assert_eq!(
            h.send(CommandBody::Undo),
            (1, Some(cmd(1, CommandBody::Undo)))
        );
        // Slidev reloads the page by itself: a commit with no load.
        h.load_committed();
        assert_eq!(h.send(CommandBody::Redo), (2, None));
        assert_eq!(h.on_page(&ready()), Some(vec![cmd(2, CommandBody::Redo)]));
    }

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

    #[test]
    fn only_the_talks_own_origin_matches() {
        let allowed = Some("http://localhost:4000");
        for ok in [
            "http://localhost:4000/",
            "http://localhost:4000/2?clicks=1#x",
            "about:blank",
        ] {
            assert!(origin_matches(ok, allowed), "{ok}");
        }
        for other in [
            "http://localhost:4000.evil.example/",
            "http://localhost:4000@evil.example/",
            "http://localhost:4001/",
            "https://localhost:4000/",
            "http://127.0.0.1:4000/",
            "https://example.org/some/page",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "not a url",
            "",
        ] {
            assert!(!origin_matches(other, allowed), "{other}");
        }
    }

    #[test]
    fn before_a_load_only_the_blank_page_matches() {
        assert!(origin_matches("about:blank", None));
        assert!(!origin_matches("http://localhost:4000/", None));
    }

    const TALK: Option<&str> = Some("http://localhost:4000");

    #[test]
    fn a_cross_origin_link_click_is_blocked() {
        assert!(blocks_navigation(
            NavKind::LinkClick,
            "https://example.org/some/page",
            TALK
        ));
    }

    #[test]
    fn a_same_origin_link_click_is_allowed() {
        assert!(!blocks_navigation(
            NavKind::LinkClick,
            "http://localhost:4000/2?clicks=1#x",
            TALK
        ));
    }

    #[test]
    fn a_cross_origin_frame_load_is_allowed() {
        assert!(!blocks_navigation(
            NavKind::Other,
            "https://www.youtube.com/embed/abc",
            TALK
        ));
    }

    #[test]
    fn a_srcdoc_frame_is_allowed() {
        assert!(!blocks_navigation(NavKind::Other, "about:srcdoc", TALK));
        assert!(!blocks_navigation(NavKind::LinkClick, "about:srcdoc", TALK));
    }

    #[test]
    fn a_look_alike_host_link_click_is_blocked() {
        assert!(blocks_navigation(
            NavKind::LinkClick,
            "http://localhost:4000.evil.example/",
            TALK
        ));
    }

    #[test]
    fn a_cross_origin_form_submission_is_blocked() {
        assert!(blocks_navigation(
            NavKind::FormSubmit,
            "https://example.org/post",
            TALK
        ));
    }

    #[test]
    fn nothing_is_blocked_before_the_first_load() {
        for kind in [NavKind::LinkClick, NavKind::FormSubmit, NavKind::Other] {
            assert!(!blocks_navigation(kind, "https://example.org/", None));
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn only_a_plain_ctrl_s_is_the_save_key() {
        use super::is_save_key;
        use gtk::gdk::ModifierType as Held;
        use gtk::gdk::keys::constants as key;

        assert!(is_save_key(key::s, Held::CONTROL_MASK));
        assert!(
            is_save_key(key::S, Held::CONTROL_MASK | Held::LOCK_MASK),
            "Caps Lock"
        );
        assert!(
            is_save_key(key::s, Held::CONTROL_MASK | Held::MOD2_MASK),
            "Num Lock"
        );

        assert!(!is_save_key(key::s, Held::empty()), "typing an s");
        assert!(!is_save_key(key::z, Held::CONTROL_MASK), "undo");
        assert!(
            !is_save_key(key::S, Held::CONTROL_MASK | Held::SHIFT_MASK),
            "Ctrl+Shift+S"
        );
        assert!(
            !is_save_key(key::s, Held::CONTROL_MASK | Held::MOD1_MASK),
            "Ctrl+Alt+S"
        );
    }
}
