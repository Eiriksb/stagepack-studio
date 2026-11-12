//! Editing a slide's text: pure operations on a flow's paragraphs and a selection in them.
//! Every operation takes any position, in range or not, and gives back paragraphs that are
//! tidy (no empty span, no two neighbours with one style, levels in range) and a selection
//! on the edges of characters as they're drawn.

use unicode_segmentation::UnicodeSegmentation;

use crate::model::{Span, Style};
use crate::rich::{MAX_LEVEL, Para, ParaKind};

/// A place in the text: a byte offset into paragraph `para`'s text, on the edge of a
/// character as it's drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pos {
    pub para: usize,
    pub offset: usize,
}

/// What's selected: from `anchor`, where it began, to `head`, where the caret is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Selection {
    pub anchor: Pos,
    pub head: Pos,
}

impl Selection {
    /// A caret at `at`, selecting nothing.
    pub fn caret(at: Pos) -> Self {
        Selection { anchor: at, head: at }
    }

    pub fn is_caret(&self) -> bool {
        self.anchor == self.head
    }

    pub fn start(&self) -> Pos {
        self.anchor.min(self.head)
    }

    pub fn end(&self) -> Pos {
        self.anchor.max(self.head)
    }
}

/// A style that's either on or off.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StyleBit {
    Bold,
    Italic,
    Underline,
}

impl StyleBit {
    fn get(self, style: &Style) -> bool {
        match self {
            StyleBit::Bold => style.bold,
            StyleBit::Italic => style.italic,
            StyleBit::Underline => style.underline,
        }
    }

    fn set(self, style: &mut Style, on: bool) {
        match self {
            StyleBit::Bold => style.bold = on,
            StyleBit::Italic => style.italic = on,
            StyleBit::Underline => style.underline = on,
        }
    }
}

/// Where the caret goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    WordLeft,
    WordRight,
    ParaStart,
    ParaEnd,
    FlowStart,
    FlowEnd,
}

/// What a selection holds of one thing: all of it one value, a mix, or nothing to say,
/// as when the selection holds no text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shared<T> {
    All(T),
    Mixed,
    None,
}

impl<T: PartialEq> Shared<T> {
    fn of(values: impl IntoIterator<Item = T>) -> Self {
        let mut values = values.into_iter();
        let Some(first) = values.next() else { return Shared::None };
        if values.all(|value| value == first) { Shared::All(first) } else { Shared::Mixed }
    }
}

/// What the toolbar shows. For a caret, the style what's typed next takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleState {
    pub bold: Shared<bool>,
    pub italic: Shared<bool>,
    pub underline: Shared<bool>,
    pub size: Shared<Option<u32>>,
    pub color: Shared<Option<String>>,
    pub kind: Shared<ParaKind>,
}

/// A flow's paragraphs being edited.
#[derive(Clone, Debug, PartialEq)]
pub struct TextEdit {
    pub paras: Vec<Para>,
    pub selection: Selection,
    /// The style the next typed character takes, when the caret selects nothing and a
    /// style was chosen for it. Anything but styling forgets it.
    pub typing_style: Option<Style>,
}

impl TextEdit {
    /// An edit of `paras`, the caret at the start.
    pub fn new(paras: Vec<Para>) -> Self {
        let mut edit = TextEdit { paras, selection: Selection::default(), typing_style: None };
        edit.tidy();
        edit
    }

    /// Puts `text` in place of the selection. Each newline in it starts a paragraph of the
    /// kind the selection starts in. What's typed takes the style before the caret, or,
    /// over a selection, the style of its first character.
    pub fn insert(&self, text: &str) -> Self {
        let mut edit = self.clamped();
        if text.is_empty() && edit.selection.is_caret() {
            return edit;
        }
        edit.ensure_para();
        let style = edit.style_for_typing();
        let kind = edit.paras[edit.selection.start().para].kind;
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let fragment = text.split('\n').map(|line| Para { kind, spans: span(line, &style) }).collect();
        edit.replace(fragment, true);
        edit
    }

    /// A line break inside the paragraph, in place of the selection.
    pub fn soft_break(&self) -> Self {
        let mut edit = self.clamped();
        edit.ensure_para();
        let style = edit.style_for_typing();
        edit.replace(vec![Para { kind: ParaKind::Body, spans: span("\n", &style) }], true);
        edit
    }

    /// Deletes the selection, or the character before the caret. At the start of a list
    /// item it outdents it, or, at the top level, makes it a body paragraph; at the start
    /// of anything else it joins the paragraph to the one before.
    pub fn backspace(&self) -> Self {
        let mut edit = self.clamped();
        edit.ensure_para();
        if !edit.selection.is_caret() {
            edit.replace(Vec::new(), false);
            return edit;
        }
        let at = edit.selection.head;
        if at.offset == 0 {
            let kind = &mut edit.paras[at.para].kind;
            match *kind {
                ParaKind::Bullet { level } | ParaKind::Numbered { level } => {
                    *kind = if level == 0 { ParaKind::Body } else { with_level(*kind, level - 1) };
                    edit.typing_style = None;
                    return edit;
                }
                _ if at.para == 0 => return edit,
                _ => {}
            }
        }
        edit.selection.anchor = edit.before(at);
        edit.replace(Vec::new(), false);
        edit
    }

    /// Deletes the selection, or the character after the caret. At the end of a paragraph
    /// it joins the next one to it.
    pub fn delete(&self) -> Self {
        let mut edit = self.clamped();
        edit.ensure_para();
        if edit.selection.is_caret() {
            edit.selection.head = edit.after(edit.selection.head);
        }
        edit.replace(Vec::new(), false);
        edit
    }

    /// Ends the paragraph at the caret, after deleting the selection. In an empty list item
    /// it makes the item a body paragraph instead. Ending a heading at its end starts a body
    /// paragraph; anything else goes on as the kind it was.
    pub fn enter(&self) -> Self {
        let mut edit = self.clamped();
        edit.ensure_para();
        let was_caret = edit.selection.is_caret();
        let style = edit.style_for_typing();
        edit.replace(Vec::new(), false);
        let at = edit.selection.head;
        let para = &mut edit.paras[at.para];
        let is_list = matches!(para.kind, ParaKind::Bullet { .. } | ParaKind::Numbered { .. });
        if was_caret && is_list && para.spans.is_empty() {
            para.kind = ParaKind::Body;
            return edit;
        }
        let at_end = at.offset == text_len(para);
        let tail = if at_end && matches!(para.kind, ParaKind::Heading(_)) { ParaKind::Body } else { para.kind };
        let kind = para.kind;
        edit.replace(vec![Para { kind, spans: Vec::new() }, Para { kind: tail, spans: Vec::new() }], false);
        // A new empty paragraph goes on in the style the text before it had.
        if at_end && style != Style::default() {
            edit.typing_style = Some(style);
        }
        edit
    }

    /// Turns `bit` on over the selection, or off if all of it has it. With nothing
    /// selected, it's what's typed next that changes.
    pub fn toggle(&self, bit: StyleBit) -> Self {
        let edit = self.clamped();
        if edit.selection.is_caret() {
            let on = !bit.get(&edit.caret_style());
            return edit.restyle(|style| bit.set(style, on));
        }
        let on = !edit.selected_styles().iter().all(|style| bit.get(style));
        edit.restyle(|style| bit.set(style, on))
    }

    /// Gives the selection, or what's typed next, text size `size`, or the default size.
    pub fn set_size(&self, size: Option<u32>) -> Self {
        self.clamped().restyle(|style| style.size = size)
    }

    /// Gives the selection, or what's typed next, colour `color`, or the default colour.
    pub fn set_color(&self, color: Option<String>) -> Self {
        self.clamped().restyle(|style| style.color = color.clone())
    }

    /// Makes every paragraph the selection touches `kind`.
    pub fn set_kind(&self, kind: ParaKind) -> Self {
        let kind = clamp_kind(kind);
        self.clamped().rekind(|_| kind)
    }

    /// Takes every list item the selection touches a level deeper.
    pub fn indent(&self) -> Self {
        self.clamped().rekind(|kind| match kind {
            ParaKind::Bullet { level } | ParaKind::Numbered { level } => with_level(kind, (level + 1).min(MAX_LEVEL)),
            kind => kind,
        })
    }

    /// Brings every list item the selection touches a level up. One at the top level stays.
    pub fn outdent(&self) -> Self {
        self.clamped().rekind(|kind| match kind {
            ParaKind::Bullet { level } | ParaKind::Numbered { level } => with_level(kind, level.saturating_sub(1)),
            kind => kind,
        })
    }

    pub fn select_all(&self) -> Self {
        let mut edit = self.clamped();
        edit.typing_style = None;
        edit.selection = Selection { anchor: Pos::default(), head: edit.flow_end() };
        edit
    }

    /// Selects the word at `at`, or the space or punctuation there.
    pub fn select_word_at(&self, at: Pos) -> Self {
        let mut edit = self.clamped();
        edit.typing_style = None;
        let at = edit.clamp(at, false);
        edit.selection = Selection::caret(at);
        let Some(para) = edit.paras.get(at.para) else { return edit };
        let text = para_text(para);
        let word = text
            .split_word_bound_indices()
            .map(|(start, word)| start..start + word.len())
            .find(|word| word.contains(&at.offset) || word.end == text.len());
        if let Some(word) = word {
            edit.selection = Selection { anchor: Pos { para: at.para, offset: word.start }, head: Pos { para: at.para, offset: word.end } };
        }
        edit
    }

    /// Puts the caret at `at`, selecting from the anchor to it if `extend`.
    pub fn move_to(&self, at: Pos, extend: bool) -> Self {
        let mut edit = self.clamped();
        let at = edit.clamp(at, false);
        edit.place(at, extend);
        edit
    }

    /// Moves the caret by `motion`, selecting from the anchor to where it lands if `extend`.
    /// Left or Right on a selection, not extending it, puts the caret at its start or end.
    pub fn move_by(&self, motion: Motion, extend: bool) -> Self {
        let mut edit = self.clamped();
        if edit.paras.is_empty() {
            edit.typing_style = None;
            return edit;
        }
        let selection = edit.selection;
        let head = selection.head;
        let at = match motion {
            Motion::Left if !extend && !selection.is_caret() => selection.start(),
            Motion::Right if !extend && !selection.is_caret() => selection.end(),
            Motion::Left => edit.before(head),
            Motion::Right => edit.after(head),
            Motion::WordLeft => edit.word_before(head),
            Motion::WordRight => edit.word_after(head),
            Motion::ParaStart => Pos { para: head.para, offset: 0 },
            Motion::ParaEnd => Pos { para: head.para, offset: text_len(&edit.paras[head.para]) },
            Motion::FlowStart => Pos::default(),
            Motion::FlowEnd => edit.flow_end(),
        };
        edit.place(at, extend);
        edit
    }

    /// Puts `paras` in place of the selection. One paragraph goes into the one the selection
    /// starts in. More start in it, taking its kind unless nothing's before them in it, and
    /// the last is followed by what came after the selection.
    pub fn paste(&self, paras: &[Para]) -> Self {
        let mut edit = self.clamped();
        edit.ensure_para();
        let line_breaks = |span: &Span| Span { text: span.text.replace("\r\n", "\n").replace('\r', "\n"), style: span.style.clone() };
        let fragment = paras
            .iter()
            .map(|para| Para { kind: clamp_kind(para.kind), spans: tidy(para.spans.iter().map(line_breaks).collect()) })
            .collect();
        edit.replace(fragment, true);
        edit
    }

    /// The selected paragraphs, cut to the selection, or nothing for a caret.
    pub fn copied(&self) -> Vec<Para> {
        let edit = self.clamped();
        if edit.selection.is_caret() {
            return Vec::new();
        }
        let (start, end) = (edit.selection.start(), edit.selection.end());
        (start.para..=end.para)
            .map(|index| {
                let para = &edit.paras[index];
                let from = if index == start.para { start.offset } else { 0 };
                let to = if index == end.para { end.offset } else { text_len(para) };
                Para { kind: para.kind, spans: slice(&para.spans, from, to) }
            })
            .collect()
    }

    /// The selection's style and kind, for the toolbar.
    pub fn style_at(&self) -> StyleState {
        let edit = self.clamped();
        let kind = if edit.paras.is_empty() {
            Shared::None
        } else {
            let (start, end) = (edit.selection.start(), edit.selection.end());
            Shared::of(edit.paras[start.para..=end.para].iter().map(|para| para.kind))
        };
        let styles = if edit.selection.is_caret() { vec![edit.caret_style()] } else { edit.selected_styles() };
        StyleState {
            bold: Shared::of(styles.iter().map(|style| style.bold)),
            italic: Shared::of(styles.iter().map(|style| style.italic)),
            underline: Shared::of(styles.iter().map(|style| style.underline)),
            size: Shared::of(styles.iter().map(|style| style.size)),
            color: Shared::of(styles.iter().map(|style| style.color.clone())),
            kind,
        }
    }

    // What the operations are made of.

    /// A copy with tidy paragraphs and the selection moved into them.
    fn clamped(&self) -> Self {
        let mut edit = self.clone();
        edit.tidy();
        edit
    }

    fn tidy(&mut self) {
        for para in &mut self.paras {
            para.kind = clamp_kind(para.kind);
            if para.spans.iter().any(|span| span.text.is_empty()) || para.spans.windows(2).any(|pair| pair[0].style == pair[1].style) {
                para.spans = tidy(std::mem::take(&mut para.spans));
            }
        }
        self.selection = Selection { anchor: self.clamp(self.selection.anchor, false), head: self.clamp(self.selection.head, false) };
    }

    /// An empty flow is given one empty paragraph to type in.
    fn ensure_para(&mut self) {
        if self.paras.is_empty() {
            self.paras.push(Para { kind: ParaKind::Body, spans: Vec::new() });
            self.selection = Selection::default();
        }
    }

    /// `at` moved into the paragraphs and onto the edge of a character: back, or forward
    /// if `forward`.
    fn clamp(&self, at: Pos, forward: bool) -> Pos {
        let Some(last) = self.paras.len().checked_sub(1) else { return Pos::default() };
        let para = at.para.min(last);
        let text = para_text(&self.paras[para]);
        let edges = edges(&text);
        let offset = if forward {
            edges.iter().copied().find(|&edge| edge >= at.offset).unwrap_or(text.len())
        } else {
            edges.iter().copied().rev().find(|&edge| edge <= at.offset).unwrap_or(0)
        };
        Pos { para, offset }
    }

    fn flow_end(&self) -> Pos {
        match self.paras.len().checked_sub(1) {
            Some(last) => Pos { para: last, offset: text_len(&self.paras[last]) },
            None => Pos::default(),
        }
    }

    fn place(&mut self, at: Pos, extend: bool) {
        self.typing_style = None;
        self.selection = Selection { anchor: if extend { self.selection.anchor } else { at }, head: at };
    }

    /// The position a character before `at`, or the end of the paragraph before.
    fn before(&self, at: Pos) -> Pos {
        let text = para_text(&self.paras[at.para]);
        match edges(&text).into_iter().rev().find(|&edge| edge < at.offset) {
            Some(offset) => Pos { para: at.para, offset },
            None if at.para > 0 => Pos { para: at.para - 1, offset: text_len(&self.paras[at.para - 1]) },
            None => at,
        }
    }

    /// The position a character after `at`, or the start of the next paragraph.
    fn after(&self, at: Pos) -> Pos {
        let text = para_text(&self.paras[at.para]);
        match edges(&text).into_iter().find(|&edge| edge > at.offset) {
            Some(offset) => Pos { para: at.para, offset },
            None if at.para + 1 < self.paras.len() => Pos { para: at.para + 1, offset: 0 },
            None => at,
        }
    }

    /// The start of the word before `at`, or the end of the paragraph before.
    fn word_before(&self, at: Pos) -> Pos {
        if at.offset == 0 {
            return self.before(at);
        }
        let text = para_text(&self.paras[at.para]);
        let offset = text.split_word_bound_indices().rev().find(|(start, word)| *start < at.offset && is_word(word)).map(|(start, _)| start);
        Pos { para: at.para, offset: offset.unwrap_or(0) }
    }

    /// The end of the word after `at`, or the start of the next paragraph.
    fn word_after(&self, at: Pos) -> Pos {
        let text = para_text(&self.paras[at.para]);
        if at.offset >= text.len() {
            return self.after(at);
        }
        let offset = text
            .split_word_bound_indices()
            .map(|(start, word)| (start + word.len(), word))
            .find(|(end, word)| *end > at.offset && is_word(word))
            .map(|(end, _)| end);
        Pos { para: at.para, offset: offset.unwrap_or(text.len()) }
    }

    /// The style a caret types: the one chosen for it, or that of the character before it,
    /// or, at the start of a paragraph, after it.
    fn caret_style(&self) -> Style {
        if let Some(style) = &self.typing_style {
            return style.clone();
        }
        let at = self.selection.head;
        let Some(para) = self.paras.get(at.para) else { return Style::default() };
        style_around(para, at.offset).unwrap_or_default()
    }

    /// The style typing takes: a caret's, or that of the selection's first character.
    fn style_for_typing(&self) -> Style {
        if self.selection.is_caret() {
            return self.caret_style();
        }
        self.selected_styles().into_iter().next().unwrap_or_else(|| {
            let at = self.selection.start();
            style_around(&self.paras[at.para], at.offset).unwrap_or_default()
        })
    }

    /// The style of each piece of selected text, in order.
    fn selected_styles(&self) -> Vec<Style> {
        let (start, end) = (self.selection.start(), self.selection.end());
        let mut styles = Vec::new();
        for index in start.para..=end.para {
            let Some(para) = self.paras.get(index) else { break };
            let from = if index == start.para { start.offset } else { 0 };
            let to = if index == end.para { end.offset } else { usize::MAX };
            styles.extend(slice(&para.spans, from, to).into_iter().map(|span| span.style));
        }
        styles
    }

    /// Changes the style of the selected text by `change`, or, for a caret, the style
    /// what's typed next takes.
    fn restyle(mut self, change: impl Fn(&mut Style)) -> Self {
        if self.selection.is_caret() {
            let mut style = self.caret_style();
            change(&mut style);
            self.typing_style = Some(style);
            return self;
        }
        self.typing_style = None;
        let (start, end) = (self.selection.start(), self.selection.end());
        for index in start.para..=end.para {
            let para = &mut self.paras[index];
            let from = if index == start.para { start.offset } else { 0 };
            let to = if index == end.para { end.offset } else { text_len(para) };
            let len = text_len(para);
            let mut middle = slice(&para.spans, from, to);
            for span in &mut middle {
                change(&mut span.style);
            }
            let mut spans = slice(&para.spans, 0, from);
            spans.extend(middle);
            spans.extend(slice(&para.spans, to, len));
            para.spans = tidy(spans);
        }
        self
    }

    /// Changes the kind of every paragraph the selection touches by `change`.
    fn rekind(mut self, change: impl Fn(ParaKind) -> ParaKind) -> Self {
        if self.paras.is_empty() {
            return self;
        }
        let (start, end) = (self.selection.start(), self.selection.end());
        for para in &mut self.paras[start.para..=end.para] {
            para.kind = clamp_kind(change(para.kind));
        }
        self
    }

    /// Puts `fragment` in place of the selection, which must be in range, and puts the caret
    /// after it, moved forward off a character the fragment's end joined if `forward`.
    fn replace(&mut self, fragment: Vec<Para>, forward: bool) {
        self.typing_style = None;
        let (start, end) = (self.selection.start(), self.selection.end());
        let kind = self.paras[start.para].kind;
        let head = slice(&self.paras[start.para].spans, 0, start.offset);
        let tail = slice(&self.paras[end.para].spans, end.offset, usize::MAX);
        let count = fragment.len();
        let mut fragment = fragment.into_iter();
        let (new, caret) = match (fragment.next(), fragment.next_back()) {
            (None, _) => (vec![Para { kind, spans: join(head, tail) }], start),
            (Some(only), None) => {
                let caret = Pos { para: start.para, offset: start.offset + spans_len(&only.spans) };
                (vec![Para { kind, spans: join(join(head, only.spans), tail) }], caret)
            }
            (Some(first), Some(last)) => {
                let caret = Pos { para: start.para + count - 1, offset: spans_len(&last.spans) };
                let first_kind = if head.is_empty() { first.kind } else { kind };
                let mut new = vec![Para { kind: first_kind, spans: join(head, first.spans) }];
                new.extend(fragment);
                new.push(Para { kind: last.kind, spans: join(last.spans, tail) });
                (new, caret)
            }
        };
        self.paras.splice(start.para..=end.para, new);
        let caret = self.clamp(caret, forward);
        self.selection = Selection::caret(caret);
    }
}

fn with_level(kind: ParaKind, level: u8) -> ParaKind {
    match kind {
        ParaKind::Bullet { .. } => ParaKind::Bullet { level },
        ParaKind::Numbered { .. } => ParaKind::Numbered { level },
        kind => kind,
    }
}

/// `kind` with its level in range: a list's 0 to 8, a heading's 1 to 6.
fn clamp_kind(kind: ParaKind) -> ParaKind {
    match kind {
        ParaKind::Bullet { level } | ParaKind::Numbered { level } => with_level(kind, level.min(MAX_LEVEL)),
        ParaKind::Heading(level) => ParaKind::Heading(level.clamp(1, 6)),
        ParaKind::Body => ParaKind::Body,
    }
}

fn is_word(text: &str) -> bool {
    text.chars().any(char::is_alphanumeric)
}

fn para_text(para: &Para) -> String {
    para.spans.iter().map(|span| span.text.as_str()).collect()
}

fn spans_len(spans: &[Span]) -> usize {
    spans.iter().map(|span| span.text.len()).sum()
}

fn text_len(para: &Para) -> usize {
    spans_len(&para.spans)
}

/// The offsets in `text` a caret may be at: the edges of its characters as they're drawn.
fn edges(text: &str) -> Vec<usize> {
    text.grapheme_indices(true).map(|(at, _)| at).chain([text.len()]).collect()
}

/// A span of `text` in `style`, or none for no text.
fn span(text: &str, style: &Style) -> Vec<Span> {
    if text.is_empty() { Vec::new() } else { vec![Span { text: text.to_string(), style: style.clone() }] }
}

/// The style of the character before `offset` in `para`, or, if there's none, after it.
fn style_around(para: &Para, offset: usize) -> Option<Style> {
    let mut start = 0;
    let mut after = None;
    for span in &para.spans {
        let end = start + span.text.len();
        if start < offset && offset <= end {
            return Some(span.style.clone());
        }
        if after.is_none() && offset <= start {
            after = Some(span.style.clone());
        }
        start = end;
    }
    after
}

/// The part of `spans` from byte `from` to byte `to` of their text, the ends moved back
/// onto the edges of characters.
fn slice(spans: &[Span], from: usize, to: usize) -> Vec<Span> {
    let mut out = Vec::new();
    let mut start = 0;
    for span in spans {
        let end = start + span.text.len();
        let (a, b) = (from.clamp(start, end) - start, to.clamp(start, end) - start);
        if a < b {
            let text = &span.text;
            let (a, b) = (floor_char(text, a), floor_char(text, b));
            if a < b {
                out.push(Span { text: text[a..b].to_string(), style: span.style.clone() });
            }
        }
        start = end;
    }
    out
}

fn floor_char(text: &str, mut at: usize) -> usize {
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

fn join(mut left: Vec<Span>, right: Vec<Span>) -> Vec<Span> {
    left.extend(right);
    tidy(left)
}

/// `spans` with the empty ones dropped and neighbours of one style merged.
fn tidy(spans: Vec<Span>) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::with_capacity(spans.len());
    for span in spans {
        if span.text.is_empty() {
            continue;
        }
        match out.last_mut() {
            Some(last) if last.style == span.style => last.text.push_str(&span.text),
            _ => out.push(span),
        }
    }
    out
}
