//! Source edits: each one replaces a byte range, so a visual change touches
//! only the bytes it means to.

use std::ops::Range;

use crate::model::{Slide, Tag};

#[derive(Clone, Debug, PartialEq)]
pub struct SourceEdit {
    pub range: Range<usize>,
    pub text: String,
}

impl SourceEdit {
    pub fn apply(&self, source: &str) -> String {
        let mut out = String::with_capacity(source.len() - self.range.len() + self.text.len());
        out.push_str(&source[..self.range.start]);
        out.push_str(&self.text);
        out.push_str(&source[self.range.end..]);
        out
    }

    /// The edit that undoes this one: it covers the range the new text
    /// occupies and restores the old text.
    pub fn inverse(&self, source: &str) -> SourceEdit {
        SourceEdit {
            range: self.range.start..self.range.start + self.text.len(),
            text: source[self.range.clone()].to_string(),
        }
    }
}

fn escape(value: &str) -> String {
    value.replace('"', "&quot;")
}

/// Sets attribute `name` on `tag` to `value`.
pub fn set_attr(source: &str, tag: &Tag, name: &str, value: &str) -> SourceEdit {
    let bytes = source.as_bytes();
    let value = escape(value);
    if let Some(attr) = tag.attrs.iter().find(|a| a.name == name) {
        let r = &attr.value_range;
        let bare = attr.value == "true" && r.is_empty() && r.start == attr.name_range.end;
        if bare {
            return SourceEdit { range: r.clone(), text: format!("=\"{value}\"") };
        }
        let quoted = r.start > 0
            && matches!(bytes.get(r.start - 1), Some(b'"' | b'\''))
            && bytes.get(r.end) == bytes.get(r.start - 1);
        return if quoted {
            SourceEdit { range: r.clone(), text: value }
        } else {
            SourceEdit { range: r.clone(), text: format!("\"{value}\"") }
        };
    }
    let at = match tag.attrs.last() {
        Some(last) => {
            let r = &last.value_range;
            let bare = last.value == "true" && r.is_empty() && r.start == last.name_range.end;
            let quoted = !bare
                && r.start > 0
                && matches!(bytes.get(r.start - 1), Some(b'"' | b'\''))
                && bytes.get(r.end) == bytes.get(r.start - 1);
            if quoted { r.end + 1 } else { r.end }
        }
        None => {
            let mut i = tag.range.start + 1;
            while i < tag.open_end && !bytes[i].is_ascii_whitespace() && !matches!(bytes[i], b'>' | b'/') {
                i += 1;
            }
            i
        }
    };
    SourceEdit { range: at..at, text: format!(" {name}=\"{value}\"") }
}

fn eol_in(text: &str) -> &'static str {
    match text.find('\n') {
        Some(i) if text[..i].ends_with('\r') => "\r\n",
        _ => "\n",
    }
}

/// Sets `key` in the slide's settings block to `value`, adding the entry or
/// the whole block when it is missing.
pub fn set_setting(source: &str, slide: &Slide, key: &str, value: &str) -> SourceEdit {
    if let Some(settings) = &slide.settings {
        if let Some(entry) = settings.entries.iter().find(|e| e.key == key) {
            let r = entry.value_range.clone();
            let text = if r.is_empty() { format!(" {value}") } else { value.to_string() };
            return SourceEdit { range: r, text };
        }
        let block = &source[settings.range.clone()];
        let eol = eol_in(block);
        let body = block.trim_end_matches(['\r', '\n']);
        let close = settings.range.start + body.rfind('\n').map_or(0, |i| i + 1);
        return SourceEdit { range: close..close, text: format!("{key}: {value}{eol}") };
    }

    let start = slide.range.start;
    let bom = if start == 0 && source.starts_with('\u{feff}') { 3 } else { 0 };
    let first_end = source[start..].find('\n').map_or(source.len(), |i| start + i + 1);
    let first = source[start + bom..first_end].trim_end_matches(['\r', '\n']);
    if first.trim_end_matches(' ') == "---" {
        // A bare rule: the settings go straight after it.
        let had_eol = source[..first_end].ends_with('\n');
        let eol = if had_eol { eol_in(&source[start..first_end]) } else { eol_in(source) };
        let text = if had_eol {
            format!("{key}: {value}{eol}---{eol}")
        } else {
            format!("{eol}{key}: {value}{eol}---")
        };
        return SourceEdit { range: first_end..first_end, text };
    }
    let eol = eol_in(source);
    let at = start + bom;
    SourceEdit { range: at..at, text: format!("---{eol}{key}: {value}{eol}---{eol}") }
}
