use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::thread;

use markdown::mdast::{AttributeContent, AttributeValue, Node};
use markdown::{Constructs, ParseOptions};

use crate::attrs::scan_open_tag;
use crate::model::*;
use crate::split::{RawSlide, split_slides};

/// How many levels a slide can nest and still be read.
const MAX_NESTING: usize = 64;
const TOO_DEEP: &str = "This slide is nested too deeply to read";
/// The most a slide's body can hold and still be read, far more than any slide needs. The
/// `markdown` crate's time grows with the square of some slides' length, and the worst
/// found at this size takes about 4 seconds in a release build: a long run of `*`.
const MAX_BODY: usize = 32 * 1024;
const TOO_LARGE: &str = "This slide is too large to read";
const UNREADABLE: &str = "This slide could not be read";
/// The `markdown` crate goes further down the stack for every level of a slide, however
/// deep, and so does dropping its tree. Only what gets used of this is ever more than
/// reserved addresses, and it's enough for the deepest slide `MAX_BODY` allows.
const PARSER_STACK: usize = 1024 * 1024 * 1024;

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
    let raw_slides = split_slides(source);
    let slides = thread::scope(|scope| {
        let reader = thread::Builder::new()
            .stack_size(PARSER_STACK)
            .spawn_scoped(scope, || raw_slides.iter().map(|raw| read_slide(source, raw)).collect::<Vec<_>>());
        reader.ok().and_then(|reader| reader.join().ok())
    });
    let slides = slides.unwrap_or_else(|| raw_slides.iter().map(|raw| refused_slide(raw, UNREADABLE)).collect());
    Talk { source: source.to_string(), slides }
}

fn empty_slide(raw: &RawSlide) -> Slide {
    Slide { range: raw.range.clone(), settings: raw.settings.clone(), blocks: Vec::new(), notes: None, error: None }
}

fn refused_slide(raw: &RawSlide, message: &str) -> Slide {
    Slide { error: Some(SlideError { message: message.to_string() }), ..empty_slide(raw) }
}

/// Reads one slide, on the parser's thread. Inside `read_body` a tree can be as deep as
/// the slide is long. Nothing deeper than `MAX_NESTING` comes out of it.
fn read_slide(source: &str, raw: &RawSlide) -> Slide {
    if raw.body.len() > MAX_BODY {
        return refused_slide(raw, TOO_LARGE);
    }
    if a_line_opens_too_many_levels(&source[raw.body.clone()]) {
        return refused_slide(raw, TOO_DEEP);
    }
    // A slide that trips a bug in the `markdown` crate shouldn't take the rest of the talk with it.
    match catch_unwind(AssertUnwindSafe(|| read_body(source, raw.body.clone()))) {
        Ok(Ok((blocks, notes))) => Slide { blocks, notes, ..empty_slide(raw) },
        Ok(Err(message)) => refused_slide(raw, &message),
        Err(_) => refused_slide(raw, UNREADABLE),
    }
}

fn read_body(source: &str, body: Range<usize>) -> Result<(Vec<Block>, Option<Notes>), String> {
    let root = markdown::to_mdast(&source[body.clone()], &options()).map_err(|message| message.to_string())?;
    if nests_too_deep(&root) {
        return Err(TOO_DEEP.to_string());
    }
    let ctx = Ctx { source, base: body.start };
    let (mut blocks, mut notes) = (Vec::new(), None);
    for node in root.children().into_iter().flatten() {
        ctx.block(node, &mut blocks, &mut notes);
    }
    Ok((blocks, notes))
}

/// Whether a line starts with more quote and list markers than a slide can nest. Markers
/// repeated along one line take the `markdown` crate far longer than anything else of
/// their size, so these never reach it. The price is that a code block with a line of
/// 65 markers is turned away too. A bare carriage return ends a line for Markdown too.
fn a_line_opens_too_many_levels(body: &str) -> bool {
    body.split(['\n', '\r']).any(|line| markers_starting(line.as_bytes()) > MAX_NESTING)
}

/// How many quote and list markers start `line`, and go on to its first other text.
fn markers_starting(line: &[u8]) -> usize {
    let (mut at, mut markers) = (0, 0);
    loop {
        match line.get(at) {
            Some(b' ' | b'\t') => at += 1,
            Some(b'>') => {
                markers += 1;
                at += 1;
            }
            _ => {
                let Some(length) = list_marker_length(&line[at..]) else { return markers };
                markers += 1;
                at += length;
            }
        }
    }
}

/// The length of the list marker that starts `text`, if one does.
fn list_marker_length(text: &[u8]) -> Option<usize> {
    let digits = text.iter().take_while(|byte| byte.is_ascii_digit()).count();
    let length = match (digits, text.get(digits)) {
        (0, Some(b'-' | b'*' | b'+')) => 1,
        (1..=9, Some(b'.' | b')')) => digits + 1,
        _ => return None,
    };
    matches!(text.get(length), None | Some(b' ' | b'\t')).then_some(length)
}

/// Whether anywhere in the tree under `root` is more than `MAX_NESTING` levels in. Walks
/// the tree with its own stack, because the tree can be as deep as the slide is long.
fn nests_too_deep(root: &Node) -> bool {
    let mut to_visit = vec![(root, 0)];
    while let Some((node, levels_above)) = to_visit.pop() {
        let levels = levels_above + usize::from(is_a_level(node));
        if levels > MAX_NESTING {
            return true;
        }
        to_visit.extend(node.children().into_iter().flatten().map(|child| (child, levels)));
    }
    false
}

/// Whether `node` is a level of nesting. Anything that holds other nodes is, except for
/// the parts of a node that can't sit inside themselves: a list counts once, not again
/// for each item, and a paragraph, heading or table adds nothing to what's inside it.
fn is_a_level(node: &Node) -> bool {
    let plain_holder = matches!(
        node,
        Node::Root(_) | Node::Paragraph(_) | Node::Heading(_) | Node::ListItem(_) | Node::Table(_) | Node::TableRow(_) | Node::TableCell(_)
    );
    !plain_holder && node.children().is_some()
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
                        inner.apply_text_attr(&p.name, value);
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
