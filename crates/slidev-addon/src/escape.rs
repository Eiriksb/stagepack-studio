//! Text as Slidev reads it back as the same text. Slidev's Markdown goes through more readers
//! than Markdown's own: its slides are split on `---` lines and their `::name::` lines, its
//! notes are the comment at a slide's end, `$` starts maths, and what comes out is a Vue
//! template, where `{{` starts code. Each of these is written so that it reads as text.

/// `text`, a run of a paragraph's text, written so that Markdown, Slidev and Vue all read it
/// as that text. `line_start` is whether it starts the line, where more starts a block.
///
/// `{{` isn't escaped here, as Vue reads it after the Markdown is gone: a block that holds it
/// is written inside `v-pre`, which [`needs_v_pre`] says.
pub fn text(text: &str, line_start: bool) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0;
    if line_start {
        // White space at the start of a line can make code of it, and is never seen.
        while chars.get(at).is_some_and(|c| *c == ' ' || *c == '\t') {
            at += 1;
        }
        // A list marker, a heading's mark or a rule of `-`, `+` or `=`.
        match chars.get(at) {
            Some('-' | '+' | '=') => {
                out.push('\\');
                out.push(chars[at]);
                at += 1;
            }
            Some(c) if c.is_ascii_digit() => {
                let digits = chars[at..].iter().take_while(|c| c.is_ascii_digit()).count();
                if matches!(chars.get(at + digits), Some('.' | ')')) {
                    out.extend(&chars[at..at + digits]);
                    out.push('\\');
                    out.push(chars[at + digits]);
                    at += digits + 1;
                }
            }
            _ => {}
        }
    }
    for i in at..chars.len() {
        let c = chars[i];
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            // `::name::` on a line of its own starts one of a layout's slots.
            ':' if chars.get(i + 1) == Some(&':') || (i > 0 && chars[i - 1] == ':') => out.push_str("\\:"),
            '\\' | '`' | '*' | '_' | '[' | ']' | '|' | '~' | '$' | '#' | '!' => {
                out.push('\\');
                out.push(c);
            }
            // A no-break space reads as one.
            '\u{a0}' => out.push_str("&nbsp;"),
            _ => out.push(c),
        }
    }
    no_drag(&out)
}

/// `text` with Slidev's `v-drag`, which it rewrites wherever it finds it, made so it doesn't
/// read as that. The `-` is written as its entity, which shows as `-`.
fn no_drag(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    if !lower.contains("v-drag") {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len() + 8);
    let mut at = 0;
    while let Some(found) = lower[at..].find("v-drag") {
        let start = at + found;
        out.push_str(&text[at..=start]);
        out.push_str("&#45;");
        at = start + 2;
    }
    out.push_str(&text[at..]);
    out
}

/// `text` as HTML text: `&`, `<` and `>` as entities, and nothing else changed.
pub fn html(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Whether `text`, code, holds what Slidev's raw passes read before Markdown does: `<!--`,
/// which starts a comment for its slide splitter and its notes.
pub fn opens_comment(text: &str) -> bool {
    text.contains("<!--")
}

/// `code` as the lines of a `<pre>`, which Markdown leaves alone: as HTML text, with what
/// starts one of Slidev's own lines (`---`, `::name::`) written as entities, so its slide
/// splitter and slots don't read it. `{` is an entity too, as Vue reads `{{` as code.
pub fn pre_lines(code: &str) -> String {
    code.split('\n')
        .map(|line| {
            let line = html(line).replace('{', "&#123;").replace('}', "&#125;");
            match line.chars().next() {
                Some('-') => format!("&#45;{}", &line[1..]),
                Some(':') => format!("&#58;{}", &line[1..]),
                _ => line,
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether `written`, a block as written for Slidev, has to be inside `v-pre` for Vue to show
/// it as written: whether it holds `{{`, which Vue reads as code.
pub fn needs_v_pre(written: &str) -> bool {
    written.contains("{{")
}

/// `text` as the value of an HTML attribute in double quotes.
pub fn attribute(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// A speaker's notes as the text of the HTML comment Slidev reads them from. Slidev shows
/// them as Markdown turned to HTML, so `&`, `<` and `>` are written as entities: no HTML in
/// them reaches the presenter, and neither `-->` nor `--!>` can end the comment early. `[` is
/// one too, as `[click]` is a mark Slidev reads in notes. `{{` stays as it is: the notes are
/// shown as HTML, never as a Vue template.
pub fn notes(text: &str) -> String {
    html(text).replace('[', "&#91;")
}

/// `url` as a link's target, or `None` for one that could run code when it is followed:
/// only web, mail and telephone links, and links within the talk, are kept. Characters that
/// would end the target early are written as `%` escapes.
pub fn link(url: &str) -> Option<String> {
    let url = url.trim();
    let scheme = url.split_once(':').map(|(scheme, _)| scheme).filter(|scheme| !scheme.contains(['/', '?', '#']));
    if let Some(scheme) = scheme
        && !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https" | "mailto" | "tel")
    {
        return None;
    }
    let mut out = String::with_capacity(url.len());
    for c in url.chars() {
        match c {
            ' ' | '(' | ')' | '<' | '>' | '"' | '\'' | '\\' | '`' | '{' | '}' | '|' => out.push_str(&format!("%{:02X}", c as u32)),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    Some(out)
}

/// The CSS name of the font `family`, in double quotes, with what could end the quote or
/// the rule left out.
pub fn font_family(family: &str) -> String {
    let kept: String = family.chars().filter(|c| !c.is_control() && !matches!(c, '"' | '\\' | '<' | '>' | ';' | '{' | '}')).collect();
    format!("\"{}\"", kept.trim())
}

/// A code block's language, kept to the characters a language's name has. Slidev reads more
/// than a language after the fence, such as line ranges in `{}` and a code runner, which
/// this leaves out; and it draws some languages as diagrams rather than code, which become
/// plain text.
pub fn language(language: &str) -> String {
    let name: String = language.chars().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '#' | '.' | '-')).collect();
    match name.to_ascii_lowercase().as_str() {
        "mermaid" | "plantuml" => "text".to_string(),
        _ => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_starts_a_block_is_escaped_only_at_the_start_of_a_line() {
        assert_eq!(text("---", true), "\\---");
        assert_eq!(text("a --- b", false), "a --- b");
        assert_eq!(text("1. one", true), "1\\. one");
        assert_eq!(text("1. one", false), "1. one");
        assert_eq!(text("+ x", true), "\\+ x");
    }

    #[test]
    fn markup_is_escaped_everywhere() {
        assert_eq!(text("<!-- x -->", false), "&lt;\\!-- x --&gt;");
        assert_eq!(text("::right::", true), "\\:\\:right\\:\\:");
        assert_eq!(text("a: b", false), "a: b");
        assert_eq!(text("*a* $1 `c` #d", false), "\\*a\\* \\$1 \\`c\\` \\#d");
    }

    #[test]
    fn links_that_run_code_are_refused() {
        assert_eq!(link("javascript:alert(1)"), None);
        assert_eq!(link(" JavaScript:x"), None);
        assert_eq!(link("https://example.com/a b(c)"), Some("https://example.com/a%20b%28c%29".into()));
        assert_eq!(link("#top"), Some("#top".into()));
        assert_eq!(link("page?x=a:b"), Some("page?x=a:b".into()));
    }

    #[test]
    fn notes_cannot_end_their_comment_or_hold_html() {
        assert_eq!(notes("a --> b"), "a --&gt; b");
        assert_eq!(notes("a --!> b"), "a --!&gt; b");
        assert_eq!(notes("<script>alert(1)</script> & [click]"), "&lt;script&gt;alert(1)&lt;/script&gt; &amp; &#91;click]");
        assert_eq!(notes("{{ x }}"), "{{ x }}");
    }

    #[test]
    fn v_drag_in_text_is_not_slidevs() {
        assert_eq!(text("use v-drag here, V-Drag too", false), "use v&#45;drag here, V&#45;Drag too");
    }

    #[test]
    fn code_in_a_pre_reads_as_itself() {
        assert_eq!(pre_lines("---\n::right::\n<!-- x -->\n{{ y }}"), "&#45;--\n&#58;:right::\n&lt;!-- x --&gt;\n&#123;&#123; y &#125;&#125;");
    }

    #[test]
    fn languages_keep_their_name_only() {
        assert_eq!(language("ts {monaco-run}"), "ts");
        assert_eq!(language("rust"), "rust");
        assert_eq!(language("Mermaid"), "text");
    }
}
