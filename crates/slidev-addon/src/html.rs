//! The HTML in a Slidev slide, as Markdown-it and Vue find it: tags, their attributes,
//! character references and inline CSS. Only ASCII is ever looked for, so every place text is
//! cut at is a character's boundary.

/// The longest an opening tag can be and still be taken for one. Without a limit, a slide of
/// `<a b="` over and over would take time with the square of its length.
const MAX_TAG: usize = 4096;

/// An opening tag: its name, its attributes as written (a bare one has no value), and where it
/// ends, just past its `>`.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenTag {
    pub name: String,
    pub attrs: Vec<(String, Option<String>)>,
    pub end: usize,
    pub self_closing: bool,
}

impl OpenTag {
    /// The attribute `name`'s value; a bare attribute's is empty.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(attr, _)| attr == name).map(|(_, value)| value.as_deref().unwrap_or(""))
    }

    pub fn has(&self, name: &str) -> bool {
        self.attrs.iter().any(|(attr, _)| attr == name)
    }
}

fn is_name_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic()
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':')
}

/// The name of a tag at `at` in `text`, and where it ends.
fn name_at(text: &str, at: usize) -> Option<(String, usize)> {
    let bytes = text.as_bytes();
    if !bytes.get(at).copied().is_some_and(is_name_start) {
        return None;
    }
    let mut end = at + 1;
    while bytes.get(end).copied().is_some_and(is_name_byte) {
        end += 1;
    }
    Some((text.get(at..end)?.to_string(), end))
}

/// The opening tag at `at`, where `text` has a `<`, if one starts there.
pub fn open_tag(text: &str, at: usize) -> Option<OpenTag> {
    let bytes = text.as_bytes();
    if bytes.get(at) != Some(&b'<') {
        return None;
    }
    let (name, mut i) = name_at(text, at + 1)?;
    let limit = (at + MAX_TAG).min(bytes.len());
    let mut attrs = Vec::new();
    loop {
        let before = i;
        while i < limit && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= limit {
            return None;
        }
        match bytes[i] {
            b'>' => return Some(OpenTag { name, attrs, end: i + 1, self_closing: false }),
            b'/' if bytes.get(i + 1) == Some(&b'>') => return Some(OpenTag { name, attrs, end: i + 2, self_closing: true }),
            _ => {}
        }
        // Attributes are set apart by white space.
        if i == before {
            return None;
        }
        let start = i;
        while i < limit && !bytes[i].is_ascii_whitespace() && !matches!(bytes[i], b'"' | b'\'' | b'>' | b'/' | b'=' | b'<' | b'`') {
            i += 1;
        }
        if i == start {
            return None;
        }
        let attr = text.get(start..i)?.to_string();
        let mut j = i;
        while j < limit && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j < limit && bytes[j] == b'=' {
            j += 1;
            while j < limit && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j >= limit {
                return None;
            }
            let value = match bytes[j] {
                quote @ (b'"' | b'\'') => {
                    let close = text.get(j + 1..limit)?.find(char::from(quote))? + j + 1;
                    let value = text.get(j + 1..close)?.to_string();
                    j = close + 1;
                    value
                }
                _ => {
                    let from = j;
                    while j < limit && !bytes[j].is_ascii_whitespace() && !matches!(bytes[j], b'"' | b'\'' | b'=' | b'<' | b'>' | b'`') {
                        j += 1;
                    }
                    if j == from {
                        return None;
                    }
                    text.get(from..j)?.to_string()
                }
            };
            attrs.push((attr, Some(value)));
            i = j;
        } else {
            attrs.push((attr, None));
        }
    }
}

/// The closing tag at `at`, where `text` has `</`: its name and where it ends.
pub fn close_tag(text: &str, at: usize) -> Option<(String, usize)> {
    let bytes = text.as_bytes();
    if bytes.get(at) != Some(&b'<') || bytes.get(at + 1) != Some(&b'/') {
        return None;
    }
    let (name, mut i) = name_at(text, at + 2)?;
    while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
        i += 1;
    }
    (bytes.get(i) == Some(&b'>')).then_some((name, i + 1))
}

/// Whether tag names `a` and `b` are the same: HTML's in any case, Vue's components exactly.
pub fn same_name(a: &str, b: &str) -> bool {
    if a.bytes().any(|byte| byte.is_ascii_uppercase()) || b.bytes().any(|byte| byte.is_ascii_uppercase()) {
        a == b || (is_html(a) && a.eq_ignore_ascii_case(b))
    } else {
        a == b
    }
}

/// HTML's own elements, which can't hold anything.
pub fn is_void(name: &str) -> bool {
    matches!(name.to_ascii_lowercase().as_str(), "area" | "base" | "br" | "col" | "embed" | "hr" | "img" | "input" | "link" | "meta" | "source" | "track" | "wbr")
}

/// Whether `name` is one of HTML's own elements, rather than a component.
pub fn is_html(name: &str) -> bool {
    const NAMES: [&str; 92] = [
        "a", "abbr", "address", "area", "article", "aside", "audio", "b", "bdi", "bdo", "blockquote", "br", "button", "canvas", "caption", "center", "cite", "code", "col",
        "colgroup", "data", "dd", "del", "details", "dfn", "dialog", "div", "dl", "dt", "em", "embed", "fieldset", "figcaption", "figure", "font", "footer", "form", "h1",
        "h2", "h3", "h4", "h5", "h6", "header", "hr", "i", "iframe", "img", "input", "ins", "kbd", "label", "legend", "li", "main", "mark", "menu", "meter", "nav", "object",
        "ol", "optgroup", "option", "output", "p", "picture", "pre", "progress", "q", "s", "samp", "script", "section", "select", "small", "source", "span", "strong", "style",
        "sub", "summary", "sup", "svg", "table", "tbody", "td", "template", "textarea", "tfoot", "th", "thead", "tr",
    ];
    let lower = name.to_ascii_lowercase();
    NAMES.contains(&lower.as_str()) || matches!(lower.as_str(), "u" | "ul" | "var" | "video" | "time" | "track" | "wbr")
}

/// Elements whose start Markdown-it reads as the start of an HTML block, even in the middle of
/// a paragraph.
pub fn starts_a_block(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "address" | "article" | "aside" | "blockquote" | "center" | "details" | "dialog" | "dd" | "div" | "dl" | "dt" | "fieldset" | "figcaption" | "figure" | "footer"
            | "form" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "header" | "hr" | "li" | "main" | "menu" | "nav" | "ol" | "p" | "pre" | "script" | "section" | "style"
            | "summary" | "table" | "tbody" | "td" | "tfoot" | "th" | "thead" | "tr" | "ul" | "template" | "textarea"
    )
}

/// `text` with its character references read: the few named ones HTML text is written with,
/// and every numbered one. Others are kept as they are.
pub fn decode(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let end = rest.bytes().take(40).position(|byte| byte == b';');
        let read = end.and_then(|end| {
            let name = &rest[1..end];
            let c = match name {
                "lt" => Some('<'),
                "gt" => Some('>'),
                "amp" => Some('&'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                _ => {
                    let number = name.strip_prefix('#')?;
                    let code = match number.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                        None => number.parse::<u32>().ok()?,
                    };
                    Some(char::from_u32(code).filter(|c| *c != '\0').unwrap_or('\u{fffd}'))
                }
            }?;
            Some((c, end + 1))
        });
        match read {
            Some((c, length)) => {
                out.push(c);
                rest = &rest[length..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The declarations of inline CSS, each property in lower case with its value, in order.
pub fn style(css: &str) -> Vec<(String, String)> {
    css.split(';')
        .filter_map(|declaration| {
            let (property, value) = declaration.split_once(':')?;
            let property = property.trim().to_ascii_lowercase();
            let value = value.trim().trim_end_matches("!important").trim().to_string();
            (!property.is_empty() && !value.is_empty()).then_some((property, value))
        })
        .collect()
}

/// A CSS length in pixels, as written in `px` or with no unit.
pub fn pixels(value: &str) -> Option<f32> {
    let value = value.trim();
    let number = value.strip_suffix("px").unwrap_or(value).trim();
    number.parse::<f32>().ok().filter(|number| number.is_finite())
}

/// The turn in degrees of a CSS `transform` that only turns.
pub fn rotation(transform: &str) -> Option<f32> {
    let inner = transform.trim().strip_prefix("rotate(")?.strip_suffix(')')?.trim();
    let degrees = inner.strip_suffix("deg").unwrap_or(inner).trim();
    degrees.parse::<f32>().ok().filter(|degrees| degrees.is_finite())
}

/// `%`-escapes in a URL's path read as the bytes they stand for, if they make UTF-8.
pub fn percent_decode(url: &str) -> String {
    let bytes = url.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |at: usize| bytes.get(at).and_then(|byte| char::from(*byte).to_digit(16));
        match (bytes[i], hex(i + 1), hex(i + 2)) {
            (b'%', Some(high), Some(low)) => {
                out.push((high * 16 + low) as u8);
                i += 3;
            }
            (byte, _, _) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_and_their_attributes() {
        let tag = open_tag("<div v-click class=\"a b\" at='3' x=1 :click-2=\"{y:0}\">rest", 0).unwrap();
        assert_eq!(tag.name, "div");
        assert_eq!(tag.attr("v-click"), Some(""));
        assert_eq!(tag.attr("class"), Some("a b"));
        assert_eq!(tag.attr("at"), Some("3"));
        assert_eq!(tag.attr("x"), Some("1"));
        assert_eq!(tag.attr(":click-2"), Some("{y:0}"));
        assert_eq!(&"<div v-click class=\"a b\" at='3' x=1 :click-2=\"{y:0}\">rest"[tag.end..], "rest");
        assert!(open_tag("<Counter />", 0).unwrap().self_closing);
        assert!(open_tag("<carbon:moon/>", 0).unwrap().self_closing);
        assert_eq!(open_tag("< div>", 0), None);
        assert_eq!(open_tag("<3", 0), None);
        assert_eq!(open_tag("<a b=\"open", 0), None);
        assert_eq!(open_tag("<a b", 0), None);
        assert_eq!(open_tag("<a\nb=\"x\"\n>", 0).map(|tag| tag.attrs.len()), Some(1));
        assert_eq!(close_tag("</div >x", 0), Some(("div".to_string(), 7)));
        assert_eq!(close_tag("</ div>", 0), None);
    }

    #[test]
    fn references_are_read() {
        assert_eq!(decode("a &lt;b&gt; &amp;amp; &#91;x&#x5d; &nbsp; &copy; &#0; &"), "a <b> &amp; [x] \u{a0} &copy; \u{fffd} &");
    }

    #[test]
    fn css_is_read() {
        assert_eq!(style("position:absolute; LEFT: 10px;;bad;color:red !important"), vec![("position".into(), "absolute".into()), ("left".into(), "10px".into()), ("color".into(), "red".into())]);
        assert_eq!(pixels("12.5px"), Some(12.5));
        assert_eq!(pixels("3em"), None);
        assert_eq!(rotation("rotate(3deg)"), Some(3.0));
        assert_eq!(percent_decode("/a%20b%E2%82%AC.png"), "/a b€.png");
        assert_eq!(percent_decode("%ff%"), "%ff%");
    }
}
