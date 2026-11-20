//! Random runs of typing on a talk's slides, with undo and redo among them: the text being
//! typed always reads as the talk holds it, other slides keep their bytes, a refusal changes
//! nothing, and undoing everything gives back the talk as it was opened. Random runs of
//! arranging and animating things too, among typing, undo and redo: a place written reads back
//! as the place asked for, and undoing everything gives back the talk as it was opened. And
//! random runs of presses, clicks and keys in the window, after each of which nothing on the
//! slide is selected, typed in or waiting while the list of slides or the notes have the
//! keyboard. Half of those runs start on the start screen with a template picked at random and
//! save the new presentation for the first time at a random step; what's saved, opened again at
//! the end, has the text the window has. Among the window's steps a made video is put on the
//! slide, from the Video tool's dialog or dropped, played and stopped; a made 3D model is put on
//! the slide the same ways, turned by its button or Space, dragged round and zoomed, its view
//! set, and turning done; a made terminal recording is put on the slide the same ways, played and
//! stopped by its button or Space; and the show started, stepped through, dragged on, clicked
//! through a recording's marks and back, and ended: a video plays, a model turns and a recording
//! plays only while it alone is selected in the editor, nothing plays once the show is over and
//! every replay's thread ends, and at the end no model is turned or held on the graphics card.
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
/// text (a table, a picture, a list), a box holding words, a box holding a thing alone, areas
/// placed by a slide's settings, steps a person wrote with gaps in them, and notes after the
/// things of a slide.
const ARRANGED: &[&str] = &[
    "---\ntitle: T\n---\n\n# Shapes\n\nSome words here\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n![A cat](cat.png)\n\n\
<Shape kind=\"rect\" fill=\"#2563eb\" x=\"200\" y=\"500\" w=\"200\" h=\"200\" />\n\n\
<Shape kind=\"ellipse\" fill=\"#dc2626\" x=\"700\" y=\"550\" w=\"100\" h=\"100\" click=\"3\" />\n\n\
<Icon name=\"arrow-right\" x=\"1400\" y=\"800\" size=\"96\" />\n\n\
<Box x=\"900\" y=\"200\" w=\"300\" h=\"200\" rotate=\"37\">\n\nIn a box\n\n</Box>\n\n---\nlayout: two-columns\n---\n\n# Two\n\n- one\n- two\n\n<Region name=\"right\" />\n\nright words\n\n---\n\n# Three\n\n<Shape kind=\"rect\" x='10' y=20 />\n",
    "---\r\nlayout: content\r\nregion.body: y 300   x 100 w 900\r\nregion.title: rotate 10 click 2\r\n---\r\n\r\n# Placed\r\n\r\n- a\r\n- b\r\n- c\r\n\r\n<Shape\r\n  kind=\"ellipse\"\r\n  x=\"1200.5\" y=\"-20\" w=\"3840\" h=\"1\"\r\n/>\r\n\r\n<Icon name=\"star\" x=\"400\" y=\"700\" w=\"120\" click=\"x\" enter=\"fade\" />\r\n\r\n<Shape kind=\"rect\" x=\"0\" y=\"0\" w=\"1920\" h=\"1080\" click=\"9\" />\r\n\r\n---\r\n\r\n# Two\r\n\r\nwords\r\n\r\n<Notes>\r\nsay this\r\n</Notes>\r\n",
    "# One\n\n<Box x=\"100\" y=\"100\" w=\"400\" h=\"300\">\n\n<Icon name=\"star\" />\n\n</Box>\n\n<Shape kind=\"rect\" x=\"600\" y=\"600\" w=\"300\" h=\"200\" rotate=\"-360\" />\n\n<Shape kind=\"rect\" x=\"600\" y=\"600\" w=\"300\" h=\"200\" rotate=\"359.9\" />\n\n<Shape kind=\"rect\" x=\"600\" y=\"600\" w=\"300\" h=\"200\" rotate=\"90\" />\n\n| x | y |\n| - | - |\n| 1 | 2 |\n\n<Notes>\nSay this first.\n</Notes>\n\n---\nlayout: blank\ntransition: push\n---\n\n<Shape kind=\"line\" x=\"100\" y=\"500\" w=\"800\" h=\"8\" />\n",
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
    assert_eq!(now.len(), asked.len(), "{ctx}: {} things were placed, and {} read back\n{}", asked.len(), now.len(), doc.source());
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
                // Arranging, animating or changing the layout of one slide never changes
                // another's bytes.
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

/// Clicks the element `id` if it's drawn, and says whether it was.
fn click_if_shown(typing: &typing::Typing, cx: &mut TestAppContext, id: impl Into<gpui_kit::ElementId>) -> bool {
    let id = id.into();
    if typing::bounds_of(typing, cx, id.clone()).is_none() {
        return false;
    }
    typing::click_on(typing, cx, id);
    true
}

/// Clicks item `at` of the menu open, if one is, of `count` items; if it's still open after, as
/// after an item that's off or a line between them, Escape closes it.
fn pick_from_menu(typing: &typing::Typing, cx: &mut TestAppContext, at: usize, count: usize) {
    use gpui_kit::test::TestWindowExt as _;
    let open = |typing: &typing::Typing, cx: &mut TestAppContext| {
        typing.with_window(cx, |window, cx| {
            window.render_frame(cx);
            window.try_find("popup-menu").is_some()
        })
    };
    if !open(typing, cx) {
        return;
    }
    typing.with_window(cx, |window, cx| window.within("popup-menu").click(at % count, cx));
    typing.settle(cx);
    if open(typing, cx) {
        typing.press(cx, "escape");
    }
}

/// The thing selected, if it's a video and nothing else is.
fn video_selected(typing: &typing::Typing, cx: &TestAppContext) -> bool {
    let source = typing.source(cx);
    let things = typing.document(cx).read_with(cx, |document, _| document.selection().map_or(0, |selection| selection.things.len()));
    things == 1 && typing::selected(typing, cx).is_some_and(|(_, range)| source.get(range).is_some_and(|thing| thing.starts_with("<Video")))
}

/// The thing selected, if it's a 3D model and nothing else is.
fn model_selected(typing: &typing::Typing, cx: &TestAppContext) -> bool {
    let source = typing.source(cx);
    let things = typing.document(cx).read_with(cx, |document, _| document.selection().map_or(0, |selection| selection.things.len()));
    things == 1 && typing::selected(typing, cx).is_some_and(|(_, range)| source.get(range).is_some_and(|thing| thing.starts_with("<Model")))
}

/// The thing selected, if it's a terminal recording and nothing else is.
fn recording_selected(typing: &typing::Typing, cx: &TestAppContext) -> bool {
    let source = typing.source(cx);
    let things = typing.document(cx).read_with(cx, |document, _| document.selection().map_or(0, |selection| selection.things.len()));
    things == 1 && typing::selected(typing, cx).is_some_and(|(_, range)| source.get(range).is_some_and(|thing| thing.starts_with("<Terminal")))
}

/// The recordings playing in the editor.
fn recordings_playing(typing: &typing::Typing, cx: &TestAppContext) -> usize {
    cx.read(|cx| typing.view.read(cx).slide_canvas().read(cx).playing_recordings().len())
}

/// The canvas of the slide the audience sees, among the show's windows `shows`: it's looked for
/// afresh each time, as the audience's two canvases swap as a slide comes in.
fn audience_canvas(cx: &mut TestAppContext, shows: &[gpui_kit::AnyWindowHandle]) -> Option<gpui_kit::Entity<stagepack_studio::slide_canvas::SlideCanvas>> {
    use stagepack_studio::present_view::AudienceView;
    cx.read(|cx| {
        shows.iter().find_map(|window| {
            let root = window.downcast::<gpui_kit::base::Root>()?.read(cx).ok()?;
            let view = root.view().clone().downcast::<AudienceView>().ok()?;
            Some(view.read(cx).current().clone())
        })
    })
}

/// Waits, in real time, for every replay's thread to end, drawing the windows meanwhile.
fn replays_end(cx: &mut TestAppContext, what: &str) {
    let began = std::time::Instant::now();
    while stagepack_studio::replay::threads_running() > 0 {
        assert!(began.elapsed() < std::time::Duration::from_secs(10), "{what}: a replay's thread didn't end");
        a_moment(cx);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

/// The models turned in the editor.
fn models_turned(typing: &typing::Typing, cx: &TestAppContext) -> usize {
    cx.read(|cx| typing.view.read(cx).slide_canvas().read(cx).models_turned().len())
}

/// The app's drawer, on the software adapter where there is one and on the best there is
/// otherwise, closed as the run ends, as a program that ends while a driver lets go of a device can
/// crash in the driver. Where there's none at all, a model shows a plain panel, which the run
/// takes as it comes.
struct Drawing;

impl Drawing {
    fn start() -> Drawing {
        use slides_model::{AdapterKind, Drawer, DrawerOptions};
        let drawer = Drawer::start_with(DrawerOptions { only: Some(AdapterKind::Software), ..DrawerOptions::default() });
        let began = std::time::Instant::now();
        let software = loop {
            if let Some(ready) = drawer.ready() {
                break ready.is_ok();
            }
            assert!(began.elapsed() < std::time::Duration::from_secs(60), "the drawer never said whether it can draw");
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        let drawer = if software { drawer } else { Drawer::start() };
        if let Some(old) = stagepack_studio::model_view::use_drawer(drawer) {
            assert!(old.join().unwrap(), "the drawer before let go of its device");
        }
        Drawing
    }
}

impl Drop for Drawing {
    fn drop(&mut self) {
        let closed = stagepack_studio::model_view::close_drawer(std::time::Duration::from_secs(30));
        if !std::thread::panicking() {
            assert!(closed, "the drawer's thread never ended");
        }
    }
}

/// How many times the window runs put a model on a slide, turned one, and wrote a view: so
/// that a run of them can't pass without doing any of it.
static MODELS_INSERTED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static MODELS_TURNED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static VIEWS_SET: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// How many times the window runs put a terminal recording on a slide, played one in the editor,
/// and clicked in the show while a recording's slide showed.
static RECORDINGS_INSERTED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static RECORDINGS_PLAYED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static RECORDING_CLICKS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// How many drags the show's windows took, whole.
static SHOW_DRAGS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The videos playing in the editor.
fn videos_playing(typing: &typing::Typing, cx: &TestAppContext) -> usize {
    cx.read(|cx| typing.view.read(cx).slide_canvas().read(cx).playing_videos().len())
}

/// The windows open now that weren't among `before`.
fn windows_since(before: &[gpui_kit::AnyWindowHandle], cx: &TestAppContext) -> Vec<gpui_kit::AnyWindowHandle> {
    cx.read(|cx| cx.windows()).into_iter().filter(|window| before.iter().all(|was| was.window_id() != window.window_id())).collect()
}

/// Lets a twentieth of a second pass and draws every window, as a video playing asks for.
fn a_moment(cx: &mut TestAppContext) {
    use gpui_kit::AppContext as _;
    use gpui_kit::test::TestWindowExt as _;
    cx.executor().advance_clock(std::time::Duration::from_millis(50));
    cx.run_until_parked();
    for window in cx.read(|cx| cx.windows()) {
        cx.update_window(window, |_, window, cx| {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
        })
        .ok();
    }
    cx.run_until_parked();
}

/// A drag with the mouse in a show's window `show`, from `from` to `to`, and whether it was
/// made whole: a window that closed part-way, as the show ending on its click closes it, stops it.
fn show_drag(cx: &mut TestAppContext, show: gpui_kit::AnyWindowHandle, from: gpui_kit::Point<gpui_kit::Pixels>, to: gpui_kit::Point<gpui_kit::Pixels>) -> bool {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AppContext as _, InputEvent as _, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent};
    let held = Some(MouseButton::Left);
    let events = [
        MouseDownEvent { button: MouseButton::Left, position: from, modifiers: Modifiers::default(), click_count: 1, first_mouse: false }.to_platform_input(),
        MouseMoveEvent { position: gpui_kit::point((from.x + to.x) / 2., (from.y + to.y) / 2.), pressed_button: held, modifiers: Modifiers::default() }.to_platform_input(),
        MouseMoveEvent { position: to, pressed_button: held, modifiers: Modifiers::default() }.to_platform_input(),
        MouseUpEvent { button: MouseButton::Left, position: to, modifiers: Modifiers::default(), click_count: 1 }.to_platform_input(),
    ];
    for event in events {
        let sent = cx.update_window(show, |_, window, cx| {
            window.render_frame(cx);
            window.dispatch_event(event, cx);
        });
        if sent.is_err() {
            let open = cx.read(|cx| cx.windows()).iter().any(|window| window.window_id() == show.window_id());
            assert!(!open, "the show's window is open, and took no drag: {:?}", sent.err());
            return false;
        }
        cx.run_until_parked();
    }
    true
}

/// A turn of the wheel by `lines` at `at` on the slide.
fn model_wheel(typing: &typing::Typing, cx: &mut TestAppContext, at: gpui_kit::Point<gpui_kit::Pixels>, lines: f32) {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{InputEvent as _, ScrollDelta, ScrollEvent};
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        let event = ScrollEvent { position: at, delta: ScrollDelta::Lines(gpui_kit::point(0., lines)), ..ScrollEvent::default() };
        window.dispatch_event(event.to_platform_input(), cx);
    });
    cx.run_until_parked();
    typing.settle(cx);
}

/// Drops `path` from outside the app on the slide at (`x`, `y`), and lets its copy run.
fn drop_file(typing: &typing::Typing, cx: &mut TestAppContext, path: std::path::PathBuf, x: f32, y: f32) {
    use gpui_kit::InputEvent as _;
    use gpui_kit::test::TestWindowExt as _;
    let at = typing.at(cx, x, y);
    typing.with_window(cx, |window, cx| {
        window.render_frame(cx);
        let paths = gpui_kit::ExternalPaths([path].into_iter().collect());
        window.dispatch_event(gpui_kit::FileDropEvent::Entered { position: at, paths }.to_platform_input(), cx);
        window.dispatch_event(gpui_kit::FileDropEvent::Pending { position: at }.to_platform_input(), cx);
        window.dispatch_event(gpui_kit::FileDropEvent::Submit { position: at }.to_platform_input(), cx);
        window.render_frame(cx);
    });
    cx.run_until_parked();
    typing.settle(cx);
}

/// What a run in the window starts from.
#[derive(Clone, Copy)]
enum Start<'a> {
    /// A talk opened from its folder.
    Talk(&'a str),
    /// The start screen, a template picked at random on it, and a first save at a random step.
    StartScreen,
}

/// Where a new presentation made in a run is first saved: as a packed file or a folder.
struct FirstSave {
    at: usize,
    packed: bool,
    to: std::path::PathBuf,
}

/// A start window whose working copies, Documents folder and build permissions are all under
/// `root`, never the app's own.
fn open_start_window(cx: &mut TestAppContext, root: &std::path::Path) -> typing::Typing {
    use gpui_kit::{App, AppContext as _, Bounds, Point, WindowBounds, WindowOptions, px, size};
    use stagepack_studio::permissions::BuildPermissions;
    use stagepack_studio::recovery::Recovery;
    use stagepack_studio::workspace_view::{self, WorkspaceView};
    let places = typing::UntitledPlaces::under(root);
    let scratch = tempfile::tempdir().unwrap();
    let file = scratch.path().join("no file yet");
    let (handle, view) = cx.update(|cx: &mut App| {
        gpui_kit::init(cx);
        workspace_view::init(cx);
        let bounds = Bounds { origin: Point::default(), size: size(px(1280.), px(800.)) };
        gpui_kit::open_window(WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() }, cx, |window, cx| {
            cx.new(|cx| {
                let mut view = WorkspaceView::for_start(Some(Recovery::new(places.recovery)), window, cx);
                view.set_permissions_for_test(BuildPermissions::load(places.permissions));
                view.set_documents_folder_for_test(places.documents);
                view.set_no_watching_for_test();
                view
            })
        })
        .expect("open test window")
    });
    let typing = typing::Typing { _folder: scratch, file, handle, view };
    typing.settle(cx);
    typing
}

/// Saves the untitled talk for the first time, with Ctrl+S, in the form and at the place `save`
/// says, and checks it's no longer untitled.
fn first_save(typing: &typing::Typing, cx: &mut TestAppContext, save: &FirstSave, seed: u64) {
    use stagepack_studio::workspace_view::{SAVE_AS_FOLDER_ID, SAVE_AS_PACKED_ID};
    // Whatever menu a step left open goes first.
    typing.press(cx, "escape");
    typing.press(cx, "ctrl-s");
    let form = if save.packed { SAVE_AS_PACKED_ID } else { SAVE_AS_FOLDER_ID };
    assert!(click_if_shown(typing, cx, form), "seed {seed} step {}: Ctrl+S on an untitled talk asks which form", save.at);
    cx.run_until_parked();
    assert!(cx.did_prompt_for_new_path(), "seed {seed} step {}: a Save dialog is showing", save.at);
    let to = save.to.clone();
    cx.simulate_new_path_selection(move |_| Some(to));
    cx.run_until_parked();
    typing.settle(cx);
    let (untitled, unsaved) = typing.document(cx).read_with(cx, |document, _| (document.is_untitled(), document.has_unsaved_changes()));
    assert!(!untitled && !unsaved, "seed {seed} step {}: the first save to {:?} didn't take: {:?}", save.at, save.to, typing.status(cx));
}

/// Saves the talk where it was first saved, if it has changed since, and checks that what's
/// written there, opened again, has the text the window has.
fn saved_reads_as_the_window(typing: &typing::Typing, cx: &mut TestAppContext, save: &FirstSave, seed: u64) {
    if typing.document(cx).read_with(cx, |document, _| document.has_unsaved_changes()) {
        typing.press(cx, "ctrl-s");
        cx.run_until_parked();
        typing.settle(cx);
    }
    let shown = typing.source(cx);
    let opened = if save.packed {
        Document::open_packed(&save.to, Fonts::bundled_only()).map_err(|problem| format!("{problem:?}"))
    } else {
        Document::open(&save.to, Fonts::bundled_only())
    }
    .unwrap_or_else(|problem| panic!("seed {seed}: the saved talk can't be opened again: {problem}"));
    assert_eq!(opened.source(), shown, "seed {seed}: the talk saved at {:?} doesn't have the text the window has", save.to);
}

fn window_run(cx: &mut TestAppContext, start: Start, seed: u64) {
    use stagepack_studio::animate_tools::{EARLIER_ID, ENTRANCE_ID, LATER_ID, ONE_AT_A_TIME_ID, TRANSITION_ID};
    use stagepack_studio::arrange::Handle;
    use stagepack_studio::arrange_tools::ARRANGE_ID;
    use stagepack_studio::insert_tools::INSERT_TEXT_BOX_ID;
    use stagepack_studio::layout_picker::layout_tile_id;
    use stagepack_studio::ribbon::{LAYOUT_ID, TAB_ANIMATE_ID, TAB_HOME_ID, TAB_INSERT_ID, TAB_VIEW_ID, TOGGLE_INSPECTOR_ID};
    use stagepack_studio::slide_editor::handle_id;
    use stagepack_studio::start_screen::template_tile_id;
    use stagepack_studio::templates::templates;
    use stagepack_studio::slide_editor::{MODEL_DONE_ID, MODEL_SET_VIEW_ID, MODEL_TURN_ID, TERMINAL_PLAY_ID, TERMINAL_STOP_ID, VIDEO_PLAY_ID, VIDEO_STOP_ID};
    // The scratch folder a new presentation keeps all it keeps in, and is saved to.
    let root = tempfile::tempdir().unwrap();
    // A made video, outside the talk, to put on its slides: six pictures of a colour each.
    let clip = root.path().join("Made clip.mp4");
    std::fs::write(&clip, slides_video::make_test_video(64, 36, 6, 10, |at| [40 * at as u8, 255 - 40 * at as u8, 90, 255].repeat(64 * 36))).unwrap();
    // And a made 3D model: a box of three materials.
    let model = root.path().join("Made box.glb");
    std::fs::write(&model, slides_model::make_test_glb(slides_model::MadeGlb::Boxes)).unwrap();
    // And a made terminal recording: a short session with a mark, and a program in the alternate
    // screen.
    let recording = root.path().join("Made run.cast");
    std::fs::write(&recording, slides_terminal::make_demo_cast()).unwrap();
    let (typing, mut first_save_due) = match start {
        Start::Talk(talk) => (typing::open_with(cx, talk, Some(stagepack_studio::recovery::Recovery::new(root.path().join("recovery")))), None),
        Start::StartScreen => {
            // Its own generator, so that the steps a seed makes are those a talk's run makes.
            let mut pick = Rng(seed.wrapping_mul(0xD6E8_FEB8_6659_FD93) | 1);
            let typing = open_start_window(cx, root.path());
            let template = templates()[pick.below(templates().len())].id;
            typing::click_on(&typing, cx, template_tile_id(template));
            assert!(
                typing.view.read_with(cx, |view, _| view.document().is_some()),
                "seed {seed}: picking {template} on the start screen made no talk"
            );
            let packed = pick.below(2) == 0;
            let to = if packed { root.path().join("Documents").join("Saved.sptx") } else { root.path().join("Saved talk") };
            (typing, Some(FirstSave { at: pick.below(WINDOW_STEPS), packed, to }))
        }
    };
    // The active window, as one being used is: only then does the keyboard going to the list or
    // the notes let go of what's selected.
    typing.with_window(cx, |window, _| window.activate_window());
    cx.simulate_window_resize(typing.handle, gpui_kit::size(gpui_kit::px(1800.), gpui_kit::px(900.)));
    typing.settle(cx);
    let original = typing.source(cx);
    let mut saved = None;
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    for step in 0..WINDOW_STEPS {
        if first_save_due.as_ref().is_some_and(|save| save.at == step) {
            let save = first_save_due.take().unwrap();
            first_save(&typing, cx, &save, seed);
            saved = Some(save);
        }
        // Choices 0 to 32 each do something, and the three past them type a letter: with choice
        // 5, typing has 4 chances in 36, as it had 4 in 30 before the models' and the recordings'
        // three arms each came, so it stays among the most done.
        let choice = rng.below(36);
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
            // The Arrange menu, any of its items.
            (18, _) => {
                click_if_shown(&typing, cx, TAB_HOME_ID);
                if click_if_shown(&typing, cx, ARRANGE_ID) {
                    pick_from_menu(&typing, cx, rng.below(14), 14);
                }
            }
            // The Animate tab: an entrance, a click earlier or later, one at a time, a transition.
            (19, _) => {
                click_if_shown(&typing, cx, TAB_ANIMATE_ID);
                match rng.below(5) {
                    0 => {
                        if click_if_shown(&typing, cx, ENTRANCE_ID) {
                            pick_from_menu(&typing, cx, rng.below(4), 4);
                        }
                    }
                    1 => {
                        click_if_shown(&typing, cx, EARLIER_ID);
                    }
                    2 => {
                        click_if_shown(&typing, cx, LATER_ID);
                    }
                    3 => {
                        click_if_shown(&typing, cx, ONE_AT_A_TIME_ID);
                    }
                    _ => {
                        if click_if_shown(&typing, cx, TRANSITION_ID) {
                            pick_from_menu(&typing, cx, rng.below(3), 3);
                        }
                    }
                }
                if rng.below(2) == 0 {
                    click_if_shown(&typing, cx, TAB_HOME_ID);
                }
            }
            // A layout's tile.
            (20, _) => {
                click_if_shown(&typing, cx, TAB_HOME_ID);
                if click_if_shown(&typing, cx, LAYOUT_ID) {
                    let layout = layouts()[rng.below(layouts().len())].name;
                    if !click_if_shown(&typing, cx, layout_tile_id(layout)) {
                        typing.press(cx, "escape");
                    }
                }
            }
            // The form beside the slide, hidden or shown.
            (21, _) => {
                click_if_shown(&typing, cx, TAB_VIEW_ID);
                click_if_shown(&typing, cx, TOGGLE_INSPECTOR_ID);
                click_if_shown(&typing, cx, TAB_HOME_ID);
            }
            // A handle of the thing selected, dragged, with Shift or Ctrl held from the press.
            (22, _) => {
                let handle = Handle::ALL[rng.below(Handle::ALL.len())];
                if let Some(bounds) = typing::bounds_of(&typing, cx, handle_id(handle)) {
                    let from = bounds.center();
                    let to = gpui_kit::point(from.x + gpui_kit::px(rng.below(121) as f32 - 60.), from.y + gpui_kit::px(rng.below(121) as f32 - 60.));
                    typing.mouse_down(cx, from, 1, modifiers);
                    typing.move_held(cx, gpui_kit::point((from.x + to.x) / 2., (from.y + to.y) / 2.), modifiers);
                    typing.move_held(cx, to, modifiers);
                    typing.mouse_up(cx, to, 1, modifiers);
                }
            }
            // A made video put on the slide, from the Video tool's dialog or dropped.
            (23, _) => {
                if rng.below(2) == 0 {
                    let view = typing.view.clone();
                    let path = clip.clone();
                    cx.update(|cx| view.update(cx, |view, cx| view.chosen_videos(vec![path], cx)));
                    cx.run_until_parked();
                    typing.settle(cx);
                } else {
                    drop_file(&typing, cx, clip.clone(), rng.below(1920) as f32, rng.below(1080) as f32);
                }
            }
            // A video on the slide selected and played, or stopped, by its button or Space.
            (24, _) | (25, _) => {
                let source = typing.source(cx);
                let videos: Vec<_> = elements.iter().filter(|element| source.get(element.source.clone()).is_some_and(|thing| thing.starts_with("<Video"))).cloned().collect();
                if !videos.is_empty() && !video_selected(&typing, cx) {
                    let video = &videos[rng.below(videos.len())];
                    let at = typing.at(cx, video.rect.x + video.rect.w / 5., video.rect.y + video.rect.h / 5.);
                    typing.click(cx, at);
                }
                match rng.below(3) {
                    0 => typing.press(cx, "space"),
                    _ => {
                        if !click_if_shown(&typing, cx, VIDEO_PLAY_ID) {
                            click_if_shown(&typing, cx, VIDEO_STOP_ID);
                        }
                    }
                }
                for _ in 0..rng.below(4) {
                    a_moment(cx);
                }
            }
            // The show, started, stepped through, dragged on, as a model is turned, and ended.
            (26, _) => {
                typing.press(cx, "escape");
                let before = cx.read(|cx| cx.windows());
                typing.press(cx, ["f5", "shift-f5"][rng.below(2)]);
                cx.run_until_parked();
                let shows = windows_since(&before, cx);
                assert!(!shows.is_empty(), "seed {seed} step {step}: the show didn't start");
                for _ in 0..rng.below(6) {
                    let show = shows[rng.below(shows.len())];
                    if rng.below(5) == 0 {
                        let from = gpui_kit::point(gpui_kit::px(rng.below(600) as f32 + 100.), gpui_kit::px(rng.below(400) as f32 + 100.));
                        if show_drag(cx, show, from, gpui_kit::point(from.x + gpui_kit::px(80.), from.y + gpui_kit::px(20.))) {
                            SHOW_DRAGS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                    } else {
                        cx.simulate_keystrokes(show, ["right", "right", "left", "space"][rng.below(4)]);
                    }
                    a_moment(cx);
                }
                let which = rng.below(shows.len());
                cx.simulate_keystrokes(shows[which], "escape");
                cx.run_until_parked();
                assert!(windows_since(&before, cx).is_empty(), "seed {seed} step {step}: Escape in show window {which} didn't end the show");
                typing.with_window(cx, |window, _| window.activate_window());
                typing.settle(cx);
            }
            // A made model put on the slide, from the Model tool's dialog or dropped.
            (27, _) => {
                let before = typing.source(cx).matches("<Model").count();
                if rng.below(2) == 0 {
                    let view = typing.view.clone();
                    let path = model.clone();
                    cx.update(|cx| view.update(cx, |view, cx| view.chosen_models(vec![path], cx)));
                    cx.run_until_parked();
                    typing.settle(cx);
                } else {
                    drop_file(&typing, cx, model.clone(), rng.below(1920) as f32, rng.below(1080) as f32);
                }
                if typing.source(cx).matches("<Model").count() > before {
                    MODELS_INSERTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
            // A model on the slide selected and turned, or done, by its button or Space, and
            // dragged round or zoomed by the wheel while it's turned.
            (28, _) => {
                let source = typing.source(cx);
                let models: Vec<_> = elements.iter().filter(|element| source.get(element.source.clone()).is_some_and(|thing| thing.starts_with("<Model"))).cloned().collect();
                if !models.is_empty() && !model_selected(&typing, cx) {
                    let model = &models[rng.below(models.len())];
                    let at = typing.at(cx, model.rect.x + model.rect.w / 5., model.rect.y + model.rect.h / 5.);
                    typing.click(cx, at);
                }
                match rng.below(3) {
                    0 => typing.press(cx, "space"),
                    _ => {
                        if !click_if_shown(&typing, cx, MODEL_TURN_ID) {
                            click_if_shown(&typing, cx, MODEL_DONE_ID);
                        }
                    }
                }
                if models_turned(&typing, cx) > 0 {
                    MODELS_TURNED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let model = typing::thing(&typing, cx, "<Model");
                    let from = typing.at(cx, model.rect.x + model.rect.w / 3., model.rect.y + model.rect.h / 2.);
                    if rng.below(2) == 0 {
                        typing.drag(cx, from, gpui_kit::point(from.x + gpui_kit::px(rng.below(121) as f32 - 60.), from.y + gpui_kit::px(rng.below(61) as f32 - 30.)));
                    } else {
                        model_wheel(&typing, cx, from, [1., -1., 2.][rng.below(3)]);
                    }
                }
                for _ in 0..rng.below(4) {
                    a_moment(cx);
                }
            }
            // A model turned, if none is, dragged round, and the view it shows written, as one
            // step; and turning done, or not.
            (29, _) => {
                let source = typing.source(cx);
                let models: Vec<_> = elements.iter().filter(|element| source.get(element.source.clone()).is_some_and(|thing| thing.starts_with("<Model"))).cloned().collect();
                if models_turned(&typing, cx) == 0 && !models.is_empty() {
                    let model = &models[rng.below(models.len())];
                    let at = typing.at(cx, model.rect.x + model.rect.w / 5., model.rect.y + model.rect.h / 5.);
                    typing.click(cx, at);
                    click_if_shown(&typing, cx, MODEL_TURN_ID);
                    if models_turned(&typing, cx) > 0 {
                        let from = typing.at(cx, model.rect.x + model.rect.w / 3., model.rect.y + model.rect.h / 2.);
                        typing.drag(cx, from, gpui_kit::point(from.x + gpui_kit::px(rng.below(121) as f32 - 60.), from.y));
                        a_moment(cx);
                    }
                }
                if click_if_shown(&typing, cx, MODEL_SET_VIEW_ID) {
                    VIEWS_SET.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                if rng.below(2) == 0 {
                    click_if_shown(&typing, cx, MODEL_DONE_ID);
                }
            }
            // A made recording put on the slide, from the Terminal recording tool's dialog or dropped.
            (30, _) => {
                let before = typing.source(cx).matches("<Terminal").count();
                if rng.below(2) == 0 {
                    let view = typing.view.clone();
                    let path = recording.clone();
                    cx.update(|cx| view.update(cx, |view, cx| view.chosen_recordings(vec![path], cx)));
                    cx.run_until_parked();
                    typing.settle(cx);
                } else {
                    drop_file(&typing, cx, recording.clone(), rng.below(1920) as f32, rng.below(1080) as f32);
                }
                if typing.source(cx).matches("<Terminal").count() > before {
                    RECORDINGS_INSERTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
            // A recording on the slide selected and played, or stopped, by its button or Space.
            (31, _) => {
                let source = typing.source(cx);
                let recordings: Vec<_> = elements.iter().filter(|element| source.get(element.source.clone()).is_some_and(|thing| thing.starts_with("<Terminal"))).cloned().collect();
                if !recordings.is_empty() && !recording_selected(&typing, cx) {
                    let recording = &recordings[rng.below(recordings.len())];
                    let at = typing.at(cx, recording.rect.x + recording.rect.w / 5., recording.rect.y + recording.rect.h / 5.);
                    typing.click(cx, at);
                }
                match rng.below(3) {
                    0 => typing.press(cx, "space"),
                    _ => {
                        if !click_if_shown(&typing, cx, TERMINAL_PLAY_ID) {
                            click_if_shown(&typing, cx, TERMINAL_STOP_ID);
                        }
                    }
                }
                if recordings_playing(&typing, cx) > 0 {
                    RECORDINGS_PLAYED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                // A little real time for its thread to play in, and the frames it asks for.
                for _ in 0..rng.below(4) {
                    std::thread::sleep(std::time::Duration::from_millis(rng.below(40) as u64));
                    a_moment(cx);
                }
            }
            // The show, with a recording in the talk, clicked through, now and then back a step,
            // as its marks stop it, and ended: every replay's thread ends after.
            (32, _) => {
                if slices(&typing.source(cx)).iter().any(|slide| slide.contains("<Terminal")) {
                    typing.press(cx, "escape");
                    let before = cx.read(|cx| cx.windows());
                    typing.press(cx, ["f5", "shift-f5"][rng.below(2)]);
                    cx.run_until_parked();
                    let shows = windows_since(&before, cx);
                    assert!(!shows.is_empty(), "seed {seed} step {step}: the show didn't start");
                    for _ in 0..4 + rng.below(12) {
                        let canvas = audience_canvas(cx, &shows).expect("the audience's window");
                        if cx.read(|cx| !canvas.read(cx).replay_times().is_empty()) {
                            RECORDING_CLICKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                        let show = shows[rng.below(shows.len())];
                        cx.simulate_keystrokes(show, ["right", "right", "space", "left"][rng.below(4)]);
                        std::thread::sleep(std::time::Duration::from_millis(rng.below(30) as u64));
                        a_moment(cx);
                    }
                    let which = rng.below(shows.len());
                    cx.simulate_keystrokes(shows[which], "escape");
                    cx.run_until_parked();
                    assert!(windows_since(&before, cx).is_empty(), "seed {seed} step {step}: Escape in show window {which} didn't end the show");
                    replays_end(cx, &format!("seed {seed} step {step}: the show over"));
                    typing.with_window(cx, |window, _| window.activate_window());
                    typing.settle(cx);
                }
            }
            _ => typing.type_text(cx, "b"),
        }
        let replaying = recordings_playing(&typing, cx);
        assert!(
            replaying == 0 || (replaying == 1 && recording_selected(&typing, cx)),
            "seed {seed} step {step}, choice {choice}: {replaying} recordings play in the editor, with the recording selected {}, {:?} selected, typing {}\n{}",
            recording_selected(&typing, cx),
            typing.document(cx).read_with(cx, |document, _| document.selection().cloned()),
            typing.editing(cx).is_some(),
            typing.source(cx)
        );
        let turned = models_turned(&typing, cx);
        assert!(
            turned == 0 || (turned == 1 && model_selected(&typing, cx)),
            "seed {seed} step {step}, choice {choice}: {turned} models turn in the editor, with the model selected {}, {:?} selected, typing {}\n{}",
            model_selected(&typing, cx),
            typing.document(cx).read_with(cx, |document, _| document.selection().cloned()),
            typing.editing(cx).is_some(),
            typing.source(cx)
        );
        let playing = videos_playing(&typing, cx);
        assert!(
            playing == 0 || (playing == 1 && video_selected(&typing, cx)),
            "seed {seed} step {step}, choice {choice}: {playing} videos play in the editor, with the video selected {}, {:?} selected, typing {}\n{}",
            video_selected(&typing, cx),
            typing.document(cx).read_with(cx, |document, _| document.selection().cloned()),
            typing.editing(cx).is_some(),
            typing.source(cx)
        );
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
    assert_eq!(videos_playing(&typing, cx), 0, "seed {seed}: a video plays with nothing selected");
    assert_eq!(models_turned(&typing, cx), 0, "seed {seed}: a model turns with nothing selected");
    assert_eq!(recordings_playing(&typing, cx), 0, "seed {seed}: a recording plays with nothing selected");
    replays_end(cx, &format!("seed {seed}: at the end"));
    cx.run_until_parked();
    assert_eq!(stagepack_studio::model_view::scenes_held(), 0, "seed {seed}: a model is held on the graphics card with none turned");
    if let Some(save) = &saved {
        saved_reads_as_the_window(&typing, cx, save, seed);
    }
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
    // Half the runs open a talk, and half start on the start screen with a template.
    let starts: Vec<Start> = IN_THE_WINDOW.iter().map(|talk| Start::Talk(talk)).chain([Start::StartScreen, Start::StartScreen]).collect();
    let _drawing = Drawing::start();
    for (t, start) in starts.into_iter().enumerate() {
        for seed in seeds() {
            window_run(cx, start, seed * 31 + t as u64 + 1);
        }
    }
    let count = |counted: &std::sync::atomic::AtomicUsize| counted.load(std::sync::atomic::Ordering::Relaxed);
    let (inserted, turned, set, dragged) = (count(&MODELS_INSERTED), count(&MODELS_TURNED), count(&VIEWS_SET), count(&SHOW_DRAGS));
    let (recorded, played, clicked) = (count(&RECORDINGS_INSERTED), count(&RECORDINGS_PLAYED), count(&RECORDING_CLICKS));
    let said = format!(
        "the window runs put {inserted} models on slides, turned one {turned} times, set {set} views and dragged in the show {dragged} times; \
         put {recorded} recordings on slides, played one {played} times and clicked in the show on a recording's slide {clicked} times"
    );
    println!("{said}");
    if seeds().count() >= 5 {
        assert!(inserted > 0 && turned > 0 && set > 0 && dragged > 0, "{said}");
        assert!(recorded > 0 && played > 0 && clicked > 0, "{said}");
    }
}
