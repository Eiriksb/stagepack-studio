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
    /// style was chosen for it, or Enter at the end of styled text carried that style on.
    /// It lasts through changes of kind and level, Enter and a line break, and is used up
    /// by typing or pasting. Moving the caret, selecting, Backspace and Delete forget it.
    pub typing_style: Option<Style>,
}

impl TextEdit {
    /// An edit of `paras`, the caret at the start.
    pub fn new(paras: Vec<Para>) -> Self {
        let mut edit = TextEdit { paras, selection: Selection::default(), typing_style: None };
        edit.tidy();
        edit
    }

    /// Puts `text` in place of the selection. Each newline or paragraph separator in it
    /// starts a paragraph of the kind the selection starts in; a line separator is a line
    /// break inside the paragraph. Other control characters are dropped. What's typed takes
    /// the style before the caret, or, over a selection, the style of its first character.
    pub fn insert(&self, text: &str) -> Self {
        let mut edit = self.clamped();
        if text.is_empty() && edit.selection.is_caret() {
            return edit;
        }
        edit.ensure_para();
        let style = edit.style_for_typing();
        let kind = edit.paras[edit.selection.start().para].kind;
        let text = text.replace("\r\n", "\n").replace(['\r', PARA_SEPARATOR], "\n");
        let fragment = text.split('\n').map(|line| Para { kind, spans: span(&clean_line(line), &style) }).collect();
        edit.replace(fragment, true);
        edit
    }

    /// A line break inside the paragraph, in place of the selection.
    pub fn soft_break(&self) -> Self {
        let mut edit = self.clamped();
        edit.ensure_para();
        let pending = edit.pending();
        let style = edit.style_for_typing();
        let kind = edit.paras[edit.selection.start().para].kind;
        edit.replace(vec![Para { kind, spans: span("\n", &style) }], true);
        edit.typing_style = pending;
        edit
    }

    /// Deletes the selection, or the character before the caret. At the start of a list
    /// item it outdents it, or, at the top level, makes it a body paragraph; at the start
    /// of anything else it joins the paragraph to the one before. An empty flow stays as
    /// it is.
    pub fn backspace(&self) -> Self {
        let mut edit = self.clamped();
        if edit.paras.is_empty() {
            return edit;
        }
        edit.typing_style = None;
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
    /// it joins the next one to it. An empty flow stays as it is.
    pub fn delete(&self) -> Self {
        let mut edit = self.clamped();
        if edit.paras.is_empty() {
            return edit;
        }
        if edit.selection.is_caret() {
            edit.selection.head = edit.after(edit.selection.head);
        }
        edit.replace(Vec::new(), false);
        edit
    }

    /// Ends the paragraph at the caret, after deleting the selection. In an empty list item
    /// it makes the item a body paragraph instead. Ending a heading at its end starts a body
    /// paragraph; anything else goes on as the kind it was. A style chosen for typing lasts,
    /// and, at the end of styled text, the new paragraph goes on in that text's style.
    pub fn enter(&self) -> Self {
        let mut edit = self.clamped();
        edit.ensure_para();
        let was_caret = edit.selection.is_caret();
        let pending = edit.pending();
        let style = edit.style_for_typing();
        edit.replace(Vec::new(), false);
        edit.typing_style = pending.clone();
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
        edit.typing_style = pending.or_else(|| (at_end && style != Style::default()).then_some(style));
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

    /// Makes every paragraph the selection touches a bulleted list item, or a numbered one
    /// with `numbered`, keeping the level of one that's a list item already. If all of them
    /// are that kind of item already, they become body paragraphs instead.
    pub fn toggle_list(&self, numbered: bool) -> Self {
        let edit = self.clamped();
        let is_one = |kind: ParaKind| match kind {
            ParaKind::Bullet { .. } => !numbered,
            ParaKind::Numbered { .. } => numbered,
            _ => false,
        };
        let (start, end) = (edit.selection.start(), edit.selection.end());
        let all = edit.paras.get(start.para..=end.para).is_some_and(|paras| paras.iter().all(|para| is_one(para.kind)));
        edit.rekind(|kind| {
            let level = match kind {
                ParaKind::Bullet { level } | ParaKind::Numbered { level } => level,
                _ => 0,
            };
            match (all, numbered) {
                (true, _) => ParaKind::Body,
                (false, true) => ParaKind::Numbered { level },
                (false, false) => ParaKind::Bullet { level },
            }
        })
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

    /// Selects the word at `at`. At the end of a paragraph, or on spaces or punctuation, it
    /// selects the word before them in the paragraph, or, with none, the spaces or
    /// punctuation themselves.
    pub fn select_word_at(&self, at: Pos) -> Self {
        let mut edit = self.clamped();
        edit.typing_style = None;
        let at = edit.clamp(at, false);
        edit.selection = Selection::caret(at);
        let Some(para) = edit.paras.get(at.para) else { return edit };
        let text = para_text(para);
        let words: Vec<(usize, &str)> = text.split_word_bound_indices().collect();
        let Some(under) = words.iter().position(|(start, word)| at.offset < start + word.len()).or(words.len().checked_sub(1)) else {
            return edit;
        };
        let chosen = words[..=under].iter().rev().find(|(_, word)| is_word(word)).unwrap_or(&words[under]);
        let edges = edges(&text);
        let (start, end) = (floor_edge(&edges, chosen.0), ceil_edge(&edges, chosen.0 + chosen.1.len()));
        edit.selection = Selection { anchor: Pos { para: at.para, offset: start }, head: Pos { para: at.para, offset: end } };
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
    /// starts in, and gives it its kind if that's left empty. More start in it, taking its
    /// kind unless nothing's before them in it, and the last is followed by what came after
    /// the selection. Line endings become line breaks, a paragraph separator ends a
    /// paragraph, and other control characters are dropped.
    pub fn paste(&self, paras: &[Para]) -> Self {
        let mut edit = self.clamped();
        edit.ensure_para();
        let fragment = paras.iter().flat_map(clean_para).collect();
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
        let offset = if forward { ceil_edge(&edges, at.offset) } else { floor_edge(&edges, at.offset) };
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
        Pos { para: at.para, offset: floor_edge(&edges(&text), offset.unwrap_or(0)) }
    }

    /// The start of the word after `at`, or, with none, the end of the paragraph, or, from
    /// there, the start of the next one.
    fn word_after(&self, at: Pos) -> Pos {
        let text = para_text(&self.paras[at.para]);
        if at.offset >= text.len() {
            return self.after(at);
        }
        let offset = text.split_word_bound_indices().find(|(start, word)| *start > at.offset && is_word(word)).map(|(start, _)| start);
        Pos { para: at.para, offset: ceil_edge(&edges(&text), offset.unwrap_or(text.len())) }
    }

    /// The style chosen for what a caret types next, if one was.
    fn pending(&self) -> Option<Style> {
        if self.selection.is_caret() { self.typing_style.clone() } else { None }
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
                let kind = if head.is_empty() && tail.is_empty() { only.kind } else { kind };
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

/// The last of `edges` at or before `at`.
fn floor_edge(edges: &[usize], at: usize) -> usize {
    edges.iter().copied().rev().find(|&edge| edge <= at).unwrap_or(0)
}

/// The first of `edges` at or after `at`.
fn ceil_edge(edges: &[usize], at: usize) -> usize {
    edges.iter().copied().find(|&edge| edge >= at).unwrap_or(edges.last().copied().unwrap_or(0))
}

const LINE_SEPARATOR: char = '\u{2028}';
const PARA_SEPARATOR: char = '\u{2029}';
const BYTE_ORDER_MARK: char = '\u{feff}';

/// Whether `c` is a control character the model never holds: a byte order mark, a control
/// character other than tab and newline, or a bidirectional embedding, override or isolate,
/// which could make text read otherwise than it is. Joiners and direction marks are kept, as
/// emoji and several scripts need them.
pub(crate) fn is_dropped(c: char) -> bool {
    c == BYTE_ORDER_MARK || is_bidi_control(c) || (c.is_control() && c != '\t' && c != '\n')
}

/// Whether `c` is a bidirectional embedding, override or isolate: U+202A to U+202E, or U+2066
/// to U+2069.
pub fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// A line of typed text as the model holds it: a line separator a line break, other
/// control characters dropped.
fn clean_line(line: &str) -> String {
    line.chars().filter(|&c| !is_dropped(c)).map(|c| if c == LINE_SEPARATOR { '\n' } else { c }).collect()
}

/// A pasted paragraph as the model holds it, its kind in range: line endings, across
/// spans as well, and line separators made line breaks, other control characters dropped,
/// and split where it holds a paragraph separator.
fn clean_para(para: &Para) -> Vec<Para> {
    let kind = clamp_kind(para.kind);
    let mut out = Vec::new();
    let mut spans = Vec::new();
    let mut after_cr = false;
    for span in &para.spans {
        let mut text = String::new();
        for c in span.text.chars() {
            let skip = c == '\n' && after_cr;
            after_cr = c == '\r';
            match c {
                _ if skip => {}
                '\r' | '\n' | LINE_SEPARATOR => text.push('\n'),
                PARA_SEPARATOR => {
                    spans.push(Span { text: std::mem::take(&mut text), style: span.style.clone() });
                    out.push(Para { kind, spans: tidy(std::mem::take(&mut spans)) });
                }
                c if is_dropped(c) => {}
                c => text.push(c),
            }
        }
        spans.push(Span { text, style: span.style.clone() });
    }
    out.push(Para { kind, spans: tidy(spans) });
    out
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
