//! Random runs of typing on a talk's slides, with undo and redo among them: the text being
//! typed always reads as the talk holds it, other slides keep their bytes, a refusal changes
//! nothing, and undoing everything gives back the talk as it was opened. Random runs of
//! arranging and animating things too, among typing, undo and redo: a place written reads back
//! as the place asked for, and undoing everything gives back the talk as it was opened. And
//! random runs of presses, clicks and keys in the window, after each of which nothing on the
//! slide is selected, typed in or waiting while the list of slides or the notes have the
//! keyboard.
//!
//! Each talk runs with a few seeds; `SEEDS=n` runs seed `n` alone, and `SEEDS=a..b` a range.
//! `PROPTEST_CASES=n` runs the arranging with `n` seeds in all, spread over its talks.

#[path = "support/typing.rs"]
mod typing;

use gpui_kit::{Modifiers, TestAppContext};
use slides_engine::{Enter, Fonts, Transition, layouts};
use slides_format::{FlowPlace, Motion, Order, ParaKind, Pos, StyleBit, parse_talk, split_slides, storable, text_flows};
use stagepack_studio::animate::Choice;
use stagepack_studio::arrange::{AlignTo, Handle, Placed, aligned, distributed, kept_move, moved, resized, turned_to, whole};
use stagepack_studio::document::{Document, EditRun, MOVE, NUDGE, RESIZE, TURN, Thing};
use stagepack_studio::notes_panel::NOTES_ID;
use stagepack_studio::slide_strip::strip_row_id;

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

/// Talks with things to arrange: placed things written by hand in every way, things in the
/// text (a table, a picture, a list), a box holding words, areas placed by a slide's settings,
/// and steps a person wrote with gaps in them.
const ARRANGED: &[&str] = &[
    "---\ntitle: T\n---\n\n# Shapes\n\nSome words here\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n![A cat](cat.png)\n\n\
<Shape kind=\"rect\" fill=\"#2563eb\" x=\"200\" y=\"500\" w=\"200\" h=\"200\" />\n\n\
<Shape kind=\"ellipse\" fill=\"#dc2626\" x=\"700\" y=\"550\" w=\"100\" h=\"100\" click=\"3\" />\n\n\
<Icon name=\"arrow-right\" x=\"1400\" y=\"800\" size=\"96\" />\n\n\
<Box x=\"900\" y=\"200\" w=\"300\" h=\"200\" rotate=\"37\">\n\nIn a box\n\n</Box>\n\n---\nlayout: two-columns\n---\n\n# Two\n\n- one\n- two\n\n<Region name=\"right\" />\n\nright words\n\n---\n\n# Three\n\n<Shape kind=\"rect\" x='10' y=20 />\n",
    "---\r\nlayout: content\r\nregion.body: y 300   x 100 w 900\r\nregion.title: rotate 10 click 2\r\n---\r\n\r\n# Placed\r\n\r\n- a\r\n- b\r\n- c\r\n\r\n<Shape\r\n  kind=\"ellipse\"\r\n  x=\"1200.5\" y=\"-20\" w=\"3840\" h=\"1\"\r\n/>\r\n\r\n<Icon name=\"star\" x=\"400\" y=\"700\" w=\"120\" click=\"x\" enter=\"fade\" />\r\n\r\n<Shape kind=\"rect\" x=\"0\" y=\"0\" w=\"1920\" h=\"1080\" click=\"9\" />\r\n\r\n---\r\n\r\n# Two\r\n\r\nwords\r\n\r\n<Notes>\r\nsay this\r\n</Notes>\r\n",
    "# One\n\n<Box x=\"100\" y=\"100\" w=\"400\" h=\"300\">\n\n<Icon name=\"star\" />\n\n</Box>\n\n<Shape kind=\"rect\" x=\"600\" y=\"600\" w=\"300\" h=\"200\" rotate=\"-360\" />\n\n<Shape kind=\"rect\" x=\"600\" y=\"600\" w=\"300\" h=\"200\" rotate=\"359.9\" />\n\n<Shape kind=\"rect\" x=\"600\" y=\"600\" w=\"300\" h=\"200\" rotate=\"90\" />\n\n| x | y |\n| - | - |\n| 1 | 2 |\n\n---\nlayout: blank\ntransition: push\n---\n\n<Shape kind=\"line\" x=\"100\" y=\"500\" w=\"800\" h=\"8\" />\n",
];

/// How many steps a run of arranging makes.
const ARRANGE_STEPS: usize = 160;
/// The turns a thing is given, written by hand or by a drag.
const TURNS: [f32; 8] = [0., 15., 37., 90., 180., -45., 12.5, -180.];
const ALIGNS: [AlignTo; 6] = [AlignTo::Left, AlignTo::Centre, AlignTo::Right, AlignTo::Top, AlignTo::Middle, AlignTo::Bottom];
const ORDERS: [Order; 4] = [Order::Forward, Order::Backward, Order::Front, Order::Back];

/// Every thing and area of the current slide, as Tab reaches them.
fn things_here(doc: &mut Document) -> Vec<Thing> {
    let slide = doc.current();
    doc.tab_order(slide)
}

/// Whether what the slide shows of each of `asked` is where it was asked to go, as written: its
/// position and size to a whole unit, its turn to a tenth of a degree.
fn reads_back(doc: &mut Document, asked: &[Placed], ctx: &str) {
    let (now, _) = doc.movable_selected();
    if now.len() != asked.len() {
        return;
    }
    for ((thing, now), asked) in now.iter().zip(asked) {
        let (a, b) = (now.rect, asked.rect);
        let near = |is: f32, was: f32, by: f32| (is - was).abs() <= by;
        assert!(
            near(a.x, b.x, 0.51) && near(a.y, b.y, 0.51) && near(a.w, b.w, 0.51) && near(a.h, b.h, 0.51),
            "{ctx}: {thing:?} asked for {b:?}, reads back {a:?}\n{}",
            doc.source()
        );
        let turn = (now.rotate - asked.rotate).rem_euclid(360.);
        assert!(turn.min(360. - turn) <= 0.051, "{ctx}: {thing:?} turned {} for {}\n{}", now.rotate, asked.rotate, doc.source());
    }
}

fn arrange_run(talk: &str, seed: u64, steps: usize) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), talk).unwrap();
    let mut doc = Document::open(dir.path(), Fonts::bundled_only()).unwrap();
    let original = doc.source().to_string();
    let errors = |source: &str| parse_talk(source).slides.iter().filter(|slide| slide.error.is_some()).count();
    let first_errors = errors(&original);
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    for step in 0..steps {
        let ctx = format!("arranging seed {seed} step {step}");
        let before_src = doc.source().to_string();
        let before_hist = (doc.can_undo(), doc.can_redo(), doc.undo_label());
        let here = doc.current();
        let choice = rng.below(22);
        // What a place written should read back as.
        let mut asked: Vec<Placed> = Vec::new();
        let result = match choice {
            0 => {
                doc.go_to(rng.below(doc.slide_count()));
                Ok(vec![])
            }
            // Picking: one thing, one more, an area, every thing, or nothing.
            1 | 2 => {
                let things = things_here(&mut doc);
                if !things.is_empty() {
                    let thing = things[rng.below(things.len())].clone();
                    if choice == 1 {
                        doc.deselect();
                    }
                    doc.select_also(here, thing);
                }
                Ok(vec![])
            }
            3 => {
                if rng.below(2) == 0 { doc.select_all_things(); } else { doc.deselect(); }
                Ok(vec![])
            }
            // A drag of what's selected, or a run of arrow keys.
            4 | 5 => {
                let (things, _) = doc.movable_selected();
                let starts: Vec<Placed> = things.iter().map(|(_, placed)| *placed).collect();
                let by = if choice == 4 {
                    (rng.below(2401) as f32 - 1200., rng.below(1401) as f32 - 700.)
                } else {
                    [(1., 0.), (-1., 0.), (0., 10.), (0., -10.)][rng.below(4)]
                };
                let by = kept_move(&starts, by);
                let places: Vec<(Thing, Placed)> = things.iter().map(|(thing, placed)| (thing.clone(), moved(placed, by))).collect();
                asked = places.iter().map(|(_, placed)| *placed).collect();
                let run = match (choice, doc.open_run()) {
                    (5, Some(run)) if doc.undo_label() == Some(NUDGE) && rng.below(3) != 0 => EditRun::Continue(run),
                    _ => EditRun::New,
                };
                doc.place_things(if choice == 4 { MOVE } else { NUDGE }, &places, run)
            }
            // A handle dragged, or the turn handle.
            6 | 7 => {
                let (things, _) = doc.movable_selected();
                match things.as_slice() {
                    [(thing, start)] => {
                        let to = (rng.below(2200) as f32 - 140., rng.below(1300) as f32 - 110.);
                        let (label, now) = if choice == 6 {
                            let handle = Handle::ALL[rng.below(8)];
                            (RESIZE, whole(start, &resized(start, handle, to, rng.below(2) == 0), handle))
                        } else {
                            let turned = turned_to(start, to, rng.below(2) == 0);
                            let turned = if rng.below(4) == 0 { Placed { rotate: TURNS[rng.below(TURNS.len())], ..turned } } else { turned };
                            (TURN, turned)
                        };
                        asked = vec![now];
                        doc.place_things(label, &[(thing.clone(), now)], EditRun::New)
                    }
                    _ => Ok(vec![]),
                }
            }
            8 => {
                let to = ALIGNS[rng.below(ALIGNS.len())];
                let (things, _) = doc.movable_selected();
                asked = aligned(&things.iter().map(|(_, placed)| *placed).collect::<Vec<_>>(), to);
                doc.align_things(to)
            }
            9 => {
                let across = rng.below(2) == 0;
                let (things, _) = doc.movable_selected();
                asked = distributed(&things.iter().map(|(_, placed)| *placed).collect::<Vec<_>>(), across).unwrap_or_default();
                doc.distribute_things(across)
            }
            10 => doc.reorder_selected(ORDERS[rng.below(ORDERS.len())]),
            11 | 12 => {
                let choice = [Choice::Animate, Choice::Remove, Choice::Earlier, Choice::Later, Choice::Each(true), Choice::Each(false)][rng.below(6)];
                doc.animate(choice)
            }
            13 => doc.set_entrance([Enter::Appear, Enter::Fade, Enter::FlyUp][rng.below(3)]),
            14 => doc.set_slide_transition([Transition::None, Transition::Fade, Transition::Push][rng.below(3)], rng.below(4) == 0),
            // A thing in the text dragged out to a place of its own.
            15 => match doc.liftable_selected() {
                Some(Ok((_, placed))) => {
                    let to = moved(&placed, (rng.below(400) as f32 - 200., rng.below(400) as f32 - 200.));
                    doc.lift_selected(to)
                }
                _ => Ok(vec![]),
            },
            16 => doc.set_layout(layouts()[rng.below(layouts().len())].name),
            // Shown moved, as during a drag, then let go of with Escape.
            17 => {
                let (things, _) = doc.movable_selected();
                let places: Vec<(Thing, Placed)> = things.iter().map(|(thing, placed)| (thing.clone(), moved(placed, (50., 50.)))).collect();
                doc.preview_places(&places);
                doc.drop_preview();
                Ok(vec![])
            }
            // Typing among it: in an area, a letter, and the end of it.
            18 => {
                let region = ["title", "body", "right"][rng.below(3)];
                doc.begin_text_in_region(here, region);
                if doc.text().is_some() { doc.apply_text(|edit| edit.insert("a")) } else { Ok(vec![]) }
            }
            19 => doc.end_text(),
            20 => doc.undo().map(|_| vec![]),
            _ => doc.redo().map(|_| vec![]),
        };
        match result {
            Ok(changed) => {
                // Arranging and animating one slide never changes another's bytes.
                let one_slide = matches!(choice, 4..=13 | 15..=17);
                if one_slide {
                    let a = slices(&before_src);
                    let b = slices(doc.source());
                    assert_eq!(a.len(), b.len(), "{ctx}: slide count changed by choice {choice}");
                    for i in (0..a.len()).filter(|&i| i != here) {
                        assert_eq!(a[i], b[i], "{ctx}: slide {i} changed by choice {choice}");
                    }
                }
                if choice == 17 {
                    assert_eq!(doc.source(), before_src, "{ctx}: a preview wrote something");
                    assert!(doc.placing().is_none(), "{ctx}: the preview is still shown");
                }
                if !changed.is_empty() && !asked.is_empty() {
                    reads_back(&mut doc, &asked, &ctx);
                }
                assert!(errors(doc.source()) <= first_errors, "{ctx}: a slide can't be read after choice {choice}\n{}", doc.source());
                check_model(&doc, &ctx);
            }
            Err(_) => {
                assert_eq!(doc.source(), before_src, "{ctx}: refused but talk changed");
                assert_eq!((doc.can_undo(), doc.can_redo(), doc.undo_label()), before_hist, "{ctx}: refused but history changed");
            }
        }
    }
    doc.end_text().ok();
    while doc.redo().expect("redo to tip").is_some() {}
    let tip = doc.source().to_string();
    while doc.undo_label().is_some() {
        doc.undo().expect("undo");
    }
    assert_eq!(doc.source(), original, "arranging seed {seed}: undo all did not restore");
    while doc.redo().expect("redo").is_some() {}
    assert_eq!(doc.source(), tip, "arranging seed {seed}: redo all did not give the final text");
}

/// How many seeds the arranging runs with, over all its talks.
fn arrange_seeds() -> std::ops::Range<u64> {
    match std::env::var("PROPTEST_CASES").ok().and_then(|cases| cases.parse::<u64>().ok()) {
        Some(cases) if std::env::var("SEEDS").is_err() => 0..cases.div_ceil(ARRANGED.len() as u64),
        _ => seeds(),
    }
}

#[test]
fn random_arranging_reads_back_and_undoes_to_the_first_bytes() {
    for (t, talk) in ARRANGED.iter().enumerate() {
        for seed in arrange_seeds() {
            arrange_run(talk, seed * 31 + t as u64 + 1, ARRANGE_STEPS);
        }
    }
}

/// How many steps a run in the window makes.
const WINDOW_STEPS: usize = 70;

/// Whether the list of slides or the notes have the keyboard.
fn list_or_notes_have_keyboard(typing: &typing::Typing, cx: &mut TestAppContext) -> bool {
    typing.with_window(cx, |window, cx| {
        let view = typing.view.read(cx);
        view.slide_strip().read(cx).focus_handle().contains_focused(window, cx) || view.notes_panel().read(cx).is_focused(window, cx)
    })
}

/// A right-click at `at`.
fn right_click(typing: &typing::Typing, cx: &mut TestAppContext, at: gpui_kit::Point<gpui_kit::Pixels>) {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{InputEvent as _, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent};
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        let modifiers = Modifiers::default();
        window.dispatch_event(MouseMoveEvent { position: at, pressed_button: None, modifiers }.to_platform_input(), cx);
        window.dispatch_event(
            MouseDownEvent { button: MouseButton::Right, position: at, modifiers, click_count: 1, first_mouse: false }.to_platform_input(),
            cx,
        );
        window.dispatch_event(MouseUpEvent { button: MouseButton::Right, position: at, modifiers, click_count: 1 }.to_platform_input(), cx);
    });
    typing.settle(cx);
}

fn window_run(cx: &mut TestAppContext, talk: &str, seed: u64) {
    use stagepack_studio::insert_tools::INSERT_TEXT_BOX_ID;
    use stagepack_studio::ribbon::TAB_INSERT_ID;
    let typing = typing::open(cx, talk);
    cx.simulate_window_resize(typing.handle, gpui_kit::size(gpui_kit::px(1800.), gpui_kit::px(900.)));
    typing.settle(cx);
    let original = typing.source(cx);
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    for step in 0..WINDOW_STEPS {
        let choice = rng.below(22);
        let elements = typing.layout(cx).elements().to_vec();
        let element = (!elements.is_empty()).then(|| elements[rng.below(elements.len())].clone());
        let modifiers = [Modifiers::default(), Modifiers::shift(), Modifiers::control()][rng.below(3)];
        match (choice, element) {
            (0, Some(element)) => {
                let at = typing::middle(&typing, cx, &element);
                typing.clicks(cx, at, 1, modifiers);
            }
            (1, Some(element)) => {
                let at = typing::middle(&typing, cx, &element);
                typing.clicks(cx, at, 2, Modifiers::default());
            }
            (2, Some(element)) => {
                let from = typing::middle(&typing, cx, &element);
                let to = typing.at(cx, rng.below(1900) as f32, rng.below(1060) as f32);
                typing.drag(cx, from, to);
            }
            (3, Some(element)) => {
                let at = typing::middle(&typing, cx, &element);
                right_click(&typing, cx, at);
                typing.press(cx, "escape");
            }
            (4, _) => {
                let at = typing.at(cx, rng.below(1920) as f32, rng.below(1080) as f32);
                typing.click(cx, at);
            }
            (5, _) => typing.type_text(cx, ["a", "Z", " ", "-"][rng.below(4)]),
            (6, _) => typing.press(cx, ["right", "left", "up", "down", "shift-right", "shift-down"][rng.below(6)]),
            (7, _) => typing.press(cx, "escape"),
            (8, _) => typing.press(cx, ["delete", "backspace"][rng.below(2)]),
            (9, _) => typing.press(cx, ["ctrl-z", "ctrl-z", "ctrl-shift-z", "ctrl-y"][rng.below(4)]),
            (10, _) | (11, _) => {
                let row = rng.below(typing.slide_count(cx));
                if let Some(bounds) = typing::bounds_of(&typing, cx, strip_row_id(row)) {
                    let at = bounds.center();
                    match rng.below(3) {
                        0 => typing.click(cx, at),
                        // Pressed and moved a few pixels before letting go.
                        1 => typing.drag(cx, at, gpui_kit::point(at.x + gpui_kit::px(7.), at.y)),
                        _ => {
                            right_click(&typing, cx, at);
                            typing.press(cx, "escape");
                        }
                    }
                }
            }
            (12, _) => typing::click_on(&typing, cx, NOTES_ID),
            (13, _) => typing.press(cx, "ctrl-a"),
            (14, _) => typing.press(cx, ["tab", "shift-tab"][rng.below(2)]),
            (15, _) => typing.press(cx, ["pagedown", "pageup"][rng.below(2)]),
            (16, _) => {
                typing::click_on(&typing, cx, TAB_INSERT_ID);
                typing::click_on(&typing, cx, INSERT_TEXT_BOX_ID);
            }
            (17, _) => typing.press(cx, "ctrl-d"),
            _ => typing.type_text(cx, "b"),
        }
        let held = list_or_notes_have_keyboard(&typing, cx);
        if held {
            let document = typing.document(cx);
            let (selected, editing, waiting) =
                document.read_with(cx, |document, _| (document.selection().cloned(), document.text().is_some(), document.new_box().is_some()));
            assert!(
                selected.is_none() && !editing && !waiting,
                "seed {seed} step {step}, choice {choice}: the list or the notes have the keyboard while {selected:?} is selected, \
                 typing {editing}, a box waiting {waiting}\n{}",
                typing.source(cx)
            );
        }
        assert!(
            parse_talk(&typing.source(cx)).slides.iter().all(|slide| slide.error.is_none()),
            "seed {seed} step {step}: a slide can't be read\n{}",
            typing.source(cx)
        );
    }
    typing.press(cx, "escape");
    let document = typing.document(cx);
    let back = document.update(cx, |document, _| {
        document.end_text().ok();
        while document.undo().expect("undo").is_some() {}
        document.source().to_string()
    });
    assert_eq!(back, original, "seed {seed}: undo all in the window did not restore");
}

const IN_THE_WINDOW: &[&str] = &[
    "---\ntitle: T\n---\n\n# Shapes\n\nSome words here\n\n\
<Shape kind=\"rect\" fill=\"#2563eb\" x=\"200\" y=\"500\" w=\"200\" h=\"200\" />\n\n\
<Shape kind=\"ellipse\" fill=\"#dc2626\" x=\"700\" y=\"550\" w=\"100\" h=\"100\" />\n\n\
<Icon name=\"arrow-right\" x=\"1400\" y=\"800\" size=\"96\" />\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n<Notes>\nsay it\n</Notes>\n\n---\n\n# Two\n\n- one\n- two\n",
    "---\nlayout: two-columns\nregion.left: x 100 y 300\n---\n\n# Columns\n\nleft words\n\n<Region name=\"right\" />\n\n<Box x=\"1000\" y=\"600\" w=\"400\" h=\"200\" rotate=\"30\">\n\nIn a box\n\n</Box>\n\n---\n\n# Next\n\n---\n\n# Last\n",
];

#[gpui_kit::test]
fn random_presses_keep_the_slide_free_while_the_list_or_notes_have_the_keyboard(cx: &mut TestAppContext) {
    for (t, talk) in IN_THE_WINDOW.iter().enumerate() {
        for seed in seeds() {
            window_run(cx, talk, seed * 31 + t as u64 + 1);
        }
    }
}
