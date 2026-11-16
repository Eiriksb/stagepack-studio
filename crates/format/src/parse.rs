use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::thread;

use markdown::mdast::{AttributeContent, AttributeValue, Node};
use markdown::{Constructs, ParseOptions};

use crate::attrs::scan_open_tag;
use crate::model::*;
use crate::notes::unescape_notes;
use crate::split::{RawSlide, split_slides};

/// How many levels a slide can nest and still be read.
const MAX_NESTING: usize = 64;
const TOO_DEEP: &str = "This slide is nested too deeply to read";
/// The most a slide's body can hold and still be read, far more than any slide needs. The
/// `markdown` crate's time grows with the square of some slides' length, and the worst
/// found at this size takes about 4 seconds in a release build: a long run of `*`.
pub const MAX_BODY: usize = 32 * 1024;
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

thread_local! {
    /// How many slides have been read on this thread, for tests.
    static SLIDES_PARSED: Cell<usize> = const { Cell::new(0) };
}

/// How many slides this thread has had read, by [`parse_talk`] or [`crate::reparse`], so
/// that tests can see which are read again. Only for tests.
#[doc(hidden)]
pub fn slides_parsed_on_this_thread() -> usize {
    SLIDES_PARSED.with(Cell::get)
}

/// Reads a whole file into slides. A slide that fails to parse carries an
/// error and no blocks; the others are unaffected.
pub fn parse_talk(source: &str) -> Talk {
    let raw_slides = split_slides(source);
    let slides = read_slides(source, &raw_slides.iter().collect::<Vec<_>>());
    Talk { source: source.to_string(), slides }
}

/// Reads each of `raw_slides`, slides of `source`, on the parser's thread.
pub(crate) fn read_slides(source: &str, raw_slides: &[&RawSlide]) -> Vec<Slide> {
    if raw_slides.is_empty() {
        return Vec::new();
    }
    SLIDES_PARSED.with(|parsed| parsed.set(parsed.get() + raw_slides.len()));
    let slides = thread::scope(|scope| {
        let reader = thread::Builder::new()
            .stack_size(PARSER_STACK)
            .spawn_scoped(scope, || raw_slides.iter().map(|raw| read_slide(source, raw)).collect::<Vec<_>>());
        reader.ok().and_then(|reader| reader.join().ok())
    });
    slides.unwrap_or_else(|| raw_slides.iter().map(|raw| refused_slide(raw, UNREADABLE)).collect())
}

fn empty_slide(raw: &RawSlide) -> Slide {
    Slide { range: raw.range.clone(), settings: raw.settings.clone(), blocks: Vec::new(), notes: None, error: None, inline_tags: Vec::new(), quotes: Vec::new() }
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
        Ok(Ok(Body { blocks, notes, inline_tags, quotes })) => Slide { blocks, notes, inline_tags, quotes, ..empty_slide(raw) },
        Ok(Err(message)) => refused_slide(raw, &message),
        Err(_) => refused_slide(raw, UNREADABLE),
    }
}

/// What's read from a slide's body.
struct Body {
    blocks: Vec<Block>,
    notes: Option<Notes>,
    /// The tags in the middle of a line.
    inline_tags: Vec<String>,
    quotes: Vec<Range<usize>>,
}

fn read_body(source: &str, body: Range<usize>) -> Result<Body, String> {
    let root = markdown::to_mdast(&source[body.clone()], &options()).map_err(|message| message.to_string())?;
    if nests_too_deep(&root) {
        return Err(TOO_DEEP.to_string());
    }
    // Nothing past the slide is looked at, so that a slide reads the same wherever it sits.
    let ctx = Ctx { source: &source[..body.end], base: body.start, inline_tags: RefCell::new(Vec::new()), quotes: RefCell::new(Vec::new()) };
    let (mut blocks, mut notes) = (Vec::new(), None);
    for node in root.children().into_iter().flatten() {
        ctx.block(node, &mut blocks, &mut notes);
    }
    Ok(Body { blocks, notes, inline_tags: ctx.inline_tags.into_inner(), quotes: ctx.quotes.into_inner() })
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
    /// The tags found in the middle of a line, other than `<Text>`.
    inline_tags: RefCell<Vec<String>>,
    /// The block quotes found, at any depth.
    quotes: RefCell<Vec<Range<usize>>>,
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
                inline: self.inline_range(&h.children, &range),
                only_text: only_text(&h.children),
                range,
            }),
            Node::Paragraph(p) => match p.children.as_slice() {
                [Node::Image(image)] => out.push(Block::Picture {
                    src: image.url.clone(),
                    alt: image.alt.clone(),
                    range,
                }),
                children => out.push(Block::Paragraph {
                    spans: self.spans(children),
                    inline: self.inline_range(children, &range),
                    only_text: only_text(children),
                    range,
                }),
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
                item_ranges: list.children.iter().map(|item| self.range(item)).collect(),
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
                self.quotes.borrow_mut().push(range);
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
                            text: unescape_notes(self.source[open_end.min(end)..end].trim()),
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

    /// The bytes from the first of `nodes`, a block's content, to the end of the last, or an
    /// empty range at the block's end when there are none. Text that starts with an escaped
    /// character starts at its backslash, which the text's own position leaves out.
    fn inline_range(&self, nodes: &[Node], block: &Range<usize>) -> Range<usize> {
        match (nodes.first(), nodes.last()) {
            (Some(first), Some(last)) => {
                let mut start = self.range(first).start;
                let escaped = matches!(first, Node::Text(_)) && start > block.start && self.source.as_bytes().get(start - 1) == Some(&b'\\');
                if escaped {
                    start -= 1;
                }
                start..self.range(last).end
            }
            _ => block.end..block.end,
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
                if let Some(name) = element.name.as_deref().filter(|name| *name != "Text") {
                    let mut found = self.inline_tags.borrow_mut();
                    if !found.iter().any(|known| known == name) {
                        found.push(name.to_string());
                    }
                }
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

/// Whether `nodes`, a paragraph's or heading's content, are text alone: plain, bold,
/// italic, code or a link with no title, or inside `<Text>` tags whose attributes are all
/// ones a [`Style`] holds, with values it reads.
/// Anything else is drawn as something the spans don't say, or means something they leave out.
fn only_text(nodes: &[Node]) -> bool {
    nodes.iter().all(|node| match node {
        Node::Text(_) | Node::Break(_) | Node::InlineCode(_) => true,
        Node::Strong(s) => only_text(&s.children),
        Node::Emphasis(e) => only_text(&e.children),
        // A link's title isn't kept.
        Node::Link(l) => l.title.is_none() && only_text(&l.children),
        Node::MdxJsxTextElement(element) => {
            element.name.as_deref() == Some("Text")
                && element.attributes.iter().all(|attribute| {
                    let AttributeContent::Property(p) = attribute else { return false };
                    let value = match &p.value {
                        None => "true",
                        Some(AttributeValue::Literal(value)) => value.as_str(),
                        Some(_) => return false,
                    };
                    Style::knows_text_attr(&p.name, value)
                })
                && only_text(&element.children)
        }
        _ => false,
    })
}

fn push_span(out: &mut Vec<Span>, text: &str, style: &Style) {
    match out.last_mut() {
        Some(last) if last.style == *style => last.text.push_str(text),
        _ => out.push(Span { text: text.to_string(), style: style.clone() }),
    }
}
