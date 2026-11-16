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

    /// Whether the attribute `name` of a `<Text>` tag is one [`Style::apply_text_attr`]
    /// knows, with a value it reads as written, so that the style holds all it says.
    pub fn knows_text_attr(name: &str, value: &str) -> bool {
        match name {
            "color" => !value.is_empty(),
            "size" => value.parse::<u32>().is_ok(),
            "bold" | "italic" | "underline" => matches!(value, "true" | "false"),
            _ => false,
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
    /// `inline` is the bytes of the heading's content, without its marker. `only_text` is
    /// whether that content is text alone, as [`Block::Paragraph`]'s is.
    Heading { level: u8, spans: Vec<Span>, range: Range<usize>, inline: Range<usize>, only_text: bool },
    /// `inline` is the bytes of the paragraph's content. `only_text` is whether it holds
    /// nothing but text, styled by Markdown or `<Text>` tags: no picture, no other tag, nothing
    /// whose look or meaning the spans leave out.
    Paragraph { spans: Vec<Span>, range: Range<usize>, inline: Range<usize>, only_text: bool },
    /// `item_ranges` holds each item's bytes, its marker included, beside `items`.
    List { ordered: bool, items: Vec<Vec<Block>>, range: Range<usize>, item_ranges: Vec<Range<usize>> },
    /// `rows[0]` is the header. `cells` holds where each cell of `rows` is, beside it, and
    /// `rule` is the alignment row under the header, from its first `|`, `-` or `:` to its last.
    /// A row has the cells written in it, more or fewer than the header has.
    Table { rows: Vec<Vec<Vec<Span>>>, cells: Vec<Vec<CellSource>>, rule: Range<usize>, range: Range<usize> },
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

/// Where one cell of a table is.
#[derive(Clone, Debug, PartialEq)]
pub struct CellSource {
    /// The cell's bytes: from the `|` before it, if it has one, to the next `|`, and, for the
    /// row's last cell, through the `|` after it.
    pub range: Range<usize>,
    /// The cell's content, without the spaces around it. An empty cell's is empty, after the
    /// `|` before it and one space, if there is one: where text typed in it goes.
    pub inline: Range<usize>,
    /// Whether the content is text alone, as [`Block::Paragraph`]'s `only_text` says.
    pub only_text: bool,
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
    /// Every block quote on the slide, at any depth. A quote has no block of its own: what's
    /// in it is among `blocks`, and these say which of it was quoted.
    pub quotes: Vec<Range<usize>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Talk {
    pub source: String,
    pub slides: Vec<Slide>,
}
