use std::ops::Range;

/// One `key: value` line of a settings block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingEntry {
    pub key: String,
    pub value: String,
    /// The whole line, without its line ending.
    pub line: Range<usize>,
    /// The value with surrounding whitespace trimmed away.
    pub value_range: Range<usize>,
}

/// A settings block, from the opening `---` through the closing `---` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub range: Range<usize>,
    pub entries: Vec<SettingEntry>,
}

impl Settings {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|e| e.key == key)
            .map(|e| e.value.as_str())
    }
}

/// Reads `key: value` from a line (no line ending). `offset` is the line's
/// position in the source.
pub(crate) fn parse_entry(line: &str, offset: usize) -> Option<SettingEntry> {
    let mut chars = line.char_indices();
    let (_, first) = chars.next()?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return None;
    }
    let colon = loop {
        let (i, c) = chars.next()?;
        if c == ':' {
            break i;
        }
        if !(c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')) {
            return None;
        }
    };
    let after = &line[colon + 1..];
    let value = after.trim();
    let lead = after.len() - after.trim_start().len();
    let start = if value.is_empty() {
        colon + 1
    } else {
        colon + 1 + lead
    };
    Some(SettingEntry {
        key: line[..colon].to_string(),
        value: value.to_string(),
        line: offset..offset + line.len(),
        value_range: offset + start..offset + start + value.len(),
    })
}
