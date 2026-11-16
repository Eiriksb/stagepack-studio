//! Reading a talk again after an edit: only the slides that changed are parsed, and what
//! comes out is what reading the whole talk would give.

mod common;

use common::*;
use proptest::prelude::*;
use slides_format::*;

/// How many slides `work` parses on this thread.
fn slides_parsed_by(work: impl FnOnce() -> Talk) -> (Talk, usize) {
    let before = slides_parsed_on_this_thread();
    let talk = work();
    (talk, slides_parsed_on_this_thread() - before)
}

const TALK: &str = "---\ntitle: T\n---\n# One\n\n---\n\n# Two\n\n<Notes>\nSay it\n</Notes>\n\n---\n\n# Three <Box x=\"1\">\n\n- a\n- b\n";

#[test]
fn an_edit_to_one_slide_parses_only_that_slide() {
    let old = parse_talk(TALK);
    let new = TALK.replace("Say it", "Say it louder");
    let (talk, parsed) = slides_parsed_by(|| reparse(&old, new.clone()));
    assert_eq!(parsed, 1);
    assert_eq!(talk, parse_talk(&new));
}

#[test]
fn a_slide_added_in_the_middle_is_the_only_one_parsed() {
    let old = parse_talk(TALK);
    let new = TALK.replace("---\n\n# Two", "---\n\n# New\n\n---\n\n# Two");
    let (talk, parsed) = slides_parsed_by(|| reparse(&old, new.clone()));
    assert_eq!(parsed, 1);
    assert_eq!(talk.slides.len(), 4);
    assert_eq!(talk, parse_talk(&new));
}

#[test]
fn the_same_text_parses_nothing() {
    let old = parse_talk(TALK);
    let (talk, parsed) = slides_parsed_by(|| reparse(&old, TALK.to_string()));
    assert_eq!(parsed, 0);
    assert_eq!(talk, old);
}

#[test]
fn a_new_code_fence_that_swallows_the_rest_reads_as_a_full_parse_does() {
    let old = parse_talk(TALK);
    let new = TALK.replace("# One\n", "# One\n```\n");
    assert_eq!(reparse(&old, new.clone()), parse_talk(&new));
}

/// Something done to a talk's text.
#[derive(Clone, Debug)]
enum Edit {
    /// One of the slide operations, a notes edit among them.
    Operation(Op),
    /// Notes typed into a slide.
    Notes(usize, String),
    /// Bytes taken out somewhere and others put in their place. The place is a share of the
    /// text's length, moved back to the start of a character.
    Splice { at: f64, remove: usize, insert: String },
}

fn spliced_text() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop::sample::select(vec![
            "", "a", " ", "\n", "\r\n", "\n\n", "---", "---\n", "\n---\n", "\n---  \n", "```", "\n```\n", "~~~", "\n~~~md\n",
            "# ", "- ", "> ", "<Notes>", "</Notes>", "<Box x=\"1\">", "</Box>", "key: value\n", "layout: section\n", "é", "*",
        ]),
        0..4,
    )
    .prop_map(|parts| parts.concat())
}

fn edit() -> impl Strategy<Value = Edit> {
    prop_oneof![
        op().prop_map(Edit::Operation),
        (0..8usize, typed_notes()).prop_map(|(slide, typed)| Edit::Notes(slide, typed)),
        (0.0..=1.0f64, 0..12usize, spliced_text()).prop_map(|(at, remove, insert)| Edit::Splice { at, remove, insert }),
    ]
}

fn char_boundary_at_or_before(text: &str, mut at: usize) -> usize {
    at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// The talk's text once `edit` is made to it, or `None` if it doesn't apply.
fn edited(talk: &Talk, edit: &Edit) -> Option<String> {
    let source = &talk.source;
    let n = talk.slides.len();
    Some(match edit {
        Edit::Operation(op) => run(talk, op)?.0.apply(source),
        Edit::Notes(slide, typed) => set_notes(talk, slide % n, typed).apply(source),
        Edit::Splice { at, remove, insert } => {
            let start = char_boundary_at_or_before(source, (source.len() as f64 * at) as usize);
            let end = char_boundary_at_or_before(source, start + remove);
            format!("{}{insert}{}", &source[..start], &source[end..])
        }
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn reparse_equals_a_full_parse(generated in gen_talk(), edit in edit()) {
        let old = parse_talk(&generated.text());
        let Some(new) = edited(&old, &edit) else { return Ok(()) };
        prop_assert_eq!(reparse(&old, new.clone()), parse_talk(&new));
    }

    #[test]
    fn unchanged_slides_are_not_parsed_again(generated in gen_talk(), slide in 0..8usize, typed in typed_notes()) {
        let old = parse_talk(&generated.text());
        let slide = slide % old.slides.len();
        let new = set_notes(&old, slide, &typed).apply(&old.source);
        let (_, parsed) = slides_parsed_by(|| reparse(&old, new.clone()));
        let full = parse_talk(&new);
        prop_assert_eq!(full.slides.len(), old.slides.len());
        let changed = (0..old.slides.len()).filter(|&i| old.source[old.slides[i].range.clone()] != new[full.slides[i].range.clone()]).count();
        prop_assert!(changed <= 1, "notes typed into one slide changed {} of them", changed);
        prop_assert_eq!(parsed, changed);
    }
}
