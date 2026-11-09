use std::path::PathBuf;

use stagepack_studio::document::{
    BUSY_MESSAGE, DocState, Document, Effect, MISMATCH_MESSAGE, NOT_TRUSTED_MESSAGE,
};
use studio_protocol::{PageMessage, SelectionInfo};
use studio_sidecar::SidecarEvent;

const BASE_URL: &str = "http://127.0.0.1:40755";

fn doc() -> Document {
    Document::new(PathBuf::from("/talks/hello.sptx"))
}

fn unpacked(title: &str) -> SidecarEvent {
    SidecarEvent::Unpacked {
        id: "abc".into(),
        title: title.into(),
        entry: "slides.md".into(),
        work_dir: PathBuf::from("/tmp/work"),
        code: Vec::new(),
        trusted: true,
    }
}

fn ready() -> SidecarEvent {
    SidecarEvent::Ready {
        port: 40755,
        base_url: BASE_URL.into(),
        work_dir: PathBuf::from("/tmp/work"),
    }
}

fn failed(message: &str, log: &str) -> DocState {
    DocState::Failed {
        message: message.into(),
        log: log.into(),
    }
}

/// A document whose talk is open in the canvas.
fn open_doc() -> Document {
    let mut d = doc();
    d.on_sidecar(unpacked("Hello fixture"));
    d.on_sidecar(ready());
    d
}

fn edited() -> PageMessage {
    PageMessage::Edited {
        slide: 2,
        label: "Move".into(),
    }
}

fn selection() -> SelectionInfo {
    SelectionInfo {
        slide: 2,
        range: Some([3, 5]),
        kind: "element".into(),
        tag: Some("div".into()),
        label: "<div>".into(),
        positioned: true,
        nested: false,
        pos: None,
        can_reorder: true,
    }
}

#[test]
fn trusted_flow_loads_canvas() {
    let mut d = doc();
    assert_eq!(d.state, DocState::Opening);
    assert_eq!(
        d.title, "hello.sptx",
        "the file name until serve names the talk"
    );

    assert_eq!(d.on_sidecar(unpacked("Hello fixture")), vec![]);
    assert_eq!(d.title, "Hello fixture");
    assert_eq!(d.state, DocState::Opening);

    assert_eq!(
        d.on_sidecar(ready()),
        vec![Effect::LoadCanvas {
            base_url: BASE_URL.into()
        }]
    );
    assert_eq!(
        d.state,
        DocState::Ready {
            base_url: BASE_URL.into()
        }
    );
    assert!(!d.dirty);
}

#[test]
fn an_untitled_talk_keeps_its_file_name() {
    let mut d = doc();
    d.on_sidecar(unpacked(""));
    assert_eq!(d.title, "hello.sptx");
}

#[test]
fn confirm_asks_trust() {
    let mut d = doc();
    d.on_sidecar(unpacked("Hello fixture"));
    let question = "\"Hello fixture\" (hello.sptx) contains code.";
    assert_eq!(
        d.on_sidecar(SidecarEvent::Confirm {
            question: question.into()
        }),
        vec![Effect::AskTrust(question.into())]
    );
    assert_eq!(
        d.state,
        DocState::AwaitingTrust {
            question: question.into()
        }
    );

    // Answered: back to opening until serve says ready (or exits 3).
    assert_eq!(d.answered_trust(), vec![]);
    assert_eq!(d.state, DocState::Opening);
    assert_eq!(
        d.on_sidecar(ready()),
        vec![Effect::LoadCanvas {
            base_url: BASE_URL.into()
        }]
    );
}

#[test]
fn busy_shows_message() {
    for base_url in [BASE_URL, ""] {
        let mut d = doc();
        assert_eq!(
            d.on_sidecar(SidecarEvent::Busy {
                base_url: base_url.into()
            }),
            vec![]
        );
        assert_eq!(d.state, failed(BUSY_MESSAGE, ""), "busy at {base_url:?}");
        d.on_sidecar(SidecarEvent::Exited { code: Some(4) });
        assert_eq!(
            d.state,
            failed(BUSY_MESSAGE, ""),
            "the exit keeps the message"
        );
    }
}

#[test]
fn not_trusted_exit_fails_with_message() {
    let mut d = doc();
    d.on_sidecar(SidecarEvent::Confirm {
        question: "Trust it?".into(),
    });
    d.answered_trust();
    assert_eq!(d.on_sidecar(SidecarEvent::Exited { code: Some(3) }), vec![]);
    assert_eq!(d.state, failed(NOT_TRUSTED_MESSAGE, ""));
}

#[test]
fn slidev_death_fails_with_log() {
    let mut d = open_doc();
    d.on_page(&edited());
    d.on_sidecar(SidecarEvent::Error {
        message: "slidev exited".into(),
        log: "boom".into(),
    });
    assert_eq!(d.state, failed("slidev exited", "boom"));
    d.on_sidecar(SidecarEvent::Stopped);
    assert_eq!(d.state, failed("slidev exited", "boom"), "after stopped");
    d.on_sidecar(SidecarEvent::Exited { code: Some(1) });
    assert_eq!(d.state, failed("slidev exited", "boom"), "after the exit");
    assert!(!d.wants_close_prompt(), "nothing left to save");
}

#[test]
fn protocol_error_fails_with_mismatch() {
    let mut d = open_doc();
    assert_eq!(d.on_protocol_error("Unsupported bridge version: 2"), vec![]);
    assert_eq!(
        d.state,
        failed(
            &format!("{MISMATCH_MESSAGE}Unsupported bridge version: 2"),
            ""
        )
    );
    assert_eq!(
        MISMATCH_MESSAGE,
        "This copy of the editor and its runtime don't match: "
    );
}

#[test]
fn edited_marks_dirty_saved_clears() {
    let mut d = open_doc();
    assert!(!d.wants_close_prompt());
    assert_eq!(d.on_page(&edited()), vec![]);
    assert!(d.dirty);
    assert!(d.wants_close_prompt());
    assert!(d.can_save());

    assert_eq!(
        d.on_sidecar(SidecarEvent::Saved {
            updated: "2026-11-09T12:00:00Z".into()
        }),
        vec![]
    );
    assert!(!d.dirty);
    assert!(!d.wants_close_prompt());
    assert!(!d.can_save());
}

#[test]
fn conflict_keeps_dirty() {
    let mut d = open_doc();
    d.on_page(&edited());
    let message = "The file changed on disk since it was opened.";
    assert_eq!(
        d.on_sidecar(SidecarEvent::Conflict {
            message: message.into()
        }),
        vec![Effect::ShowMessage(message.into())]
    );
    assert!(d.dirty);

    assert_eq!(
        d.on_sidecar(SidecarEvent::SaveFailed {
            message: "Some names can't be saved.".into(),
            names: vec!["a:b".into()],
        }),
        vec![Effect::ShowMessage("Some names can't be saved.".into())]
    );
    assert!(d.dirty);
    assert!(matches!(d.state, DocState::Ready { .. }));
}

#[test]
fn reload_clears_selection() {
    let mut d = open_doc();
    d.on_page(&PageMessage::Selection {
        selection: Some(selection()),
        missed: false,
    });
    assert_eq!(d.selection, Some(selection()));

    d.on_page(&PageMessage::Ready {
        studio: "0.0.0".into(),
        slide: 2,
        total: 3,
    });
    assert_eq!(d.selection, None);
}

#[test]
fn selection_and_history_follow_the_page() {
    let mut d = open_doc();
    d.on_page(&PageMessage::Selection {
        selection: Some(selection()),
        missed: false,
    });
    d.on_page(&PageMessage::Selection {
        selection: None,
        missed: false,
    });
    assert_eq!(d.selection, None);

    d.on_page(&PageMessage::History {
        can_undo: true,
        can_redo: false,
        last_action: Some("Move".into()),
    });
    assert!(d.can_undo && !d.can_redo);
    d.on_page(&PageMessage::History {
        can_undo: false,
        can_redo: true,
        last_action: None,
    });
    assert!(!d.can_undo && d.can_redo);
}

#[test]
fn a_page_error_is_shown() {
    let mut d = open_doc();
    assert_eq!(
        d.on_page(&PageMessage::Error {
            message: "Could not write slides.md".into()
        }),
        vec![Effect::ShowMessage("Could not write slides.md".into())]
    );
    assert!(matches!(d.state, DocState::Ready { .. }));
}

#[test]
fn stopped_and_clean_exit_close() {
    let mut d = open_doc();
    d.on_sidecar(SidecarEvent::Stopped);
    assert_eq!(d.state, DocState::Closed);
    d.on_sidecar(SidecarEvent::Exited { code: Some(0) });
    assert_eq!(d.state, DocState::Closed);

    let mut d = open_doc();
    d.on_sidecar(SidecarEvent::Exited { code: Some(0) });
    assert_eq!(d.state, DocState::Closed);
}

#[test]
fn an_unexpected_exit_fails() {
    for code in [Some(2), None] {
        let mut d = open_doc();
        d.on_sidecar(SidecarEvent::Exited { code });
        match &d.state {
            DocState::Failed { message, log } => {
                assert!(
                    message.starts_with("Stagepack stopped unexpectedly"),
                    "{message}"
                );
                assert_eq!(log, "");
            }
            other => panic!("exit {code:?} gave {other:?}"),
        }
    }
}

#[test]
fn save_then_close_closes_once_saved() {
    let mut d = open_doc();
    d.on_page(&edited());
    d.close_after_save();
    assert!(d.closing_after_save());
    assert_eq!(
        d.on_sidecar(SidecarEvent::Saved {
            updated: "2026-11-09T12:00:00Z".into()
        }),
        vec![Effect::CloseWindow]
    );
    assert!(!d.dirty);
    assert!(!d.closing_after_save());
}

#[test]
fn a_conflict_cancels_the_close() {
    let mut d = open_doc();
    d.on_page(&edited());
    d.close_after_save();
    assert_eq!(
        d.on_sidecar(SidecarEvent::Conflict {
            message: "Changed on disk.".into()
        }),
        vec![Effect::ShowMessage("Changed on disk.".into())]
    );
    assert!(!d.closing_after_save());
    assert!(d.dirty);
    // A later save that works doesn't close the window by itself.
    assert_eq!(
        d.on_sidecar(SidecarEvent::Saved {
            updated: "2026-11-09T12:00:01Z".into()
        }),
        vec![]
    );
}

#[test]
fn an_error_while_saving_to_close_keeps_the_window() {
    let mut d = open_doc();
    d.on_page(&edited());
    d.close_after_save();
    d.on_sidecar(SidecarEvent::Error {
        message: "slidev exited".into(),
        log: String::new(),
    });
    assert!(!d.closing_after_save());
    assert_eq!(d.state, failed("slidev exited", ""));
}

fn saved() -> SidecarEvent {
    SidecarEvent::Saved {
        updated: "2026-11-09T12:00:00Z".into(),
    }
}

#[test]
fn an_edit_during_a_save_stays_dirty() {
    let mut d = open_doc();
    d.on_page(&edited());
    d.save_sent();
    // The page wrote after serve had read the files: that edit isn't in the save.
    d.on_page(&edited());
    assert_eq!(d.on_sidecar(saved()), vec![]);
    assert!(d.dirty, "the second edit is still unsaved");
    assert!(d.wants_close_prompt());
    assert!(d.can_save());
}

#[test]
fn an_edit_during_save_then_close_asks_again() {
    let mut d = open_doc();
    d.on_page(&edited());
    d.close_after_save();
    d.save_sent();
    d.on_page(&edited());
    assert_eq!(
        d.on_sidecar(saved()),
        vec![Effect::AskSaveChanges],
        "the window must not close over an unsaved edit"
    );
    assert!(d.dirty);
    assert!(!d.closing_after_save());
}

#[test]
fn saves_in_flight_settle_in_order() {
    let mut d = open_doc();
    d.on_page(&edited());
    d.save_sent();
    d.on_page(&edited());
    d.save_sent();
    d.on_sidecar(saved());
    assert!(d.dirty, "the first save has only the first edit");
    d.on_sidecar(saved());
    assert!(!d.dirty, "the second save has both");

    // A refused save gives up its place in the queue too.
    d.on_page(&edited());
    d.save_sent();
    d.on_sidecar(SidecarEvent::Conflict {
        message: "Changed on disk.".into(),
    });
    assert!(d.dirty);
    d.save_sent();
    d.on_sidecar(saved());
    assert!(!d.dirty);
}

#[test]
fn a_save_then_close_waits_for_the_save_that_has_every_edit() {
    let mut d = open_doc();
    d.on_page(&edited());
    d.save_sent(); // Ctrl+S
    d.on_page(&edited());
    d.close_after_save();
    d.save_sent(); // the close prompt's Save
    assert_eq!(
        d.on_sidecar(saved()),
        vec![],
        "the next save may have the rest"
    );
    assert!(d.closing_after_save());
    assert_eq!(d.on_sidecar(saved()), vec![Effect::CloseWindow]);
}

#[test]
fn stopping_ends_a_pending_save_then_close() {
    let mut d = open_doc();
    d.on_page(&edited());
    d.close_after_save();
    d.save_sent();
    // Serve answered a signal with `stopped` and never said `saved`.
    d.on_sidecar(SidecarEvent::Stopped);
    assert!(!d.closing_after_save());
    d.on_sidecar(SidecarEvent::Exited { code: Some(0) });
    assert!(!d.closing_after_save());
    assert_eq!(d.state, DocState::Closed);
    assert!(!d.wants_close_prompt(), "nothing is left to save");

    let mut d = open_doc();
    d.on_page(&edited());
    d.close_after_save();
    d.on_sidecar(SidecarEvent::Exited { code: Some(0) });
    assert!(!d.closing_after_save(), "an exit without `stopped` too");
}

#[test]
fn a_failure_while_serve_runs_keeps_the_edits_savable() {
    let mut d = open_doc();
    d.on_page(&edited());
    d.on_protocol_error("Unsupported bridge version: 2");
    assert!(matches!(d.state, DocState::Failed { .. }));
    assert!(d.wants_close_prompt(), "serve still has the edits");
    assert!(d.can_save());

    // An unreadable line from serve is an `Error` while serve keeps serving.
    let mut d = open_doc();
    d.on_page(&edited());
    d.on_sidecar(SidecarEvent::Error {
        message: "Unreadable message from Stagepack: x".into(),
        log: String::new(),
    });
    assert!(d.wants_close_prompt());
    assert!(d.can_save());
}

#[test]
fn an_error_then_stopped_has_nothing_to_save() {
    let mut d = open_doc();
    d.on_page(&edited());
    d.on_sidecar(SidecarEvent::Error {
        message: "slidev exited".into(),
        log: "boom".into(),
    });
    d.on_sidecar(SidecarEvent::Stopped);
    assert!(d.dirty);
    assert!(!d.wants_close_prompt(), "serve removed the work folder");
    assert!(!d.can_save());
}

// --- Text being typed in place goes first (finish-edit).

fn page_ready() -> PageMessage {
    PageMessage::Ready {
        studio: "0.0.0".into(),
        slide: 1,
        total: 2,
    }
}

/// A document whose page is up and has said `ready`.
fn page_up() -> Document {
    let mut d = open_doc();
    d.on_page(&page_ready());
    d
}

fn answered(id: i64) -> PageMessage {
    PageMessage::Result {
        id,
        ok: true,
        error: None,
    }
}

fn refused(id: i64, error: &str) -> PageMessage {
    PageMessage::Result {
        id,
        ok: false,
        error: Some(error.into()),
    }
}

#[test]
fn ctrl_s_on_a_clean_talk_with_text_being_typed_ends_up_saved() {
    let mut d = page_up();
    assert_eq!(
        d.request_save(),
        vec![Effect::FinishEdit],
        "the edit goes first"
    );
    d.finish_edit_sent(5);
    // The page applies the editor: `edited`, then the result.
    assert_eq!(d.on_page(&edited()), vec![]);
    assert_eq!(d.on_page(&answered(5)), vec![Effect::SendSave]);
}

#[test]
fn save_with_nothing_typed_on_a_clean_talk_sends_nothing() {
    let mut d = page_up();
    d.request_save();
    d.finish_edit_sent(5);
    assert_eq!(d.on_page(&answered(5)), vec![]);
}

#[test]
fn save_waits_for_the_result_of_its_own_command() {
    let mut d = page_up();
    d.on_page(&edited());
    d.request_save();
    d.finish_edit_sent(5);
    assert_eq!(d.on_page(&answered(4)), vec![], "another command's result");
    assert_eq!(d.on_page(&answered(5)), vec![Effect::SendSave]);
}

#[test]
fn a_dead_page_cannot_block_a_save() {
    let mut d = page_up();
    d.on_page(&edited());
    d.request_save();
    d.finish_edit_sent(5);
    assert_eq!(d.finish_edit_timed_out(5), vec![Effect::SendSave]);
    assert_eq!(
        d.on_page(&answered(5)),
        vec![],
        "the late result does nothing"
    );
    assert_eq!(d.finish_edit_timed_out(5), vec![], "nor a second timeout");
}

#[test]
fn a_stale_timeout_is_ignored() {
    let mut d = page_up();
    d.on_page(&edited());
    d.request_save();
    d.finish_edit_sent(5);
    d.on_page(&answered(5));
    d.request_save();
    d.finish_edit_sent(6);
    assert_eq!(
        d.finish_edit_timed_out(5),
        vec![],
        "the first command's timer"
    );
    assert_eq!(d.on_page(&answered(6)), vec![Effect::SendSave]);
}

#[test]
fn with_no_page_up_a_save_goes_straight_to_serve() {
    let mut d = open_doc();
    assert_eq!(d.request_save(), vec![], "nothing to save");
    d.on_page(&edited());
    assert_eq!(d.request_save(), vec![Effect::SendSave]);

    // The page of a failed talk is hidden; there is nothing left to wait for.
    let mut d = page_up();
    d.on_page(&edited());
    d.on_sidecar(SidecarEvent::Error {
        message: "slidev exited".into(),
        log: String::new(),
    });
    assert_eq!(d.request_save(), vec![Effect::SendSave]);
}

#[test]
fn a_new_load_means_the_page_is_not_up() {
    let mut d = page_up();
    d.on_page(&edited());
    d.on_sidecar(ready()); // loads the canvas again
    assert_eq!(d.request_save(), vec![Effect::SendSave]);
}

#[test]
fn a_second_save_while_waiting_does_nothing() {
    let mut d = page_up();
    d.on_page(&edited());
    d.request_save();
    d.finish_edit_sent(5);
    assert_eq!(d.request_save(), vec![]);
    assert_eq!(d.on_page(&answered(5)), vec![Effect::SendSave]);
}

#[test]
fn closing_finishes_the_edit_first() {
    let mut d = page_up();
    assert_eq!(d.request_close(), vec![Effect::FinishEdit]);
    d.finish_edit_sent(7);
    assert_eq!(
        d.on_page(&answered(7)),
        vec![Effect::CloseWindow],
        "still clean"
    );

    let mut d = page_up();
    d.request_close();
    d.finish_edit_sent(7);
    d.on_page(&edited());
    assert_eq!(
        d.on_page(&answered(7)),
        vec![Effect::AskSaveChanges],
        "the text typed made it dirty"
    );
}

#[test]
fn a_dead_page_cannot_block_a_close() {
    let mut d = page_up();
    d.request_close();
    d.finish_edit_sent(7);
    assert_eq!(d.finish_edit_timed_out(7), vec![Effect::CloseWindow]);

    let mut d = page_up();
    d.on_page(&edited());
    d.request_close();
    d.finish_edit_sent(7);
    assert_eq!(d.finish_edit_timed_out(7), vec![Effect::AskSaveChanges]);
}

#[test]
fn with_no_page_up_a_close_decides_at_once() {
    let mut d = open_doc();
    assert_eq!(d.request_close(), vec![Effect::CloseWindow]);
    let mut d = open_doc();
    d.on_page(&edited());
    assert_eq!(d.request_close(), vec![Effect::AskSaveChanges]);
}

#[test]
fn a_close_during_the_wait_of_a_save_wins() {
    let mut d = page_up();
    d.on_page(&edited());
    d.request_save();
    d.finish_edit_sent(5);
    assert_eq!(d.request_close(), vec![], "already waiting");
    assert_eq!(d.on_page(&answered(5)), vec![Effect::AskSaveChanges]);
}

#[test]
fn an_edit_that_cannot_be_written_back_shows_and_the_save_still_goes() {
    let message = "That edit could not be written back as Markdown, so nothing was changed.";
    let mut d = page_up();
    d.on_page(&edited());
    d.request_save();
    d.finish_edit_sent(5);
    d.on_page(&PageMessage::Error {
        message: message.into(),
    });
    assert_eq!(
        d.on_page(&refused(5, message)),
        vec![Effect::SendSave],
        "once for the message, and the earlier edit is saved"
    );
}

#[test]
fn a_refused_finish_calls_off_the_close_so_the_message_can_be_read() {
    let message = "That edit could not be written back as Markdown, so nothing was changed.";
    let mut d = page_up();
    d.request_close();
    d.finish_edit_sent(5);
    assert_eq!(
        d.on_page(&refused(5, message)),
        vec![Effect::ShowMessage(message.into())]
    );
    // The reason has been shown; the next close decides at once.
    assert_eq!(d.request_close(), vec![Effect::CloseWindow]);
}

#[test]
fn a_page_that_refuses_every_finish_cannot_hold_the_window_open() {
    let message = "That edit could not be written back as Markdown, so nothing was changed.";
    let mut d = page_up();
    d.request_close();
    d.finish_edit_sent(5);
    d.on_page(&refused(5, message));
    // The second request doesn't ask the page again, so it decides as usual.
    assert_eq!(d.request_close(), vec![Effect::CloseWindow]);

    let mut d = page_up();
    d.request_close();
    d.finish_edit_sent(5);
    d.on_page(&refused(5, message));
    d.on_page(&edited());
    assert_eq!(d.request_close(), vec![Effect::AskSaveChanges], "dirty");
}

#[test]
fn a_finish_that_succeeds_asks_again_the_next_time() {
    let message = "That edit could not be written back as Markdown, so nothing was changed.";
    let mut d = page_up();
    d.request_close();
    d.finish_edit_sent(5);
    d.on_page(&refused(5, message));
    // A save asks the page, and the page does it this time.
    assert_eq!(d.request_save(), vec![Effect::FinishEdit]);
    d.finish_edit_sent(6);
    d.on_page(&answered(6));
    assert_eq!(d.request_close(), vec![Effect::FinishEdit]);
}

// --- Every result ok:false is answered.

#[test]
fn a_refusal_shows_its_reason() {
    let mut d = page_up();
    d.command_sent(9);
    assert_eq!(d.refusals(), 0);
    assert_eq!(
        d.on_page(&refused(9, "This element is not freely positioned.")),
        vec![Effect::ShowMessage(
            "This element is not freely positioned.".into()
        )]
    );
    assert_eq!(d.refusals(), 1, "the inspector goes back to the selection");
    assert!(matches!(d.state, DocState::Ready { .. }));
}

#[test]
fn a_result_that_is_ok_says_nothing() {
    let mut d = page_up();
    d.command_sent(9);
    assert_eq!(d.on_page(&answered(9)), vec![]);
    assert_eq!(d.refusals(), 0);
}

#[test]
fn a_mismatch_in_a_result_is_a_protocol_error() {
    for error in [
        "Unsupported bridge version: 2",
        "Unknown command: explode",
        "Bad command: not JSON",
    ] {
        let mut d = page_up();
        d.command_sent(9);
        assert_eq!(d.on_page(&refused(9, error)), vec![], "{error}");
        assert_eq!(d.state, failed(&format!("{MISMATCH_MESSAGE}{error}"), ""));
        assert_eq!(d.refusals(), 0, "{error}");
    }
}

#[test]
fn a_reported_failure_shows_once() {
    let mut d = page_up();
    d.command_sent(9);
    let message = "Could not write slides.md";
    assert_eq!(
        d.on_page(&PageMessage::Error {
            message: message.into()
        }),
        vec![Effect::ShowMessage(message.into())]
    );
    assert_eq!(d.on_page(&refused(9, message)), vec![], "already on screen");
    assert_eq!(d.refusals(), 1, "the inspector still goes back");
}

#[test]
fn an_older_error_does_not_swallow_a_refusal() {
    let mut d = page_up();
    let message = "Could not write slides.md";
    d.on_page(&PageMessage::Error {
        message: message.into(),
    });
    d.command_sent(9); // went out after that error
    assert_eq!(
        d.on_page(&refused(9, message)),
        vec![Effect::ShowMessage(message.into())]
    );
}

#[test]
fn a_different_error_does_not_swallow_a_refusal() {
    let mut d = page_up();
    d.command_sent(9);
    d.on_page(&PageMessage::Error {
        message: "Something else".into(),
    });
    assert_eq!(
        d.on_page(&refused(9, "The reason")),
        vec![Effect::ShowMessage("The reason".into())]
    );
}

// --- What the Failed screen says.

#[test]
fn edits_are_lost_only_once_serve_has_ended() {
    let mut d = open_doc();
    d.on_page(&edited());
    d.on_sidecar(SidecarEvent::Error {
        message: "slidev exited".into(),
        log: String::new(),
    });
    assert!(!d.changes_lost(), "serve still has them");
    assert!(d.wants_close_prompt());
    d.on_sidecar(SidecarEvent::Stopped);
    assert!(d.changes_lost());
    assert!(!d.wants_close_prompt());

    let mut d = open_doc();
    d.on_sidecar(SidecarEvent::Stopped);
    assert!(!d.changes_lost(), "a clean talk lost nothing");
}

// --- A click on nothing traceable.

#[test]
fn missed_follows_the_page_until_the_next_selection() {
    let mut d = open_doc();
    assert!(!d.missed);
    d.on_page(&PageMessage::Selection {
        selection: None,
        missed: true,
    });
    assert!(d.missed);
    d.on_page(&PageMessage::Selection {
        selection: Some(selection()),
        missed: false,
    });
    assert!(!d.missed);

    d.on_page(&PageMessage::Selection {
        selection: None,
        missed: true,
    });
    d.on_page(&page_ready());
    assert!(!d.missed, "a new page has heard no click");
}
