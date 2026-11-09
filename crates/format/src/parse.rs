use std::ops::Range;

use markdown::mdast::{AttributeContent, AttributeValue, Node};
use markdown::{Constructs, ParseOptions};

use crate::attrs::scan_open_tag;
use crate::model::*;
use crate::split::split_slides;

fn options() -> ParseOptions {
    ParseOptions {
        constructs: Constructs {
            gfm_table: true,
            gfm_strikethrough: false,
            mdx_jsx_flow: true,
            mdx_jsx_text: true,
            html_flow: false,
            html_text: false,
            autolink: false,
            mdx_esm: false,
            mdx_expression_flow: false,
            mdx_expression_text: false,
            ..Constructs::default()
        },
        ..ParseOptions::default()
    }
}

/// Reads a whole file into slides. A slide that fails to parse carries an
/// error and no blocks; the others are unaffected.
pub fn parse_talk(source: &str) -> Talk {
    let slides = split_slides(source)
        .into_iter()
        .map(|raw| {
            let mut slide = Slide {
                range: raw.range,
                settings: raw.settings,
                blocks: Vec::new(),
                notes: None,
                error: None,
            };
            let body = &source[raw.body.clone()];
            match markdown::to_mdast(body, &options()) {
                Ok(root) => {
                    let ctx = Ctx { source, base: raw.body.start };
                    let mut notes = None;
                    for node in root.children().into_iter().flatten() {
                        ctx.block(node, &mut slide.blocks, &mut notes);
                    }
                    slide.notes = notes;
                }
                Err(message) => {
                    slide.error = Some(SlideError { message: message.to_string() });
                }
            }
            slide
        })
        .collect();
    Talk { source: source.to_string(), slides }
}

struct Ctx<'a> {
    source: &'a str,
    /// Where the parsed text starts in the file.
    base: usize,
}

impl Ctx<'_> {
    fn range(&self, node: &Node) -> Range<usize> {
        node.position()
            .map(|p| self.base + p.start.offset..self.base + p.end.offset)
            .unwrap_or(self.base..self.base)
    }

    fn block(&self, node: &Node, out: &mut Vec<Block>, notes: &mut Option<Notes>) {
        let range = self.range(node);
        match node {
            Node::Heading(h) => out.push(Block::Heading {
                level: h.depth,
                spans: self.spans(&h.children),
                range,
            }),
            Node::Paragraph(p) => match p.children.as_slice() {
                [Node::Image(image)] => out.push(Block::Picture {
                    src: image.url.clone(),
                    alt: image.alt.clone(),
                    range,
                }),
                children => out.push(Block::Paragraph { spans: self.spans(children), range }),
            },
            Node::List(list) => out.push(Block::List {
                ordered: list.ordered,
                items: list
                    .children
                    .iter()
                    .map(|item| {
                        let mut blocks = Vec::new();
                        for child in item.children().into_iter().flatten() {
                            self.block(child, &mut blocks, &mut None);
                        }
                        blocks
                    })
                    .collect(),
                range,
            }),
            Node::Table(table) => out.push(Block::Table {
                rows: table
                    .children
                    .iter()
                    .map(|row| {
                        row.children()
                            .into_iter()
                            .flatten()
                            .map(|cell| self.spans(cell.children().map_or(&[][..], |c| c)))
                            .collect()
                    })
                    .collect(),
                range,
            }),
            Node::Code(code) => out.push(Block::Code {
                language: code.lang.clone(),
                code: code.value.clone(),
                range,
            }),
            // Quotes have no block of their own; their content is kept.
            Node::Blockquote(quote) => {
                for child in &quote.children {
                    self.block(child, out, notes);
                }
            }
            Node::MdxJsxFlowElement(element) => {
                let Some(name) = element.name.clone() else { return };
                let (attrs, open_end) = scan_open_tag(self.source, range.start);
                let attr = |n: &str| attrs.iter().find(|a| a.name == n).map(|a| a.value.clone());
                match name.as_str() {
                    "Region" => out.push(Block::Region {
                        name: attr("name").unwrap_or_default(),
                        range,
                    }),
                    "Notes" => {
                        let end = self.source[open_end..range.end]
                            .rfind("</Notes")
                            .map_or(range.end, |i| open_end + i);
                        *notes = Some(Notes {
                            text: self.source[open_end.min(end)..end].trim().to_string(),
                            range,
                        });
                    }
                    _ => {
                        let mut children = Vec::new();
                        for child in &element.children {
                            self.block(child, &mut children, &mut None);
                        }
                        out.push(Block::Tag(Tag { name, attrs, children, range, open_end }));
                    }
                }
            }
            _ => {}
        }
    }

    fn spans(&self, nodes: &[Node]) -> Vec<Span> {
        let mut out = Vec::new();
        for node in nodes {
            self.inline(node, &Style::default(), &mut out);
        }
        out
    }

    fn inline(&self, node: &Node, style: &Style, out: &mut Vec<Span>) {
        let nested = |children: &[Node], inner: Style, out: &mut Vec<Span>| {
            for child in children {
                self.inline(child, &inner, out);
            }
        };
        match node {
            Node::Text(t) => push_span(out, &t.value, style),
            Node::Break(_) => push_span(out, "\n", style),
            Node::InlineCode(c) => push_span(out, &c.value, &Style { code: true, ..style.clone() }),
            Node::Strong(s) => nested(&s.children, Style { bold: true, ..style.clone() }, out),
            Node::Emphasis(e) => nested(&e.children, Style { italic: true, ..style.clone() }, out),
            Node::Link(l) => {
                nested(&l.children, Style { link: Some(l.url.clone()), ..style.clone() }, out)
            }
            Node::Image(i) => push_span(out, &i.alt, style),
            Node::MdxJsxTextElement(element) => {
                let mut inner = style.clone();
                if element.name.as_deref() == Some("Text") {
                    for attribute in &element.attributes {
                        let AttributeContent::Property(p) = attribute else { continue };
                        let value = match &p.value {
                            Some(AttributeValue::Literal(v)) => v.as_str(),
                            None => "true",
                            _ => continue,
                        };
                        match p.name.as_str() {
                            "color" => inner.color = Some(value.to_string()),
                            "size" => inner.size = value.parse().ok(),
                            "bold" => inner.bold = value == "true",
                            "italic" => inner.italic = value == "true",
                            "underline" => inner.underline = value == "true",
                            _ => {}
                        }
                    }
                }
                nested(&element.children, inner, out);
            }
            other => {
                if let Some(children) = other.children() {
                    nested(children, style.clone(), out);
                }
            }
        }
    }
}

fn push_span(out: &mut Vec<Span>, text: &str, style: &Style) {
    match out.last_mut() {
        Some(last) if last.style == *style => last.text.push_str(text),
        _ => out.push(Span { text: text.to_string(), style: style.clone() }),
    }
}
