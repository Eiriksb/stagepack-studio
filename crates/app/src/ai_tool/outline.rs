//! A talk's outline: each slide's number, layout, first words, clicks, and whether it's hidden
//! or has notes.

use std::panic::{AssertUnwindSafe, catch_unwind};

use serde::Serialize;
use slides_format::text_flows;

use super::talk::OpenTalk;

/// How many characters of a slide's text its outline shows.
const FIRST_WORDS: usize = 80;

/// The layout a slide has without a `layout:` setting.
const DEFAULT_LAYOUT: &str = "content";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OutlineSlide {
    /// From 1, in the order of the text, hidden slides included.
    pub slide: usize,
    pub layout: String,
    /// The first 80 characters of the slide's text, whitespace collapsed, with `…` after them
    /// when the text goes on.
    pub first_words: String,
    /// How many clicks the slide takes.
    pub steps: u32,
    pub hidden: bool,
    pub notes: bool,
}

/// Every slide of the talk, in order. A slide that can't be laid out has no clicks.
pub fn outline(talk: &mut OpenTalk) -> Vec<OutlineSlide> {
    let deck = &mut talk.deck;
    (0..deck.slide_count())
        .map(|index| {
            let layout = deck.talk().slides[index]
                .settings
                .as_ref()
                .and_then(|settings| settings.get("layout"))
                .filter(|name| !name.is_empty())
                .unwrap_or(DEFAULT_LAYOUT)
                .to_string();
            let words = catch_unwind(AssertUnwindSafe(|| first_words(deck.talk(), index))).unwrap_or_default();
            let steps = catch_unwind(AssertUnwindSafe(|| deck.timeline(index).steps)).unwrap_or(0);
            OutlineSlide {
                slide: index + 1,
                layout,
                first_words: words,
                steps,
                hidden: deck.is_hidden(index),
                notes: deck.notes(index).is_some(),
            }
        })
        .collect()
}

/// The slide's text as it reads: the text flows of its blocks in order, the title first, each
/// paragraph's spans joined, whitespace collapsed, cut at a character boundary.
fn first_words(talk: &slides_format::Talk, index: usize) -> String {
    let mut text = String::new();
    for flow in text_flows(talk, index) {
        for para in flow.paras {
            for span in para.spans {
                text.push_str(&span.text);
            }
            text.push(' ');
        }
    }
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match collapsed.char_indices().nth(FIRST_WORDS) {
        Some((cut, _)) => format!("{}…", &collapsed[..cut]),
        None => collapsed,
    }
}

/// The outline as plain text, a line a slide: `1. Title — "Hello there" (2 clicks, notes)`.
pub fn outline_text(slides: &[OutlineSlide]) -> String {
    let mut out = String::new();
    for slide in slides {
        let mut name = slide.layout.chars();
        let layout: String = name.next().map(|first| first.to_uppercase().chain(name).collect()).unwrap_or_default();
        out.push_str(&format!("{}. {layout}", slide.slide));
        if !slide.first_words.is_empty() {
            out.push_str(&format!(" — \"{}\"", slide.first_words));
        }
        let mut notes = Vec::new();
        if slide.steps > 0 {
            notes.push(format!("{} click{}", slide.steps, if slide.steps == 1 { "" } else { "s" }));
        }
        if slide.notes {
            notes.push("notes".to_string());
        }
        if slide.hidden {
            notes.push("hidden".to_string());
        }
        if !notes.is_empty() {
            out.push_str(&format!(" ({})", notes.join(", ")));
        }
        out.push('\n');
    }
    out
}
