//! The trust and close dialogs, driven through a document with no sidecar and
//! no web view. `DocumentView::for_test` records what it would have asked of
//! the sidecar, and the tests feed it the sidecar's and the page's events.

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::test::{TestAppContextExt, TestWindowExt};
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Entity, Point, TestAppContext, VisualTestContext,
    WindowBounds, WindowOptions, px, size,
};
use stagepack_studio::document::NOT_TRUSTED_MESSAGE;
use stagepack_studio::document_view::{DocumentView, SidecarCommand};
use stagepack_studio::workspace_view::{self, WorkspaceView};
use studio_protocol::PageMessage;
use studio_sidecar::SidecarEvent;

const WAIT: Duration = Duration::from_secs(1);

fn open_document(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<DocumentView>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_reduce_motion(true);
        workspace_view::bind_keys(cx);
    });
    cx.update(|cx| {
        let bounds = Bounds {
            origin: Point::default(),
            size: size(px(1280.), px(800.)),
        };
        let (window, view) = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            cx,
            |window, cx| {
                cx.new(|cx| {
                    WorkspaceView::for_test_with_document(
                        PathBuf::from("/talks/hello.sptx"),
                        window,
                        cx,
                    )
                })
            },
        )
        .expect("open test window");
        let document = view.read(cx).document_view().expect("a document").clone();
        (window, document)
    })
}

fn sidecar(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    doc: &Entity<DocumentView>,
    event: SidecarEvent,
) {
    cx.update_window(handle, |_, window, cx| {
        doc.update(cx, |doc, cx| doc.on_sidecar(event, window, cx));
    })
    .unwrap();
}

fn page(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    doc: &Entity<DocumentView>,
    msg: PageMessage,
) {
    cx.update_window(handle, |_, window, cx| {
        doc.update(cx, |doc, cx| doc.on_page(&msg, window, cx));
    })
    .unwrap();
}

fn sent(cx: &mut TestAppContext, doc: &Entity<DocumentView>) -> Vec<SidecarCommand> {
    cx.update(|cx| doc.read(cx).sent().to_vec())
}

fn is_open(cx: &mut TestAppContext, handle: AnyWindowHandle) -> bool {
    cx.update(|cx| cx.windows().contains(&handle))
}

/// What the window manager's close button does: asks the window first.
fn request_close(cx: &mut TestAppContext, handle: AnyWindowHandle) -> bool {
    VisualTestContext::from_window(handle, cx).simulate_close()
}

fn ready() -> SidecarEvent {
    SidecarEvent::Ready {
        port: 40755,
        base_url: "http://127.0.0.1:40755".into(),
        work_dir: PathBuf::from("/tmp/work"),
    }
}

/// A talk that is open, titled, and has an unsaved edit.
fn open_dirty(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<DocumentView>) {
    let (handle, doc) = open_document(cx);
    sidecar(
        cx,
        handle,
        &doc,
        SidecarEvent::Unpacked {
            id: "abc".into(),
            title: "Hello fixture".into(),
            entry: "slides.md".into(),
            work_dir: PathBuf::from("/tmp/work"),
            code: Vec::new(),
            trusted: true,
        },
    );
    sidecar(cx, handle, &doc, ready());
    page(
        cx,
        handle,
        &doc,
        PageMessage::Edited {
            slide: 2,
            label: "Move".into(),
        },
    );
    (handle, doc)
}

/// Closes the window with the close prompt's `button`, once the prompt is up.
async fn answer_close_prompt(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    button: &'static str,
) {
    assert!(
        !request_close(cx, handle),
        "a dirty talk asks before closing"
    );
    cx.wait_for(handle, WAIT, |window, _| window.try_find(button).is_some())
        .await;
    cx.update_window(handle, |_, window, cx| {
        window.within("dialog").click(button, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
async fn trust_dialog_shows_question_and_buttons(cx: &mut TestAppContext) {
    let (handle, doc) = open_document(cx);
    let question = "\"Hello fixture\" (hello.sptx) contains code.\n\n\
                    Yes: show the talk.\nNo: close it without running anything.";
    sidecar(
        cx,
        handle,
        &doc,
        SidecarEvent::Confirm {
            question: question.into(),
        },
    );
    cx.wait_for(handle, WAIT, |window, _| {
        window.try_find("trust-yes").is_some()
    })
    .await;
    cx.update_window(handle, |_, window, cx| {
        let mut dialog = window.within("dialog");
        assert_eq!(dialog.find("trust-question").label(), Some(question));
        assert!(dialog.find("trust-yes").visible());
        assert!(dialog.find("trust-no").visible());
        dialog.click("trust-yes", cx);
    })
    .unwrap();
    cx.wait_for(handle, WAIT, |window, _| {
        window.try_find("dialog").is_none()
    })
    .await;
    assert_eq!(sent(cx, &doc), vec![SidecarCommand::Answer(true)]);
}

#[gpui_kit::test]
async fn answering_no_shows_the_talk_was_not_opened(cx: &mut TestAppContext) {
    let (handle, doc) = open_document(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("opening").label(), Some("Opening hello.sptx…"));
    })
    .unwrap();
    sidecar(
        cx,
        handle,
        &doc,
        SidecarEvent::Confirm {
            question: "Trust it?".into(),
        },
    );
    cx.wait_for(handle, WAIT, |window, _| {
        window.try_find("trust-no").is_some()
    })
    .await;
    cx.update_window(handle, |_, window, cx| {
        window.within("dialog").click("trust-no", cx);
    })
    .unwrap();
    cx.wait_for(handle, WAIT, |window, _| {
        window.try_find("dialog").is_none()
    })
    .await;
    assert_eq!(sent(cx, &doc), vec![SidecarCommand::Answer(false)]);

    sidecar(cx, handle, &doc, SidecarEvent::Exited { code: Some(3) });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("failed-message").label(),
            Some(NOT_TRUSTED_MESSAGE)
        );
        assert!(window.try_find("failed-log").is_none(), "no log to show");
    })
    .unwrap();
}

#[gpui_kit::test]
async fn close_when_dirty_prompts(cx: &mut TestAppContext) {
    let (handle, doc) = open_dirty(cx);
    assert!(
        !request_close(cx, handle),
        "a dirty talk asks before closing"
    );
    cx.wait_for(handle, WAIT, |window, _| {
        window.try_find("save-changes-save").is_some()
    })
    .await;
    cx.update_window(handle, |_, window, cx| {
        let mut dialog = window.within("dialog");
        assert_eq!(
            dialog.find("save-changes-title").label(),
            Some("Save changes to Hello fixture?")
        );
        for id in [
            "save-changes-save",
            "save-changes-discard",
            "save-changes-cancel",
        ] {
            assert!(dialog.find(id).visible(), "{id}");
        }
        dialog.click("save-changes-cancel", cx);
    })
    .unwrap();
    cx.wait_for(handle, WAIT, |window, _| {
        window.try_find("dialog").is_none()
    })
    .await;
    assert!(is_open(cx, handle), "Cancel keeps the window");
    assert_eq!(sent(cx, &doc), vec![], "nothing saved or stopped");
    assert!(cx.update(|cx| doc.read(cx).document().dirty));
}

#[gpui_kit::test]
async fn save_in_the_close_prompt_closes_once_saved(cx: &mut TestAppContext) {
    let (handle, doc) = open_dirty(cx);
    answer_close_prompt(cx, handle, "save-changes-save").await;
    assert_eq!(sent(cx, &doc), vec![SidecarCommand::Save]);
    assert!(is_open(cx, handle), "the window waits for the save");

    sidecar(
        cx,
        handle,
        &doc,
        SidecarEvent::Saved {
            updated: "2026-11-09T12:00:00Z".into(),
        },
    );
    cx.run_until_parked();
    assert!(!is_open(cx, handle), "saved, then closed");
    assert_eq!(
        sent(cx, &doc),
        vec![SidecarCommand::Save, SidecarCommand::Stop]
    );
}

#[gpui_kit::test]
async fn a_conflict_while_closing_keeps_the_window(cx: &mut TestAppContext) {
    let (handle, doc) = open_dirty(cx);
    answer_close_prompt(cx, handle, "save-changes-save").await;

    let message = "The file changed on disk since it was opened.";
    sidecar(
        cx,
        handle,
        &doc,
        SidecarEvent::Conflict {
            message: message.into(),
        },
    );
    cx.wait_for(handle, WAIT, |window, _| {
        window.try_find("message").is_some()
    })
    .await;
    cx.update_window(handle, |_, window, _| {
        assert_eq!(
            window.within("dialog").find("message").label(),
            Some(message)
        );
    })
    .unwrap();
    assert!(is_open(cx, handle));
    assert_eq!(sent(cx, &doc), vec![SidecarCommand::Save], "not stopped");
    assert!(cx.update(|cx| doc.read(cx).document().dirty));
}

#[gpui_kit::test]
async fn dont_save_closes(cx: &mut TestAppContext) {
    let (handle, doc) = open_dirty(cx);
    answer_close_prompt(cx, handle, "save-changes-discard").await;
    assert!(!is_open(cx, handle));
    assert_eq!(sent(cx, &doc), vec![SidecarCommand::Stop]);
}

#[gpui_kit::test]
async fn a_clean_talk_closes_without_asking(cx: &mut TestAppContext) {
    let (handle, doc) = open_document(cx);
    sidecar(cx, handle, &doc, ready());
    assert!(request_close(cx, handle));
    assert_eq!(sent(cx, &doc), vec![SidecarCommand::Stop]);
}

#[gpui_kit::test]
async fn ctrl_s_saves_a_dirty_talk(cx: &mut TestAppContext) {
    let (handle, doc) = open_dirty(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("title").label(), Some("Hello fixture"));
        assert!(window.find("dirty").visible(), "the unsaved dot shows");
        assert_ne!(
            window.find("save").disabled(),
            Some(true),
            "Save is enabled"
        );
        window.press("ctrl-s", cx);
    })
    .unwrap();
    assert_eq!(sent(cx, &doc), vec![SidecarCommand::Save]);

    sidecar(
        cx,
        handle,
        &doc,
        SidecarEvent::Saved {
            updated: "2026-11-09T12:00:00Z".into(),
        },
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(!window.find("dirty").visible());
        assert_eq!(window.find("save").disabled(), Some(true));
    })
    .unwrap();
    assert!(is_open(cx, handle), "a plain save doesn't close");
}

#[gpui_kit::test]
async fn failed_state_shows_message_and_log(cx: &mut TestAppContext) {
    let (handle, doc) = open_document(cx);
    sidecar(cx, handle, &doc, ready());
    sidecar(
        cx,
        handle,
        &doc,
        SidecarEvent::Error {
            message: "slidev exited".into(),
            log: "boom\nat line 2".into(),
        },
    );
    sidecar(cx, handle, &doc, SidecarEvent::Stopped);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let message = window.find("failed-message");
        assert!(message.visible());
        assert_eq!(message.label(), Some("slidev exited"));
        let log = window.find("failed-log");
        assert!(log.visible());
        assert_eq!(log.label(), Some("boom\nat line 2"));
    })
    .unwrap();
}
