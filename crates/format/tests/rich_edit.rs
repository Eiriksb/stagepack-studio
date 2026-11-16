use proptest::prelude::*;
use slides_format::*;
use unicode_segmentation::UnicodeSegmentation;

fn plain(text: &str) -> Span {
    Span { text: text.to_string(), style: Style::default() }
}

fn styled(text: &str, style: Style) -> Span {
    Span { text: text.to_string(), style }
}

fn bold() -> Style {
    Style { bold: true, ..Style::default() }
}

fn italic() -> Style {
    Style { italic: true, ..Style::default() }
}

fn para(kind: ParaKind, text: &str) -> Para {
    let spans = if text.is_empty() { Vec::new() } else { vec![plain(text)] };
    Para { kind, spans }
}

fn body(text: &str) -> Para {
    para(BODY, text)
}

const BODY: ParaKind = ParaKind::Body;
const BULLET: ParaKind = ParaKind::Bullet { level: 0 };
const NUMBERED: ParaKind = ParaKind::Numbered { level: 0 };
const HEADING: ParaKind = ParaKind::Heading(1);

fn pos(para: usize, offset: usize) -> Pos {
    Pos { para, offset }
}

fn caret(para: usize, offset: usize) -> Selection {
    Selection { anchor: pos(para, offset), head: pos(para, offset) }
}

fn range(anchor: Pos, head: Pos) -> Selection {
    Selection { anchor, head }
}

/// An edit of `paras` with `selection`.
fn at(paras: Vec<Para>, selection: Selection) -> TextEdit {
    TextEdit { paras, selection, typing_style: None }
}

/// The paragraphs' text, one string each.
fn texts(edit: &TextEdit) -> Vec<String> {
    edit.paras.iter().map(|para| para.spans.iter().map(|span| span.text.as_str()).collect()).collect()
}

fn kinds(edit: &TextEdit) -> Vec<ParaKind> {
    edit.paras.iter().map(|para| para.kind).collect()
}

fn boundaries(text: &str) -> Vec<usize> {
    text.grapheme_indices(true).map(|(at, _)| at).chain([text.len()]).collect()
}

/// Panics, saying what's wrong, unless `edit` is well formed.
fn assert_well_formed(edit: &TextEdit) {
    for para in &edit.paras {
        for span in &para.spans {
            assert!(!span.text.is_empty(), "an empty span in {para:?}");
        }
        for pair in para.spans.windows(2) {
            assert_ne!(pair[0].style, pair[1].style, "unmerged neighbours in {para:?}");
        }
        match para.kind {
            ParaKind::Bullet { level } | ParaKind::Numbered { level } => assert!(level <= 8, "{para:?}"),
            ParaKind::Heading(level) => assert!((1..=6).contains(&level), "{para:?}"),
            ParaKind::Body => {}
        }
    }
    let texts = texts(edit);
    for end in [edit.selection.anchor, edit.selection.head] {
        if texts.is_empty() {
            assert_eq!(end, pos(0, 0));
            continue;
        }
        assert!(end.para < texts.len(), "{end:?} past the paragraphs");
        assert!(boundaries(&texts[end.para]).contains(&end.offset), "{end:?} inside a character of {:?}", texts[end.para]);
    }
}

// Typing.

#[test]
fn typing_puts_text_at_the_caret() {
    let edit = at(vec![body("helo")], caret(0, 3)).insert("l");
    assert_eq!(texts(&edit), ["hello"]);
    assert_eq!(edit.selection, caret(0, 4));
}

#[test]
fn typing_replaces_the_selection() {
    let edit = at(vec![body("hello world")], range(pos(0, 6), pos(0, 11))).insert("there");
    assert_eq!(texts(&edit), ["hello there"]);
    assert_eq!(edit.selection, caret(0, 11));
}

#[test]
fn newlines_in_typed_text_split_paragraphs() {
    let edit = at(vec![para(BULLET, "ad")], caret(0, 1)).insert("b\nc");
    assert_eq!(texts(&edit), ["ab", "cd"]);
    assert_eq!(kinds(&edit), [BULLET, BULLET]);
    assert_eq!(edit.selection, caret(1, 1));
    let edit = at(vec![body("ab")], caret(0, 1)).insert("x\r\ny\rz");
    assert_eq!(texts(&edit), ["ax", "y", "zb"]);
}

#[test]
fn typing_takes_the_style_before_the_caret() {
    let paras = vec![Para { kind: BODY, spans: vec![styled("ab", bold()), plain("cd")] }];
    let edit = at(paras.clone(), caret(0, 2)).insert("x");
    assert_eq!(edit.paras[0].spans, [styled("abx", bold()), plain("cd")]);
    let edit = at(paras.clone(), caret(0, 3)).insert("x");
    assert_eq!(edit.paras[0].spans, [styled("ab", bold()), plain("cxd")]);
    // At the start of a paragraph there's nothing before, so it takes what's after.
    let edit = at(paras, caret(0, 0)).insert("x");
    assert_eq!(edit.paras[0].spans, [styled("xab", bold()), plain("cd")]);
}

#[test]
fn typing_over_a_selection_takes_the_style_of_its_first_character() {
    let paras = vec![Para { kind: BODY, spans: vec![plain("ab"), styled("cd", bold())] }];
    let edit = at(paras, range(pos(0, 4), pos(0, 2))).insert("x");
    assert_eq!(edit.paras[0].spans, [plain("ab"), styled("x", bold())]);
}

#[test]
fn a_soft_break_stays_in_the_paragraph() {
    let edit = at(vec![body("ab")], caret(0, 1)).soft_break();
    assert_eq!(texts(&edit), ["a\nb"]);
    assert_eq!(edit.selection, caret(0, 2));
    let edit = at(vec![body("ab")], caret(0, 2)).soft_break();
    assert_eq!(texts(&edit), ["ab\n"]);
}

// Enter.

#[test]
fn enter_splits_a_paragraph_and_keeps_its_kind() {
    let edit = at(vec![para(NUMBERED, "abcd")], caret(0, 2)).enter();
    assert_eq!(texts(&edit), ["ab", "cd"]);
    assert_eq!(kinds(&edit), [NUMBERED, NUMBERED]);
    assert_eq!(edit.selection, caret(1, 0));
}

#[test]
fn enter_at_the_end_makes_an_empty_paragraph() {
    let edit = at(vec![body("ab")], caret(0, 2)).enter();
    assert_eq!(edit.paras, [body("ab"), body("")]);
    assert_eq!(edit.selection, caret(1, 0));
    let edit = at(vec![para(BULLET, "ab")], caret(0, 2)).enter();
    assert_eq!(kinds(&edit), [BULLET, BULLET]);
}

#[test]
fn enter_in_an_empty_bullet_or_number_makes_it_a_body_paragraph() {
    for kind in [BULLET, NUMBERED, ParaKind::Bullet { level: 3 }] {
        let edit = at(vec![para(BULLET, "a"), para(kind, "")], caret(1, 0)).enter();
        assert_eq!(edit.paras, [para(BULLET, "a"), body("")]);
        assert_eq!(edit.selection, caret(1, 0));
    }
}

#[test]
fn enter_at_the_end_of_a_heading_starts_a_body_paragraph() {
    let edit = at(vec![para(HEADING, "Title")], caret(0, 5)).enter();
    assert_eq!(edit.paras, [para(HEADING, "Title"), body("")]);
    let edit = at(vec![para(HEADING, "Title")], caret(0, 2)).enter();
    assert_eq!(kinds(&edit), [HEADING, HEADING]);
}

#[test]
fn enter_replaces_the_selection() {
    let edit = at(vec![body("abc"), body("def")], range(pos(0, 1), pos(1, 2))).enter();
    assert_eq!(texts(&edit), ["a", "f"]);
    assert_eq!(edit.selection, caret(1, 0));
}

// Backspace and Delete.

#[test]
fn backspace_deletes_the_character_before() {
    let edit = at(vec![body("abc")], caret(0, 2)).backspace();
    assert_eq!(texts(&edit), ["ac"]);
    assert_eq!(edit.selection, caret(0, 1));
}

#[test]
fn backspace_deletes_the_selection() {
    let edit = at(vec![body("abc"), body("def"), body("ghi")], range(pos(2, 1), pos(0, 1))).backspace();
    assert_eq!(texts(&edit), ["ahi"]);
    assert_eq!(edit.selection, caret(0, 1));
}

#[test]
fn backspace_at_the_start_joins_to_the_paragraph_before() {
    let edit = at(vec![para(HEADING, "ab"), body("cd")], caret(1, 0)).backspace();
    assert_eq!(edit.paras, [para(HEADING, "abcd")]);
    assert_eq!(edit.selection, caret(0, 2));
}

#[test]
fn backspace_at_the_start_of_a_bullet_outdents_then_unlists_then_joins() {
    let edit = at(vec![body("ab"), para(ParaKind::Bullet { level: 1 }, "cd")], caret(1, 0));
    let edit = edit.backspace();
    assert_eq!(kinds(&edit), [BODY, BULLET]);
    let edit = edit.backspace();
    assert_eq!(kinds(&edit), [BODY, BODY]);
    assert_eq!(texts(&edit), ["ab", "cd"]);
    let edit = edit.backspace();
    assert_eq!(edit.paras, [body("abcd")]);
    let edit = at(vec![para(NUMBERED, "x")], caret(0, 0)).backspace();
    assert_eq!(edit.paras, [body("x")]);
}

#[test]
fn delete_deletes_the_character_after() {
    let edit = at(vec![body("abc")], caret(0, 1)).delete();
    assert_eq!(texts(&edit), ["ac"]);
    assert_eq!(edit.selection, caret(0, 1));
}

#[test]
fn delete_at_the_end_joins_the_next_paragraph() {
    let edit = at(vec![para(BULLET, "ab"), para(HEADING, "cd")], caret(0, 2)).delete();
    assert_eq!(edit.paras, [para(BULLET, "abcd")]);
    assert_eq!(edit.selection, caret(0, 2));
}

#[test]
fn deleting_keeps_spans_tidy() {
    let paras = vec![Para { kind: BODY, spans: vec![plain("a"), styled("b", bold()), plain("c")] }];
    let edit = at(paras, caret(0, 2)).backspace();
    assert_eq!(edit.paras[0].spans, [plain("ac")]);
}

#[test]
fn edges_never_lose_or_invent_text() {
    let edit = at(vec![body("ab"), body("cd")], caret(0, 0)).backspace();
    assert_eq!(edit.paras, [body("ab"), body("cd")]);
    assert_eq!(edit.selection, caret(0, 0));

    let edit = at(vec![body("ab"), body("cd")], caret(1, 2)).delete();
    assert_eq!(edit.paras, [body("ab"), body("cd")]);
    assert_eq!(edit.selection, caret(1, 2));

    let edit = at(Vec::new(), caret(0, 0)).enter();
    assert_eq!(edit.paras, [body(""), body("")]);
    assert_eq!(edit.selection, caret(1, 0));
    let edit = at(Vec::new(), caret(0, 0)).backspace().delete();
    assert!(texts(&edit).concat().is_empty());

    let edit = at(vec![body("abc"), para(BULLET, "def"), body("ghi")], range(pos(0, 1), pos(2, 2))).insert("x");
    assert_eq!(edit.paras, [body("axi")]);
    assert_eq!(edit.selection, caret(0, 2));

    let pasted = [para(BULLET, "one"), para(HEADING, "two"), para(NUMBERED, "three")];
    let edit = at(vec![body("word")], caret(0, 2)).paste(&pasted);
    assert_eq!(edit.paras, [body("woone"), para(HEADING, "two"), para(NUMBERED, "threerd")]);
    assert_eq!(edit.selection, caret(2, 5));
}

// Graphemes.

const COMBINING: &str = "e\u{301}";
const FAMILY: &str = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
const FLAG: &str = "\u{1F1F3}\u{1F1F4}";
const SURROGATE_PAIR: &str = "\u{1F600}";

#[test]
fn carets_never_land_inside_a_character() {
    for grapheme in [COMBINING, FAMILY, FLAG, SURROGATE_PAIR] {
        let text = format!("a{grapheme}b");
        let after = 1 + grapheme.len();
        let edit = || at(vec![body(&text)], caret(0, after));

        assert_eq!(edit().move_by(Motion::Left, false).selection, caret(0, 1), "{grapheme:?}");
        assert_eq!(edit().move_by(Motion::Left, false).move_by(Motion::Right, false).selection, caret(0, after));
        let extended = edit().move_by(Motion::Left, true);
        assert_eq!(extended.selection, range(pos(0, after), pos(0, 1)));

        let gone = edit().backspace();
        assert_eq!(texts(&gone), ["ab"], "{grapheme:?}");
        assert_eq!(gone.selection, caret(0, 1));
        let gone = at(vec![body(&text)], caret(0, 1)).delete();
        assert_eq!(texts(&gone), ["ab"], "{grapheme:?}");

        // A position inside the character is moved off it.
        for inside in 2..after {
            let edit = at(vec![body(&text)], caret(0, inside)).move_to(pos(0, inside), false);
            assert_well_formed(&edit);
            let edit = at(vec![body(&text)], caret(0, inside)).insert("x");
            assert_well_formed(&edit);
            assert_eq!(texts(&edit).concat().replace('x', ""), text);
        }
    }
}

#[test]
fn a_character_that_joins_the_one_before_leaves_the_caret_after_both() {
    // A combining accent at the start of a paragraph, then a letter typed before it.
    let edit = at(vec![body("\u{301}b")], caret(0, 0)).insert("e");
    assert_eq!(texts(&edit), [format!("{COMBINING}b")]);
    assert_eq!(edit.selection, caret(0, COMBINING.len()));
    assert_well_formed(&edit);
}

// Styling.

#[test]
fn a_toggle_styles_the_selection() {
    let edit = at(vec![body("abcd")], range(pos(0, 1), pos(0, 3))).toggle(StyleBit::Bold);
    assert_eq!(edit.paras[0].spans, [plain("a"), styled("bc", bold()), plain("d")]);
    assert_eq!(edit.selection, range(pos(0, 1), pos(0, 3)));
    let edit = edit.toggle(StyleBit::Bold);
    assert_eq!(edit.paras[0].spans, [plain("abcd")]);
}

#[test]
fn a_toggle_over_mixed_text_turns_it_on() {
    let paras = vec![Para { kind: BODY, spans: vec![styled("ab", bold()), plain("cd")] }];
    let edit = at(paras, range(pos(0, 0), pos(0, 4))).toggle(StyleBit::Bold);
    assert_eq!(edit.paras[0].spans, [styled("abcd", bold())]);
}

#[test]
fn a_toggle_reaches_every_selected_paragraph() {
    let edit = at(vec![body("ab"), body("cd")], range(pos(0, 1), pos(1, 1))).toggle(StyleBit::Italic);
    assert_eq!(edit.paras[0].spans, [plain("a"), styled("b", italic())]);
    assert_eq!(edit.paras[1].spans, [styled("c", italic()), plain("d")]);
}

#[test]
fn a_toggle_with_no_selection_styles_what_is_typed_next() {
    let edit = at(vec![body("ab")], caret(0, 1)).toggle(StyleBit::Bold);
    assert_eq!(edit.paras, [body("ab")]);
    assert_eq!(edit.typing_style, Some(bold()));
    let edit = edit.insert("x").insert("y");
    assert_eq!(edit.paras[0].spans, [plain("a"), styled("xy", bold()), plain("b")]);
    // Moving forgets it.
    let edit = at(vec![body("ab")], caret(0, 1)).toggle(StyleBit::Bold).move_by(Motion::Right, false).insert("x");
    assert_eq!(edit.paras[0].spans, [plain("abx")]);
}

#[test]
fn size_and_colour_style_the_selection_or_what_is_typed_next() {
    let edit = at(vec![body("ab")], range(pos(0, 0), pos(0, 1))).set_size(Some(40)).set_color(Some("red".into()));
    let style = Style { size: Some(40), color: Some("red".into()), ..Style::default() };
    assert_eq!(edit.paras[0].spans, [styled("a", style.clone()), plain("b")]);
    let edit = edit.set_size(None);
    assert_eq!(edit.paras[0].spans[0].style, Style { color: Some("red".into()), ..Style::default() });
    let edit = at(vec![body("ab")], caret(0, 2)).set_size(Some(12)).insert("c");
    assert_eq!(edit.paras[0].spans, [plain("ab"), styled("c", Style { size: Some(12), ..Style::default() })]);
}

#[test]
fn style_at_says_what_the_caret_types() {
    let paras = vec![Para { kind: BULLET, spans: vec![styled("ab", bold()), plain("cd")] }];
    let state = at(paras.clone(), caret(0, 1)).style_at();
    assert_eq!(state.bold, Shared::All(true));
    assert_eq!(state.italic, Shared::All(false));
    assert_eq!(state.size, Shared::All(None));
    assert_eq!(state.color, Shared::All(None));
    assert_eq!(state.kind, Shared::All(BULLET));
    let state = at(paras, caret(0, 1)).toggle(StyleBit::Bold).style_at();
    assert_eq!(state.bold, Shared::All(false));
}

#[test]
fn style_at_says_mixed_when_it_is() {
    let red = Style { color: Some("red".into()), bold: true, ..Style::default() };
    let paras = vec![Para { kind: BODY, spans: vec![styled("ab", red), styled("cd", bold())] }, para(HEADING, "ef")];
    let state = at(paras.clone(), range(pos(0, 0), pos(0, 4))).style_at();
    assert_eq!(state.bold, Shared::All(true));
    assert_eq!(state.color, Shared::Mixed);
    assert_eq!(state.kind, Shared::All(BODY));
    let state = at(paras.clone(), range(pos(0, 3), pos(1, 1))).style_at();
    assert_eq!(state.bold, Shared::Mixed);
    assert_eq!(state.color, Shared::All(None));
    assert_eq!(state.kind, Shared::Mixed);
    // A selection holding no text has no style to show.
    let state = at(vec![body(""), body("")], range(pos(0, 0), pos(1, 0))).style_at();
    assert_eq!(state.bold, Shared::None);
    assert_eq!(state.kind, Shared::All(BODY));
}

/// Panics unless every list item `op` gave a level to, by making or changing it, is no more
/// than a level deeper than the item before it, and at the top level after anything else.
fn assert_levels_fit(before: &TextEdit, op: &Op, after: &TextEdit) {
    let given = match op {
        Op::SetKind(_) | Op::Indent | Op::Outdent => {
            let (start, end) = (before.selection.start().para, before.selection.end().para);
            start..(end + 1).min(after.paras.len())
        }
        Op::Paste(_) => {
            let (start, end) = (before.selection.start().para, before.selection.end().para);
            let replaced = if before.paras.is_empty() { 0 } else { end - start + 1 };
            start..start + after.paras.len() + replaced - before.paras.len()
        }
        _ => return,
    };
    let level = |kind: ParaKind| match kind {
        ParaKind::Bullet { level } | ParaKind::Numbered { level } => Some(level),
        _ => None,
    };
    for index in given {
        let Some(at) = level(after.paras[index].kind) else { continue };
        let most = index.checked_sub(1).and_then(|before| level(after.paras[before].kind)).map_or(0, |above| above + 1);
        assert!(at <= most, "{op:?} made item {index} too deep in {:?}", after.paras);
    }
}

// Kinds and levels.

#[test]
fn set_kind_changes_every_paragraph_the_selection_touches() {
    let edit = at(vec![body("a"), body("b"), body("c")], range(pos(1, 0), pos(0, 1))).set_kind(NUMBERED);
    assert_eq!(kinds(&edit), [NUMBERED, NUMBERED, BODY]);
    let deep: Vec<Para> = (0..=8).map(|level| para(ParaKind::Bullet { level }, "a")).chain([body("b")]).collect();
    let edit = at(deep, caret(9, 0)).set_kind(ParaKind::Bullet { level: 20 });
    assert_eq!(kinds(&edit)[9], ParaKind::Bullet { level: 8 });
}

#[test]
fn a_list_item_is_never_made_deeper_than_the_one_before_it_allows() {
    let level = |level| ParaKind::Bullet { level };
    // A first item, empty or not, and one after a body paragraph, stay at the top level.
    assert_eq!(kinds(&at(vec![para(BULLET, "")], caret(0, 0)).indent()), [BULLET]);
    assert_eq!(kinds(&TextEdit::new(vec![body("")]).toggle_list(false).indent()), [BULLET]);
    assert_eq!(kinds(&at(vec![body("a"), para(BULLET, "b")], caret(1, 0)).indent()), [BODY, BULLET]);
    // One under another goes a level deeper than it, and no further.
    let two = at(vec![para(BULLET, "a"), para(BULLET, "b")], caret(1, 0));
    assert_eq!(kinds(&two.indent().indent()), [BULLET, level(1)]);
    // Made items at once, each one goes no deeper than the one before allows.
    let three = at(vec![body("a"), body("b"), body("c")], range(pos(0, 0), pos(2, 1)));
    assert_eq!(kinds(&three.set_kind(level(3))), [BULLET, level(1), level(2)]);
    // Pasted items too, wherever they go.
    let pasted = [para(level(2), "x"), para(level(5), "y")];
    assert_eq!(kinds(&at(vec![body("")], caret(0, 0)).paste(&pasted)), [BULLET, level(1)]);
    assert_eq!(kinds(&at(vec![para(BULLET, "a"), body("")], caret(1, 0)).paste(&pasted)), [BULLET, level(1), level(2)]);
}

#[test]
fn indent_and_outdent_change_list_levels_from_0_to_8() {
    let list = vec![para(BULLET, "top"), para(BULLET, "a"), body("b"), para(NUMBERED, "c"), para(NUMBERED, "d")];
    let edit = at(list, range(pos(1, 0), pos(4, 0))).indent();
    assert_eq!(kinds(&edit), [BULLET, ParaKind::Bullet { level: 1 }, BODY, NUMBERED, ParaKind::Numbered { level: 1 }]);
    let edit = edit.outdent().outdent();
    assert_eq!(kinds(&edit), [BULLET, BULLET, BODY, NUMBERED, NUMBERED]);
    let mut edit = at((0..10).map(|_| para(BULLET, "a")).collect(), range(pos(0, 0), pos(9, 1)));
    for _ in 0..12 {
        edit = edit.indent();
    }
    let levels: Vec<u8> = (0..=8).chain([8]).collect();
    assert_eq!(kinds(&edit), levels.into_iter().map(|level| ParaKind::Bullet { level }).collect::<Vec<_>>());
}

#[test]
fn toggle_list_makes_the_paragraphs_a_list_keeping_levels_and_back() {
    let two = ParaKind::Numbered { level: 1 };
    let edit = at(vec![body("a"), para(two, "b"), para(HEADING, "c"), body("d")], range(pos(0, 0), pos(2, 1)));
    let bullets = edit.toggle_list(false);
    assert_eq!(kinds(&bullets), [BULLET, ParaKind::Bullet { level: 1 }, BULLET, BODY]);
    assert_eq!(texts(&bullets), ["a", "b", "c", "d"]);
    assert_eq!(bullets.selection, edit.selection, "the selection stays");
    // All of them bullets already: they go back to body paragraphs.
    assert_eq!(kinds(&bullets.toggle_list(false)), [BODY, BODY, BODY, BODY]);
    // Bullets numbered keep their levels; one paragraph not yet numbered numbers them all.
    let numbered = bullets.toggle_list(true);
    assert_eq!(kinds(&numbered), [NUMBERED, two, NUMBERED, BODY]);
    let mixed = at(vec![para(NUMBERED, "a"), body("b")], range(pos(0, 0), pos(1, 0))).toggle_list(true);
    assert_eq!(kinds(&mixed), [NUMBERED, NUMBERED]);
}

#[test]
fn toggle_list_with_a_caret_changes_its_paragraph_and_keeps_a_chosen_style() {
    let edit = at(vec![body("a"), body("b")], caret(1, 1)).toggle(StyleBit::Bold);
    let listed = edit.toggle_list(false);
    assert_eq!(kinds(&listed), [BODY, BULLET]);
    assert_eq!(listed.typing_style, edit.typing_style, "what's typed next is still bold");
    assert_eq!(kinds(&at(Vec::new(), caret(0, 0)).toggle_list(true)), Vec::<ParaKind>::new());
}

// Selecting and moving.

#[test]
fn select_all_selects_the_flow() {
    let edit = at(vec![body("ab"), body("cde")], caret(0, 1)).select_all();
    assert_eq!(edit.selection, range(pos(0, 0), pos(1, 3)));
}

#[test]
fn select_word_at_selects_the_word() {
    let edit = at(vec![body("one two, three")], caret(0, 0));
    assert_eq!(edit.select_word_at(pos(0, 5)).selection, range(pos(0, 4), pos(0, 7)));
    assert_eq!(edit.select_word_at(pos(0, 4)).selection, range(pos(0, 4), pos(0, 7)));
    assert_eq!(edit.select_word_at(pos(0, 14)).selection, range(pos(0, 9), pos(0, 14)));
    assert_eq!(at(vec![body("")], caret(0, 0)).select_word_at(pos(0, 0)).selection, caret(0, 0));
}

#[test]
fn select_word_at_leaves_out_punctuation_and_spaces_after_the_word() {
    let edit = at(vec![body("hello, world.")], caret(0, 0));
    assert_eq!(edit.select_word_at(pos(0, 13)).selection, range(pos(0, 7), pos(0, 12)));
    assert_eq!(edit.select_word_at(pos(0, 12)).selection, range(pos(0, 7), pos(0, 12)));
    assert_eq!(edit.select_word_at(pos(0, 5)).selection, range(pos(0, 0), pos(0, 5)));
    assert_eq!(edit.select_word_at(pos(0, 6)).selection, range(pos(0, 0), pos(0, 5)));
    let edit = at(vec![body("end.  ")], caret(0, 0));
    assert_eq!(edit.select_word_at(pos(0, 6)).selection, range(pos(0, 0), pos(0, 3)));
    // With no word before them, the spaces or punctuation themselves.
    let edit = at(vec![body("  ...")], caret(0, 0));
    assert_eq!(edit.select_word_at(pos(0, 1)).selection, range(pos(0, 0), pos(0, 2)));
}

#[test]
fn word_edges_are_moved_onto_the_edges_of_characters() {
    // Word edges here (a regional indicator with an accent on it, then a flag) aren't
    // the edges of what's drawn, which are 0, 6 and 14.
    let text = "\u{1F1F3}\u{301}\u{1F1F3}\u{1F1F3}";
    assert_eq!(boundaries(text), [0, 6, 14]);
    let edit = at(vec![body(text)], caret(0, 0));
    for at in 0..=text.len() {
        assert_well_formed(&edit.select_word_at(pos(0, at)));
    }
    let picked = edit.select_word_at(pos(0, 0)).selection;
    assert_eq!(picked.anchor, pos(0, 0));
    assert!([6, 14].contains(&picked.head.offset), "{picked:?}");
    for motion in [Motion::WordLeft, Motion::WordRight] {
        for at in [0, 6, 14] {
            let moved = edit.move_to(pos(0, at), false).move_by(motion, false);
            assert_well_formed(&moved);
        }
    }
}

#[test]
fn moves_by_character_cross_paragraphs() {
    let edit = at(vec![body("ab"), body("cd")], caret(1, 0));
    assert_eq!(edit.move_by(Motion::Left, false).selection, caret(0, 2));
    let edit = at(vec![body("ab"), body("cd")], caret(0, 2));
    assert_eq!(edit.move_by(Motion::Right, false).selection, caret(1, 0));
    assert_eq!(at(vec![body("ab")], caret(0, 0)).move_by(Motion::Left, false).selection, caret(0, 0));
    assert_eq!(at(vec![body("ab")], caret(0, 2)).move_by(Motion::Right, false).selection, caret(0, 2));
}

#[test]
fn left_and_right_collapse_a_selection() {
    let edit = at(vec![body("abcd")], range(pos(0, 3), pos(0, 1)));
    assert_eq!(edit.move_by(Motion::Left, false).selection, caret(0, 1));
    assert_eq!(edit.move_by(Motion::Right, false).selection, caret(0, 3));
    assert_eq!(edit.move_by(Motion::Right, true).selection, range(pos(0, 3), pos(0, 2)));
}

#[test]
fn moves_by_word() {
    // Right goes to the start of the next word, or the end of the paragraph.
    let edit = at(vec![body("hello, world."), body("four")], caret(0, 0));
    let right = edit.move_by(Motion::WordRight, false);
    assert_eq!(right.selection, caret(0, 7));
    let right = right.move_by(Motion::WordRight, false);
    assert_eq!(right.selection, caret(0, 13));
    let right = right.move_by(Motion::WordRight, false);
    assert_eq!(right.selection, caret(1, 0));
    let right = at(vec![body("one two")], caret(0, 1)).move_by(Motion::WordRight, true);
    assert_eq!(right.selection, range(pos(0, 1), pos(0, 4)));
    let left = at(vec![body("one two, three")], caret(0, 9)).move_by(Motion::WordLeft, false);
    assert_eq!(left.selection, caret(0, 4));
    let left = at(vec![body("one two, three")], caret(0, 6)).move_by(Motion::WordLeft, true);
    assert_eq!(left.selection, range(pos(0, 6), pos(0, 4)));
    let left = at(vec![body("ab"), body("cd")], caret(1, 0)).move_by(Motion::WordLeft, false);
    assert_eq!(left.selection, caret(0, 2));
}

#[test]
fn moves_to_the_ends_of_a_paragraph_and_the_flow() {
    let edit = at(vec![body("ab"), body("cde"), body("f")], caret(1, 1));
    assert_eq!(edit.move_by(Motion::ParaStart, false).selection, caret(1, 0));
    assert_eq!(edit.move_by(Motion::ParaEnd, false).selection, caret(1, 3));
    assert_eq!(edit.move_by(Motion::FlowStart, true).selection, range(pos(1, 1), pos(0, 0)));
    assert_eq!(edit.move_by(Motion::FlowEnd, false).selection, caret(2, 1));
}

#[test]
fn move_to_places_or_extends() {
    let edit = at(vec![body("abc"), body("def")], caret(0, 1));
    assert_eq!(edit.move_to(pos(1, 2), false).selection, caret(1, 2));
    assert_eq!(edit.move_to(pos(1, 2), true).selection, range(pos(0, 1), pos(1, 2)));
    assert_eq!(edit.move_to(pos(9, 9), false).selection, caret(1, 3));
    assert_eq!(edit.move_to(pos(0, 99), false).selection, caret(0, 3));
}

#[test]
fn nothing_panics_on_a_selection_out_of_range() {
    let wild = [pos(0, 0), pos(0, 999), pos(5, 0), pos(usize::MAX, usize::MAX)];
    for anchor in wild {
        for head in wild {
            for paras in [Vec::new(), vec![body("ab"), body("")]] {
                let edit = at(paras, range(anchor, head));
                for edit in [
                    edit.insert("x"),
                    edit.backspace(),
                    edit.delete(),
                    edit.enter(),
                    edit.soft_break(),
                    edit.toggle(StyleBit::Underline),
                    edit.set_kind(BULLET),
                    edit.indent(),
                    edit.outdent(),
                    edit.select_word_at(head),
                    edit.move_to(head, true),
                    edit.move_by(Motion::WordRight, true),
                    edit.paste(&[body("p"), body("q")]),
                ] {
                    assert_well_formed(&edit);
                }
                let _ = edit.copied();
                let _ = edit.style_at();
            }
        }
    }
}

// Copy and paste.

#[test]
fn copied_is_the_selected_text() {
    let paras = vec![Para { kind: BULLET, spans: vec![plain("ab"), styled("cd", bold())] }, para(HEADING, "ef")];
    let edit = at(paras, range(pos(1, 1), pos(0, 1)));
    assert_eq!(
        edit.copied(),
        [Para { kind: BULLET, spans: vec![plain("b"), styled("cd", bold())] }, para(HEADING, "e")]
    );
    assert!(edit.move_by(Motion::Left, false).copied().is_empty());
}

#[test]
fn a_paste_of_one_paragraph_keeps_its_styles() {
    let pasted = [Para { kind: HEADING, spans: vec![styled("x", bold())] }];
    let edit = at(vec![para(BULLET, "ab")], caret(0, 1)).paste(&pasted);
    assert_eq!(edit.paras, [Para { kind: BULLET, spans: vec![plain("a"), styled("x", bold()), plain("b")] }]);
    assert_eq!(edit.selection, caret(0, 2));
}

#[test]
fn a_paste_into_an_empty_paragraph_brings_its_kinds() {
    let pasted = [para(BULLET, "a"), para(BULLET, "b")];
    let edit = at(vec![body("")], caret(0, 0)).paste(&pasted);
    assert_eq!(edit.paras, pasted);
}

#[test]
fn what_is_copied_pastes_back() {
    let paras = vec![para(BULLET, "ab"), Para { kind: HEADING, spans: vec![styled("cd", bold())] }, body("ef")];
    let edit = at(paras.clone(), range(pos(0, 1), pos(2, 1)));
    let copied = edit.copied();
    assert_eq!(edit.backspace().paste(&copied).paras, paras);
}

#[test]
fn a_paste_of_one_paragraph_into_an_empty_one_brings_its_kind() {
    let edit = at(vec![body("")], caret(0, 0)).paste(&[para(HEADING, "Title")]);
    assert_eq!(edit.paras, [para(HEADING, "Title")]);
    let edit = at(vec![body("")], caret(0, 0)).paste(&[para(HEADING, "Title"), body("")]);
    assert_eq!(edit.paras, [para(HEADING, "Title"), body("")]);
    // A paragraph left empty by the selection counts as empty.
    let edit = at(vec![body("abc")], range(pos(0, 0), pos(0, 3))).paste(&[para(BULLET, "x")]);
    assert_eq!(edit.paras, [para(BULLET, "x")]);
}

// Control characters.

#[test]
fn typed_line_endings_and_separators_break_lines_and_paragraphs() {
    let edit = at(vec![body("")], caret(0, 0)).insert("a\u{2028}b");
    assert_eq!(texts(&edit), ["a\nb"]);
    assert_eq!(edit.selection, caret(0, 3));
    let edit = at(vec![body("")], caret(0, 0)).insert("a\u{2029}b\r\nc\rd");
    assert_eq!(texts(&edit), ["a", "b", "c", "d"]);
}

#[test]
fn typed_control_characters_are_dropped() {
    let edit = at(vec![body("")], caret(0, 0)).insert("\u{feff}a\u{7}b\u{0}\u{1b}c\td\u{7f}\u{85}");
    assert_eq!(texts(&edit), ["abc\td"]);
    let edit = at(vec![body("ab")], caret(0, 1)).insert("\u{feff}\u{7}");
    assert_eq!(edit.paras, [body("ab")]);
    assert_eq!(edit.selection, caret(0, 1));
}

#[test]
fn pasted_control_characters_are_made_breaks_or_dropped() {
    let pasted = [Para { kind: BODY, spans: vec![plain("a\r"), styled("\nb\u{2028}c\u{7}\u{feff}\r\rd\te", bold())] }];
    let edit = at(vec![body("")], caret(0, 0)).paste(&pasted);
    assert_eq!(edit.paras, [Para { kind: BODY, spans: vec![plain("a\n"), styled("b\nc\n\nd\te", bold())] }]);
    let pasted = [Para { kind: BULLET, spans: vec![plain("a\u{2029}"), styled("b", bold())] }];
    let edit = at(vec![body("")], caret(0, 0)).paste(&pasted);
    assert_eq!(edit.paras, [para(BULLET, "a"), Para { kind: BULLET, spans: vec![styled("b", bold())] }]);
    assert_eq!(edit.selection, caret(1, 1));
    let edit = at(vec![body("xy")], caret(0, 1)).paste(&[body("\u{0}\u{feff}")]);
    assert_eq!(edit.paras, [body("xy")]);
}

// The style chosen for typing.

#[test]
fn a_pending_style_lasts_through_enter_breaks_and_kinds() {
    let pending = at(vec![body("abcd")], caret(0, 2)).toggle(StyleBit::Bold);
    let edit = pending.enter();
    assert_eq!(texts(&edit), ["ab", "cd"]);
    assert_eq!(edit.typing_style, Some(bold()));
    assert_eq!(edit.insert("x").paras[1].spans, [styled("x", bold()), plain("cd")]);
    let edit = pending.soft_break();
    assert_eq!(edit.typing_style, Some(bold()));
    assert_eq!(edit.paras[0].spans, [plain("ab"), styled("\n", bold()), plain("cd")]);
    for edit in [pending.set_kind(BULLET), pending.set_kind(BULLET).indent(), pending.set_kind(BULLET).indent().outdent()] {
        assert_eq!(edit.typing_style, Some(bold()));
    }
    let empty_bullet = at(vec![para(BULLET, "")], caret(0, 0)).toggle(StyleBit::Bold).enter();
    assert_eq!(empty_bullet.typing_style, Some(bold()));
}

#[test]
fn a_pending_style_is_forgotten_by_moving_selecting_and_deleting() {
    let pending = at(vec![body("abcd")], caret(0, 2)).toggle(StyleBit::Bold);
    for edit in [
        pending.move_by(Motion::Left, false),
        pending.move_to(pos(0, 2), false),
        pending.select_all(),
        pending.select_word_at(pos(0, 1)),
        pending.backspace(),
        pending.delete(),
        pending.move_to(pos(0, 0), false).toggle(StyleBit::Bold).backspace(),
        pending.insert("x"),
    ] {
        assert_eq!(edit.typing_style, None, "{edit:?}");
    }
}

#[test]
fn backspace_and_delete_leave_an_empty_flow_as_it_is() {
    let empty = at(Vec::new(), caret(0, 0));
    assert_eq!(empty.backspace(), empty);
    assert_eq!(empty.delete(), empty);
    let pending = empty.toggle(StyleBit::Italic);
    assert_eq!(pending.backspace(), pending);
}

// Properties.

/// Characters to type: whole ones, and pieces that join the characters beside them (a lone
/// regional indicator, a combining accent, a zero-width joiner, a skin-tone modifier).
const LETTERS: [&str; 14] = [
    "a",
    "b",
    " ",
    ",",
    "\u{e9}",
    COMBINING,
    FAMILY,
    FLAG,
    SURROGATE_PAIR,
    "\n",
    "\u{1F1F3}",
    "\u{301}",
    "\u{200D}",
    "\u{1F3FB}",
];

fn letters(max: usize) -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(LETTERS.to_vec()), 0..max).prop_map(|letters| letters.concat())
}

fn kind() -> impl Strategy<Value = ParaKind> {
    prop_oneof![
        Just(BODY),
        (1u8..=6).prop_map(ParaKind::Heading),
        (0u8..=8).prop_map(|level| ParaKind::Bullet { level }),
        (0u8..=8).prop_map(|level| ParaKind::Numbered { level }),
    ]
}

fn style() -> impl Strategy<Value = Style> {
    prop_oneof![Just(Style::default()), Just(bold()), Just(italic())]
}

/// A paragraph, its text in spans of random styles, tidied.
fn gen_para() -> impl Strategy<Value = Para> {
    (kind(), prop::collection::vec((letters(4), style()), 0..4)).prop_map(|(kind, spans)| {
        let mut tidy: Vec<Span> = Vec::new();
        for (text, style) in spans {
            let text = text.replace('\n', "");
            if text.is_empty() {
                continue;
            }
            match tidy.last_mut() {
                Some(last) if last.style == style => last.text.push_str(&text),
                _ => tidy.push(Span { text, style }),
            }
        }
        Para { kind, spans: tidy }
    })
}

#[derive(Clone, Debug)]
enum Op {
    Insert(String),
    SoftBreak,
    Backspace,
    Delete,
    Enter,
    Toggle(StyleBit),
    SetKind(ParaKind),
    Indent,
    Outdent,
    SelectAll,
    /// A paragraph and a grapheme within it, as fractions of what's there.
    MoveTo(u8, u8, bool),
    MoveBy(Motion, bool),
    Paste(Vec<Para>),
    /// Operations whose effect on the selection the plain model doesn't follow.
    SelectWord(u8, u8),
    WordMove(bool, bool),
    SetSize(Option<u32>),
    Wild(usize, usize),
}

fn op() -> impl Strategy<Value = Op> {
    let motion = prop::sample::select(vec![
        Motion::Left,
        Motion::Right,
        Motion::ParaStart,
        Motion::ParaEnd,
        Motion::FlowStart,
        Motion::FlowEnd,
    ]);
    let bit = prop::sample::select(vec![StyleBit::Bold, StyleBit::Italic, StyleBit::Underline]);
    prop_oneof![
        4 => letters(4).prop_map(Op::Insert),
        1 => Just(Op::SoftBreak),
        3 => Just(Op::Backspace),
        2 => Just(Op::Delete),
        2 => Just(Op::Enter),
        1 => bit.prop_map(Op::Toggle),
        1 => kind().prop_map(Op::SetKind),
        1 => Just(Op::Indent),
        1 => Just(Op::Outdent),
        1 => Just(Op::SelectAll),
        3 => (any::<u8>(), any::<u8>(), any::<bool>()).prop_map(|(para, at, extend)| Op::MoveTo(para, at, extend)),
        3 => (motion, any::<bool>()).prop_map(|(motion, extend)| Op::MoveBy(motion, extend)),
        1 => prop::collection::vec(gen_para(), 0..4).prop_map(Op::Paste),
        1 => (any::<u8>(), any::<u8>()).prop_map(|(para, at)| Op::SelectWord(para, at)),
        1 => (any::<bool>(), any::<bool>()).prop_map(|(right, extend)| Op::WordMove(right, extend)),
        1 => prop::option::of(1u32..100).prop_map(Op::SetSize),
        1 => (0usize..8, 0usize..40).prop_map(|(para, offset)| Op::Wild(para, offset)),
    ]
}

/// The position `para` and `at`, fractions of 256, name in `paras`.
fn pick(paras: &[String], para: u8, at: u8) -> Pos {
    if paras.is_empty() {
        return pos(0, 0);
    }
    let para = para as usize * paras.len() / 256;
    let bounds = boundaries(&paras[para]);
    pos(para, bounds[at as usize * bounds.len() / 256])
}

fn apply(edit: &TextEdit, op: &Op) -> TextEdit {
    match op {
        Op::Insert(text) => edit.insert(text),
        Op::SoftBreak => edit.soft_break(),
        Op::Backspace => edit.backspace(),
        Op::Delete => edit.delete(),
        Op::Enter => edit.enter(),
        Op::Toggle(bit) => edit.toggle(*bit),
        Op::SetKind(kind) => edit.set_kind(*kind),
        Op::Indent => edit.indent(),
        Op::Outdent => edit.outdent(),
        Op::SelectAll => edit.select_all(),
        Op::MoveTo(para, at, extend) => edit.move_to(pick(&texts(edit), *para, *at), *extend),
        Op::MoveBy(motion, extend) => edit.move_by(*motion, *extend),
        Op::Paste(paras) => edit.paste(paras),
        Op::SelectWord(para, at) => edit.select_word_at(pick(&texts(edit), *para, *at)),
        Op::WordMove(right, extend) => edit.move_by(if *right { Motion::WordRight } else { Motion::WordLeft }, *extend),
        Op::SetSize(size) => edit.set_size(*size),
        Op::Wild(para, offset) => edit.move_to(pos(*para, *offset), true),
    }
}

/// The same operations on plain strings: each paragraph a kind and its text.
#[derive(Clone, Debug, PartialEq)]
struct Plain {
    paras: Vec<(ParaKind, String)>,
    anchor: Pos,
    head: Pos,
}

impl Plain {
    fn of(edit: &TextEdit) -> Plain {
        let texts = texts(edit);
        Plain {
            paras: edit.paras.iter().map(|para| para.kind).zip(texts).collect(),
            anchor: edit.selection.anchor,
            head: edit.selection.head,
        }
    }

    fn texts(&self) -> Vec<String> {
        self.paras.iter().map(|(_, text)| text.clone()).collect()
    }

    fn ends(&self) -> (Pos, Pos) {
        (self.anchor.min(self.head), self.anchor.max(self.head))
    }

    fn caret(&mut self, at: Pos) {
        self.anchor = at;
        self.head = at;
    }

    fn ensure(&mut self) {
        if self.paras.is_empty() {
            self.paras.push((BODY, String::new()));
        }
    }

    fn len(&self, para: usize) -> usize {
        self.paras[para].1.len()
    }

    /// `at` moved onto the edge of a character: forward, or back.
    fn snap(&self, at: Pos, forward: bool) -> Pos {
        let bounds = boundaries(&self.paras[at.para].1);
        let offset = if forward {
            bounds.iter().copied().find(|&b| b >= at.offset).unwrap()
        } else {
            bounds.iter().copied().rev().find(|&b| b <= at.offset).unwrap()
        };
        pos(at.para, offset)
    }

    /// Replaces the selection with `pasted`, as the editor does, the caret after it moved
    /// forward off a character it ended inside if `forward`, else back.
    fn replace(&mut self, pasted: &[(ParaKind, String)], forward: bool) {
        self.ensure();
        let (start, end) = self.ends();
        let kind = self.paras[start.para].0;
        let head = self.paras[start.para].1[..start.offset].to_string();
        let tail = self.paras[end.para].1[end.offset..].to_string();
        let mut new = Vec::new();
        let at = match pasted {
            [] => {
                new.push((kind, head + &tail));
                start
            }
            [one] => {
                let kind = if head.is_empty() && tail.is_empty() { one.0 } else { kind };
                new.push((kind, head + &one.1 + &tail));
                pos(start.para, start.offset + one.1.len())
            }
            [first, middle @ .., last] => {
                new.push((if head.is_empty() { first.0 } else { kind }, head + &first.1));
                new.extend(middle.iter().cloned());
                new.push((last.0, last.1.clone() + &tail));
                pos(start.para + pasted.len() - 1, last.1.len())
            }
        };
        self.paras.splice(start.para..=end.para, new);
        let at = self.snap(at, forward);
        self.caret(at);
    }

    fn prev(&self, at: Pos) -> Pos {
        let bounds = boundaries(&self.paras[at.para].1);
        match bounds.iter().rev().find(|&&b| b < at.offset) {
            Some(&b) => pos(at.para, b),
            None if at.para > 0 => pos(at.para - 1, self.len(at.para - 1)),
            None => at,
        }
    }

    fn next(&self, at: Pos) -> Pos {
        let bounds = boundaries(&self.paras[at.para].1);
        match bounds.iter().find(|&&b| b > at.offset) {
            Some(&b) => pos(at.para, b),
            None if at.para + 1 < self.paras.len() => pos(at.para + 1, 0),
            None => at,
        }
    }

    /// No list item of `range` deeper than the paragraph before it allows.
    fn fit_levels(&mut self, range: std::ops::Range<usize>) {
        for index in range {
            let most = match index.checked_sub(1).map(|before| self.paras[before].0) {
                Some(ParaKind::Bullet { level } | ParaKind::Numbered { level }) => level + 1,
                _ => 0,
            };
            self.paras[index].0 = match self.paras[index].0 {
                ParaKind::Bullet { level } => ParaKind::Bullet { level: level.min(most) },
                ParaKind::Numbered { level } => ParaKind::Numbered { level: level.min(most) },
                kind => kind,
            };
        }
    }

    /// Applies `op`, or returns false if the model doesn't follow it.
    fn apply(&mut self, op: &Op) -> bool {
        let clamp = |level: u8| level.min(8);
        match op {
            Op::Insert(text) => {
                let kind = if self.paras.is_empty() { BODY } else { self.paras[self.ends().0.para].0 };
                let lines: Vec<(ParaKind, String)> = text.split('\n').map(|line| (kind, line.to_string())).collect();
                if !(text.is_empty() && self.anchor == self.head) {
                    self.replace(&lines, true);
                }
            }
            Op::SoftBreak => {
                self.ensure();
                let kind = self.paras[self.ends().0.para].0;
                self.replace(&[(kind, "\n".to_string())], true);
            }
            Op::Backspace | Op::Delete if self.paras.is_empty() => {}
            Op::Backspace | Op::Delete if self.anchor != self.head => self.replace(&[], false),
            Op::Backspace => {
                let at = self.head;
                let kind = &mut self.paras[at.para].0;
                match (at.offset, *kind) {
                    (0, ParaKind::Bullet { level } | ParaKind::Numbered { level }) if level > 0 => {
                        *kind = match *kind {
                            ParaKind::Bullet { .. } => ParaKind::Bullet { level: level - 1 },
                            _ => ParaKind::Numbered { level: level - 1 },
                        }
                    }
                    (0, ParaKind::Bullet { .. } | ParaKind::Numbered { .. }) => *kind = BODY,
                    _ => {
                        self.anchor = self.prev(at);
                        self.replace(&[], false);
                    }
                }
            }
            Op::Delete => {
                self.head = self.next(self.head);
                self.replace(&[], false);
            }
            Op::Enter => {
                let was_caret = self.anchor == self.head;
                self.replace(&[], false);
                let at = self.head;
                let (kind, text) = self.paras[at.para].clone();
                let is_list = matches!(kind, ParaKind::Bullet { .. } | ParaKind::Numbered { .. });
                if was_caret && is_list && text.is_empty() {
                    self.paras[at.para].0 = BODY;
                } else {
                    let tail = if matches!(kind, ParaKind::Heading(_)) && at.offset == text.len() { BODY } else { kind };
                    self.replace(&[(kind, String::new()), (tail, String::new())], false);
                }
            }
            Op::Toggle(_) | Op::SetSize(_) => {}
            Op::SetKind(kind) => {
                if !self.paras.is_empty() {
                    let (start, end) = self.ends();
                    for para in &mut self.paras[start.para..=end.para] {
                        para.0 = *kind;
                    }
                    self.fit_levels(start.para..end.para + 1);
                }
            }
            Op::Indent | Op::Outdent => {
                if !self.paras.is_empty() {
                    let (start, end) = self.ends();
                    let up = matches!(op, Op::Indent);
                    for para in &mut self.paras[start.para..=end.para] {
                        let step = |level: u8| if up { clamp(level + 1) } else { level.saturating_sub(1) };
                        para.0 = match para.0 {
                            ParaKind::Bullet { level } => ParaKind::Bullet { level: step(level) },
                            ParaKind::Numbered { level } => ParaKind::Numbered { level: step(level) },
                            kind => kind,
                        };
                    }
                    self.fit_levels(start.para..end.para + 1);
                }
            }
            Op::SelectAll => {
                if !self.paras.is_empty() {
                    let last = self.paras.len() - 1;
                    self.anchor = pos(0, 0);
                    self.head = pos(last, self.len(last));
                }
            }
            Op::MoveTo(para, at, extend) => {
                let to = pick(&self.texts(), *para, *at);
                self.head = to;
                if !extend {
                    self.anchor = to;
                }
            }
            Op::MoveBy(motion, extend) => {
                if self.paras.is_empty() {
                    return true;
                }
                let (start, end) = self.ends();
                let last = self.paras.len() - 1;
                let to = match motion {
                    Motion::Left if !extend && start != end => start,
                    Motion::Right if !extend && start != end => end,
                    Motion::Left => self.prev(self.head),
                    Motion::Right => self.next(self.head),
                    Motion::ParaStart => pos(self.head.para, 0),
                    Motion::ParaEnd => pos(self.head.para, self.len(self.head.para)),
                    Motion::FlowStart => pos(0, 0),
                    Motion::FlowEnd => pos(last, self.len(last)),
                    _ => unreachable!(),
                };
                self.head = to;
                if !extend {
                    self.anchor = to;
                }
            }
            Op::Paste(paras) => {
                let pasted: Vec<(ParaKind, String)> = paras
                    .iter()
                    .map(|para| (para.kind, para.spans.iter().map(|span| span.text.as_str()).collect()))
                    .collect();
                self.ensure();
                let (start, end) = self.ends();
                let before = self.paras.len();
                self.replace(&pasted, true);
                let made = self.paras.len() + (end.para - start.para + 1) - before;
                self.fit_levels(start.para..start.para + made);
            }
            Op::SelectWord(..) | Op::WordMove(..) | Op::Wild(..) => return false,
        }
        true
    }
}

fn start() -> impl Strategy<Value = TextEdit> {
    prop::collection::vec(gen_para(), 0..4).prop_map(|paras| at(paras, caret(0, 0)))
}

proptest! {
    #[test]
    fn the_model_stays_well_formed(edit in start(), ops in prop::collection::vec(op(), 0..40)) {
        let mut edit = edit;
        for op in &ops {
            let before = edit.clone();
            edit = apply(&edit, op);
            assert_well_formed(&edit);
            assert_levels_fit(&before, op, &edit);
            let _ = edit.style_at();
            let copied = edit.copied();
            assert_well_formed(&at(copied, caret(0, 0)));
        }
    }

    #[test]
    fn the_text_is_what_the_operations_say(edit in start(), ops in prop::collection::vec(op(), 0..40)) {
        let mut edit = edit;
        let mut plain = Plain::of(&edit);
        for op in &ops {
            edit = apply(&edit, op);
            if plain.apply(op) {
                prop_assert_eq!(&Plain::of(&edit), &plain, "after {:?}", op);
            } else {
                plain = Plain::of(&edit);
            }
        }
    }
}

#[test]
fn bidi_overrides_and_isolates_are_dropped_but_joiners_and_marks_kept() {
    let edit = TextEdit::new(vec![para(ParaKind::Body, "")]);
    let hostile = "a\u{202a}b\u{202b}c\u{202c}d\u{202d}e\u{202e}f\u{2066}g\u{2067}h\u{2068}i\u{2069}j";
    let kept = "👨\u{200d}👩 x\u{200c}y \u{200e}z\u{200f}";
    let typed = edit.insert(&format!("{hostile}{kept}"));
    assert_eq!(typed.paras[0].spans[0].text, format!("abcdefghij{kept}"));
    let pasted = edit.paste(&[para(ParaKind::Body, &format!("{hostile}{kept}"))]);
    assert_eq!(pasted.paras[0].spans[0].text, format!("abcdefghij{kept}"));
    for c in ['\u{202a}', '\u{202e}', '\u{2066}', '\u{2069}'] {
        assert!(is_bidi_control(c));
    }
    for c in ['\u{200d}', '\u{200c}', '\u{200e}', '\u{200f}', '\u{2029}', 'a'] {
        assert!(!is_bidi_control(c));
    }
}
