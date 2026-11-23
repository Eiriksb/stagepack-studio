//! The dialog that asks for the yes: what the talk wants to run, in full, and where, with *Run*,
//! *Always for this folder* (for a talk kept in a folder) and *Don't run*. *Don't run* has the
//! keyboard, and is what Escape and Enter choose. A click beside the dialog does nothing.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

pub const RUN_ASK_ID: &str = "run-ask";
pub const RUN_YES_ID: &str = "run-yes";
/// Not there for a packed or untitled talk, whose yes is never remembered.
pub const RUN_ALWAYS_ID: &str = "run-always";
pub const RUN_NO_ID: &str = "run-no";
/// Each line of the question has this id with its number.
pub const RUN_LINE_ID: &str = "run-line";

pub const RUN_QUESTION: &str = "This talk wants to run a program on your computer.";
pub const RUN_WARNING: &str = "It can do anything you can do. Run it only if you trust this talk.";
pub const RUN_IN_NEW_FOLDER: &str = "in a new empty folder";
pub const RUN_PROGRAM_IN_TALK: &str = "The program is a file that comes with this talk.";

/// What is asked: the command's words as they will run (the program as the file that runs), and the
/// folder it would run in for a talk kept in one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunQuestion {
    pub command: Vec<String>,
    /// Whether the program is a file the talk brings with it.
    pub program_in_talk: bool,
    /// The talk's folder, or none for a packed or untitled talk, which runs in a new empty folder.
    pub folder: Option<PathBuf>,
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
            let lines = lines_of(&question);
            dialog
                .title("Run a program?")
                .close_button(false)
                .overlay_closable(false)
                .on_cancel(move |_, window, cx| escape(RunAnswer::DontRun, window, cx))
                // Enter with the keyboard on nothing in particular is not a yes.
                .on_ok(move |_, window, cx| enter(RunAnswer::DontRun, window, cx))
                .child(
                    v_flex()
                        .id(RUN_ASK_ID)
                        .test_support()
                        .aria_label(RUN_QUESTION)
                        .gap_2()
                        .child(div().child(RUN_QUESTION))
                        .child(v_flex().font_family(mono).children(lines.into_iter().enumerate().map(|(index, line)| {
                            div().id((RUN_LINE_ID, index)).test_support().aria_label(line.clone()).child(line)
                        })))
                        .child(div().text_sm().child(RUN_WARNING)),
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
            c => shown.push(c),
        }
    }
    shown.push('"');
    shown
}

/// The lines of the question, one to a line: each word of the command, then where it runs.
fn lines_of(question: &RunQuestion) -> Vec<SharedString> {
    let mut lines: Vec<SharedString> = question.command.iter().map(|word| SharedString::from(visible(word))).collect();
    lines.push("in".into());
    lines.push(match &question.folder {
        Some(folder) => visible(&folder.display().to_string()).into(),
        None => RUN_IN_NEW_FOLDER.into(),
    });
    if question.program_in_talk {
        lines.push(RUN_PROGRAM_IN_TALK.into());
    }
    lines
}
