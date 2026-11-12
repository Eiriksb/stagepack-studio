//! Typing on a slide: which of its text is being edited, and how that text is found again
//! once the talk has changed. The text itself is always the talk's: each edit is written to
//! the talk as it's made, so there's nothing to keep anywhere else.

use slides_format::{FlowPlace, Para, ParaKind, Selection, TextEdit, TextFlow};

use crate::history::RunId;

/// What Undo calls a run of typing.
pub const TYPING: &str = "Typing";
/// What Undo calls a change to how text looks or what kind of paragraph it is.
pub const FORMAT_TEXT: &str = "Format text";

/// What the window says when typing can't be written to the talk.
pub const COULD_NOT_TYPE: &str = "That couldn't be typed here, so nothing was changed.";

/// One of a slide's runs of text, named so that the name lasts through edits to it: an edit
/// to a flow changes only that flow's bytes, so it stays the same one of its kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlowKey {
    /// The `nth` run of text in the layout's region `name`.
    Region { name: String, nth: usize },
    /// The `nth` box of text on the slide.
    Box { nth: usize },
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
            FlowPlace::Tag(_) => FlowKey::Box {
                nth: before
                    .iter()
                    .filter(|other| matches!(other.place, FlowPlace::Tag(_)))
                    .count(),
            },
        })
    }

    /// Which of `flows` this is, if the slide has it.
    pub fn find(&self, flows: &[TextFlow]) -> Option<usize> {
        let mut seen = 0;
        for (index, flow) in flows.iter().enumerate() {
            let same_kind = match (self, &flow.place) {
                (FlowKey::Region { name, .. }, FlowPlace::Region(region)) => name == region,
                (FlowKey::Box { .. }, FlowPlace::Tag(_)) => true,
                _ => false,
            };
            if !same_kind {
                continue;
            }
            let nth = match self {
                FlowKey::Region { nth, .. } | FlowKey::Box { nth } => *nth,
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
            FlowKey::Box { .. } => None,
        }
    }
}

/// Where a step of the history was typed: the selection before it and after it, so that
/// undoing it, or making it again, goes back to typing there.
#[derive(Clone, Debug, PartialEq)]
pub struct TextMark {
    pub slide: usize,
    pub flow: FlowKey,
    pub before: Selection,
    pub after: Selection,
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
        }
    }

    /// Whether this is the slide's title, where a paragraph can't be ended.
    pub fn is_title(&self) -> bool {
        self.flow.region() == Some("title")
    }
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

/// Whether `paras` are nothing but empty body paragraphs, left by typing and taking it away.
pub(crate) fn only_empty_lines(paras: &[Para]) -> bool {
    paras
        .iter()
        .all(|para| para.kind == ParaKind::Body && para.spans.is_empty())
}
