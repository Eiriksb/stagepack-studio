//! Random runs of typing on a talk's slides, with undo and redo among them: the text being
//! typed always reads as the talk holds it, other slides keep their bytes, a refusal changes
//! nothing, and undoing everything gives back the talk as it was opened.
//!
//! Each talk runs with a few seeds; `SEEDS=n` runs seed `n` alone, and `SEEDS=a..b` a range.

use slides_engine::Fonts;
use slides_format::{FlowPlace, Motion, ParaKind, Pos, StyleBit, split_slides, storable, text_flows};
use stagepack_studio::document::Document;

/// A small xorshift, so that a failing run can be run again from its seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

const TALKS: &[&str] = &[
    "# T\n\n- one\n- two\n  - three\n\n1. a\n2. b\n\nWords here\n\n---\n\n# Two\n\nx\n\n---\n\n# Three\n",
    "---\nlayout: two-columns\n---\n\n# T\n\nleft text\n\n<Region name=\"right\" />\n\nright side\n\n---\n\n# Two\n\n---\n\n---\nlayout: two-columns\n---\n\n# Three\n",
    "# T\n\n<Text x=\"600\" y=\"300\" w=\"600\" h=\"200\">\n\nHello *there*\n\n</Text>\n\nbody words\n\n---\n\n# Two\n\n---\n\n# Three\n",
    "\u{feff}---\r\ntitle: T  \r\n---\r\n\r\n# Hi  \r\n\r\nbody **b** end  \r\n\r\n- a\r\n- b\r\n\r\n---\r\n\r\n# Two\r\n\r\n---\r\n\r\n# Three\r\n",
    "# T\n\nwords\n\n<Notes>\nsay this\n</Notes>\n\n---\n\n# Two\n\n<Notes>\nmore\n</Notes>\n\n---\n\n# Three\n",
    "# One\n\n---\n\n---\n\n# Three\n\nend\n",
    // A region whose text a table splits in two, and in three.
    "# T\n\nfirst\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nsecond\n",
    "# T\n\nfirst\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nsecond\n\n| c | d |\n|---|---|\n| 3 | 4 |\n\nthird\n",
];

const INSERTS: &[&str] = &[
    "a", " ", "Z", "*", "#", "1. ", "- ", "\\", "é", "x y", "<", "`", "_", "[", "&",
];

/// How many seeds each talk runs with, unless `SEEDS` says.
const SEEDS_EACH: u64 = 5;
/// How many operations a run makes.
const STEPS: usize = 250;

fn slices(source: &str) -> Vec<String> {
    split_slides(source)
        .iter()
        .map(|slide| source[slide.range.clone()].to_string())
        .collect()
}

/// The text being typed reads as the talk holds it: its flow's paragraphs, or, in an empty
/// region before anything is written, one empty paragraph in a region with no text.
fn check_model(doc: &Document, ctx: &str) {
    let Some(editing) = doc.text() else { return };
    let flows = text_flows(doc.talk(), editing.slide);
    match doc.text_flow() {
        Some(index) => assert_eq!(
            flows[index].paras,
            storable(&editing.edit.paras),
            "{ctx}: talk and model diverged\n{}",
            doc.source()
        ),
        None => {
            assert!(
                editing.edit.paras.len() == 1 && editing.edit.paras[0].spans.is_empty(),
                "{ctx}: model holds text the talk doesn't: {:?}\n{}",
                editing.edit.paras,
                doc.source()
            );
            let region = editing.flow.region().expect("only a region can be empty");
            assert!(
                !flows
                    .iter()
                    .any(|flow| flow.place == FlowPlace::Region(region.to_string())),
                "{ctx}: typing into an empty {region} that has text\n{}",
                doc.source()
            );
        }
    }
}

/// Whether the region text being typed is in the talk and holds nothing but empty paragraphs.
fn editing_only_empty(doc: &Document) -> bool {
    let Some(editing) = doc.text() else { return false };
    let Some(index) = doc.text_flow() else { return false };
    editing.flow.region().is_some()
        && text_flows(doc.talk(), editing.slide)[index]
            .paras
            .iter()
            .all(|para| para.spans.is_empty())
}

fn run(talk: &str, seed: u64, steps: usize) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), talk).unwrap();
    let mut doc = Document::open(dir.path(), Fonts::bundled_only()).unwrap();
    let original = doc.source().to_string();
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    for step in 0..steps {
        let ctx = format!("seed {seed} step {step}");
        if doc.text().is_none() || rng.below(25) == 0 {
            let slide = rng.below(doc.slide_count());
            let flows = text_flows(doc.talk(), slide);
            if !flows.is_empty() && rng.below(3) != 0 {
                let flow = rng.below(flows.len());
                let para = rng.below(flows[flow].paras.len());
                let text: String = flows[flow].paras[para]
                    .spans
                    .iter()
                    .map(|span| span.text.as_str())
                    .collect();
                let chars = text.chars().count();
                let offset = text
                    .char_indices()
                    .map(|(at, _)| at)
                    .chain([text.len()])
                    .nth(rng.below(chars + 1))
                    .unwrap();
                doc.begin_text(slide, flow, Pos { para, offset });
            } else {
                let region = ["title", "body", "right"][rng.below(3)];
                doc.begin_text_in_region(slide, region);
            }
            check_model(&doc, &ctx);
            continue;
        }
        let before_src = doc.source().to_string();
        let before_edit = doc.text().cloned();
        let before_hist = (doc.can_undo(), doc.can_redo(), doc.undo_label());
        let editing_slide = doc.text().unwrap().slide;
        let was_only_empty = editing_only_empty(&doc);
        let choice = rng.below(24);
        let result = match choice {
            0..=5 => {
                let text = INSERTS[rng.below(INSERTS.len())];
                doc.apply_text(|edit| edit.insert(text))
            }
            6 => doc.apply_text(|edit| edit.enter()),
            7 => doc.apply_text(|edit| edit.soft_break()),
            8 => doc.apply_text(|edit| edit.backspace()),
            9 => doc.apply_text(|edit| edit.delete()),
            10 => doc.apply_text(|edit| edit.toggle(StyleBit::Bold)),
            11 => doc.apply_text(|edit| edit.toggle(StyleBit::Italic)),
            12 => {
                let kind = [
                    ParaKind::Body,
                    ParaKind::Heading(1),
                    ParaKind::Heading(2),
                    ParaKind::Bullet { level: 0 },
                    ParaKind::Numbered { level: 0 },
                ][rng.below(5)];
                doc.apply_text(|edit| edit.set_kind(kind))
            }
            13 => doc.apply_text(|edit| edit.indent()),
            14 => doc.apply_text(|edit| edit.outdent()),
            15 | 16 => {
                let motion = [
                    Motion::Left,
                    Motion::Right,
                    Motion::WordLeft,
                    Motion::WordRight,
                    Motion::ParaStart,
                    Motion::ParaEnd,
                    Motion::FlowStart,
                    Motion::FlowEnd,
                ][rng.below(8)];
                let extend = rng.below(2) == 0;
                doc.apply_text(|edit| edit.move_by(motion, extend))
            }
            17 => doc.apply_text(|edit| edit.select_all()),
            18 => {
                doc.end_typing_run();
                Ok(vec![])
            }
            19 => doc.end_text(),
            20 => doc.undo().map(|_| vec![]),
            21 => doc.redo().map(|_| vec![]),
            // As before a save, a show or an export.
            22 => doc.settle_text(),
            // Another edit made while typing.
            _ => match rng.below(3) {
                0 => doc.add_slide("content"),
                1 => doc.duplicate_current(),
                _ => {
                    let hidden = doc.is_hidden(doc.current());
                    doc.set_current_hidden(!hidden)
                }
            },
        };
        match result {
            Ok(_) => {
                if choice < 18 {
                    // Other slides' bytes never change.
                    let a = slices(&before_src);
                    let b = slices(doc.source());
                    assert_eq!(a.len(), b.len(), "{ctx}: slide count changed");
                    for i in 0..a.len() {
                        if i != editing_slide {
                            assert_eq!(a[i], b[i], "{ctx}: slide {i} changed by choice {choice}");
                        }
                    }
                }
                if choice == 22 {
                    assert!(
                        !editing_only_empty(&doc),
                        "{ctx}: empty lines left in the talk\n{:?}\n{:?}",
                        doc.source(), doc.text()
                    );
                    if !was_only_empty {
                        assert_eq!(doc.text().cloned(), before_edit, "{ctx}: settling moved typing");
                    }
                }
                if choice >= 23 {
                    assert!(doc.text().is_none(), "{ctx}: typing goes on after another edit");
                }
                check_model(&doc, &ctx);
            }
            Err(_) => {
                assert_eq!(doc.source(), before_src, "{ctx}: refused but talk changed");
                if choice < 18 {
                    assert_eq!(doc.text().cloned(), before_edit, "{ctx}: refused but model changed");
                }
                assert_eq!(
                    (doc.can_undo(), doc.can_redo(), doc.undo_label()),
                    before_hist,
                    "{ctx}: refused but history changed"
                );
            }
        }
    }
    doc.end_text().ok();
    while doc.redo().expect("redo to tip").is_some() {}
    let tip = doc.source().to_string();
    let mut states = vec![tip.clone()];
    let mut labels = vec![];
    while let Some(label) = doc.undo_label() {
        labels.push(label);
        doc.undo().expect("undo");
        states.push(doc.source().to_string());
        check_model(&doc, &format!("seed {seed}: undo"));
        assert!(states.len() < 5000);
    }
    assert_eq!(doc.source(), original, "seed {seed}: undo all did not restore");
    let mut at = states.len() - 1;
    while doc.redo().expect("redo").is_some() {
        at -= 1;
        check_model(&doc, &format!("seed {seed}: redo"));
        assert_eq!(
            doc.source(),
            states[at],
            "seed {seed}: redo of {:?} gave the wrong text",
            labels[at]
        );
    }
    assert_eq!(doc.source(), tip, "seed {seed}: redo all did not give the final text");
}

fn seeds() -> std::ops::Range<u64> {
    match std::env::var("SEEDS") {
        Ok(seeds) => match seeds.split_once("..") {
            Some((from, to)) => from.parse().unwrap()..to.parse().unwrap(),
            None => {
                let seed: u64 = seeds.parse().unwrap();
                seed..seed + 1
            }
        },
        Err(_) => 0..SEEDS_EACH,
    }
}

#[test]
fn random_typing_keeps_the_text_and_the_talk_together() {
    for (t, talk) in TALKS.iter().enumerate() {
        for seed in seeds() {
            run(talk, seed * 31 + t as u64 + 1, STEPS);
        }
    }
}
