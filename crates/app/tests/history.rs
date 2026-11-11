use slides_format::{Change, SourceEdit};
use stagepack_studio::history::{History, RunId};

/// Replaces `range` of `text` with `with`, records the undo, and returns the new text.
fn edit(
    history: &mut History,
    label: &'static str,
    text: &str,
    range: std::ops::Range<usize>,
    with: &str,
) -> String {
    let change = Change::single(SourceEdit {
        range,
        text: with.to_string(),
    });
    history.record(label, change.inverse(text), 0, 0, false);
    change.apply(text)
}

/// As [`edit`], opening a run that later edits can join.
fn edit_run(
    history: &mut History,
    text: &str,
    range: std::ops::Range<usize>,
    with: &str,
) -> (String, RunId) {
    let change = Change::single(SourceEdit {
        range,
        text: with.to_string(),
    });
    let run = history
        .record("Edit notes", change.inverse(text), 0, 0, true)
        .expect("a run");
    (change.apply(text), run)
}

fn undo(history: &mut History, text: &str) -> String {
    history
        .undo(text)
        .expect("something to undo")
        .change
        .apply(text)
}

fn redo(history: &mut History, text: &str) -> String {
    history
        .redo(text)
        .expect("something to redo")
        .change
        .apply(text)
}

#[test]
fn undo_then_redo_gives_the_same_text() {
    let mut history = History::new();
    let start = "# One\r\n\r\n---\r\n\r\n# Two\r\n";
    let one = edit(&mut history, "Edit", start, 2..5, "First");
    let two = edit(&mut history, "Edit", &one, 0..0, "\u{feff}");
    let back = undo(&mut history, &two);
    assert_eq!(back, one);
    let back = undo(&mut history, &back);
    assert_eq!(back, start);
    assert!(history.undo(&back).is_none());
    let forward = redo(&mut history, &back);
    assert_eq!(forward, one);
    let forward = redo(&mut history, &forward);
    assert_eq!(forward, two);
    assert!(history.redo(&forward).is_none());
}

#[test]
fn a_new_edit_drops_what_could_be_redone() {
    let mut history = History::new();
    let one = edit(&mut history, "Edit", "abc", 0..1, "x");
    let back = undo(&mut history, &one);
    assert!(history.can_redo());
    edit(&mut history, "Edit", &back, 2..3, "y");
    assert!(!history.can_redo());
    assert!(history.can_undo());
}

#[test]
fn saved_is_true_again_after_undoing_back_to_it() {
    let mut history = History::new();
    assert!(history.is_saved());
    let one = edit(&mut history, "Edit", "abc", 0..1, "x");
    assert!(!history.is_saved());
    history.mark_saved();
    assert!(history.is_saved());
    let two = edit(&mut history, "Edit", &one, 1..2, "y");
    assert!(!history.is_saved());
    undo(&mut history, &two);
    assert!(history.is_saved());
}

#[test]
fn and_false_after_redoing_past_it() {
    let mut history = History::new();
    let one = edit(&mut history, "Edit", "abc", 0..1, "x");
    let back = undo(&mut history, &one);
    assert!(history.is_saved(), "back where it started");
    redo(&mut history, &back);
    assert!(!history.is_saved());
}

#[test]
fn a_saved_point_that_was_undone_and_replaced_cannot_come_back() {
    let mut history = History::new();
    let one = edit(&mut history, "Edit", "abc", 0..1, "x");
    history.mark_saved();
    let back = undo(&mut history, &one);
    let replaced = edit(&mut history, "Edit", &back, 0..1, "z");
    assert!(!history.is_saved());
    let back = undo(&mut history, &replaced);
    assert_eq!(back, "abc");
    assert!(!history.is_saved());
}

#[test]
fn the_oldest_steps_go_when_there_are_too_many() {
    let mut history = History::new();
    let mut text = String::new();
    for _ in 0..1_005 {
        text = edit(&mut history, "Edit", &text, text.len()..text.len(), "a");
    }
    let mut undone = 0;
    while history.can_undo() {
        text = undo(&mut history, &text);
        undone += 1;
    }
    assert_eq!(undone, 1_000);
    assert_eq!(text, "a".repeat(5), "the five oldest can't be taken back");
}

#[test]
fn the_oldest_steps_go_when_they_hold_too_much_text() {
    let mut history = History::new();
    let big = "a".repeat(20 * 1024 * 1024);
    let mut text = String::new();
    for _ in 0..4 {
        // A step keeps the text it took away: here, all of it.
        text = edit(
            &mut history,
            "Edit",
            &text,
            0..text.len(),
            &big.replace('a', "b"),
        );
        text = edit(&mut history, "Edit", &text, 0..text.len(), &big);
    }
    let mut undone = 0;
    while history.can_undo() {
        text = undo(&mut history, &text);
        undone += 1;
    }
    assert_eq!(
        undone, 3,
        "three steps of 20 MiB fit in 64 MiB, and a fourth doesn't"
    );
}

#[test]
fn saved_cannot_come_back_once_its_step_has_gone() {
    let mut history = History::new();
    let mut text = String::new();
    for _ in 0..1_001 {
        text = edit(&mut history, "Edit", &text, text.len()..text.len(), "a");
    }
    while history.can_undo() {
        text = undo(&mut history, &text);
    }
    assert_eq!(text, "a");
    assert!(!history.is_saved());
}

#[test]
fn labels_name_the_next_undo_and_redo() {
    let mut history = History::new();
    assert_eq!(history.undo_label(), None);
    assert_eq!(history.redo_label(), None);
    let one = edit(&mut history, "Add slide", "abc", 0..0, "x");
    let two = edit(&mut history, "Hide slide", &one, 0..0, "y");
    assert_eq!(history.undo_label(), Some("Hide slide"));
    assert_eq!(history.redo_label(), None);
    undo(&mut history, &two);
    assert_eq!(history.undo_label(), Some("Add slide"));
    assert_eq!(history.redo_label(), Some("Hide slide"));
}

#[test]
fn a_step_says_which_slide_to_show() {
    let mut history = History::new();
    let change = Change::single(SourceEdit {
        range: 0..1,
        text: "x".into(),
    });
    history.record("Move slide", change.inverse("abc"), 4, 1, false);
    let undone = history.undo("xbc").unwrap();
    assert_eq!(undone.current, 4);
    assert_eq!(history.redo("abc").unwrap().current, 1);
}

#[test]
fn an_edit_made_straight_after_can_join_its_run() {
    let mut history = History::new();
    let (text, run) = edit_run(&mut history, "Say hi", 6..6, "!");
    assert!(history.extend(run, &text, "Say hi!!"));
    assert!(history.extend(run, "Say hi!!", "Say hi!!!"));
    assert_eq!(undo(&mut history, "Say hi!!!"), "Say hi");
    assert!(!history.can_undo());
    assert_eq!(redo(&mut history, "Say hi"), "Say hi!!!");
}

#[test]
fn a_run_ends_with_anything_else() {
    let mut history = History::new();
    let (text, run) = edit_run(&mut history, "Say hi", 6..6, "!");
    let other = edit(&mut history, "Hide slide", &text, 0..0, "x");
    assert!(
        !history.extend(run, &other, "xSay hi!!"),
        "another step came after it"
    );

    let mut history = History::new();
    let (text, run) = edit_run(&mut history, "Say hi", 6..6, "!");
    let back = undo(&mut history, &text);
    let again = redo(&mut history, &back);
    assert!(
        !history.extend(run, &again, "Say hi!!"),
        "undone and done again"
    );
}

#[test]
fn a_run_that_comes_back_to_where_it_started_is_gone_and_ended() {
    let mut history = History::new();
    let before = edit(&mut history, "Delete slide", "abc", 0..1, "");
    let (text, run) = edit_run(&mut history, &before, 2..2, "a");
    assert!(
        history.extend(run, &text, &before),
        "typed, then taken back"
    );
    assert_eq!(history.undo_label(), Some("Delete slide"));
    assert!(
        !history.extend(run, &before, "bcb"),
        "the run's step has gone, and the run with it"
    );
    assert_eq!(history.undo_label(), Some("Delete slide"));
}

#[test]
fn saving_in_the_middle_of_a_run_ends_it() {
    let mut history = History::new();
    let (text, run) = edit_run(&mut history, "Say hi", 6..6, "!");
    history.mark_saved();
    assert!(
        !history.extend(run, &text, "Say hi!!"),
        "what was saved stays a step of its own"
    );
    let (more, _) = edit_run(&mut history, &text, 7..7, "!");
    assert!(!history.is_saved());
    assert_eq!(undo(&mut history, &more), "Say hi!");
    assert!(history.is_saved(), "back at what was saved");
}

#[test]
fn a_run_that_comes_back_to_the_saved_text_reads_saved() {
    let mut history = History::new();
    let (text, run) = edit_run(&mut history, "Say hi", 6..6, "!");
    assert!(!history.is_saved());
    assert!(history.extend(run, &text, "Say hi"));
    assert!(history.is_saved());
    assert!(!history.can_undo());
}

#[test]
fn an_empty_change_records_nothing() {
    let mut history = History::new();
    assert!(
        history
            .record("Edit", Change { edits: Vec::new() }, 0, 0, true)
            .is_none()
    );
    assert!(!history.can_undo());
    assert!(history.is_saved());
}
