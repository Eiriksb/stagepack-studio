use std::ops::Range;

use crate::settings::Settings;

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub code: bool,
    pub color: Option<String>,
    pub size: Option<u32>,
    pub link: Option<String>,
}

impl Style {
    /// Changes this style as the attribute `name` of a `<Text>` tag says to. Any
    /// attribute that isn't about text changes nothing.
    pub fn apply_text_attr(&mut self, name: &str, value: &str) {
        match name {
            "color" => self.color = Some(value.to_string()),
            "size" => self.size = value.parse().ok(),
            "bold" => self.bold = value == "true",
            "italic" => self.italic = value == "true",
            "underline" => self.underline = value == "true",
            _ => {}
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub text: String,
    pub style: Style,
}

/// One attribute of an opening tag, with the exact bytes of its name and value.
/// For a bare attribute both ranges are empty or cover the name, and the value is `true`.
#[derive(Clone, Debug, PartialEq)]
pub struct Attr {
    pub name: String,
    pub value: String,
    pub name_range: Range<usize>,
    pub value_range: Range<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tag {
    pub name: String,
    pub attrs: Vec<Attr>,
    pub children: Vec<Block>,
    /// The whole element, opening tag through closing tag.
    pub range: Range<usize>,
    /// The offset just past the opening tag's `>` or `/>`.
    pub open_end: usize,
}

impl Tag {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.value.as_str())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Heading { level: u8, spans: Vec<Span>, range: Range<usize> },
    Paragraph { spans: Vec<Span>, range: Range<usize> },
    List { ordered: bool, items: Vec<Vec<Block>>, range: Range<usize> },
    Table { rows: Vec<Vec<Vec<Span>>>, range: Range<usize> },
    Code { language: Option<String>, code: String, range: Range<usize> },
    Picture { src: String, alt: String, range: Range<usize> },
    Tag(Tag),
    Region { name: String, range: Range<usize> },
}

impl Block {
    pub fn range(&self) -> &Range<usize> {
        match self {
            Block::Heading { range, .. }
            | Block::Paragraph { range, .. }
            | Block::List { range, .. }
            | Block::Table { range, .. }
            | Block::Code { range, .. }
            | Block::Picture { range, .. }
            | Block::Region { range, .. } => range,
            Block::Tag(tag) => &tag.range,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Notes {
    /// The words as they were typed: unescaped, with `\n` line endings.
    pub text: String,
    /// The whole `<Notes>` element.
    pub range: Range<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SlideError {
    pub message: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Slide {
    pub range: Range<usize>,
    pub settings: Option<Settings>,
    pub blocks: Vec<Block>,
    pub notes: Option<Notes>,
    pub error: Option<SlideError>,
    /// The tags, other than `<Text>`, that sit in the middle of a line, where nothing is
    /// drawn for them. Each is named once.
    pub inline_tags: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Talk {
    pub source: String,
    pub slides: Vec<Slide>,
}
