//! Deleting, duplicating and pasting several things at once writes exactly what doing it to one
//! thing at a time writes, on slides of every shape: settings or none, notes straight after,
//! area markers, boxes with things in them, lists, things first and last, things on lines next
//! to each other, any number of blank lines between, no line ending at the end, and either kind
//! of line ending.

use std::ops::Range;

use proptest::prelude::*;
use slides_engine::Fonts;
use slides_format::{Block, parse_talk};
use stagepack_studio::document::{Document, Thing};

/// One block of the slide, its text with `\n` line endings, given its place on the slide.
fn block(kind: u8, n: usize) -> String {
    let x = 100 + n * 200;
    match kind {
        0 => "# Head".to_string(),
        1 => "Some words".to_string(),
        2 => "<Region name=\"right\" />".to_string(),
        3 => format!("<Box x=\"{x}\" y=\"600\" w=\"150\" h=\"150\">\n\n<Icon name=\"star\" />\n\n</Box>"),
        4 => "- a\n- b".to_string(),
        _ => format!("<Icon name=\"arrow-right\" x=\"{x}\" y=\"300\" size=\"96\" />"),
    }
}

const SEPARATORS: [&str; 6] = ["\n", "\n\n", "\n\n\n", "\n\n\n\n", " \n\n", "\n \n"];
const NOTES: [&str; 3] = ["", "\n\n<Notes>\nSay hi\n</Notes>\n", "\n<Notes>\nSay hi\n</Notes>\n"];
const ENDS: [&str; 4] = ["", "\n", "\n\n---\n\n# Last\n", "\n---\n# Last\n"];

#[derive(Debug, Clone)]
struct Shape {
    /// Talk settings, and whether the slide is the first or the second.
    opening: u8,
    second: bool,
    layout: bool,
    blocks: Vec<(u8, usize)>,
    notes: usize,
    end: usize,
    crlf: bool,
    /// Which of the things on it are picked, by a bit each.
    picks: u16,
}

fn shape() -> impl Strategy<Value = Shape> {
    (
        0u8..3,
        any::<bool>(),
        any::<bool>(),
        prop::collection::vec((0u8..9, 0..SEPARATORS.len()), 2..8),
        0..NOTES.len(),
        0..ENDS.len(),
        any::<bool>(),
        any::<u16>(),
    )
        .prop_map(|(opening, second, layout, blocks, notes, end, crlf, picks)| Shape { opening, second, layout, blocks, notes, end, crlf, picks })
}

/// The talk `shape` gives, and the index of the slide that has its blocks.
fn talk_of(shape: &Shape) -> (String, usize) {
    let mut talk = String::new();
    match shape.opening {
        0 => talk.push_str("---\ntitle: T\n---\n\n"),
        1 => talk.push_str("---\ntitle: T\nlayout: two-columns\n---\n"),
        _ => {}
    }
    if shape.second {
        talk.push_str("# First\n\nWords\n\n---\n");
        if shape.layout {
            talk.push_str("layout: two-columns\n---\n");
        }
    }
    for (index, (kind, separator)) in shape.blocks.iter().enumerate() {
        talk.push_str(&block(*kind, index));
        if index + 1 < shape.blocks.len() {
            talk.push_str(SEPARATORS[*separator]);
        }
    }
    talk.push_str(NOTES[shape.notes]);
    talk.push_str(ENDS[shape.end]);
    if shape.crlf {
        talk = talk.replace('\n', "\r\n");
    }
    (talk, usize::from(shape.second))
}

fn open(talk: &str) -> (tempfile::TempDir, Document) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), talk).unwrap();
    let document = Document::open(dir.path(), Fonts::bundled_only()).unwrap();
    (dir, document)
}

/// Opens `talk` on slide `slide` with `picked` selected, or `None` if they can't all be picked.
fn with_picked(talk: &str, slide: usize, picked: &[Range<usize>]) -> Option<(tempfile::TempDir, Document)> {
    let (dir, mut document) = open(talk);
    document.go_to(slide);
    picked.iter().all(|thing| document.select_also(slide, Thing::Block(thing.clone()))).then_some((dir, document))
}

/// The things' own texts, with `\n` line endings, in the order written.
fn texts_of(talk: &str, picked: &[Range<usize>]) -> Vec<String> {
    picked.iter().map(|thing| talk[thing.clone()].replace("\r\n", "\n")).collect()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 160, ..ProptestConfig::default() })]

    #[test]
    fn several_at_once_write_what_one_at_a_time_would(shape in shape()) {
        let (talk, slide) = talk_of(&shape);
        let parsed = parse_talk(&talk);
        prop_assume!(parsed.slides.get(slide).is_some_and(|slide| slide.error.is_none()));
        // The tags at the top level that the slide draws as things, some of them picked.
        let (_dir, mut probe) = open(&talk);
        let layout = probe.text_layout(slide);
        let tags: Vec<Range<usize>> = parsed.slides[slide]
            .blocks
            .iter()
            .filter(|block| matches!(block, Block::Tag(_)))
            .map(|block| block.range().clone())
            .filter(|range| layout.element(range).is_some())
            .collect();
        prop_assume!(tags.len() >= 2);
        let mut picked: Vec<Range<usize>> = tags.iter().enumerate().filter(|(at, _)| shape.picks & (1 << at) != 0).map(|(_, tag)| tag.clone()).collect();
        if picked.len() < 2 {
            picked = tags.clone();
        }

        // Deleting: all at once, and one at a time from the last written.
        let Some((_a, mut all)) = with_picked(&talk, slide, &picked) else { return Ok(()) };
        let at_once = all.delete_selected().ok().map(|_| all.source().to_string());
        let (_b, mut each) = open(&talk);
        let mut in_turn = Some(());
        for thing in picked.iter().rev() {
            if !each.select(slide, thing.clone()) || each.delete_selected().is_err() {
                in_turn = None;
                break;
            }
        }
        let in_turn = in_turn.map(|_| each.source().to_string());
        prop_assert_eq!(&at_once, &in_turn, "deleting {:?} from {:?}", texts_of(&talk, &picked), talk);

        // Duplicating, and copying then pasting: all at once, and pasting each copy in turn. Each
        // thing has a place of its own, so each copy, alone or together, moves aside by one step.
        let texts = texts_of(&talk, &picked);
        let (_c, mut each) = open(&talk);
        each.go_to(slide);
        let mut in_turn = Some(());
        for text in &texts {
            if each.paste_element(text, Default::default()).is_err() {
                in_turn = None;
                break;
            }
        }
        let in_turn = in_turn.map(|_| each.source().to_string());
        let Some((_d, mut all)) = with_picked(&talk, slide, &picked) else { return Ok(()) };
        let duplicated = all.duplicate_selected().ok().map(|_| all.source().to_string());
        prop_assert_eq!(&duplicated, &in_turn, "duplicating {:?} on {:?}", &texts, talk);
        let Some((_e, mut all)) = with_picked(&talk, slide, &picked) else { return Ok(()) };
        let copied = all.copy_selected().ok();
        prop_assert!(copied.is_some(), "copying {:?}", &texts);
        let (text, _) = copied.unwrap();
        prop_assert_eq!(&text, &texts.join("\n\n"));
        let pasted = all.paste_element(&text, Default::default()).ok().map(|_| all.source().to_string());
        prop_assert_eq!(&pasted, &in_turn, "pasting {:?} on {:?}", &texts, talk);
    }
}
