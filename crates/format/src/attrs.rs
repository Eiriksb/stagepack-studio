use crate::model::Attr;

const ENTITIES: [(&str, char); 3] = [("&quot;", '"'), ("&apos;", '\''), ("&amp;", '&')];

/// The value an attribute's raw text stands for. It goes through once from the start, so
/// that `&amp;quot;` is the text `&quot;` and not a quote.
fn decode_entities(raw: &str) -> String {
    let mut value = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(at) = rest.find('&') {
        value.push_str(&rest[..at]);
        rest = &rest[at..];
        let entity = ENTITIES.iter().find(|(entity, _)| rest.starts_with(entity));
        let (character, length) = entity.map_or(('&', 1), |(entity, character)| (*character, entity.len()));
        value.push(character);
        rest = &rest[length..];
    }
    value.push_str(rest);
    value
}

/// Reads the opening tag that starts at `start` (`source[start] == '<'`).
/// Returns its attributes and the offset just past `>` or `/>`.
/// The markdown crate gives attributes no positions, so they are found here.
pub(crate) fn scan_open_tag(source: &str, start: usize) -> (Vec<Attr>, usize) {
    let bytes = source.as_bytes();
    let mut i = start + 1;
    while i < bytes.len() && !bytes[i].is_ascii_whitespace() && !matches!(bytes[i], b'>' | b'/') {
        i += 1;
    }
    let mut attrs = Vec::new();
    loop {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        match bytes.get(i) {
            None => return (attrs, i),
            Some(b'>') => return (attrs, i + 1),
            Some(b'/') => {
                i += 1;
                continue;
            }
            _ => {}
        }
        let name_start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && !matches!(bytes[i], b'=' | b'>' | b'/')
        {
            i += 1;
        }
        let name_range = name_start..i;
        let mut j = i;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if bytes.get(j) == Some(&b'=') {
            j += 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            let value_range = match bytes.get(j) {
                Some(&q) if q == b'"' || q == b'\'' => {
                    let from = j + 1;
                    let to = source[from..].find(q as char).map_or(source.len(), |k| from + k);
                    i = (to + 1).min(source.len());
                    from..to
                }
                _ => {
                    let from = j;
                    let mut to = from;
                    while to < bytes.len() && !bytes[to].is_ascii_whitespace() && bytes[to] != b'>' {
                        to += 1;
                    }
                    i = to;
                    from..to
                }
            };
            attrs.push(Attr {
                name: source[name_range.clone()].to_string(),
                value: decode_entities(&source[value_range.clone()]),
                name_range,
                value_range,
            });
        } else {
            attrs.push(Attr {
                name: source[name_range.clone()].to_string(),
                value: "true".to_string(),
                name_range: name_range.clone(),
                value_range: name_range.end..name_range.end,
            });
        }
    }
}
