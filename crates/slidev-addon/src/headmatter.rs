//! A Slidev slide's front matter, read as YAML. Each block is read on its own, as Slidev reads
//! it, and nothing in it is ever evaluated: an alias is not followed, and a tag says nothing.

use yaml_rust2::parser::{Event, Parser};
use yaml_rust2::scanner::TScalarStyle;

/// The most a block can hold and still be read. Front matter is a few lines.
const MAX_LEN: usize = 64 * 1024;
/// How many lists and maps deep a block can go and still be read.
const MAX_DEPTH: usize = 32;

/// A value as the YAML has it.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    /// A scalar as written, and whether it was plain, with no quotes.
    Scalar { text: String, plain: bool },
    List(Vec<Value>),
    Map(Vec<(String, Value)>),
}

/// A list or map being read, and the key a map's next value goes with.
enum Open {
    List(Vec<Value>),
    Map(Vec<(String, Value)>, Option<String>),
}

impl Value {
    /// The value at `key`, if this is a map that has one.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(entries) => entries.iter().rev().find(|(name, _)| name == key).map(|(_, value)| value),
            _ => None,
        }
    }

    /// The text of a scalar.
    pub fn text(&self) -> Option<&str> {
        match self {
            Value::Scalar { text, .. } => Some(text),
            _ => None,
        }
    }

    /// The text of a scalar, or of each scalar of a list, joined by commas, as Slidev reads
    /// a list of names.
    pub fn texts(&self) -> Option<String> {
        match self {
            Value::Scalar { text, .. } => Some(text.clone()),
            Value::List(items) => Some(items.iter().filter_map(Value::text).collect::<Vec<_>>().join(",")),
            _ => None,
        }
    }

    /// Whether JavaScript would take the value as true, as Slidev tests `hide` and `disabled`.
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Scalar { text, plain: false } => !text.is_empty(),
            Value::Scalar { text, plain: true } => {
                let text = text.trim();
                !(text.is_empty() || matches!(text, "false" | "False" | "FALSE" | "null" | "Null" | "NULL" | "~") || text.parse::<f64>().is_ok_and(|number| number == 0.0 || number.is_nan()))
            }
            Value::List(_) | Value::Map(_) => true,
        }
    }

    /// The entries of a map, or none.
    pub fn entries(&self) -> &[(String, Value)] {
        match self {
            Value::Map(entries) => entries,
            _ => &[],
        }
    }
}

/// The block `text` as YAML, or a sentence that says why it can't be read. A block with
/// nothing in it is an empty map.
pub fn read(text: &str) -> Result<Value, String> {
    if text.len() > MAX_LEN {
        return Err(format!("it holds more than {} KiB", MAX_LEN / 1024));
    }
    let mut parser = Parser::new_from_str(text);
    let mut open: Vec<Open> = Vec::new();
    let mut documents = 0;
    let mut root = None;
    loop {
        let (event, _) = parser.next_token().map_err(|error| error.info().to_string())?;
        let value = match event {
            Event::StreamEnd => break,
            Event::DocumentStart => {
                documents += 1;
                if documents > 1 {
                    return Err("it holds more than one document".to_string());
                }
                continue;
            }
            Event::Nothing | Event::StreamStart | Event::DocumentEnd => continue,
            Event::SequenceStart(..) | Event::MappingStart(..) => {
                if open.len() >= MAX_DEPTH {
                    return Err(format!("it goes more than {MAX_DEPTH} levels deep"));
                }
                open.push(if matches!(event, Event::SequenceStart(..)) { Open::List(Vec::new()) } else { Open::Map(Vec::new(), None) });
                continue;
            }
            Event::SequenceEnd | Event::MappingEnd => match open.pop() {
                Some(Open::List(items)) => Value::List(items),
                Some(Open::Map(entries, _)) => Value::Map(entries),
                None => return Err("it ends a list or map it never started".to_string()),
            },
            // An alias is never followed: one that names itself, or a chain of them, could
            // otherwise grow without end.
            Event::Alias(_) => Value::Null,
            Event::Scalar(text, style, _, _) => {
                let plain = style == TScalarStyle::Plain;
                if plain && matches!(text.as_str(), "" | "~" | "null" | "Null" | "NULL") { Value::Null } else { Value::Scalar { text, plain } }
            }
        };
        match open.last_mut() {
            None => root = Some(value),
            Some(Open::List(items)) => items.push(value),
            Some(Open::Map(entries, key)) => match key.take() {
                Some(key) => entries.push((key, value)),
                // A key that isn't a scalar names nothing Slidev reads.
                None => *key = Some(match value {
                    Value::Scalar { text, .. } => text,
                    Value::Null => String::new(),
                    _ => "\u{0}".to_string(),
                }),
            },
        }
    }
    Ok(match root {
        None | Some(Value::Null) => Value::Map(Vec::new()),
        Some(value) => value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scalar(text: &str) -> Value {
        Value::Scalar { text: text.to_string(), plain: true }
    }

    #[test]
    fn a_block_is_a_map_of_scalars_lists_and_maps() {
        let value = read("layout: cover\nhide: true\nfonts:\n  sans: Inter\nlist: [a, b]\nquoted: 'x'\n").unwrap();
        assert_eq!(value.get("layout"), Some(&scalar("cover")));
        assert!(value.get("hide").unwrap().is_truthy());
        assert_eq!(value.get("fonts").and_then(|fonts| fonts.get("sans")), Some(&scalar("Inter")));
        assert_eq!(value.get("list").and_then(Value::texts).as_deref(), Some("a,b"));
        assert_eq!(value.get("quoted"), Some(&Value::Scalar { text: "x".into(), plain: false }));
        assert_eq!(read("").unwrap(), Value::Map(Vec::new()));
    }

    #[test]
    fn what_cant_be_read_says_why() {
        assert!(read("layout: [section").is_err());
        assert!(read(&format!("a: {}", "[".repeat(100))).is_err());
        assert!(read("a: 1\n...\n---\nb: 2\n").is_err());
        assert!(read(&"x".repeat(MAX_LEN + 1)).is_err());
    }

    #[test]
    fn aliases_are_never_followed() {
        let value = read("a: &a [1, 2]\nb: *a\nc: &c [*c]\n").unwrap();
        assert_eq!(value.get("b"), Some(&Value::Null));
    }

    #[test]
    fn truth_is_javascripts() {
        for falsy in ["false", "0", "null", "''", "~", "0.0"] {
            assert!(!read(&format!("a: {falsy}")).unwrap().get("a").is_some_and(Value::is_truthy), "{falsy}");
        }
        for truthy in ["true", "1", "yes", "'false'", "[]"] {
            assert!(read(&format!("a: {truthy}")).unwrap().get("a").is_some_and(Value::is_truthy), "{truthy}");
        }
    }
}
