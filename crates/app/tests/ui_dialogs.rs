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
use stagepack_studio::canvas::CanvasEvent;
use stagepack_studio::document::NOT_TRUSTED_MESSAGE;
use stagepack_studio::document_view::{DocumentView, SidecarCommand};
use stagepack_studio::workspace_view::{self, WorkspaceView};
use studio_protocol::{CommandBody, PageMessage};
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

fn saved() -> SidecarEvent {
    SidecarEvent::Saved {
        updated: "2026-11-09T12:00:00Z".into(),
    }
}

fn edited() -> PageMessage {
    PageMessage::Edited {
        slide: 2,
        label: "Move".into(),
    }
}

fn is_dirty(cx: &mut TestAppContext, doc: &Entity<DocumentView>) -> bool {
    cx.update(|cx| doc.read(cx).document().dirty)
}

#[gpui_kit::test]
async fn an_edit_during_save_then_close_asks_again(cx: &mut TestAppContext) {
    let (handle, doc) = open_dirty(cx);
    answer_close_prompt(cx, handle, "save-changes-save").await;
    // The page wrote after serve had read the files, so the save misses it.
    page(cx, handle, &doc, edited());
    sidecar(cx, handle, &doc, saved());
    cx.run_until_parked();
    assert!(is_open(cx, handle), "the window stays over an unsaved edit");
    assert!(is_dirty(cx, &doc));
    cx.wait_for(handle, WAIT, |window, _| {
        window.try_find("save-changes-save").is_some()
    })
    .await;
}

#[gpui_kit::test]
async fn a_second_close_while_saving_offers_to_close_anyway(cx: &mut TestAppContext) {
    let (handle, doc) = open_dirty(cx);
    answer_close_prompt(cx, handle, "save-changes-save").await;

    assert!(!request_close(cx, handle));
    cx.wait_for(handle, WAIT, |window, _| {
        window.try_find("still-saving-wait").is_some()
    })
    .await;
    cx.update_window(handle, |_, window, cx| {
        let mut dialog = window.within("dialog");
        assert_eq!(
            dialog.find("still-saving-title").label(),
            Some("Still saving. Close anyway?")
        );
        assert!(dialog.find("still-saving-close").visible());
        dialog.click("still-saving-wait", cx);
    })
    .unwrap();
    cx.wait_for(handle, WAIT, |window, _| {
        window.try_find("dialog").is_none()
    })
    .await;
    assert!(is_open(cx, handle), "Keep Waiting keeps the window");
    assert_eq!(sent(cx, &doc), vec![SidecarCommand::Save]);

    assert!(!request_close(cx, handle));
    cx.wait_for(handle, WAIT, |window, _| {
        window.try_find("still-saving-close").is_some()
    })
    .await;
    cx.update_window(handle, |_, window, cx| {
        window.within("dialog").click("still-saving-close", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(!is_open(cx, handle), "Close Anyway closes");
    assert_eq!(
        sent(cx, &doc),
        vec![SidecarCommand::Save, SidecarCommand::Stop]
    );
}

#[gpui_kit::test]
async fn a_stop_during_save_then_close_lets_the_window_close(cx: &mut TestAppContext) {
    let (handle, doc) = open_dirty(cx);
    answer_close_prompt(cx, handle, "save-changes-save").await;
    // Serve answered a signal with `stopped` and never said `saved`.
    sidecar(cx, handle, &doc, SidecarEvent::Stopped);
    sidecar(cx, handle, &doc, SidecarEvent::Exited { code: Some(0) });
    assert!(request_close(cx, handle), "nothing is waiting any more");
}

#[gpui_kit::test]
async fn a_failed_talk_with_edits_still_offers_save(cx: &mut TestAppContext) {
    let (handle, doc) = open_dirty(cx);
    // Serve keeps serving after an unreadable line.
    sidecar(
        cx,
        handle,
        &doc,
        SidecarEvent::Error {
            message: "Unreadable message from Stagepack: x".into(),
            log: String::new(),
        },
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("failed-message").visible());
        assert_eq!(
            window.find("failed-unsaved").label(),
            Some("Your unsaved changes are still here: use Save before closing.")
        );
        assert_ne!(window.find("save").disabled(), Some(true), "Save works");
    })
    .unwrap();

    assert!(!request_close(cx, handle), "closing asks first");
    cx.wait_for(handle, WAIT, |window, _| {
        window.try_find("save-changes-cancel").is_some()
    })
    .await;
    cx.update_window(handle, |_, window, cx| {
        window.within("dialog").click("save-changes-cancel", cx);
    })
    .unwrap();
    cx.wait_for(handle, WAIT, |window, _| {
        window.try_find("dialog").is_none()
    })
    .await;

    // Once serve has stopped, its work folder is gone: nothing to save or ask.
    sidecar(cx, handle, &doc, SidecarEvent::Stopped);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("failed-unsaved").is_none());
        assert_eq!(window.find("save").disabled(), Some(true));
    })
    .unwrap();
    assert!(request_close(cx, handle));
}

#[gpui_kit::test]
async fn ctrl_s_in_the_page_saves(cx: &mut TestAppContext) {
    let (handle, doc) = open_dirty(cx);
    // What the canvas emits when the web view has focus and sees Ctrl+S.
    cx.update_window(handle, |_, window, cx| {
        doc.update(cx, |doc, cx| {
            doc.on_canvas(&CanvasEvent::SaveShortcut, window, cx)
        });
    })
    .unwrap();
    assert_eq!(sent(cx, &doc), vec![SidecarCommand::Save]);
}

// --- Text being typed in place goes first (finish-edit).

const FINISH_EDIT_TIMEOUT: Duration = Duration::from_secs(2);

fn page_ready() -> PageMessage {
    PageMessage::Ready {
        studio: "0.0.0".into(),
        slide: 1,
        total: 2,
    }
}

/// A talk open in the canvas, with the page up. Commands sent to it get ids from 1.
fn open_page(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<DocumentView>) {
    let (handle, doc) = open_document(cx);
    sidecar(cx, handle, &doc, ready());
    page(cx, handle, &doc, page_ready());
    (handle, doc)
}

fn result_ok(id: i64) -> PageMessage {
    PageMessage::Result {
        id,
        ok: true,
        error: None,
    }
}

fn page_sent(cx: &mut TestAppContext, doc: &Entity<DocumentView>) -> Vec<CommandBody> {
    cx.update(|cx| doc.read(cx).page_sent().to_vec())
}

fn press_ctrl_s(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press("ctrl-s", cx);
    })
    .unwrap();
}

#[gpui_kit::test]
async fn ctrl_s_with_text_being_typed_saves_it(cx: &mut TestAppContext) {
    let (handle, doc) = open_page(cx);
    press_ctrl_s(cx, handle);
    assert_eq!(page_sent(cx, &doc), vec![CommandBody::FinishEdit]);
    assert_eq!(sent(cx, &doc), vec![], "the page goes first");

    // The page applies the editor: `edited`, then the result.
    page(cx, handle, &doc, edited());
    page(cx, handle, &doc, result_ok(1));
    assert_eq!(sent(cx, &doc), vec![SidecarCommand::Save]);
}

#[gpui_kit::test]
async fn ctrl_s_in_the_page_finishes_the_edit_too(cx: &mut TestAppContext) {
    let (handle, doc) = open_page(cx);
    cx.update_window(handle, |_, window, cx| {
        doc.update(cx, |doc, cx| {
            doc.on_canvas(&CanvasEvent::SaveShortcut, window, cx)
        });
    })
    .unwrap();
    assert_eq!(page_sent(cx, &doc), vec![CommandBody::FinishEdit]);
    page(cx, handle, &doc, edited());
    page(cx, handle, &doc, result_ok(1));
    assert_eq!(sent(cx, &doc), vec![SidecarCommand::Save]);
}

#[gpui_kit::test]
async fn a_dead_page_cannot_hold_up_ctrl_s(cx: &mut TestAppContext) {
    let (handle, doc) = open_page(cx);
    page(cx, handle, &doc, edited());
    press_ctrl_s(cx, handle);
    cx.run_until_parked();
    assert_eq!(sent(cx, &doc), vec![], "still within the wait");

    cx.executor().advance_clock(FINISH_EDIT_TIMEOUT);
    cx.run_until_parked();
    assert_eq!(sent(cx, &doc), vec![SidecarCommand::Save]);
}

#[gpui_kit::test]
async fn closing_waits_for_the_page_then_closes(cx: &mut TestAppContext) {
    let (handle, doc) = open_page(cx);
    assert!(!request_close(cx, handle), "the page goes first");
    assert_eq!(page_sent(cx, &doc), vec![CommandBody::FinishEdit]);
    assert!(is_open(cx, handle));
    page(cx, handle, &doc, result_ok(1));
    cx.run_until_parked();
    assert!(
        !is_open(cx, handle),
        "nothing was typed, and the talk is clean"
    );
    assert_eq!(sent(cx, &doc), vec![SidecarCommand::Stop]);
}

#[gpui_kit::test]
async fn closing_with_text_being_typed_asks_about_it(cx: &mut TestAppContext) {
    let (handle, doc) = open_page(cx);
    assert!(!request_close(cx, handle));
    page(cx, handle, &doc, edited());
    page(cx, handle, &doc, result_ok(1));
    cx.wait_for(handle, WAIT, |window, _| {
        window.try_find("save-changes-save").is_some()
    })
    .await;
    assert!(is_open(cx, handle));
}

#[gpui_kit::test]
async fn a_dead_page_cannot_hold_up_a_close(cx: &mut TestAppContext) {
    let (handle, _doc) = open_page(cx);
    assert!(!request_close(cx, handle));
    cx.run_until_parked();
    assert!(is_open(cx, handle));
    cx.executor().advance_clock(FINISH_EDIT_TIMEOUT);
    cx.run_until_parked();
    assert!(!is_open(cx, handle));
}

#[gpui_kit::test]
async fn a_refusal_reaches_the_person(cx: &mut TestAppContext) {
    let (handle, doc) = open_page(cx);
    let reason = "This element is not freely positioned.";
    page(
        cx,
        handle,
        &doc,
        PageMessage::Result {
            id: 9,
            ok: false,
            error: Some(reason.into()),
        },
    );
    cx.wait_for(handle, WAIT, |window, _| {
        window.try_find("message").is_some()
    })
    .await;
    cx.update_window(handle, |_, window, _| {
        assert_eq!(
            window.within("dialog").find("message").label(),
            Some(reason)
        );
    })
    .unwrap();
}

#[gpui_kit::test]
async fn once_serve_has_ended_the_failed_screen_says_the_edits_are_lost(cx: &mut TestAppContext) {
    let (handle, doc) = open_dirty(cx);
    sidecar(
        cx,
        handle,
        &doc,
        SidecarEvent::Error {
            message: "slidev exited".into(),
            log: String::new(),
        },
    );
    sidecar(cx, handle, &doc, SidecarEvent::Stopped);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("failed-lost").label(),
            Some("Your unsaved changes could not be saved.")
        );
        assert!(window.try_find("failed-unsaved").is_none(), "never both");
    })
    .unwrap();
}

#[gpui_kit::test]
async fn with_serve_still_running_the_failed_screen_does_not_say_lost(cx: &mut TestAppContext) {
    let (handle, doc) = open_dirty(cx);
    sidecar(
        cx,
        handle,
        &doc,
        SidecarEvent::Error {
            message: "slidev exited".into(),
            log: String::new(),
        },
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("failed-unsaved").visible());
        assert!(window.try_find("failed-lost").is_none());
    })
    .unwrap();
}
