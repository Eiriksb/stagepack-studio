//! Typing on a slide: which of its text is being edited, and how that text is found again
//! once the talk has changed. The text itself is always the talk's: each edit is written to
//! the talk as it's made, so there's nothing to keep anywhere else.

use std::ops::Range;

use slides_format::{FlowPlace, Para, ParaKind, Pos, Selection, TextEdit, TextFlow, is_bidi_control};
use unicode_segmentation::UnicodeSegmentation;

use crate::history::RunId;

/// What Undo calls a run of typing.
pub const TYPING: &str = "Typing";
/// What Undo calls a change to how text looks or what kind of paragraph it is.
pub const FORMAT_TEXT: &str = "Format text";
/// What Undo calls a paste into text, and a cut from it.
pub const PASTE: &str = "Paste";
pub const CUT: &str = "Cut";
/// What Undo calls aligning a region's or a box's text.
pub const ALIGN_TEXT: &str = "Align text";

/// What the window says when typing can't be written to the talk.
pub const COULD_NOT_TYPE: &str = "That couldn't be typed here, so nothing was changed.";

/// One of a slide's runs of text, named so that the name lasts through edits to it: an edit
/// to a flow changes only that flow's bytes, so it stays the same one of its kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlowKey {
    /// The `nth` run of text in the layout's region `name`.
    Region { name: String, nth: usize },
    /// The box of text whose tag starts at byte `start` of the talk, which may have no text yet:
    /// a shape typed on, or a text box just put on the slide. Typing in it changes only what's
    /// inside the tag, so its start stays where it is; any other change to the talk moves it as
    /// it moves the thing selected. A box is never named by counting the slide's boxes, as which
    /// tags are boxes of text changes as the talk's components load.
    Tag { start: usize },
    /// A text box that isn't in the talk yet, its top-left corner at (`x`, `y`) of the slide,
    /// in slide units. The first thing typed in it writes it; till then nothing of it is written.
    /// `align` is the alignment chosen for it meanwhile, as the box will be written with it:
    /// `center` or `right`, or nothing for the left.
    NewBox { x: u32, y: u32, align: Option<&'static str> },
    /// The cell at `row` and `column` of the `table`th table on the slide whose cells are
    /// typed in. Typing in a cell changes only that cell's bytes, so its table stays the same
    /// one of those.
    Cell { table: usize, row: usize, column: usize },
}

/// Which of the slide's tables whose cells are typed in each of `flows` is a cell of, if any:
/// they're counted in the order their cells come.
fn tables_of(flows: &[TextFlow]) -> impl Iterator<Item = Option<usize>> + '_ {
    let mut last: Option<&std::ops::Range<usize>> = None;
    let mut count = 0;
    flows.iter().map(move |flow| match &flow.place {
        FlowPlace::Cell { table, .. } => {
            if last != Some(table) {
                count += usize::from(last.is_some());
                last = Some(table);
            }
            Some(count)
        }
        _ => None,
    })
}

impl FlowKey {
    /// The key of `flows[index]`.
    pub fn of(flows: &[TextFlow], index: usize) -> Option<FlowKey> {
        let flow = flows.get(index)?;
        let before = &flows[..index];
        Some(match &flow.place {
            FlowPlace::Region(name) => FlowKey::Region {
                name: name.clone(),
                nth: before
                    .iter()
                    .filter(|other| other.place == flow.place)
                    .count(),
            },
            FlowPlace::Tag(range) => FlowKey::Tag { start: range.start },
            FlowPlace::Cell { row, column, .. } => {
                FlowKey::Cell { table: tables_of(flows).nth(index).flatten()?, row: *row, column: *column }
            }
        })
    }

    /// Which of `flows` this is, if the slide has it.
    pub fn find(&self, flows: &[TextFlow]) -> Option<usize> {
        if let FlowKey::Cell { table, row, column } = self {
            return tables_of(flows).zip(flows).position(|(nth, flow)| {
                nth == Some(*table) && matches!(flow.place, FlowPlace::Cell { row: r, column: c, .. } if (r, c) == (*row, *column))
            });
        }
        let mut seen = 0;
        for (index, flow) in flows.iter().enumerate() {
            let same_kind = match (self, &flow.place) {
                (FlowKey::Region { name, .. }, FlowPlace::Region(region)) => name == region,
                (FlowKey::Tag { start }, FlowPlace::Tag(range)) => {
                    if range.start == *start {
                        return Some(index);
                    }
                    continue;
                }
                _ => false,
            };
            if !same_kind {
                continue;
            }
            let nth = match self {
                FlowKey::Region { nth, .. } => *nth,
                FlowKey::Tag { .. } | FlowKey::NewBox { .. } | FlowKey::Cell { .. } => continue,
            };
            if seen == nth {
                return Some(index);
            }
            seen += 1;
        }
        None
    }

    /// The region this is a run of, if it's one.
    pub fn region(&self) -> Option<&str> {
        match self {
            FlowKey::Region { name, .. } => Some(name),
            FlowKey::Tag { .. } | FlowKey::NewBox { .. } | FlowKey::Cell { .. } => None,
        }
    }

    /// Where the tag this is the box of starts, if it's named by its tag.
    pub fn tag_start(&self) -> Option<usize> {
        match self {
            FlowKey::Tag { start } => Some(*start),
            _ => None,
        }
    }

    /// This key once `change` is made to the talk: a box's tag moved as the thing selected is,
    /// or `None` if the change took its start away. Other keys name what doesn't move.
    pub fn moved(&self, change: &slides_format::Change) -> Option<FlowKey> {
        match self {
            FlowKey::Tag { start } => {
                crate::selection::moved(&(*start..*start + 1), change).map(|range| FlowKey::Tag { start: range.start })
            }
            other => Some(other.clone()),
        }
    }

    /// The alignment chosen for the text box not yet written, if this is one: `None` for the left.
    pub fn new_box_align(&self) -> Option<&'static str> {
        match self {
            FlowKey::NewBox { align, .. } => *align,
            _ => None,
        }
    }

    /// Where the text box not yet written is, if this is one: its top-left corner.
    pub fn new_box(&self) -> Option<(u32, u32)> {
        match self {
            FlowKey::NewBox { x, y, .. } => Some((*x, *y)),
            _ => None,
        }
    }

    /// Whether this is a cell of a table.
    pub fn is_cell(&self) -> bool {
        matches!(self, FlowKey::Cell { .. })
    }
}

/// Where a step of the history was typed: the selection before it and after it, so that
/// undoing it, or making it again, goes back to typing there. A step that took away the text
/// it was typed in has nothing after it, and making it again ends the typing.
#[derive(Clone, Debug, PartialEq)]
pub struct TextMark {
    pub slide: usize,
    pub flow: FlowKey,
    pub before: Selection,
    pub after: Option<Selection>,
    /// Where `after` is, if that's another flow than `flow`: a step that changed a table puts the
    /// caret in another of its cells.
    pub flow_after: Option<FlowKey>,
    /// The time of typing the step was made in, from a click on the text until it ended.
    pub(crate) session: u64,
}

/// The text being edited on a slide: which slide, which of its flows, and the flow's
/// paragraphs with the selection in them. The paragraphs are what the talk holds, as they
/// read back, apart from an empty region's first paragraph, which isn't written until it
/// has something in it.
#[derive(Clone, Debug, PartialEq)]
pub struct EditingText {
    pub slide: usize,
    pub flow: FlowKey,
    pub edit: TextEdit,
    /// The run of the history typing here joins, while it's open.
    pub(crate) run: Option<RunId>,
    /// Which time of typing this is: it starts with a click on the text, and goes on through
    /// an undo or a redo of a step made in it.
    pub(crate) session: u64,
    /// What the system's input method is composing in place of the selection, as a dead key
    /// waits for its letter. It isn't in the talk: committing it types it, and anything else
    /// done to the text, or anything that ends typing, drops it.
    pub composing: Option<Composing>,
}

/// Text the system's input method is composing, shown at the caret until it's committed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Composing {
    pub text: String,
    /// The system's selection inside it, in UTF-16 from its start.
    pub selected: Range<usize>,
}

impl Composing {
    /// `text` being composed, the system's selection in it `selected`, or after it. `None` for
    /// no text, as an empty composition is none.
    pub fn new(text: &str, selected: Option<Range<usize>>) -> Option<Composing> {
        let len = utf16_len(text);
        let selected = selected.map_or(len..len, |range| {
            let (start, end) = (range.start.min(range.end), range.start.max(range.end));
            start.min(len)..end.min(len)
        });
        (!text.is_empty()).then(|| Composing {
            text: text.to_string(),
            selected,
        })
    }

    fn len(&self) -> usize {
        utf16_len(&self.text)
    }
}

/// The text being typed as the system's input method sees it: the flow's paragraphs one line
/// apart, a line break in one a line too, and any composition in place of the selection.
/// Offsets in it are UTF-16, as the system counts; turned into places in the paragraphs, they
/// land on the edges of characters as they're drawn.
impl EditingText {
    /// The text the system sees.
    pub fn system_text(&self) -> String {
        let flow = flow_text(&self.edit.paras);
        match &self.composing {
            None => flow,
            Some(composing) => {
                let selection = self.edit.selection;
                let start = byte_in_flow(&self.edit.paras, selection.start());
                let end = byte_in_flow(&self.edit.paras, selection.end());
                format!("{}{}{}", &flow[..start], composing.text, &flow[end..])
            }
        }
    }

    /// Where `pos` is in the text the system sees. A place the composition stands over is its
    /// start.
    pub fn system_offset(&self, pos: Pos) -> usize {
        let paras = &self.edit.paras;
        let at = utf16_in_flow(paras, pos);
        let Some(composing) = &self.composing else {
            return at;
        };
        let selection = self.edit.selection;
        let (start, end) = (selection.start(), selection.end());
        if pos <= start {
            at
        } else if pos >= end {
            at - utf16_in_flow(paras, end) + utf16_in_flow(paras, start) + composing.len()
        } else {
            utf16_in_flow(paras, start)
        }
    }

    /// The place in the paragraphs of offset `at` of the text the system sees: on the edge of a
    /// character before it, or after it with `forward`. A place inside the composition is the
    /// start of the selection it stands in for.
    pub fn system_pos(&self, at: usize, forward: bool) -> Pos {
        let paras = &self.edit.paras;
        let Some(composing) = &self.composing else {
            return pos_in_flow(paras, at, forward);
        };
        let selection = self.edit.selection;
        let start = utf16_in_flow(paras, selection.start());
        if at <= start {
            pos_in_flow(paras, at, forward)
        } else if at >= start + composing.len() {
            let end = utf16_in_flow(paras, selection.end());
            pos_in_flow(paras, at - start - composing.len() + end, forward)
        } else {
            selection.start()
        }
    }

    /// The selection as the system sees it, and whether its caret is at its start. While
    /// composing, it's the system's own selection inside the composition.
    pub fn system_selection(&self) -> (Range<usize>, bool) {
        let selection = self.edit.selection;
        match (&self.composing, self.marked_range()) {
            (Some(composing), Some(marked)) => (
                marked.start + composing.selected.start..marked.start + composing.selected.end,
                false,
            ),
            _ => {
                let anchor = self.system_offset(selection.anchor);
                let head = self.system_offset(selection.head);
                (anchor.min(head)..anchor.max(head), head < anchor)
            }
        }
    }

    /// Where the composition is in the text the system sees, if there is one.
    pub fn marked_range(&self) -> Option<Range<usize>> {
        let composing = self.composing.as_ref()?;
        let start = utf16_in_flow(&self.edit.paras, self.edit.selection.start());
        Some(start..start + composing.len())
    }
}

/// `text` from the system as the text holds it, as typing it would leave it: line endings as
/// newlines, control characters other than tab and newline dropped, no byte order mark, and no
/// bidirectional embedding, override or isolate, as the text model holds none
/// ([`is_bidi_control`]).
pub fn clean_typed(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .chars()
        .filter(|&c| c == '\t' || c == '\n' || !(c.is_control() || c == '\u{feff}' || is_bidi_control(c)))
        .collect()
}

/// The part of `text` in UTF-16 range `range`, widened onto whole characters, and the range it
/// is.
pub fn utf16_slice(text: &str, range: Range<usize>) -> (String, Range<usize>) {
    let (start, start16) = byte_at_utf16(text, range.start, false);
    let (end, end16) = byte_at_utf16(text, range.end.max(range.start), true);
    (text[start..end].to_string(), start16..end16)
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

fn para_text(para: &Para) -> String {
    para.spans.iter().map(|span| span.text.as_str()).collect()
}

fn flow_text(paras: &[Para]) -> String {
    paras.iter().map(para_text).collect::<Vec<_>>().join("\n")
}

/// The byte of [`flow_text`] `pos` is at.
fn byte_in_flow(paras: &[Para], pos: Pos) -> usize {
    let before: usize = paras
        .iter()
        .take(pos.para)
        .map(|para| para_text(para).len() + 1)
        .sum();
    let text = paras.get(pos.para).map(para_text).unwrap_or_default();
    before + floor_char(&text, pos.offset)
}

/// The UTF-16 offset of [`flow_text`] `pos` is at.
fn utf16_in_flow(paras: &[Para], pos: Pos) -> usize {
    let before: usize = paras
        .iter()
        .take(pos.para)
        .map(|para| utf16_len(&para_text(para)) + 1)
        .sum();
    let text = paras.get(pos.para).map(para_text).unwrap_or_default();
    before + utf16_len(&text[..floor_char(&text, pos.offset)])
}

/// The place in `paras` of UTF-16 offset `at` of [`flow_text`], on the edge of a character as
/// it's drawn: the one before, or after with `forward`. Past the end is the end.
fn pos_in_flow(paras: &[Para], at: usize, forward: bool) -> Pos {
    let mut start = 0;
    for (index, para) in paras.iter().enumerate() {
        let text = para_text(para);
        let len = utf16_len(&text);
        if at <= start + len || index + 1 == paras.len() {
            let (byte, _) = byte_at_utf16(&text, at.saturating_sub(start), forward);
            return Pos {
                para: index,
                offset: grapheme_edge(&text, byte, forward),
            };
        }
        start += len + 1;
    }
    Pos::default()
}

/// The byte of `text` at UTF-16 offset `at`, and the offset it's at: inside a character, its
/// start, or its end with `forward`. Past the end is the end.
fn byte_at_utf16(text: &str, at: usize, forward: bool) -> (usize, usize) {
    let mut units = 0;
    for (byte, c) in text.char_indices() {
        let next = units + c.len_utf16();
        if at <= units {
            return (byte, units);
        }
        if at < next {
            return if forward {
                (byte + c.len_utf8(), next)
            } else {
                (byte, units)
            };
        }
        units = next;
    }
    (text.len(), units)
}

/// `at`, a byte of `text`, moved back onto a character's start.
fn floor_char(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// `at`, a byte of `text` on a character's start, moved onto the edge of a character as it's
/// drawn: back, or forward with `forward`.
fn grapheme_edge(text: &str, at: usize, forward: bool) -> usize {
    let edges = text
        .grapheme_indices(true)
        .map(|(edge, _)| edge)
        .chain([text.len()]);
    if forward {
        edges
            .into_iter()
            .find(|&edge| edge >= at)
            .unwrap_or(text.len())
    } else {
        edges.take_while(|&edge| edge <= at).last().unwrap_or(0)
    }
}

impl EditingText {
    /// Text started in region `region` of slide `slide`, which has nothing in it yet: one
    /// empty paragraph, a heading for the title.
    pub fn in_empty_region(slide: usize, region: &str) -> EditingText {
        let kind = if region == "title" {
            ParaKind::Heading(1)
        } else {
            ParaKind::Body
        };
        EditingText {
            slide,
            flow: FlowKey::Region {
                name: region.to_string(),
                nth: 0,
            },
            edit: TextEdit::new(vec![Para {
                kind,
                spans: Vec::new(),
            }]),
            run: None,
            session: 0,
            composing: None,
        }
    }

    /// Text started in the box whose tag starts at byte `start` of slide `slide`, a shape or a
    /// box with nothing in it yet: one empty paragraph, not written until something is typed.
    pub fn in_empty_tag(slide: usize, start: usize) -> EditingText {
        EditingText {
            slide,
            flow: FlowKey::Tag { start },
            edit: TextEdit::new(vec![Para {
                kind: ParaKind::Body,
                spans: Vec::new(),
            }]),
            run: None,
            session: 0,
            composing: None,
        }
    }

    /// Text started in a text box on slide `slide`, its top-left corner at (`x`, `y`), that isn't
    /// written yet: one empty paragraph.
    pub fn in_new_box(slide: usize, x: u32, y: u32) -> EditingText {
        EditingText { flow: FlowKey::NewBox { x, y, align: None }, ..EditingText::in_empty_tag(slide, 0) }
    }

    /// Whether this is the slide's title, where a paragraph can't be ended.
    pub fn is_title(&self) -> bool {
        self.flow.region() == Some("title")
    }

    /// Whether this is a cell of a table, which holds one line of text.
    pub fn is_cell(&self) -> bool {
        self.flow.is_cell()
    }
}

/// `text` as one line, for a cell of a table: each new line in it a space.
pub fn one_line(text: &str) -> String {
    text.replace("\r\n", " ").replace(['\n', '\r', '\u{2028}', '\u{2029}'], " ")
}

/// What an operation on the text did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Nothing at all.
    Nothing,
    /// Only the selection moved.
    Moved,
    /// Characters or paragraphs were added or taken away.
    Typed,
    /// The words are the same, but how they look or what kind of paragraph they are isn't.
    Formatted,
}

impl Kind {
    pub(crate) fn of(before: &TextEdit, after: &TextEdit) -> Kind {
        if before.paras == after.paras {
            return if before.selection == after.selection
                && before.typing_style == after.typing_style
            {
                Kind::Nothing
            } else {
                Kind::Moved
            };
        }
        let words = |edit: &TextEdit| -> Vec<String> {
            edit.paras
                .iter()
                .map(|para| para.spans.iter().map(|span| span.text.as_str()).collect())
                .collect()
        };
        if words(before) != words(after) {
            Kind::Typed
        } else {
            Kind::Formatted
        }
    }
}

/// Whether `paras` hold any text, or more than one paragraph.
pub(crate) fn holds_something(paras: &[Para]) -> bool {
    paras.len() > 1 || paras.iter().any(|para| !para.spans.is_empty())
}

/// Whether `paras` have no words at all, whatever kind of paragraph each is: what typing
/// leaves once everything typed or there before is taken away.
pub(crate) fn holds_no_words(paras: &[Para]) -> bool {
    paras.iter().all(|para| para.spans.is_empty())
}

/// Whether any of `flows` is text in the layout's region `region`.
pub(crate) fn region_has_text(flows: &[TextFlow], region: &str) -> bool {
    flows
        .iter()
        .any(|flow| matches!(&flow.place, FlowPlace::Region(name) if name == region))
}
