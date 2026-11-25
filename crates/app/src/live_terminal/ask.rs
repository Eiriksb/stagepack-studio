//! The dialog that asks for the yes: what the talk wants to run, in full, and where, with *Run*,
//! *Always for this folder* (for a talk kept in a folder) and *Don't run*. *Don't run* has the
//! keyboard, and is what Escape and Enter choose. A click beside the dialog does nothing.
//!
//! The same dialog asks whether a button may type a text into a live terminal: the text, in full,
//! then *into* and the terminal's command.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::scroll::{Scrollbar, ScrollbarMode};
use gpui_kit::component::{ActiveTheme as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

pub const RUN_ASK_ID: &str = "run-ask";
pub const RUN_YES_ID: &str = "run-yes";
/// Not there for a packed or untitled talk, whose yes is never remembered.
pub const RUN_ALWAYS_ID: &str = "run-always";
pub const RUN_NO_ID: &str = "run-no";
/// Each word of the command, one to a line, has this id with its number.
pub const RUN_LINE_ID: &str = "run-line";
/// The part above the words, which doesn't scroll: where it runs, what to know, the warning.
pub const RUN_HEAD_ID: &str = "run-head";
/// Each line of the head after the question has this id with its number.
pub const RUN_HEAD_LINE_ID: &str = "run-head-line";
/// The words, which scroll if there are many.
pub const RUN_WORDS_ID: &str = "run-words";
/// The layer over the words that holds their scrollbar.
pub const RUN_WORDS_BAR_ID: &str = "run-words-bar";
/// The dialog's title.
pub const RUN_TITLE_ID: &str = "run-title";
/// The text a button would type, when that's what is asked.
pub const RUN_TYPED_ID: &str = "run-typed";
/// The line *into* after the typed text, before the terminal's command.
pub const RUN_INTO_ID: &str = "run-into";
/// The line under the terminal's command saying which folder *Always for this folder* is for, when
/// typed text is asked about for a talk kept in one.
pub const RUN_TYPED_FOLDER_ID: &str = "run-typed-folder";
/// How the words' scrollbar is shown: always, so a list that goes on beyond its box shows it does
/// before anyone scrolls. (The theme's own way shows a scrollbar only once something is scrolled.)
pub const RUN_WORDS_SCROLLBAR: ScrollbarMode = ScrollbarMode::Always;
/// The most the words' box is high.
const WORDS_HEIGHT: f32 = 240.;

pub const RUN_QUESTION: &str = "This talk wants to run a program on your computer.";
pub const RUN_TITLE: &str = "Run a program?";
pub const TYPE_TITLE: &str = "Type this into the terminal?";
pub const TYPE_INTO: &str = "into";
pub const RUN_WARNING: &str = "It can do anything you can do. Run it only if you trust this talk.";
pub const RUN_IN_NEW_FOLDER: &str = "in a new empty folder";
pub const RUN_PROGRAM_IN_TALK: &str = "This program comes with the talk.";
pub const RUN_PROGRAM_BY_PATH: &str = "The talk names this program by its path.";
/// A command is called long, and the dialog says to scroll through it, if it has more words than
/// this, more characters in all than [`CHARACTERS_THAT_FIT`], or a word longer than
/// [`LONGEST_WORD_THAT_FITS`]: any of these may go beyond the words' box. The count is of the
/// words as they are shown.
pub const WORDS_THAT_FIT: usize = 8;
pub const CHARACTERS_THAT_FIT: usize = 300;
pub const LONGEST_WORD_THAT_FITS: usize = 60;
pub const RUN_LONG: &str = "This command is long. Scroll to see all of it before you choose.";
pub const RUN_NOT_ASCII: &str = "Some of this command isn't plain ASCII, so check each letter.";

/// What is asked: the command's words as they will run (the program as the file that runs), and the
/// folder it would run in for a talk kept in one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunQuestion {
    pub command: Vec<String>,
    /// Whether the program is a file inside the talk's folder.
    pub program_in_talk: bool,
    /// Whether the talk names the program by a path, to a file outside its folder.
    pub program_by_path: bool,
    /// The talk's folder, or none for a packed or untitled talk, which runs in a new empty folder.
    pub folder: Option<PathBuf>,
    /// The text a button would type into the terminal running `command`, when that's what is
    /// asked, rather than whether to run `command`.
    pub typed: Option<String>,
}

/// What the person chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunAnswer {
    /// Run it this time, and for the rest of the session.
    Run,
    /// As `Run`, and remember it for this folder.
    Always,
    DontRun,
}

/// Asks `question` in `window`, and tells `answer` what was chosen, once. While another dialog
/// is showing nothing is asked, `answer` hears nothing, and this says `false`: the person has
/// said neither yes nor no, so the question is for the caller to ask again.
pub fn ask_to_run(
    window: &mut Window,
    cx: &mut App,
    question: RunQuestion,
    answer: impl Fn(RunAnswer, &mut Window, &mut App) + 'static,
) -> bool {
    if window.has_active_dialog(cx) {
        return false;
    }
    let answered = Rc::new(Cell::new(false));
    let answer = Rc::new(answer);
    let no_focus = cx.focus_handle();
    let no_focus_later = no_focus.clone();
    let words_scroll = ScrollHandle::new();
    // What the dialog does when it's closed some way that isn't one of its buttons.
    let finish = {
        let (answered, answer) = (answered.clone(), answer.clone());
        move |chosen: RunAnswer, window: &mut Window, cx: &mut App| -> bool {
            if answered.replace(true) {
                return false;
            }
            answer(chosen, window, cx);
            true
        }
    };
    let finish = Rc::new(finish);
    window.open_dialog(cx, {
        let finish = finish.clone();
        move |dialog, _, cx| {
            let choose = |chosen: RunAnswer| {
                let finish = finish.clone();
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    if finish(chosen, window, cx) {
                        window.close_dialog(cx);
                    }
                }
            };
            let (escape, enter) = (finish.clone(), finish.clone());
            let mono = cx.theme().mono_font_family.clone();
            let words = word_lines(&question);
            let head = head_lines(&question);
            let (title, label) = if question.typed.is_some() { (TYPE_TITLE, TYPE_TITLE) } else { (RUN_TITLE, RUN_QUESTION) };
            // The text to type, in full and as it can be seen, then *into*, before the command.
            // A text with no spaces wraps at any letter, so all of it is there to scroll to.
            let typed = question.typed.as_deref().map(|text| {
                let shown = SharedString::from(visible(text));
                v_flex()
                    .min_w_0()
                    .child(div().id(RUN_TYPED_ID).test_support().aria_label(shown.clone()).w_full().min_w_0().whitespace_normal().font_family(mono.clone()).child(shown))
                    .child(div().id(RUN_INTO_ID).test_support().aria_label(TYPE_INTO).child(TYPE_INTO))
            });
            // Which folder *Always* would be for, under the terminal's command.
            let typed_folder = question.typed.as_ref().and(question.folder.as_ref()).map(|folder| {
                let line = SharedString::from(format!("in {}", visible(&folder.display().to_string())));
                div().id(RUN_TYPED_FOLDER_ID).test_support().aria_label(line.clone()).text_sm().min_w_0().whitespace_normal().child(line)
            });
            dialog
                .title(div().id(RUN_TITLE_ID).test_support().aria_label(title).child(title))
                .close_button(false)
                .overlay_closable(false)
                .on_cancel(move |_, window, cx| escape(RunAnswer::DontRun, window, cx))
                // Enter with the keyboard on nothing in particular is not a yes.
                .on_ok(move |_, window, cx| enter(RunAnswer::DontRun, window, cx))
                .child(
                    // What the owner must see comes first and stays put; only the words scroll.
                    v_flex()
                        .id(RUN_ASK_ID)
                        .test_support()
                        .aria_label(label)
                        .gap_2()
                        .when(question.typed.is_none(), |ask| ask.child(
                            v_flex()
                                .id(RUN_HEAD_ID)
                                .test_support()
                                .gap_1()
                                .child(div().child(RUN_QUESTION))
                                .children(head.into_iter().enumerate().map(|(index, line)| {
                                    div().id((RUN_HEAD_LINE_ID, index)).test_support().aria_label(line.clone()).text_sm().child(line)
                                })),
                        ))
                        .child(
                            // The words scroll in their box, with a scrollbar that is always drawn.
                            div()
                                .id(RUN_WORDS_ID)
                                .test_support()
                                .relative()
                                .max_h(px(WORDS_HEIGHT))
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .id("run-words-area")
                                        .max_h(px(WORDS_HEIGHT))
                                        .overflow_y_scroll()
                                        .track_scroll(&words_scroll)
                                        .child(
                                            v_flex()
                                                .min_w_0()
                                                .pr(Scrollbar::width())
                                                .children(typed)
                                                .child(v_flex().font_family(mono).children(words.into_iter().enumerate().map(|(index, line)| {
                                                    div().id((RUN_LINE_ID, index)).test_support().aria_label(line.clone()).child(line)
                                                })))
                                                .children(typed_folder),
                                        ),
                                )
                                .child(
                                    div().id(RUN_WORDS_BAR_ID).test_support().absolute().inset_0().child(
                                        Scrollbar::vertical(&words_scroll).id("run-words-scrollbar").mode(RUN_WORDS_SCROLLBAR).viewport_from_layout(),
                                    ),
                                ),
                        ),
                )
                .footer(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .child(Button::new(RUN_NO_ID).primary().label("Don't run").track_focus(&no_focus).on_click(choose(RunAnswer::DontRun)))
                        .when(question.folder.is_some(), |footer| {
                            footer.child(Button::new(RUN_ALWAYS_ID).ghost().label("Always for this folder").on_click(choose(RunAnswer::Always)))
                        })
                        .child(Button::new(RUN_YES_ID).ghost().label("Run").on_click(choose(RunAnswer::Run))),
                )
        }
    });
    // The keyboard starts on Don't run.
    window.defer(cx, move |window, cx| no_focus_later.focus(window, cx));
    true
}

/// `text` as it can be seen: quoted, with a quote or a backslash escaped as the tag does, and
/// anything that hides written out, so what shows is exactly what runs.
fn visible(text: &str) -> String {
    let mut shown = String::from("\"");
    for c in text.chars() {
        match c {
            '"' | '\\' => {
                shown.push('\\');
                shown.push(c);
            }
            c if super::permission::hides(c) => shown.push_str(&format!("\\u{{{:X}}}", c as u32)),
            // A letter of a right-to-left script is never drawn, as it would turn the line round:
            // the order seen is the order sent.
            c if right_to_left(c) => shown.push_str(&format!("[U+{:04X}]", c as u32)),
            // A letter from outside Latin-1 is shown with what it is, as one that looks like
            // another can't be told by eye.
            c if c as u32 > 0xFF => shown.push_str(&format!("{c}[U+{:04X}]", c as u32)),
            c => shown.push(c),
        }
    }
    shown.push('"');
    shown
}

/// Whether `c` is in a block of a right-to-left script: Hebrew, Arabic, Syriac, Thaana, NKo,
/// Samaritan, Mandaic and the rest of U+0590..U+08FF, the Hebrew and Arabic presentation forms, and
/// the right-to-left blocks of the supplementary planes (U+10800..U+10FFF, U+1E800..U+1EFFF).
fn right_to_left(c: char) -> bool {
    matches!(c as u32, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF)
}

/// The lines above the words: where it runs, what to know about it, and the warning.
fn head_lines(question: &RunQuestion) -> Vec<SharedString> {
    let mut lines: Vec<SharedString> = vec!["in".into()];
    lines.push(match &question.folder {
        Some(folder) => visible(&folder.display().to_string()).into(),
        None => RUN_IN_NEW_FOLDER.into(),
    });
    if question.program_in_talk {
        lines.push(RUN_PROGRAM_IN_TALK.into());
    } else if question.program_by_path {
        lines.push(RUN_PROGRAM_BY_PATH.into());
    }
    if question.command.iter().any(|word| !word.is_ascii()) {
        lines.push(RUN_NOT_ASCII.into());
    }
    if is_long(&word_lines(question)) {
        lines.push(RUN_LONG.into());
    }
    lines.push(RUN_WARNING.into());
    lines
}

/// The words of the command, one to a line, each as it can be seen.
fn word_lines(question: &RunQuestion) -> Vec<SharedString> {
    question.command.iter().map(|word| SharedString::from(visible(word))).collect()
}

/// Whether the words, as shown, may go beyond their box (see [`WORDS_THAT_FIT`]). This is decided
/// the same way every time from the words, not measured, so the notice is there from the start.
fn is_long(words: &[SharedString]) -> bool {
    let length = |word: &SharedString| word.chars().count();
    words.len() > WORDS_THAT_FIT
        || words.iter().map(length).sum::<usize>() > CHARACTERS_THAT_FIT
        || words.iter().any(|word| length(word) > LONGEST_WORD_THAT_FITS)
}
