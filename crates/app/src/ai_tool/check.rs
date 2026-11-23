//! Checking a talk: every complaint about it, by slide, as the window would show them.

use std::panic::{AssertUnwindSafe, catch_unwind};

use serde::Serialize;
use slides_components::Diagnostic;
use slides_engine::ComponentState;
use slides_format::Block;

use super::talk::{OpenTalk, TalkPlace};
use crate::builds::{NEEDS_BUILDING, NO_COMPILER, build_components, bundled_compiler};
use crate::permissions::BuildPermissions;

/// What the window says of a slide it couldn't lay out.
const COULDNT_LAY_OUT: &str = "This slide couldn't be laid out.";

/// What's said when the components couldn't be looked at again after a build.
const COULDNT_REFRESH: &str = "The talk's components couldn't be looked at again after building.";

/// One slide's complaints.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SlideProblems {
    /// From 1, in the order of the text, hidden slides included.
    pub slide: usize,
    pub hidden: bool,
    pub problems: Vec<String>,
}

/// Everything a talk complains of: the slides that have complaints, and the talk's own.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Checked {
    pub slides: Vec<SlideProblems>,
    pub talk: Vec<String>,
}

impl Checked {
    /// How many complaints there are in all.
    pub fn count(&self) -> usize {
        self.slides.iter().map(|slide| slide.problems.len()).sum::<usize>() + self.talk.len()
    }
}

/// Checks the talk. A folder whose owner said to always build it is built first, if this copy
/// of the app has its compiler; nothing else is ever built.
pub fn check(talk: &mut OpenTalk, permissions: &BuildPermissions) -> Checked {
    let mut built_problems = Vec::new();
    let mut talk_problems: Vec<String> = Vec::new();
    let compiler = bundled_compiler();
    if let TalkPlace::Folder(folder) = talk.place.clone()
        && permissions.always(&folder)
        && let Some(compile) = &compiler
    {
        built_problems = build_components(&folder, compile).problems;
        let deck = &mut talk.deck;
        if catch_unwind(AssertUnwindSafe(|| deck.refresh_components())).is_err() {
            talk_problems.push(COULDNT_REFRESH.to_string());
        }
    }

    let deck = &talk.deck;
    let own = catch_unwind(AssertUnwindSafe(|| {
        deck.component_problems().iter().map(problem_line).collect::<Vec<_>>()
    }));
    talk_problems.extend(own.unwrap_or_default());
    talk_problems.extend(built_problems.iter().map(problem_line));
    if matches!(talk.place, TalkPlace::Folder(_)) && talk.deck.components().iter().any(|info| info.state != ComponentState::UpToDate) {
        // As the window's build bar says it: what's needed, or that this copy can't.
        talk_problems.push(if compiler.is_some() { NEEDS_BUILDING } else { NO_COMPILER }.to_string());
    }

    let slides = (0..talk.deck.talk().slides.len())
        .filter_map(|index| {
            let problems = slide_problems(talk, index);
            (!problems.is_empty()).then(|| SlideProblems { slide: index + 1, hidden: talk.deck.is_hidden(index), problems })
        })
        .collect();
    Checked { slides, talk: talk_problems }
}

/// A component's problem as the window's problem bar says it.
fn problem_line(problem: &Diagnostic) -> String {
    if problem.line == 0 {
        format!("{}: {}", problem.file, problem.message)
    } else {
        format!("{}:{}: {}", problem.file, problem.line, problem.message)
    }
}

/// What's wrong with slide `index` (from 0), in the order the window tells it: its parse error,
/// what laying it out says of it (its parse error is the first), then what's wrong with each video, model and terminal on it.
/// Nothing is built. A slide that can't be laid out gives one sentence saying so.
pub fn slide_problems(talk: &mut OpenTalk, index: usize) -> Vec<String> {
    let deck = &mut talk.deck;
    if index >= deck.talk().slides.len() {
        return Vec::new();
    }
    let found = catch_unwind(AssertUnwindSafe(|| {
        let mut problems: Vec<String> = Vec::new();
        problems.extend(deck.notices_whole(index));
        let mut tags = Vec::new();
        tags_on(&deck.talk().slides[index].blocks, &mut tags);
        for (name, range) in tags {
            let problem = match name.as_str() {
                "Video" => deck.video_problem(index, &range),
                "Model" => deck.model_problem(index, &range),
                _ => deck.terminal_problem(index, &range),
            };
            problems.extend(problem);
            if name == "Terminal" {
                problems.extend(deck.terminal_run_problem(index, &range));
            }
        }
        // A problem a tag has is among the slide's notices too.
        let mut once = Vec::new();
        for problem in problems {
            if !once.contains(&problem) {
                once.push(problem);
            }
        }
        once
    }));
    found.unwrap_or_else(|_| vec![COULDNT_LAY_OUT.to_string()])
}

/// The `<Video>`, `<Model>` and `<Terminal>` tags among `blocks`, at any depth, with where each is.
fn tags_on(blocks: &[Block], found: &mut Vec<(String, std::ops::Range<usize>)>) {
    for block in blocks {
        if let Block::Tag(tag) = block {
            if matches!(tag.name.as_str(), "Video" | "Model" | "Terminal") {
                found.push((tag.name.clone(), tag.range.clone()));
            }
            tags_on(&tag.children, found);
        }
    }
}

/// The result as plain text: a `Slide N: problem` line each (`Slide N (hidden): problem` for a hidden slide), the talk's, then a last line.
pub fn check_text(checked: &Checked) -> String {
    let mut out = String::new();
    for slide in &checked.slides {
        for problem in &slide.problems {
            let hidden = if slide.hidden { " (hidden)" } else { "" };
            out.push_str(&format!("Slide {}{hidden}: {problem}\n", slide.slide));
        }
    }
    for problem in &checked.talk {
        out.push_str(&format!("{problem}\n"));
    }
    let (count, slides) = (checked.count(), checked.slides.len());
    let problems = |n: usize| format!("{n} problem{}", if n == 1 { "" } else { "s" });
    let on = format!("on {slides} slide{}", if slides == 1 { "" } else { "s" });
    out.push_str(&match (count, slides, checked.talk.len()) {
        (0, _, _) => "No problems found.\n".to_string(),
        (_, 0, _) => format!("{} in the talk.\n", problems(count)),
        (_, _, 0) => format!("{} {on}.\n", problems(count)),
        _ => format!("{} {on} and in the talk.\n", problems(count)),
    });
    out
}
