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

/// Splits a file into slides. Every byte belongs to exactly one slide.
pub fn split_slides(source: &str) -> Vec<RawSlide> {
    let bom = if source.starts_with('\u{feff}') { 3 } else { 0 };
    let all = lines(source);
    let mut breaks = Vec::new(); // (line index, closing line index if settings)
    let mut fence: Option<char> = None;
    let mut i = 0;
    while i < all.len() {
        let text = all[i].text;
        let text = if i == 0 { &text[bom.min(text.len())..] } else { text };
        let trimmed = text.trim_start();
        if let Some(open) = fence {
            if trimmed.starts_with(&open.to_string().repeat(3)) {
                fence = None;
            }
        } else if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fence = trimmed.chars().next();
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
    if parse_entry(all.get(at + 1)?.text, 0).is_none() {
        return None;
    }
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
