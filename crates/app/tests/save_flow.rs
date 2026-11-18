//! The window's saving, as a table of what happens and what it should lead to.

use stagepack_studio::save_flow::{CloseAnswer, Effect, NOT_SAVED_STILL_OPEN, STILL_SAVING, SaveFlow, SaveOutcome};
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
    /// Save is asked for on an untitled talk.
    SaveUntitled,
    /// The close dialog is answered for an untitled talk.
    AnswerUntitled(CloseAnswer),
    /// A place to save in is chosen, for a talk's first save or not.
    Chosen {
        first: bool,
    },
    /// No place to save in was chosen.
    NotChosen,
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
            SaveUntitled => flow.ask_save_untitled(),
            AnswerUntitled(answer) => flow.answer_close_untitled(answer),
            Chosen { first } => flow.place_chosen(first),
            NotChosen => flow.place_not_chosen(),
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

fn failed() -> Event {
    Finished {
        outcome: SaveOutcome::Failed("The disk is full.".into()),
        unsaved: true,
    }
}

fn not_saved() -> Effect {
    Say(NOT_SAVED_STILL_OPEN.to_string())
}

#[test]
fn save_on_an_untitled_talk_asks_where_first() {
    check(
        "save",
        &[
            (SaveUntitled, vec![AskWhereToSave]),
            // Asked again while it's being chosen, the window shows no second dialog.
            (SaveUntitled, vec![AskWhereToSave]),
            (NotChosen, vec![]),
            (SaveUntitled, vec![AskWhereToSave]),
            (Chosen { first: true }, vec![StartSave]),
            (saved(false), vec![]),
        ],
    );
    check(
        "save as, from the menu",
        &[
            (Chosen { first: true }, vec![StartSave]),
            (saved(false), vec![]),
            (NotChosen, vec![]),
        ],
    );
}

#[test]
fn closing_an_untitled_talk_with_save_closes_once_its_first_save_is_done() {
    check(
        "chosen and saved",
        &[
            (CloseAsked { unsaved: true }, vec![AskBeforeClosing]),
            (AnswerUntitled(CloseAnswer::Save), vec![AskWhereToSave]),
            (Chosen { first: true }, vec![StartSave]),
            (saved(false), vec![Close]),
        ],
    );
    check(
        "edited while it saved",
        &[
            (CloseAsked { unsaved: true }, vec![AskBeforeClosing]),
            (AnswerUntitled(CloseAnswer::Save), vec![AskWhereToSave]),
            (Chosen { first: true }, vec![StartSave]),
            (saved(true), vec![StartSave]),
            (saved(false), vec![Close]),
        ],
    );
    check(
        "no place chosen",
        &[
            (CloseAsked { unsaved: true }, vec![AskBeforeClosing]),
            (AnswerUntitled(CloseAnswer::Save), vec![AskWhereToSave]),
            (NotChosen, vec![not_saved()]),
            // Nothing closes the window but a new answer.
            (SaveUntitled, vec![AskWhereToSave]),
            (Chosen { first: true }, vec![StartSave]),
            (saved(false), vec![]),
        ],
    );
    check(
        "don't save, and cancel",
        &[
            (CloseAsked { unsaved: true }, vec![AskBeforeClosing]),
            (AnswerUntitled(CloseAnswer::Cancel), vec![]),
            (CloseAsked { unsaved: true }, vec![AskBeforeClosing]),
            (AnswerUntitled(CloseAnswer::DontSave), vec![LetGo, Close]),
        ],
    );
    check(
        "asked while its first save runs",
        &[
            (SaveUntitled, vec![AskWhereToSave]),
            (Chosen { first: true }, vec![StartSave]),
            (CloseAsked { unsaved: true }, vec![AskBeforeClosing]),
            (AnswerUntitled(CloseAnswer::Save), vec![]),
            (saved(false), vec![Close]),
        ],
    );
}

#[test]
fn a_first_save_that_fails_starts_nothing_after_it() {
    check(
        "asked again while it ran",
        &[
            (SaveUntitled, vec![AskWhereToSave]),
            (Chosen { first: true }, vec![StartSave]),
            (SaveUntitled, vec![]),
            // Still untitled: there's nowhere to save it but where a person chooses.
            (failed(), vec![Say("The disk is full.".into())]),
            (SaveUntitled, vec![AskWhereToSave]),
        ],
    );
    check(
        "while closing",
        &[
            (CloseAsked { unsaved: true }, vec![AskBeforeClosing]),
            (AnswerUntitled(CloseAnswer::Save), vec![AskWhereToSave]),
            (Chosen { first: true }, vec![StartSave]),
            (failed(), vec![Say("The disk is full.".into())]),
            (CloseAsked { unsaved: true }, vec![AskBeforeClosing]),
        ],
    );
    check(
        "worked, with edits made while it ran",
        &[
            (Chosen { first: true }, vec![StartSave]),
            (SaveUntitled, vec![]),
            (saved(true), vec![StartSave]),
            (saved(false), vec![]),
        ],
    );
}
