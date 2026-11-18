//! Writing where things and areas were moved to: only their `x` and `y` values change, each a
//! whole number, and one undo gives back the talk exactly.

use proptest::prelude::*;
use slides_engine::Fonts;
use stagepack_studio::arrange::{Placed, kept_move, moved};
use stagepack_studio::document::{Document, EditRun, MOVE, Thing};

/// `source` with the value of every `x` and `y`, in a tag or on an area's settings line, put
/// as `#`: what stays the same whatever moved.
fn without_places(source: &str) -> String {
    source
        .lines()
        .map(|line| {
            if line.starts_with("region.") {
                line.split(' ')
                    .map(|word| if word.starts_with("x=") || word.starts_with("y=") { &word[..2] } else { word })
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                let mut out = String::new();
                let mut rest = line;
                while let Some(at) = rest.find(' ') {
                    out.push_str(&rest[..=at]);
                    rest = &rest[at + 1..];
                    if let Some(value) = rest.strip_prefix("x=\"").or_else(|| rest.strip_prefix("y=\"")) {
                        let end = value.find('"').expect("a closed value");
                        out.push_str(&rest[..3]);
                        out.push_str("#\"");
                        rest = &value[end + 1..];
                    }
                }
                out.push_str(rest);
                out
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every `x` and `y` value in `source`, in the order written.
fn places(source: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in source.lines() {
        if line.starts_with("region.") {
            found.extend(line.split(' ').filter_map(|word| word.strip_prefix("x=").or_else(|| word.strip_prefix("y="))).map(str::to_string));
        } else {
            let mut values: Vec<(usize, String)> = line
                .match_indices(" x=\"")
                .chain(line.match_indices(" y=\""))
                .map(|(at, _)| {
                    let value = &line[at + 4..];
                    (at, value[..value.find('"').expect("a closed value")].to_string())
                })
                .collect();
            values.sort();
            found.extend(values.into_iter().map(|(_, value)| value));
        }
    }
    found
}

fn talk(shapes: &[(f32, f32, f32, f32)], area: (i32, i32)) -> String {
    let mut talk = format!("---\ntitle: T\nregion.body: x={} y={}\n---\n\n# Moved\n\nSome words\n\n", area.0, area.1);
    for (n, (x, y, w, h)) in shapes.iter().enumerate() {
        talk.push_str(&format!("<Shape kind=\"rect\" fill=\"#2563eb\" x=\"{x:.1}\" y=\"{y:.1}\" w=\"{w:.0}\" h=\"{h:.0}\" rotate=\"{}\" />\n\n", n * 20));
    }
    talk.push_str("---\n\n# Two\n");
    talk
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    #[test]
    fn a_move_changes_only_x_and_y_and_undoes_exactly(
        shapes in proptest::collection::vec((-500f32..2500., -500f32..1500., 10f32..400., 10f32..400.), 1..5),
        area in (0i32..400, 0i32..300),
        with_area: bool,
        dx in -3000f32..3000.,
        dy in -3000f32..3000.,
    ) {
        let source = talk(&shapes, area);
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("slides.md"), &source).unwrap();
        let mut doc = Document::open(dir.path(), Fonts::bundled_only()).unwrap();
        prop_assert!(doc.select_all_things());
        if with_area {
            prop_assert!(doc.select_also(0, Thing::Region("body".into())));
        }
        let (things, left_out) = doc.movable_selected();
        prop_assert!(!left_out);
        let starts: Vec<Placed> = things.iter().map(|(_, placed)| *placed).collect();
        let by = kept_move(&starts, (dx, dy));
        let to: Vec<(Thing, Placed)> = things.iter().map(|(thing, placed)| (thing.clone(), moved(placed, by))).collect();
        doc.place_things(MOVE, &to, EditRun::New).unwrap();
        let after = doc.source().to_string();
        prop_assert_eq!(without_places(&after), without_places(&source), "only x and y values change");
        let (old, new) = (places(&source), places(&after));
        prop_assert_eq!(old.len(), new.len());
        for (old, new) in old.iter().zip(&new) {
            prop_assert!(old == new || new.parse::<i64>().is_ok(), "{} became {}", old, new);
        }
        if after == source {
            prop_assert!(!doc.can_undo(), "no move, no step");
        } else {
            doc.undo().unwrap();
            prop_assert_eq!(doc.source(), source.as_str(), "one undo gives it all back");
            prop_assert!(!doc.can_undo());
        }
    }
}
