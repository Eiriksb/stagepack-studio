use std::ops::Range;

use crate::settings::{Settings, parse_entry};

/// One slide as found in the file, before its body is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawSlide {
    /// From the slide's break line (0 for the first slide) to the next break.
    pub range: Range<usize>,
    pub settings: Option<Settings>,
    /// What follows the settings block, or the break line when there is none.
    pub body: Range<usize>,
}

struct Line<'a> {
    /// Without the line ending.
    text: &'a str,
    start: usize,
    /// After the line ending.
    end: usize,
}

fn lines(source: &str) -> Vec<Line<'_>> {
    let mut out = Vec::new();
    let mut start = 0;
    while start < source.len() {
        let rest = &source[start..];
        let (len, end) = match rest.find('\n') {
            Some(i) => (i, start + i + 1),
            None => (rest.len(), source.len()),
        };
        let text = rest[..len].strip_suffix('\r').unwrap_or(&rest[..len]);
        out.push(Line { text, start, end });
        start = end;
    }
    out
}

fn is_break(text: &str) -> bool {
    text.trim_end_matches(' ') == "---"
}

/// A code fence: the mark it's made of, how many of them open it, and the column they
/// start at.
#[derive(Clone, Copy)]
struct Fence {
    mark: u8,
    length: usize,
    column: usize,
}

/// The run of fence marks starting `text` after its blank space, if they start no further in
/// than column `most`, a tab going on to the next multiple of four.
fn fence_marks(text: &str, most: usize) -> Option<(Fence, &str)> {
    let blank = text.len() - text.trim_start_matches([' ', '\t']).len();
    let column = text[..blank].bytes().fold(0, |at, byte| if byte == b'\t' { at + 4 - at % 4 } else { at + 1 });
    if column > most {
        return None;
    }
    let rest = &text[blank..];
    let mark = *rest.as_bytes().first().filter(|mark| matches!(mark, b'`' | b'~'))?;
    let length = rest.bytes().take_while(|byte| *byte == mark).count();
    (length >= 3).then(|| (Fence { mark, length, column }, &rest[length..]))
}

/// The fence `text` opens, if it opens one: three or more tildes, or three or more
/// backticks with no backtick after them on the line, as a line with more is code in a
/// paragraph. Four columns in is code by indentation, as CommonMark has it.
fn opens(text: &str) -> Option<Fence> {
    let (fence, info) = fence_marks(text, 3)?;
    (fence.mark == b'~' || !info.contains('`')).then_some(fence)
}

/// Whether `text` closes `open`: at least as many of its marks, and nothing after them but
/// spaces and tabs. A closing line can be up to three columns further in than the opening
/// one, as far as a fence in a list item can be closed past its item's text, which the
/// opening line is never left of.
fn closes(text: &str, open: Fence) -> bool {
    fence_marks(text, open.column + 3).is_some_and(|(fence, rest)| {
        fence.mark == open.mark && fence.length >= open.length && rest.trim_matches([' ', '\t']).is_empty()
    })
}

/// Splits a file into slides. Every byte belongs to exactly one slide.
pub fn split_slides(source: &str) -> Vec<RawSlide> {
    let bom = if source.starts_with('\u{feff}') { 3 } else { 0 };
    let all = lines(source);
    let mut breaks = Vec::new(); // (line index, closing line index if settings)
    let mut fence: Option<Fence> = None;
    let mut i = 0;
    while i < all.len() {
        let text = all[i].text;
        let text = if i == 0 { &text[bom.min(text.len())..] } else { text };
        if let Some(open) = fence {
            if closes(text, open) {
                fence = None;
            }
        } else if let Some(open) = opens(text) {
            fence = Some(open);
        } else if is_break(text) {
            let close = settings_close(&all, i);
            breaks.push((i, close));
            i = close.unwrap_or(i);
        }
        i += 1;
    }

    // Content before the first break is a slide of its own, unless the file
    // opens with a break.
    let mut starts: Vec<(usize, Option<usize>)> = Vec::new();
    if breaks.first().map(|b| b.0) != Some(0) {
        starts.push((0, None));
    }
    starts.extend(breaks.iter().copied());

    let mut slides = Vec::new();
    for (n, &(line, close)) in starts.iter().enumerate() {
        let from = if n == 0 { 0 } else { all[line].start };
        let to = match starts.get(n + 1) {
            Some(&(next, _)) => all[next].start,
            None => source.len(),
        };
        let (settings, body_start) = match close {
            Some(close) => {
                let entries = all[line + 1..close]
                    .iter()
                    .filter_map(|l| parse_entry(l.text, l.start))
                    .collect();
                let range = all[line].start + if line == 0 { bom } else { 0 }..all[close].end;
                (Some(Settings { range, entries }), all[close].end)
            }
            None if n == 0 && line == 0 && breaks.first().map(|b| b.0) != Some(0) => (None, from),
            None => (None, all[line].end),
        };
        slides.push(RawSlide {
            range: from..to,
            settings,
            body: body_start..to,
        });
    }
    slides
}

/// If the break at `at` opens a settings block, the index of its closing line.
fn settings_close(all: &[Line<'_>], at: usize) -> Option<usize> {
    parse_entry(all.get(at + 1)?.text, 0)?;
    for (j, l) in all.iter().enumerate().skip(at + 1) {
        if is_break(l.text) {
            return Some(j);
        }
        if !l.text.trim().is_empty() && parse_entry(l.text, 0).is_none() {
            return None;
        }
    }
    None
}
