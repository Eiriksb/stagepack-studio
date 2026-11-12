//! The window's saving, as a table of what happens and what it should lead to.

use stagepack_studio::save_flow::{CloseAnswer, Effect, STILL_SAVING, SaveFlow, SaveOutcome};
use stagepack_studio::saving::SaveError;

/// Something that happens to the window's saving.
#[derive(Clone, Debug)]
enum Event {
    Save,
    SaveAs,
    /// The window is asked to close, with changes unsaved or not.
    CloseAsked {
        unsaved: bool,
    },
    /// The close dialog is answered.
    Answer(CloseAnswer),
    /// A read of the talk's file comes in.
    Read,
    /// The running save is done, and the talk has changes unsaved after it or not.
    Finished {
        outcome: SaveOutcome,
        unsaved: bool,
    },
}

use Effect::*;
use Event::*;

fn saved(unsaved: bool) -> Event {
    Finished {
        outcome: SaveOutcome::Saved,
        unsaved,
    }
}

fn changed_outside() -> Effect {
    Say(SaveError::ChangedOutside(String::new()).to_string())
}

/// Runs `steps` from an idle window, each event with the effects it should have.
fn check(name: &str, steps: &[(Event, Vec<Effect>)]) {
    let mut flow = SaveFlow::default();
    for (at, (event, expected)) in steps.iter().enumerate() {
        let effects = match event.clone() {
            Save => flow.ask_save(),
            SaveAs => flow.ask_save_as(),
            CloseAsked { unsaved } => flow.ask_close(unsaved),
            Answer(answer) => flow.answer_close(answer),
            Read => flow.read_arrived(),
            Finished { outcome, unsaved } => flow.finished(outcome, unsaved),
        };
        assert_eq!(&effects, expected, "{name}, step {at}: {event:?}");
    }
}

#[test]
fn a_save_starts_and_ends() {
    check("a save", &[(Save, vec![StartSave]), (saved(false), vec![])]);
    assert_eq!(SaveFlow::default(), SaveFlow::Idle);
}

#[test]
fn a_save_asked_for_during_a_save_follows_it() {
    check(
        "edits made meanwhile",
        &[
            (Save, vec![StartSave]),
            (Save, vec![]),
            (Save, vec![]),
            (saved(true), vec![StartSave]),
            (saved(false), vec![]),
        ],
    );
    check(
        "nothing made meanwhile",
        &[
            (Save, vec![StartSave]),
            (Save, vec![]),
            (saved(false), vec![]),
        ],
    );
}

#[test]
fn save_as_during_a_save_says_to_wait() {
    check(
        "save as",
        &[
            (Save, vec![StartSave]),
            (SaveAs, vec![Say(STILL_SAVING.to_string())]),
            (saved(false), vec![]),
            (SaveAs, vec![StartSave]),
            (Save, vec![]),
            (saved(true), vec![StartSave]),
        ],
    );
}

#[test]
fn closing_asks_only_with_changes_unsaved() {
    check(
        "close",
        &[
            (CloseAsked { unsaved: false }, vec![Close]),
            (CloseAsked { unsaved: true }, vec![AskBeforeClosing]),
            (Answer(CloseAnswer::Cancel), vec![]),
            (CloseAsked { unsaved: true }, vec![AskBeforeClosing]),
            (Answer(CloseAnswer::DontSave), vec![LetGo, Close]),
        ],
    );
    check(
        "close and save",
        &[
            (CloseAsked { unsaved: true }, vec![AskBeforeClosing]),
            (Answer(CloseAnswer::Save), vec![StartSave]),
            (saved(false), vec![Close]),
        ],
    );
}

#[test]
fn close_and_save_during_a_save_with_edits_made_meanwhile_saves_them_too() {
    check(
        "close and save during a save",
        &[
            (Save, vec![StartSave]),
            (CloseAsked { unsaved: true }, vec![AskBeforeClosing]),
            (Answer(CloseAnswer::Save), vec![]),
            // The edits made while it ran are saved before the window goes.
            (saved(true), vec![StartSave]),
            (saved(false), vec![Close]),
        ],
    );
    check(
        "edits made during the second save too",
        &[
            (CloseAsked { unsaved: true }, vec![AskBeforeClosing]),
            (Answer(CloseAnswer::Save), vec![StartSave]),
            (saved(true), vec![StartSave]),
            (saved(true), vec![StartSave]),
            (saved(false), vec![Close]),
        ],
    );
}

#[test]
fn a_read_held_back_by_a_save_is_read_again_after_it() {
    check(
        "a read during a save",
        &[
            (Read, vec![TakeRead]),
            (Save, vec![StartSave]),
            // It may be older than what the save writes.
            (Read, vec![]),
            (Read, vec![]),
            (saved(false), vec![Reload]),
            (Read, vec![TakeRead]),
        ],
    );
    check(
        "a read during a save that's followed by another",
        &[
            (Save, vec![StartSave]),
            (Read, vec![]),
            (Save, vec![]),
            (saved(true), vec![Reload, StartSave]),
            (Read, vec![]),
            (saved(false), vec![Reload]),
        ],
    );
}

#[test]
fn an_outside_change_found_by_the_save_is_asked_about() {
    check(
        "found by the save",
        &[
            (Save, vec![StartSave]),
            (
                Finished {
                    outcome: SaveOutcome::ChangedOutside,
                    unsaved: true,
                },
                vec![changed_outside(), AskAboutOutsideChange],
            ),
            (Save, vec![StartSave]),
        ],
    );
    check(
        "found by the save, with another asked for and a read held back",
        &[
            (Save, vec![StartSave]),
            (Save, vec![]),
            (Read, vec![]),
            (
                Finished {
                    outcome: SaveOutcome::ChangedOutside,
                    unsaved: true,
                },
                vec![changed_outside(), AskAboutOutsideChange, Reload],
            ),
        ],
    );
    check(
        "found by the save the window was closing after",
        &[
            (CloseAsked { unsaved: true }, vec![AskBeforeClosing]),
            (Answer(CloseAnswer::Save), vec![StartSave]),
            (
                Finished {
                    outcome: SaveOutcome::ChangedOutside,
                    unsaved: true,
                },
                vec![changed_outside(), AskAboutOutsideChange],
            ),
            // Nothing closes the window but a new answer.
            (Save, vec![StartSave]),
            (saved(false), vec![]),
        ],
    );
}

#[test]
fn a_failed_save_while_closing_keeps_the_window_open() {
    let failed = || Finished {
        outcome: SaveOutcome::Failed("The disk is full.".into()),
        unsaved: true,
    };
    check(
        "a failed save while closing",
        &[
            (CloseAsked { unsaved: true }, vec![AskBeforeClosing]),
            (Answer(CloseAnswer::Save), vec![StartSave]),
            (failed(), vec![Say("The disk is full.".into())]),
            // A later save that works doesn't close it.
            (Save, vec![StartSave]),
            (saved(false), vec![]),
        ],
    );
    check(
        "a failed save with edits made while it ran",
        &[
            (Save, vec![StartSave]),
            (Save, vec![]),
            (failed(), vec![Say("The disk is full.".into()), StartSave]),
        ],
    );
}
