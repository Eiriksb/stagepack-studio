//! How the words of a slide's notes are kept inside `<Notes>`: escaped where they would
//! otherwise be read as something else, and read back unescaped, so that whatever was
//! typed comes back as it was typed.

/// The characters a backslash before them stands for. Before anything else a backslash
/// is kept as written, so hand-written notes like `C:\path` read as they always did.
const ESCAPED: [char; 6] = ['\\', '<', '`', '~', '-', '|'];

/// `notes` as written inside `<Notes>`:
/// - every `\` is written `\\`, and every `<` is written `\<`, so no tag starts or ends
///   in the notes, `</Notes` included;
/// - a run of three or more `` ` `` or `~` gets a `\` before it, so it never opens a code
///   fence: one would swallow the rest of the talk, and one after a list marker makes the
///   `markdown` crate give up on the slide;
/// - a line that is only `---` (and trailing spaces) is written `\---`, so it doesn't end
///   the slide;
/// - a `|` that ends a line, but for spaces after it, is written `\|`: a quote or a list item
///   ending in one, as the notes' last line, makes the `markdown` crate read a table after the
///   notes as a paragraph.
///
/// `{`, `}` and `>` are left as they are: inside `<Notes>` the parser reads them as text.
pub(crate) fn escape_notes(notes: &str) -> String {
    let mut out = String::with_capacity(notes.len() + notes.len() / 8);
    for (n, line) in notes.split('\n').enumerate() {
        if n > 0 {
            out.push('\n');
        }
        if line.trim_end_matches(['\r', ' ']) == "---" {
            out.push('\\');
        }
        let chars: Vec<char> = line.chars().collect();
        let last_bar = chars.iter().rposition(|&c| !matches!(c, ' ' | '\r')).filter(|&at| chars[at] == '|');
        for (i, &c) in chars.iter().enumerate() {
            let starts_a_fence = matches!(c, '`' | '~')
                && (i == 0 || chars[i - 1] != c)
                && chars.get(i + 1) == Some(&c)
                && chars.get(i + 2) == Some(&c);
            if matches!(c, '\\' | '<') || starts_a_fence || last_bar == Some(i) {
                out.push('\\');
            }
            out.push(c);
        }
    }
    out
}

/// The words of notes as written inside `<Notes>`: `escape_notes` undone, and line endings
/// made `\n`.
pub(crate) fn unescape_notes(written: &str) -> String {
    let written = written.replace("\r\n", "\n");
    let mut out = String::with_capacity(written.len());
    let mut chars = written.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\'
            && let Some(&next) = chars.peek()
            && ESCAPED.contains(&next)
        {
            out.push(next);
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}
