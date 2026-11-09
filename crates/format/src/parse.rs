use std::ops::Range;

use markdown::mdast::{AttributeContent, AttributeValue, Node};
use markdown::{Constructs, ParseOptions};

use crate::attrs::scan_open_tag;
use crate::model::*;
use crate::split::split_slides;

/// How deep a slide can nest and still be read. The `markdown` crate and this parser
/// each go further down the stack for every level, so a deep enough slide would overflow it.
const MAX_NESTING: usize = 64;
const TOO_DEEP: &str = "This slide is nested too deeply to read";

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
            if nesting_estimate(body) > MAX_NESTING {
                slide.error = Some(SlideError { message: TOO_DEEP.to_string() });
                return slide;
            }
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

/// How deep `body` nests, estimated from its lines so that nothing recursive has to see a
/// slide that's too deep. It leans high, because too high only matters to a slide nested
/// far deeper than anyone writes, and too low could crash the app.
fn nesting_estimate(body: &str) -> usize {
    let mut deepest = 0;
    let mut open_tags = 0;
    let mut fence = None;
    // Set once a line can't be read on its own, because a tag runs on past it or a fence
    // doesn't start it. After that, a closing tag or a fence might only be text.
    let mut unsure = false;
    for line in body.lines() {
        // Worked out even inside what looks like a code fence, which might not be one.
        let (containers, rest) = containers_opening(line);
        let mut tags = open_tags;
        match fence {
            Some(open) if closes_fence(line, open) => fence = None,
            Some(_) => {}
            None => {
                fence = opening_fence(line).filter(|_| !unsure);
                unsure |= fence.is_none() && (rest.starts_with("```") || rest.starts_with("~~~"));
                if fence.is_none() {
                    // A closing tag in indented code or a link's target is only text.
                    let closes_count = !unsure && indentation(line) < 4 && !line.contains("](");
                    tags = follow_tags(rest, closes_count, &mut open_tags, &mut unsure);
                }
            }
        }
        deepest = deepest.max(containers + tags);
    }
    deepest
}

fn indentation(line: &str) -> usize {
    line.bytes().map_while(|byte| match byte {
        b' ' => Some(1),
        b'\t' => Some(4),
        _ => None,
    }).sum()
}

/// How many quotes and list levels start `line`, and what comes after them. Lists count
/// their markers as well as their indent, because several can start on one line.
fn containers_opening(line: &str) -> (usize, &str) {
    let bytes = line.as_bytes();
    let (mut at, mut depth, mut columns, mut in_list) = (0, 0, 0, false);
    loop {
        match bytes.get(at) {
            Some(b' ') => {
                columns += 1;
                at += 1;
            }
            Some(b'\t') => {
                columns += 4;
                at += 1;
            }
            Some(b'>') => {
                depth += 1;
                at += 1;
            }
            _ => {
                let Some(length) = list_marker_length(&bytes[at..]) else { break };
                depth += 1;
                at += length;
                in_list = true;
            }
        }
    }
    (depth + if in_list { columns / 2 } else { 0 }, &line[at..])
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

/// The marker and length of the code fence `line` opens. Only one at the very start of a
/// line counts, as an indented or quoted one can end where a scan of lines can't tell.
fn opening_fence(line: &str) -> Option<(u8, usize)> {
    let marker = *line.as_bytes().first()?;
    let length = line.bytes().take_while(|byte| *byte == marker).count();
    let is_fence = matches!(marker, b'`' | b'~') && length >= 3 && !(marker == b'`' && line[length..].contains('`'));
    is_fence.then_some((marker, length))
}

fn closes_fence(line: &str, (marker, length): (u8, usize)) -> bool {
    let text = line.trim_start_matches(' ');
    let run = text.bytes().take_while(|byte| *byte == marker).count();
    line.len() - text.len() < 4 && run >= length && text[run..].trim().is_empty()
}

/// Follows the tags in `text`, the part of a line after its quotes and list markers, and
/// returns the most that were open at once. A closing tag only counts if it starts the
/// text or matches an opening tag before it there. Elsewhere it might not be a tag at all.
fn follow_tags(text: &str, closes_count: bool, open_tags: &mut usize, unsure: &mut bool) -> usize {
    let bytes = text.as_bytes();
    let mut deepest = *open_tags;
    let mut opened_here = 0_usize;
    let mut at = 0;
    while at < bytes.len() {
        match (bytes[at], bytes.get(at + 1)) {
            (b'\\', _) => at += 2,
            (b'`', _) => at = after_code_span(bytes, at),
            (b'<', Some(b'/')) => {
                if closes_count && (at == 0 || opened_here > 0) {
                    *open_tags = open_tags.saturating_sub(1);
                    opened_here = opened_here.saturating_sub(1);
                }
                at += 2;
            }
            (b'<', Some(next)) if !next.is_ascii_whitespace() => {
                let (end, closes_itself) = tag_end(bytes, at + 1).unwrap_or_else(|| {
                    *unsure = true;
                    (bytes.len(), false)
                });
                if !closes_itself {
                    *open_tags += 1;
                    opened_here += 1;
                    deepest = deepest.max(*open_tags);
                }
                at = end;
            }
            _ => at += 1,
        }
    }
    deepest
}

/// Where the code span opened by the backticks at `start` ends, or just past those
/// backticks if nothing on the line closes it.
fn after_code_span(bytes: &[u8], start: usize) -> usize {
    let run_at = |at: usize| bytes[at..].iter().take_while(|byte| **byte == b'`').count();
    let length = run_at(start);
    let mut at = start + length;
    while at < bytes.len() {
        if bytes[at] != b'`' {
            at += 1;
            continue;
        }
        let run = run_at(at);
        if run == length {
            return at + run;
        }
        at += run;
    }
    start + length
}

/// Where the tag whose name starts at `at` ends, and whether it closes itself, or `None`
/// if it runs on past this line.
fn tag_end(bytes: &[u8], mut at: usize) -> Option<(usize, bool)> {
    let mut quote = None;
    while at < bytes.len() {
        match (quote, bytes[at]) {
            (Some(open), byte) if byte == open => quote = None,
            (Some(_), _) => {}
            (None, byte @ (b'"' | b'\'')) => quote = Some(byte),
            (None, b'>') => return Some((at + 1, bytes[at - 1] == b'/')),
            _ => {}
        }
        at += 1;
    }
    None
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
