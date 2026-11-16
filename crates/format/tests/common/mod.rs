//! What the property tests generate: hand-written talks, and what to do to them.

#![allow(dead_code)]

use proptest::prelude::*;
use slides_format::*;

#[derive(Clone, Debug)]
pub struct GenSlide {
    pub settings: Vec<(String, String)>,
    pub blocks: Vec<String>,
    pub notes: Option<String>,
    pub blank_after: bool,
    pub notes_layout: NotesLayout,
    /// A slide of `key: value` lines only, which reads as a settings block if a break
    /// comes straight after a `---` and it.
    pub key_values: bool,
}

/// Where a slide's notes go, and how they are written.
#[derive(Clone, Debug)]
pub struct NotesLayout {
    /// After the heading and this many of the slide's other blocks, as far as there are any.
    pub after_blocks: usize,
    /// Every line of the element two spaces in.
    pub indented: bool,
    /// No blank line between the heading and the element, when the element follows it.
    pub tight_before: bool,
    /// No blank line between the element and a block after it.
    pub tight_after: bool,
}

#[derive(Clone, Debug)]
pub struct GenTalk {
    pub slides: Vec<GenSlide>,
    pub crlf: bool,
    pub bom: bool,
    pub final_newline: bool,
    /// Whether a first slide with no settings starts with a `---` all the same.
    pub opening_rule: bool,
    /// A last slide with no marker in it, after the others.
    pub tail: Option<Tail>,
}

/// A last slide that holds nothing a marker could go in.
#[derive(Clone, Debug)]
pub enum Tail {
    /// A bare `---` and nothing after it.
    Empty,
    /// Notes and nothing else, with a blank line before them or not, and its own settings.
    NotesOnly { blank_before: bool, settings: Vec<(String, String)> },
}

/// Every slide holds the marker `mk<n>x`, `n` its place in the generated talk.
pub fn marker(n: usize) -> String {
    format!("mk{n}x")
}

impl GenTalk {
    pub fn text(&self) -> String {
        let mut out = String::new();
        if self.bom {
            out.push('\u{feff}');
        }
        for (n, slide) in self.slides.iter().enumerate() {
            if !slide.settings.is_empty() {
                out.push_str("---\n");
                for (key, value) in &slide.settings {
                    out.push_str(&format!("{key}: {value}\n"));
                }
                out.push_str("---\n");
            } else if n > 0 || self.opening_rule {
                out.push_str("---\n");
            }
            if slide.key_values {
                let after_a_rule = slide.settings.is_empty() && (n > 0 || self.opening_rule);
                if after_a_rule && n + 1 < self.slides.len() {
                    out.push('\n');
                }
                out.push_str(&format!("Note: {}\nOther: thing", marker(n)));
            } else {
                let mut blocks = slide.blocks.clone();
                blocks.insert(0, format!("# Slide {}", marker(n)));
                let layout = &slide.notes_layout;
                let at = 1 + layout.after_blocks.min(blocks.len() - 1);
                if let Some(notes) = &slide.notes {
                    let element = format!("<Notes>\n{notes}\n</Notes>");
                    let indent = if layout.indented { "  " } else { "" };
                    blocks.insert(at, element.lines().map(|line| format!("{indent}{line}")).collect::<Vec<_>>().join("\n"));
                }
                for (i, block) in blocks.iter().enumerate() {
                    if i > 0 {
                        let notes_here = slide.notes.is_some() && (i == at || i == at + 1);
                        let tight = notes_here && if i == at { at == 1 && layout.tight_before } else { layout.tight_after };
                        out.push_str(if tight { "\n" } else { "\n\n" });
                    }
                    out.push_str(block);
                }
            }
            out.push('\n');
            if slide.blank_after {
                out.push('\n');
            }
        }
        match &self.tail {
            None => {}
            Some(Tail::Empty) => out.push_str("---\n"),
            Some(Tail::NotesOnly { blank_before, settings }) => {
                out.push_str("---\n");
                if !settings.is_empty() {
                    for (key, value) in settings {
                        out.push_str(&format!("{key}: {value}\n"));
                    }
                    out.push_str("---\n");
                }
                if *blank_before {
                    out.push('\n');
                }
                out.push_str("<Notes>\nSay hi\n</Notes>\n");
            }
        }
        if !self.final_newline {
            out.truncate(out.trim_end_matches('\n').len());
        }
        if self.crlf { out.replace('\n', "\r\n") } else { out }
    }
}

/// How many cases a property tries: `PROPTEST_CASES` if it's set, or `default`.
pub fn cases(default: u32) -> u32 {
    std::env::var("PROPTEST_CASES").ok().and_then(|cases| cases.parse().ok()).unwrap_or(default)
}

pub fn block() -> impl Strategy<Value = String> {
    prop::sample::select(
        vec![
            "## A heading".to_string(),
            "Some *text* with trailing spaces  ".to_string(),
            "A paragraph\nover two lines.".to_string(),
            "- one\n- two".to_string(),
            "1. first\n2. second".to_string(),
            // Lists as people indent them by hand: four spaces or a tab in, one space in, and
            // straight under a paragraph's line.
            "- a\n    - deep\n- b".to_string(),
            "- a\n\t- deep\n- b".to_string(),
            "1. a\n    - nested\n    1. one\n2. b".to_string(),
            " - a\n - b".to_string(),
            "Intro:\n- a\n- b".to_string(),
            "- a\n    - b\n        - c\n- d".to_string(),
            "- a\n    - b\n    - x\n- d".to_string(),
            "- a\n\t- b\n\t- x\n- d".to_string(),
            "- &nbsp;\n  - under\n- c".to_string(),
            "```\n---\ncode\n```".to_string(),
            "~~~md\n---\n~~~".to_string(),
            "> A quote".to_string(),
            // Tables as people write them by hand.
            "a | b\n--|:-:\nc | `x\\|y`".to_string(),
            "| **b** | <Text color=\"red\">r</Text> |\n| :-- | --: |\n| \\| | |\n| short |".to_string(),
            "Intro:\n| a |\n| - |\n| <Icon name=\"x\" /> |".to_string(),
        ]
        .into_iter()
        .chain(TAG_BLOCKS.iter().map(|block| block.to_string()))
        .collect::<Vec<_>>(),
    )
}

/// Tags and other blocks that aren't text, as people write them by hand.
pub const TAG_BLOCKS: [&str; 12] = [
    "<Icon name=\"star\" />",
    "<Box x=\"10\" y=\"20\" w=\"30\">\n\nIn a *box*\n\n</Box>",
    "<Shape\n  kind=\"circle\"\n  color=\"red\"\n/>",
    "<Icon name='a \"quoted\" star' size=\"2\" />",
    "<Icon name=\"first\" flag name=\"second\" />",
    "<Icon name=\"one\" />\n<Icon name=\"two\" />",
    "- an item\n\n  <Icon name=\"in-a-list\" />",
    "<Box>\n\n<Icon name=\"in-a-box\" />\n\n</Box>",
    "![A cat](cat.png)",
    "| a | b |\n| --- | --- |\n| 1 | 2 |",
    "<Chart\tdata=\"x.csv\"   tall >\n\nSome words\n\n</Chart>",
    "<Region name=\"right\" />",
];

pub fn slide_settings() -> impl Strategy<Value = Vec<(String, String)>> {
    prop::sample::subsequence(vec![("layout", "section  "), ("hidden", "true"), ("transition", "fade")], 0..=3)
    .prop_map(|entries| entries.into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
}

pub fn talk_settings() -> impl Strategy<Value = Vec<(String, String)>> {
    prop::sample::subsequence(
        vec![("title", "My talk"), ("color.background", "#000"), ("font", "Inter"), ("font.mono", "Mono  ")],
        0..=4,
    )
    .prop_map(|entries| entries.into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
}

pub fn gen_slide() -> impl Strategy<Value = GenSlide> {
    (
        slide_settings(),
        prop::collection::vec(block(), 0..3),
        prop::option::of(prop::sample::select(vec!["Say **this**".to_string(), "- a point\n- another".to_string()])),
        any::<bool>(),
        (0..3usize, any::<[bool; 3]>()).prop_map(|(after_blocks, [indented, tight_before, tight_after])| NotesLayout {
            after_blocks,
            indented,
            tight_before,
            tight_after,
        }),
        prop::bool::weighted(0.25),
    )
        .prop_map(|(settings, blocks, notes, blank_after, notes_layout, key_values)| GenSlide {
            settings,
            blocks,
            notes,
            blank_after,
            notes_layout,
            key_values,
        })
}

pub fn tail() -> impl Strategy<Value = Option<Tail>> {
    prop_oneof![
        2 => Just(None),
        1 => Just(Some(Tail::Empty)),
        1 => (any::<bool>(), slide_settings()).prop_map(|(blank_before, settings)| Some(Tail::NotesOnly { blank_before, settings })),
    ]
}

pub fn gen_talk() -> impl Strategy<Value = GenTalk> {
    (prop::collection::vec(gen_slide(), 1..=8), talk_settings(), any::<[bool; 4]>(), tail()).prop_map(
        |(mut slides, talk, [crlf, bom, final_newline, opening_rule], tail)| {
            let own = std::mem::take(&mut slides[0].settings);
            slides[0].settings = talk.into_iter().chain(own).collect();
            GenTalk { slides, crlf, bom, final_newline, opening_rule, tail }
        },
    )
}

#[derive(Clone, Debug)]
pub enum Op {
    /// After which slide, and whether the new one is of `key: value` lines only.
    Insert(Option<usize>, bool),
    Delete(usize),
    Duplicate(usize),
    Move(usize, usize),
    Hide(usize, bool),
    Notes(usize, String),
}

pub fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (prop::option::of(0..8usize), any::<bool>()).prop_map(|(after, key_values)| Op::Insert(after, key_values)),
        (0..8usize).prop_map(Op::Delete),
        (0..8usize).prop_map(Op::Duplicate),
        (0..8usize, 0..8usize).prop_map(|(a, b)| Op::Move(a, b)),
        (0..8usize, any::<bool>()).prop_map(|(a, b)| Op::Hide(a, b)),
        (0..8usize, prop::sample::select(vec!["", "New words", "Watch </Notes> this", "**Bold**\n\n- list"]))
            .prop_map(|(a, s)| Op::Notes(a, s.to_string())),
    ]
}

/// The marker given to an inserted slide.
pub const NEW: usize = 99;

/// What `op` does to a talk of `n` slides: its change, the slides it should leave in order
/// (by marker), and the markers of the slides it names.
pub fn run(talk: &Talk, op: &Op) -> Option<(Change, Vec<usize>, Vec<usize>)> {
    let n = talk.slides.len();
    let order: Vec<usize> = (0..n).collect();
    Some(match op {
        Op::Insert(after, key_values) => {
            let after = after.map(|a| a % n);
            let mut expected = order;
            expected.insert(after.map_or(0, |a| a + 1), NEW);
            let text = if *key_values { format!("Note: {}\nOther: new\n", marker(NEW)) } else { format!("# Slide {}\n\nNew\n", marker(NEW)) };
            (insert_slide(talk, after, &text), expected, vec![NEW])
        }
        Op::Delete(i) => {
            let i = i % n;
            let mut expected = order;
            expected.remove(i);
            (delete_slide(talk, i)?, expected, vec![i])
        }
        Op::Duplicate(i) => {
            let i = i % n;
            let mut expected = order;
            expected.insert(i + 1, i);
            (duplicate_slide(talk, i), expected, vec![])
        }
        Op::Move(from, to) => {
            let (from, to) = (from % n, to % n);
            let mut expected = order;
            let moved = expected.remove(from);
            expected.insert(to, moved);
            (move_slide(talk, from, to), expected, vec![from])
        }
        Op::Hide(i, hidden) => (set_hidden(talk, i % n, *hidden), order, vec![i % n]),
        Op::Notes(i, notes) => (set_notes(talk, i % n, notes), order, vec![i % n]),
    })
}

/// Text someone might type into the notes box, with everything the parser could take for
/// something else.
pub fn typed_notes() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop::sample::select(vec![
            "a", "Z", "é", "🎉", " ", "  ", ".", "*", "_", "#", "-", "&", ":", "|", "[x](y)", "\n", "\n\n", "<", ">", "{", "}",
            "\\", "`", "```", "~~~", "\n---\n", "\n---  \n", "\n  ```\n", "<b>", "</b>", "</Notes>", "<Notes>", "<Box x=\"1\">",
            "\\<", "\\-", "\n> ", "\n- ", "\n1. ", "key: value", "a```b", "````",
        ]),
        0..24,
    )
    .prop_map(|parts| parts.concat())
}

/// A table as someone might write it by hand, with what can be in it.
#[derive(Clone, Debug)]
pub struct GenTable {
    /// What's in each cell, row by row, the header first, with the spaces around it.
    pub rows: Vec<Vec<String>>,
    /// Each alignment cell.
    pub rule: Vec<String>,
    /// Whether each row, the alignment row last, starts and ends with `|`.
    pub edges: Vec<(bool, bool)>,
    /// What starts each line: nothing, or the indent of a list item the table is in.
    pub in_a_list: bool,
    /// A paragraph's line straight before the table.
    pub after_a_paragraph: bool,
}

/// What marks the generated table: its first header cell holds it.
pub const TABLE_MARK: &str = "Tq";

impl GenTable {
    pub fn text(&self) -> String {
        let indent = if self.in_a_list { "  " } else { "" };
        let mut lines = Vec::new();
        if self.in_a_list {
            lines.push("- item".to_string());
            lines.push(String::new());
        }
        if self.after_a_paragraph {
            lines.push(format!("{indent}Before the table"));
        }
        let line = |cells: &[String], (lead, trail): (bool, bool)| {
            format!("{indent}{}{}{}", if lead { "|" } else { "" }, cells.join("|"), if trail { "|" } else { "" })
        };
        let rule_edges = *self.edges.last().unwrap();
        for (r, cells) in self.rows.iter().enumerate() {
            lines.push(line(cells, self.edges[r]));
            if r == 0 {
                lines.push(line(&self.rule, rule_edges));
            }
        }
        lines.join("\n")
    }

    /// The table with every row given the header's cells, and `|` at both ends.
    pub fn regular(mut self) -> GenTable {
        let columns = self.rows[0].len();
        for row in &mut self.rows {
            row.resize(columns, " ".to_string());
        }
        self.edges.fill((true, true));
        self
    }
}

/// What a generated cell can hold.
pub fn cell_content() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "a", "two words", "**bold**", "*it*", "<Text color=\"red\">red</Text>", "<Text bold>b</Text> c", "\\|", "x \\| y", "`c\\|d`",
        "`code`", "", "", "é 🎉", "1. x", "# h", "- l", "a\\\\", "<Icon name=\"star\" />", "[l](http://x)", "&amp;", "{x}",
    ])
    .prop_map(str::to_string)
}

pub fn gen_table() -> impl Strategy<Value = GenTable> {
    (1..=4usize, 1..=4usize).prop_flat_map(|(columns, body)| {
        let row = move || {
            (prop::collection::vec((cell_content(), 0..3usize, 0..3usize), columns..=columns), prop_oneof![4 => Just(0i32), 1 => Just(-1), 1 => Just(1)])
        };
        (
            prop::collection::vec(row(), body + 1),
            prop::collection::vec(prop::sample::select(vec!["---", " --- ", ":--", " :-: ", "--:", "-", " - "]), columns..=columns),
            prop::collection::vec((any::<bool>(), any::<bool>()), body + 2),
            any::<bool>(),
            any::<bool>(),
            any::<bool>(),
        )
            .prop_map(move |(rows, rule, edges, in_a_list, after_a_paragraph, uneven)| {
                let mut edges = edges;
                // One cell needs a `|` to be a table's row at all.
                for edge in &mut edges {
                    if columns == 1 && !edge.0 && !edge.1 {
                        edge.1 = true;
                    }
                }
                let rows: Vec<Vec<String>> = rows
                    .into_iter()
                    .enumerate()
                    .map(|(r, (cells, change))| {
                        let (lead, trail) = edges[r];
                        let last = cells.len() - 1;
                        let mut cells: Vec<String> = cells
                            .into_iter()
                            .enumerate()
                            .map(|(c, (content, before, after))| {
                                let content = match content.as_str() {
                                    _ if r == 0 && c == 0 => TABLE_MARK.to_string(),
                                    // A cell at an edge with no `|` needs something in it.
                                    "" if (c == 0 && !lead) || (c == last && !trail) => "e".to_string(),
                                    _ => content,
                                };
                                format!("{}{content}{}", " ".repeat(before), " ".repeat(after))
                            })
                            .collect();
                        if uneven && r > 0 {
                            match change {
                                -1 if cells.len() > 1 && (trail || lead) => {
                                    cells.pop();
                                    if !trail && cells.last().is_some_and(|cell| cell.trim().is_empty()) {
                                        *cells.last_mut().unwrap() = "e".to_string();
                                    }
                                }
                                1 => cells.push(" more ".to_string()),
                                _ => {}
                            }
                        }
                        cells
                    })
                    .collect();
                GenTable { rows, rule: rule.into_iter().map(str::to_string).collect(), edges, in_a_list, after_a_paragraph }
            })
    })
}

/// A generated talk with a generated table on one of its slides, and which slide.
pub fn talk_with_a_table() -> impl Strategy<Value = (String, usize, GenTable)> {
    talk_with(gen_table().boxed())
}

/// The same, with a table whose rows all have the header's cells and `|` at both ends.
pub fn talk_with_a_regular_table() -> impl Strategy<Value = (String, usize, GenTable)> {
    talk_with(gen_table().prop_map(GenTable::regular).boxed())
}

fn talk_with(table: BoxedStrategy<GenTable>) -> impl Strategy<Value = (String, usize, GenTable)> {
    (gen_talk(), any::<prop::sample::Index>(), table, any::<prop::sample::Index>()).prop_map(|(mut talk, slide, table, at)| {
        let slide = slide.index(talk.slides.len());
        let own = &mut talk.slides[slide];
        own.key_values = false;
        let at = at.index(own.blocks.len() + 1);
        own.blocks.insert(at, table.text());
        (talk.text(), slide, table)
    })
}
