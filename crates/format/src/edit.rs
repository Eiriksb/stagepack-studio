//! Source edits: each one replaces a byte range, so a visual change touches
//! only the bytes it means to.

use std::ops::Range;

use crate::model::{Attr, Slide, Tag};

#[derive(Clone, Debug, PartialEq, Eq)]
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

/// Several source edits made as one: what one visual change to a talk does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// Ranges in the text before any edit is applied, sorted, and not overlapping.
    pub edits: Vec<SourceEdit>,
}

impl Change {
    pub fn single(edit: SourceEdit) -> Change {
        Change { edits: vec![edit] }
    }

    pub fn is_empty(&self) -> bool {
        self.edits.is_empty()
    }

    pub fn apply(&self, source: &str) -> String {
        let mut out = String::with_capacity(source.len());
        let mut at = 0;
        for edit in &self.edits {
            out.push_str(&source[at..edit.range.start]);
            out.push_str(&edit.text);
            at = edit.range.end;
        }
        out.push_str(&source[at..]);
        out
    }

    /// The change that undoes this one. Its ranges are in the text after this change is
    /// applied.
    pub fn inverse(&self, source: &str) -> Change {
        let mut shift = 0isize;
        let edits = self
            .edits
            .iter()
            .map(|edit| {
                let start = edit.range.start.wrapping_add_signed(shift);
                shift += edit.text.len() as isize - edit.range.len() as isize;
                SourceEdit { range: start..start + edit.text.len(), text: source[edit.range.clone()].to_string() }
            })
            .collect();
        Change { edits }
    }
}

/// `value` as it's written between `quote` characters.
fn escape(value: &str, quote: char) -> String {
    let entity = if quote == '"' { "&quot;" } else { "&apos;" };
    value.replace('&', "&amp;").replace(quote, entity)
}

/// How an attribute's value is written in the source.
enum ValueForm {
    /// No value at all: `flag`.
    Bare,
    /// Inside these quotes; the value range excludes them.
    Quoted(char),
    /// Without quotes: `x=1`.
    Unquoted,
}

fn value_form(source: &str, attr: &Attr) -> ValueForm {
    let r = &attr.value_range;
    if attr.value == "true" && r.is_empty() && r.start == attr.name_range.end {
        return ValueForm::Bare;
    }
    let bytes = source.as_bytes();
    let quoted = r.start > 0
        && matches!(bytes.get(r.start - 1), Some(b'"' | b'\''))
        && bytes.get(r.end) == bytes.get(r.start - 1);
    if quoted { ValueForm::Quoted(char::from(bytes[r.start - 1])) } else { ValueForm::Unquoted }
}

/// Sets attribute `name` on `tag` to `value`.
pub fn set_attr(source: &str, tag: &Tag, name: &str, value: &str) -> SourceEdit {
    if let Some(attr) = tag.attrs.iter().find(|a| a.name == name) {
        let r = attr.value_range.clone();
        let text = match value_form(source, attr) {
            ValueForm::Bare => format!("=\"{}\"", escape(value, '"')),
            ValueForm::Quoted(quote) => escape(value, quote),
            ValueForm::Unquoted => format!("\"{}\"", escape(value, '"')),
        };
        return SourceEdit { range: r, text };
    }
    let at = new_attr_at(source, tag);
    SourceEdit { range: at..at, text: format!(" {name}=\"{}\"", escape(value, '"')) }
}

/// Where a new attribute goes in `tag`'s opening tag: after its last attribute, or its name.
fn new_attr_at(source: &str, tag: &Tag) -> usize {
    match tag.attrs.last() {
        Some(last) => match value_form(source, last) {
            ValueForm::Quoted(_) => last.value_range.end + 1,
            _ => last.value_range.end,
        },
        None => {
            let bytes = source.as_bytes();
            let mut i = tag.range.start + 1;
            while i < tag.open_end && !bytes[i].is_ascii_whitespace() && !matches!(bytes[i], b'>' | b'/') {
                i += 1;
            }
            i
        }
    }
}

/// `value` with each `&` that could be read as starting a character reference written as
/// `&amp;`, so that it reads back as typed: one before a name and `;`, and one before a name
/// that runs to the end, which the `markdown` crate can't read past. The rest are left as they
/// are, as `Q&A` is.
fn escape_references(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for (at, character) in value.char_indices() {
        out.push(character);
        if character == '&' {
            let name = value[at + 1..].bytes().take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'#').count();
            if name > 0 && matches!(value.as_bytes().get(at + 1 + name), None | Some(b';')) {
                out.push_str("amp;");
            }
        }
    }
    out
}

/// `value` written as a quoted attribute value that reads back as exactly `value`: between
/// `"`, or between `'` when it holds a `"`. `None` for a value holding both, or a line
/// ending or other control character, which can't be written so.
pub(crate) fn quote_value(value: &str) -> Option<String> {
    if value.chars().any(char::is_control) {
        return None;
    }
    let quote = match (value.contains('"'), value.contains('\'')) {
        (true, true) => return None,
        (true, false) => '\'',
        (false, _) => '"',
    };
    Some(format!("{quote}{}{quote}", escape_references(value)))
}

/// Sets attribute `name` on `tag` to `value`, written as [`quote_value`] has it. The first
/// attribute of that name, the one [`Tag::attr`] reads, is the one changed, and keeps its
/// quotes when `value` can go between them. `None` where `value` can't be written.
pub(crate) fn set_attr_as_typed(source: &str, tag: &Tag, name: &str, value: &str) -> Option<SourceEdit> {
    let quoted = quote_value(value)?;
    let Some(attr) = tag.attrs.iter().find(|a| a.name == name) else {
        let at = new_attr_at(source, tag);
        return Some(SourceEdit { range: at..at, text: format!(" {name}={quoted}") });
    };
    let r = attr.value_range.clone();
    Some(match value_form(source, attr) {
        ValueForm::Bare => SourceEdit { range: r, text: format!("={quoted}") },
        ValueForm::Quoted(quote) if !value.contains(quote) => SourceEdit { range: r, text: escape_references(value) },
        ValueForm::Quoted(_) => SourceEdit { range: r.start - 1..r.end + 1, text: quoted },
        ValueForm::Unquoted => SourceEdit { range: r, text: quoted },
    })
}

/// The edits that take every attribute named `name` out of `tag`, each with the space or tab
/// before it, or, for one on a line of its own, with that line's indentation and the line
/// ending before it.
pub(crate) fn remove_attr(source: &str, tag: &Tag, name: &str) -> Vec<SourceEdit> {
    tag.attrs
        .iter()
        .filter(|attr| attr.name == name)
        .map(|attr| {
            let start = attr.name_range.start;
            let before = &source[tag.range.start..start];
            let indent = before.len() - before.trim_end_matches([' ', '\t']).len();
            let line_ending = ["\r\n", "\n"].into_iter().find(|ending| before[..before.len() - indent].ends_with(ending));
            let start = match line_ending {
                Some(ending) => start - indent - ending.len(),
                None => start - indent.min(1),
            };
            let end = match value_form(source, attr) {
                ValueForm::Bare => attr.name_range.end,
                ValueForm::Quoted(_) => attr.value_range.end + 1,
                ValueForm::Unquoted => attr.value_range.end,
            };
            SourceEdit { range: start..end, text: String::new() }
        })
        .collect()
}

/// The first line ending in `text`, or `\n` when it has none.
pub(crate) fn eol_in(text: &str) -> &'static str {
    match text.find('\n') {
        Some(i) if text[..i].ends_with('\r') => "\r\n",
        _ => "\n",
    }
}

/// Sets `key` in the slide's settings block to `value`, adding the entry or
/// the whole block when it is missing.
pub fn set_setting(source: &str, slide: &Slide, key: &str, value: &str) -> SourceEdit {
    if let Some(entry) = slide.settings.as_ref().and_then(|settings| settings.entries.iter().find(|e| e.key == key)) {
        let r = entry.value_range.clone();
        let text = if r.is_empty() { format!(" {value}") } else { value.to_string() };
        return SourceEdit { range: r, text };
    }
    add_settings(source, slide, &[(key, value)])
}

/// The edit that adds a `key: value` line for each of `lines`, in order, at the end of the
/// slide's settings block, or adds the block holding them when it has none. The keys are
/// ones the block hasn't got.
pub(crate) fn add_settings(source: &str, slide: &Slide, lines: &[(&str, &str)]) -> SourceEdit {
    let written = |eol: &str| lines.iter().map(|(key, value)| format!("{key}: {value}{eol}")).collect::<String>();
    if let Some(settings) = &slide.settings {
        let block = &source[settings.range.clone()];
        let eol = eol_in(block);
        let body = block.trim_end_matches(['\r', '\n']);
        let close = settings.range.start + body.rfind('\n').map_or(0, |i| i + 1);
        return SourceEdit { range: close..close, text: written(eol) };
    }

    let start = slide.range.start;
    let bom = if start == 0 && source.starts_with('\u{feff}') { 3 } else { 0 };
    let first_end = source[start..].find('\n').map_or(source.len(), |i| start + i + 1);
    let first = source[start + bom..first_end].trim_end_matches(['\r', '\n']);
    if first.trim_end_matches(' ') == "---" {
        // A bare rule: the settings go straight after it.
        let had_eol = source[..first_end].ends_with('\n');
        let eol = if had_eol { eol_in(&source[start..first_end]) } else { eol_in(source) };
        let text = if had_eol { format!("{}---{eol}", written(eol)) } else { format!("{eol}{}---", written(eol)) };
        return SourceEdit { range: first_end..first_end, text };
    }
    let eol = eol_in(source);
    let at = start + bom;
    SourceEdit { range: at..at, text: format!("---{eol}{}---{eol}", written(eol)) }
}
