mod common;

use std::ops::Range;

use common::*;
use proptest::prelude::*;
use slides_format::*;

/// Runs `test` on text with `\n` line endings, and again with every `\n` made `\r\n`.
fn on_both(test: fn(fn(&str) -> String)) {
    test(|text| text.to_string());
    test(|text| text.replace('\n', "\r\n"));
}

fn is_placed(tag: &Tag) -> bool {
    ["x", "y", "w", "h"].iter().any(|name| tag.attr(name).and_then(|value| value.trim().parse::<f32>().ok()).is_some_and(f32::is_finite))
}

/// The slide's top-level tags, by their place among its blocks.
fn top_tags(slide: &Slide) -> Vec<(usize, &Tag)> {
    slide
        .blocks
        .iter()
        .enumerate()
        .filter_map(|(i, block)| match block {
            Block::Tag(tag) => Some((i, tag)),
            _ => None,
        })
        .collect()
}

/// The places among the slide's blocks of its placed top-level tags.
fn placed(slide: &Slide) -> Vec<usize> {
    top_tags(slide).into_iter().filter(|(_, tag)| is_placed(tag)).map(|(i, _)| i).collect()
}

/// The words of the slide's line for `region`.
fn words(slide: &Slide, region: &str) -> Vec<String> {
    let key = format!("region.{region}");
    slide.settings.as_ref().and_then(|settings| settings.get(&key)).unwrap_or_default().split_whitespace().map(str::to_string).collect()
}

/// The regions of the slide's layout, and its main one.
fn layout(slide: &Slide) -> (Vec<&'static str>, &'static str) {
    match slide.settings.as_ref().and_then(|settings| settings.get("layout")) {
        Some("two-columns") => (vec!["title", "left", "right"], "left"),
        Some("blank") => (vec!["body"], "body"),
        _ => (vec!["title", "body"], "body"),
    }
}

/// Each block of the slide that flows into a region, with its bytes and its region, in order.
fn flowing(talk: &Talk, index: usize) -> Vec<(String, &'static str)> {
    let slide = &talk.slides[index];
    let (names, main) = layout(slide);
    let mut title = names.contains(&"title").then_some("title");
    let mut current = main;
    let mut out = Vec::new();
    for block in &slide.blocks {
        let region = match block {
            Block::Region { name, .. } => {
                title = None;
                current = names.iter().find(|known| *known == name).copied().unwrap_or(main);
                continue;
            }
            Block::Tag(tag) if is_placed(tag) => continue,
            Block::Heading { .. } => title.take().unwrap_or(current),
            _ => current,
        };
        out.push((text_of(talk, block.range()), region));
    }
    out
}

/// A block's bytes, without the blank lines a list's range runs over, and with `\n` line
/// endings.
fn text_of(talk: &Talk, range: &Range<usize>) -> String {
    talk.source[range.clone()].trim_end_matches([' ', '\t', '\r', '\n']).replace("\r\n", "\n")
}

fn top_texts(talk: &Talk, index: usize) -> Vec<String> {
    talk.slides[index].blocks.iter().map(|block| text_of(talk, block.range())).collect()
}

fn sorted(mut texts: Vec<String>) -> Vec<String> {
    texts.sort();
    texts
}

/// Whether every slide but `index` has the same bytes in `after` as in `before`.
fn other_slides_alike(before: &Talk, after: &Talk, index: usize) -> bool {
    before.slides.len() == after.slides.len()
        && (0..before.slides.len())
            .filter(|&other| other != index)
            .all(|other| before.source[before.slides[other].range.clone()] == after.source[after.slides[other].range.clone()])
}

/// Whether every `\n` in `text` is part of a `\r\n`.
fn only_crlf(text: &str) -> bool {
    text.match_indices('\n').all(|(at, _)| at > 0 && text.as_bytes()[at - 1] == b'\r')
}

/// Whether the engine reads `value` for the word `name` of a region line.
fn engine_reads(name: &str, value: &str) -> bool {
    match name {
        "x" | "y" | "w" | "h" | "rotate" => value.parse::<f32>().is_ok_and(f32::is_finite),
        "click" => value == "each" || (value.bytes().all(|b| b.is_ascii_digit()) && value.parse::<u32>().is_ok_and(|c| (1..=999).contains(&c))),
        "enter" => matches!(value, "appear" | "fade" | "fly-up"),
        _ => true,
    }
}

const NAMES: [&str; 7] = ["x", "y", "w", "h", "rotate", "click", "enter"];

#[derive(Clone, Debug)]
enum Asked {
    Value(&'static str),
    Remove,
    /// What's there already.
    Same,
}

fn asked() -> impl Strategy<Value = Asked> {
    prop::sample::select(vec![
        Asked::Value("10"),
        Asked::Value("12.5"),
        Asked::Value("-3"),
        Asked::Value("each"),
        Asked::Value("fade"),
        Asked::Value("2"),
        Asked::Remove,
        Asked::Same,
        Asked::Same,
    ])
}

/// What can be given a value on a slide: its top-level tags, and every region of any layout.
fn targets(slide: &Slide) -> Vec<Target> {
    top_tags(slide)
        .into_iter()
        .map(|(_, tag)| Target::Tag(tag.range.clone()))
        .chain(["title", "body", "left", "right"].into_iter().map(|region| Target::Region(region.to_string())))
        .collect()
}

/// The value now there for `name` on `target`.
fn current(slide: &Slide, target: &Target, name: &str) -> Option<String> {
    match target {
        Target::Tag(range) => top_tags(slide).into_iter().find(|(_, tag)| tag.range == *range)?.1.attr(name).map(str::to_string),
        Target::Region(region) => region_value(slide, region, name).map(str::to_string),
    }
}

/// The edits `picks` ask for on `slide`, each target and name with the value asked.
fn asked_edits(slide: &Slide, picks: &[(usize, &'static str, Asked)]) -> Vec<(Target, &'static str, Option<String>)> {
    let all = targets(slide);
    picks
        .iter()
        .map(|(which, name, asked)| {
            let target = all[which % all.len()].clone();
            let value = match asked {
                Asked::Value(value) => Some(value.to_string()),
                Asked::Remove => None,
                Asked::Same => current(slide, &target, name),
            };
            (target, *name, value)
        })
        .collect()
}

fn value_edits<'a>(asked: &'a [(Target, &'static str, Option<String>)]) -> Vec<ValueEdit<'a>> {
    asked.iter().map(|(target, name, value)| ValueEdit { target: target.clone(), name, value: value.as_deref() }).collect()
}

/// The names asked for on `target`.
fn named<'a>(asked: &'a [(Target, &'static str, Option<String>)], target: &Target) -> Vec<&'a str> {
    asked.iter().filter(|(t, _, _)| t == target).map(|(_, name, _)| *name).collect()
}

/// `words` without those for any of `names`.
fn without(words: &[String], names: &[&str]) -> Vec<String> {
    words.iter().filter(|word| !names.iter().any(|name| word.starts_with(&format!("{name}=")))).cloned().collect()
}

fn attrs_without(tag: &Tag, names: &[&str]) -> Vec<(String, String)> {
    tag.attrs.iter().filter(|attr| !names.contains(&attr.name.as_str())).map(|attr| (attr.name.clone(), attr.value.clone())).collect()
}

/// The lines of `text`, line endings left out.
fn settings_lines(text: &str) -> Vec<&str> {
    text.lines().map(|line| line.trim_end_matches('\r')).collect()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(256), ..ProptestConfig::default() })]

    #[test]
    fn placing_changes_only_the_values_that_differ(
        source in arranged_talk(),
        index in 0..9usize,
        picks in prop::collection::vec((0..64usize, prop::sample::select(NAMES.to_vec()), asked()), 1..5),
    ) {
        let talk = parse_talk(&source);
        let index = index % talk.slides.len();
        let slide = &talk.slides[index];
        let asked = asked_edits(slide, &picks);
        let change = set_values(&talk, index, &value_edits(&asked));
        let twice = asked.iter().enumerate().any(|(i, (t, n, _))| asked[..i].iter().any(|(t2, n2, _)| t2 == t && n2 == n));
        let (regions, _) = layout(slide);
        let missing = asked.iter().any(|(target, _, _)| matches!(target, Target::Region(region) if !regions.contains(&region.as_str())));
        if twice || missing || slide.error.is_some() {
            prop_assert!(change.is_none());
            return Ok(());
        }

        // Asking for what's there already changes nothing.
        let same: Vec<_> = asked.iter().filter(|(target, name, value)| match target {
            Target::Region(region) => match value {
                Some(value) => region_value(slide, region, name) == Some(value.as_str()),
                None => !words(slide, region).iter().any(|word| word.starts_with(&format!("{name}="))),
            },
            _ => current(slide, target, name) == *value,
        }).cloned().collect();
        if !same.is_empty() {
            let nothing = set_values(&talk, index, &value_edits(&same));
            prop_assert!(nothing.as_ref().is_some_and(Change::is_empty), "{:?} for {:?}", nothing, same);
        }

        let Some(change) = change else { return Ok(()) };
        let new = change.apply(&source);
        let after = parse_talk(&new);
        prop_assert!(other_slides_alike(&talk, &after, index));
        let now = &after.slides[index];
        prop_assert_eq!(now.blocks.len(), slide.blocks.len());

        // Each value reads back as asked.
        for (target, name, value) in &asked {
            match target {
                Target::Tag(range) => {
                    let at = slide.blocks.iter().position(|block| block.range() == range).unwrap();
                    let Block::Tag(tag) = &now.blocks[at] else { panic!("not a tag any more") };
                    prop_assert_eq!(tag.attr(name), value.as_deref());
                }
                Target::Region(region) => {
                    let line = words(now, region);
                    match value {
                        Some(value) => {
                            prop_assert!(line.contains(&format!("{name}={value}")), "{:?} in {:?}", value, line);
                            if engine_reads(name, value) {
                                prop_assert_eq!(region_value(now, region, name), Some(value.as_str()));
                            }
                        }
                        None => {
                            let prefix = format!("{name}=");
                            prop_assert!(!line.iter().any(|word| word.starts_with(&prefix)), "{:?}", line);
                        }
                    }
                }
            }
        }
        // Everything not named is as it was.
        for (at, tag) in top_tags(slide) {
            let Block::Tag(new_tag) = &now.blocks[at] else { panic!("not a tag any more") };
            let names = named(&asked, &Target::Tag(tag.range.clone()));
            prop_assert_eq!(attrs_without(tag, &names), attrs_without(new_tag, &names));
        }
        for region in ["title", "body", "left", "right"] {
            let names = named(&asked, &Target::Region(region.to_string()));
            prop_assert_eq!(without(&words(slide, region), &names), without(&words(now, region), &names));
        }
        let keys = |slide: &Slide| -> Vec<(String, String)> {
            slide.settings.iter().flat_map(|settings| &settings.entries).filter(|entry| !entry.key.starts_with("region.")).map(|entry| (entry.key.clone(), entry.value.clone())).collect()
        };
        prop_assert_eq!(keys(slide), keys(now));
        // Each line of settings not named, and each attribute not named, keeps its bytes.
        let named_keys: Vec<String> = asked.iter().filter_map(|(target, _, _)| match target {
            Target::Region(region) => Some(format!("region.{region}")),
            _ => None,
        }).collect();
        let lines = |talk: &Talk| -> Vec<String> {
            talk.slides[index].settings.iter().flat_map(|settings| &settings.entries).filter(|entry| !named_keys.contains(&entry.key)).map(|entry| talk.source[entry.line.clone()].to_string()).collect()
        };
        prop_assert_eq!(lines(&talk), lines(&after));
        for (at, tag) in top_tags(slide) {
            let Block::Tag(new_tag) = &now.blocks[at] else { panic!("not a tag any more") };
            let names = named(&asked, &Target::Tag(tag.range.clone()));
            let bytes = |source: &str, tag: &Tag| -> Vec<String> {
                tag.attrs.iter().filter(|attr| !names.contains(&attr.name.as_str())).map(|attr| source[attr.name_range.start..attr.value_range.end.max(attr.name_range.end)].to_string()).collect()
            };
            prop_assert_eq!(bytes(&source, tag), bytes(&new, new_tag));
        }
        // A value asked for that's there already adds no edit, among others too.
        let rest: Vec<_> = asked.iter().filter(|edit| !same.iter().any(|other| other.0 == edit.0 && other.1 == edit.1)).cloned().collect();
        prop_assert_eq!(set_values(&talk, index, &value_edits(&rest)), Some(change.clone()));
        // Every byte outside the tags and the settings named is the same.
        let named_regions = asked.iter().any(|(target, _, _)| matches!(target, Target::Region(_)));
        for edit in &change.edits {
            let in_a_tag = asked.iter().any(|(target, _, _)| match target {
                Target::Tag(range) => {
                    let tag = top_tags(slide).into_iter().find(|(_, tag)| tag.range == *range).unwrap().1;
                    tag.range.start < edit.range.start && edit.range.end < tag.open_end
                }
                _ => false,
            });
            let in_settings = named_regions && match &slide.settings {
                Some(settings) => settings.range.start <= edit.range.start && edit.range.end <= settings.range.end,
                None => edit.range.is_empty()
                    && slide.range.start <= edit.range.start
                    && settings_lines(&edit.text).iter().all(|line| line.is_empty() || *line == "---" || line.starts_with("region.")),
            };
            prop_assert!(in_a_tag || in_settings, "{:?} in {:?}", edit, source);
        }
        if source.contains("\r\n") {
            prop_assert!(change.edits.iter().all(|edit| only_crlf(&edit.text)), "{:?}", change);
        }
    }

    #[test]
    fn setting_then_undoing_is_exact(
        source in arranged_talk(),
        index in 0..9usize,
        picks in prop::collection::vec((0..64usize, prop::sample::select(NAMES.to_vec()), asked()), 1..5),
    ) {
        let talk = parse_talk(&source);
        let index = index % talk.slides.len();
        let asked = asked_edits(&talk.slides[index], &picks);
        if let Some(change) = set_values(&talk, index, &value_edits(&asked)) {
            let new = change.apply(&source);
            prop_assert_eq!(change.inverse(&source).apply(&new), source);
        }
    }

    #[test]
    fn removing_the_last_word_removes_the_line(source in arranged_talk(), index in 0..9usize, which in 0..4usize) {
        let talk = parse_talk(&source);
        let index = index % talk.slides.len();
        let slide = &talk.slides[index];
        let (regions, _) = layout(slide);
        let region = regions[which % regions.len()];
        let key = format!("region.{region}");
        let Some(settings) = &slide.settings else { return Ok(()) };
        if settings.get(&key).is_none() || slide.error.is_some() {
            return Ok(());
        }
        // Every word with a name on the line.
        let mut names: Vec<String> = words(slide, region).iter().filter_map(|word| Some(word.split_once('=')?.0.to_string())).collect();
        names.sort();
        names.dedup();
        let edits: Vec<ValueEdit> = names.iter().map(|name| ValueEdit { target: Target::Region(region.to_string()), name, value: None }).collect();
        let change = set_values(&talk, index, &edits);
        let lines = settings.entries.iter().filter(|entry| entry.key == key).count();
        if lines > 1 {
            // The next line of that region would become the one read.
            if let Some(change) = &change {
                prop_assert!(change.is_empty() || words(slide, region).iter().any(|word| !word.contains('=')));
            }
            return Ok(());
        }
        let change = change.expect("every word can be taken off a region's only line");
        let new = change.apply(&source);
        let after = parse_talk(&new);
        prop_assert!(other_slides_alike(&talk, &after, index));
        let now = &after.slides[index];
        let left: Vec<String> = words(slide, region).into_iter().filter(|word| !word.contains('=')).collect();
        let line = now.settings.as_ref().and_then(|settings| settings.get(&key));
        if left.is_empty() && !names.is_empty() {
            prop_assert_eq!(line, None, "{:?}", new);
            if settings.entries.len() == 1 {
                prop_assert!(now.settings.as_ref().is_none_or(|settings| settings.entries.is_empty()), "{:?}", new);
            } else {
                prop_assert_eq!(now.settings.as_ref().unwrap().entries.len(), settings.entries.len() - 1);
            }
        } else {
            // A line that keeps a bare word stays, with only those.
            prop_assert_eq!(words(now, region), left);
        }
        prop_assert_eq!(change.inverse(&source).apply(&new), source);
    }

    #[test]
    fn reordering_keeps_every_blocks_bytes(
        source in arranged_talk(),
        index in 0..9usize,
        which in 0..16usize,
        order in prop::sample::select(vec![Order::Forward, Order::Backward, Order::Front, Order::Back]),
    ) {
        let talk = parse_talk(&source);
        let index = index % talk.slides.len();
        let slide = &talk.slides[index];
        let tags = top_tags(slide);
        if tags.is_empty() {
            return Ok(());
        }
        let (at, tag) = tags[which % tags.len()];
        let change = reorder(&talk, index, &tag.range, order);
        prop_assert_eq!(can_reorder(&talk, index, &tag.range, order), change.as_ref().is_some_and(|change| !change.is_empty()));
        if !is_placed(tag) {
            prop_assert!(change.is_none());
            return Ok(());
        }
        let Some(change) = change else { return Ok(()) };
        let old_placed = placed(slide);
        let k = old_placed.iter().position(|&i| i == at).unwrap();
        let last = old_placed.len() - 1;
        let to = match order {
            Order::Forward => (k + 1).min(last),
            Order::Backward => k.saturating_sub(1),
            Order::Front => last,
            Order::Back => 0,
        };
        if change.is_empty() {
            prop_assert_eq!(to, k);
            return Ok(());
        }
        let new = change.apply(&source);
        let after = parse_talk(&new);
        prop_assert!(other_slides_alike(&talk, &after, index));
        prop_assert_eq!(sorted(top_texts(&talk, index)), sorted(top_texts(&after, index)));
        let placed_texts = |talk: &Talk| -> Vec<String> { placed(&talk.slides[index]).iter().map(|&i| text_of(talk, talk.slides[index].blocks[i].range())).collect() };
        let mut expected = placed_texts(&talk);
        let moved = expected.remove(k);
        expected.insert(to, moved);
        prop_assert_eq!(placed_texts(&after), expected);
        prop_assert_eq!(flowing(&talk, index), flowing(&after, index));
        prop_assert_eq!(slide.notes.as_ref().map(|notes| &notes.text), after.slides[index].notes.as_ref().map(|notes| &notes.text));
        // Only the moved tag's lines and blank lines are taken away or written.
        let moved_text = &source[tag.range.clone()];
        for edit in &change.edits {
            let taken = &source[edit.range.clone()];
            let rest = |text: &str| text.replacen(moved_text, "", 1).chars().all(char::is_whitespace);
            prop_assert!(rest(taken) && rest(&edit.text), "{:?}", edit);
        }
        prop_assert_eq!(change.inverse(&source).apply(&new), source);
    }

    #[test]
    fn forward_then_backward_is_where_it_began(source in arranged_talk(), index in 0..9usize, which in 0..16usize) {
        let talk = parse_talk(&source);
        let index = index % talk.slides.len();
        let slide = &talk.slides[index];
        let old_placed = placed(slide);
        if old_placed.is_empty() {
            return Ok(());
        }
        let k = which % old_placed.len();
        let range = slide.blocks[old_placed[k]].range().clone();
        let Some(change) = reorder(&talk, index, &range, Order::Forward) else { return Ok(()) };
        if change.is_empty() {
            return Ok(());
        }
        let moved = parse_talk(&change.apply(&source));
        let now = &moved.slides[index];
        let range = now.blocks[placed(now)[k + 1]].range().clone();
        let back = reorder(&moved, index, &range, Order::Backward).expect("a tag moved forward can be moved back");
        let back = back.apply(&moved.source);
        // Only where nothing but blank lines lay between the two tags can the bytes be as they
        // were: a tag moved past text it lay before comes back on the near side of it.
        let next = slide.blocks[old_placed[k + 1]].range();
        if source[range_after(slide, old_placed[k])..next.start].trim().is_empty() {
            prop_assert_eq!(back, source);
        } else {
            let again = parse_talk(&back);
            let texts = |talk: &Talk| -> Vec<String> { placed(&talk.slides[index]).iter().map(|&i| text_of(talk, talk.slides[index].blocks[i].range())).collect() };
            prop_assert_eq!(texts(&again), texts(&talk));
        }
    }

    #[test]
    fn lifting_keeps_the_blocks_own_bytes(
        source in arranged_talk(),
        index in 0..9usize,
        which in 0..64usize,
        place in prop::sample::select(vec![(10.0f32, 20.0f32, 300.0f32, 100.0f32), (-5.5, 0.04, 12.25, 1e3)]),
    ) {
        let talk = parse_talk(&source);
        let index = index % talk.slides.len();
        let slide = &talk.slides[index];
        let all = all_blocks(&slide.blocks, Depth::Top);
        if all.is_empty() {
            return Ok(());
        }
        let (block, depth) = all[which % all.len()];
        let range = block.range().clone();
        let change = lift(&talk, index, &range, place);
        let refused = match block {
            Block::Region { .. } => true,
            Block::Tag(tag) => depth == Depth::Top && is_placed(tag),
            _ => false,
        };
        if depth == Depth::InATag || refused {
            prop_assert!(change.is_none());
            return Ok(());
        }
        let Some(change) = change else { return Ok(()) };
        let new = change.apply(&source);
        let after = parse_talk(&new);
        prop_assert!(other_slides_alike(&talk, &after, index));
        prop_assert_eq!(change.inverse(&source).apply(&new), source.clone());
        let now = &after.slides[index];
        let values = [place.0, place.1, place.2, place.3].map(number_text);
        let at_top = slide.blocks.iter().position(|b| b.range() == &range);
        match (block, at_top) {
            (Block::Tag(tag), Some(at)) => {
                let Block::Tag(new_tag) = &now.blocks[at] else { panic!("not a tag any more") };
                for (name, value) in ["x", "y", "w", "h"].into_iter().zip(&values) {
                    prop_assert_eq!(new_tag.attr(name), Some(value.as_str()));
                }
                prop_assert_eq!(attrs_without(tag, &["x", "y", "w", "h"]), attrs_without(new_tag, &["x", "y", "w", "h"]));
                prop_assert_eq!(top_texts(&talk, index).len(), top_texts(&after, index).len());
            }
            _ => {
                let Some(Block::Tag(boxed)) = now.blocks.last() else { panic!("no box at the end") };
                prop_assert_eq!(&boxed.name, "Box");
                for (name, value) in ["x", "y", "w", "h"].into_iter().zip(&values) {
                    prop_assert_eq!(boxed.attr(name), Some(value.as_str()));
                }
                prop_assert_eq!(boxed.children.len(), 1);
                let indent = range.start - source[..range.start].rfind('\n').map_or(0, |i| i + 1);
                prop_assert_eq!(text_of(&after, boxed.children[0].range()), dedented(&text_of(&talk, &range), indent));
                // Every other top-level block's bytes are the same.
                let mut old = top_texts(&talk, index);
                let mut kept = top_texts(&after, index);
                kept.pop();
                match at_top {
                    Some(at) => {
                        old.remove(at);
                    }
                    None => {
                        // The list it was in is as taking the block away leaves it.
                        let holder = slide.blocks.iter().position(|b| b.range().start <= range.start && range.end <= b.range().end).unwrap();
                        let deleted = parse_talk(&delete_block(&talk, index, &range).unwrap().apply(&source));
                        prop_assert_eq!(&kept[holder], &top_texts(&deleted, index)[holder]);
                        old.remove(holder);
                        kept.remove(holder);
                    }
                }
                prop_assert_eq!(old, kept);
            }
        }
    }
}

/// The end of the lines of block `at` of `slide`.
fn range_after(slide: &Slide, at: usize) -> usize {
    slide.blocks[at].range().end
}

/// `text` with up to `indent` spaces or tabs taken off the start of every line but the first.
fn dedented(text: &str, indent: usize) -> String {
    text.split('\n')
        .enumerate()
        .map(|(i, line)| {
            if i == 0 {
                return line.to_string();
            }
            let lead = line.bytes().take(indent).take_while(|b| *b == b' ' || *b == b'\t').count();
            line[lead..].to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Depth {
    Top,
    InAList,
    InATag,
}

/// Every block among `blocks` at any depth, with how deep it is.
fn all_blocks(blocks: &[Block], depth: Depth) -> Vec<(&Block, Depth)> {
    let mut out = Vec::new();
    for block in blocks {
        out.push((block, depth));
        match block {
            Block::List { items, .. } => {
                let inner = if depth == Depth::InATag { Depth::InATag } else { Depth::InAList };
                for item in items {
                    out.extend(all_blocks(item, inner));
                }
            }
            Block::Tag(tag) => out.extend(all_blocks(&tag.children, Depth::InATag)),
            _ => {}
        }
    }
    out
}

fn tag_range(talk: &Talk, slide: usize, text: &str) -> Range<usize> {
    let start = talk.slides[slide].range.start + talk.source[talk.slides[slide].range.clone()].find(text).unwrap();
    talk.slides[slide]
        .blocks
        .iter()
        .find(|block| block.range().start == start)
        .map(|block| block.range().clone())
        .expect("a block starts there")
}

fn body(region: &str) -> Target {
    Target::Region(region.to_string())
}

#[test]
fn a_region_with_no_line_gets_one() {
    on_both(|e| {
        let src = e("---\nlayout: content\n---\n# T\n\nBody\n");
        let talk = parse_talk(&src);
        let change = set_values(&talk, 0, &[ValueEdit { target: body("body"), name: "x", value: Some("10") }]).unwrap();
        assert_eq!(change.edits.len(), 1);
        assert_eq!(change.apply(&src), e("---\nlayout: content\nregion.body: x=10\n---\n# T\n\nBody\n"));

        // An empty line gets the word.
        let src = e("---\nregion.body:\n---\n# T\n");
        let change = set_values(&parse_talk(&src), 0, &[ValueEdit { target: body("body"), name: "y", value: Some("4") }]).unwrap();
        assert_eq!(change.apply(&src), e("---\nregion.body: y=4\n---\n# T\n"));
    });
}

#[test]
fn a_slide_with_no_settings_gets_a_block() {
    on_both(|e| {
        let src = e("# T\n\nBody\n");
        let talk = parse_talk(&src);
        let edits = [
            ValueEdit { target: body("body"), name: "x", value: Some("10") },
            ValueEdit { target: body("title"), name: "w", value: Some("5") },
            ValueEdit { target: body("body"), name: "y", value: Some("20") },
        ];
        let change = set_values(&talk, 0, &edits).unwrap();
        assert_eq!(change.edits.len(), 1);
        assert_eq!(change.apply(&src), e("---\nregion.body: x=10 y=20\nregion.title: w=5\n---\n# T\n\nBody\n"));

        // A later slide's `---` becomes the block's opening line.
        let src = e("# A\n\n---\n# T\n");
        let change = set_values(&parse_talk(&src), 1, &edits).unwrap();
        assert_eq!(change.apply(&src), e("# A\n\n---\nregion.body: x=10 y=20\nregion.title: w=5\n---\n# T\n"));

        // A tag and a region in one change.
        let src = e("# T\n\n<Icon name=\"a\" x=\"1\" />\n");
        let talk = parse_talk(&src);
        let tag = tag_range(&talk, 0, "<Icon");
        let edits = [
            ValueEdit { target: Target::Tag(tag.clone()), name: "x", value: Some("2") },
            ValueEdit { target: Target::Tag(tag), name: "w", value: Some("30") },
            ValueEdit { target: body("body"), name: "click", value: Some("each") },
        ];
        let change = set_values(&talk, 0, &edits).unwrap();
        assert_eq!(change.apply(&src), e("---\nregion.body: click=each\n---\n# T\n\n<Icon name=\"a\" x=\"2\" w=\"30\" />\n"));
    });
}

#[test]
fn two_edits_to_one_value_are_refused() {
    let src = "# T\n\n<Icon name=\"a\" x=\"1\" />\n";
    let talk = parse_talk(src);
    let tag = tag_range(&talk, 0, "<Icon");
    let twice = [
        ValueEdit { target: Target::Tag(tag.clone()), name: "x", value: Some("2") },
        ValueEdit { target: Target::Tag(tag.clone()), name: "x", value: None },
    ];
    assert_eq!(set_values(&talk, 0, &twice), None);
    let twice = [ValueEdit { target: body("body"), name: "x", value: Some("2") }, ValueEdit { target: body("body"), name: "x", value: Some("3") }];
    assert_eq!(set_values(&talk, 0, &twice), None);
    // The same name on two targets is fine.
    let two = [ValueEdit { target: body("body"), name: "x", value: Some("2") }, ValueEdit { target: Target::Tag(tag), name: "x", value: Some("3") }];
    assert!(set_values(&talk, 0, &two).is_some());
    // A region the layout hasn't got, a tag that isn't there, and words that can't be written.
    assert_eq!(set_values(&talk, 0, &[ValueEdit { target: body("left"), name: "x", value: Some("2") }]), None);
    assert_eq!(set_values(&talk, 0, &[ValueEdit { target: Target::Tag(0..3), name: "x", value: Some("2") }]), None);
    assert_eq!(set_values(&talk, 0, &[ValueEdit { target: body("body"), name: "x", value: Some("a b") }]), None);
    assert_eq!(set_values(&talk, 0, &[ValueEdit { target: body("body"), name: "x", value: Some("") }]), None);
    assert_eq!(set_values(&talk, 0, &[ValueEdit { target: body("body"), name: "X y", value: Some("1") }]), None);
}

#[test]
fn a_repeated_word_changes_the_one_the_engine_uses() {
    let src = "---\nregion.body: x=5 y=1 x=7 x=abc\n---\n# T\n";
    let talk = parse_talk(src);
    assert_eq!(region_value(&talk.slides[0], "body", "x"), Some("7"));
    let change = set_values(&talk, 0, &[ValueEdit { target: body("body"), name: "x", value: Some("9") }]).unwrap();
    assert_eq!(change.apply(src), "---\nregion.body: x=5 y=1 x=9 x=abc\n---\n# T\n");

    // An alignment is the first; a click the last the engine can read.
    let src = "---\nregion.body: align=left click=2 align=right click=zz\n---\n# T\n";
    let talk = parse_talk(src);
    assert_eq!(region_value(&talk.slides[0], "body", "align"), Some("left"));
    assert_eq!(region_value(&talk.slides[0], "body", "click"), Some("2"));
    let edits = [ValueEdit { target: body("body"), name: "align", value: Some("center") }, ValueEdit { target: body("body"), name: "click", value: Some("3") }];
    assert_eq!(set_values(&talk, 0, &edits).unwrap().apply(src), "---\nregion.body: align=center click=3 align=right click=zz\n---\n# T\n");

    // With none the engine reads, the last is written.
    let src = "---\nregion.body: x=abc\tx=def\n---\n# T\n";
    let talk = parse_talk(src);
    assert_eq!(region_value(&talk.slides[0], "body", "x"), None);
    let change = set_values(&talk, 0, &[ValueEdit { target: body("body"), name: "x", value: Some("1") }]).unwrap();
    assert_eq!(change.apply(src), "---\nregion.body: x=abc\tx=1\n---\n# T\n");

    // A tag's is the first, as the engine reads it.
    let src = "# T\n\n<Icon name=\"a\" x=\"1\" x=\"2\" />\n";
    let talk = parse_talk(src);
    let tag = tag_range(&talk, 0, "<Icon");
    let change = set_values(&talk, 0, &[ValueEdit { target: Target::Tag(tag), name: "x", value: Some("3") }]).unwrap();
    assert_eq!(change.apply(src), "# T\n\n<Icon name=\"a\" x=\"3\" x=\"2\" />\n");
}

#[test]
fn words_taken_off_a_line_by_hand() {
    on_both(|e| {
        let remove = |src: &str, slide: usize, region: &str, names: &[&'static str]| {
            let edits: Vec<ValueEdit> = names.iter().map(|name| ValueEdit { target: body(region), name, value: None }).collect();
            set_values(&parse_talk(src), slide, &edits).unwrap().apply(src)
        };
        let src = e("---\nlayout: content\nregion.body: x=5\tx=6\n---\n# T\n");
        assert_eq!(remove(&src, 0, "body", &["x"]), e("---\nlayout: content\n---\n# T\n"));
        let src = e("---\nregion.body: x=5 y=2\n---\n# T\n");
        assert_eq!(remove(&src, 0, "body", &["x"]), e("---\nregion.body: y=2\n---\n# T\n"));
        assert_eq!(remove(&src, 0, "body", &["y"]), e("---\nregion.body: x=5\n---\n# T\n"));
        // And the block with it, when it was the only line.
        assert_eq!(remove(&src, 0, "body", &["x", "y"]), e("# T\n"));
        let src = e("# A\n\n---\nregion.body: x=5\nregion.title: y=1\n---\n# T\n");
        assert_eq!(remove(&src, 1, "body", &["x"]), e("# A\n\n---\nregion.title: y=1\n---\n# T\n"));
        let talk = parse_talk(&src);
        let both = [ValueEdit { target: body("body"), name: "x", value: None }, ValueEdit { target: body("title"), name: "y", value: None }];
        assert_eq!(set_values(&talk, 1, &both).unwrap().apply(&src), e("# A\n\n---\n# T\n"));
        // A word that isn't there takes nothing away.
        assert!(set_values(&talk, 1, &[ValueEdit { target: body("body"), name: "w", value: None }]).unwrap().is_empty());
        // A word the line keeps keeps it.
        let src = e("---\nregion.body: x=5 foo=bar\n---\n# T\n");
        assert_eq!(remove(&src, 0, "body", &["x"]), e("---\nregion.body: foo=bar\n---\n# T\n"));
    });
}

#[test]
fn lifting_a_heading_keeps_the_title() {
    on_both(|e| {
        // Lifted out, the first heading would leave the second to be the title.
        let src = e("# T\n\n## Sub\n\nText\n");
        let talk = parse_talk(&src);
        assert_eq!(lift(&talk, 0, &tag_range(&talk, 0, "# T"), (1.0, 2.0, 3.0, 4.0)), None);
        let new = lift(&talk, 0, &tag_range(&talk, 0, "## Sub"), (1.0, 2.0, 3.0, 4.0)).unwrap().apply(&src);
        assert_eq!(new, e("# T\n\nText\n\n<Box x=\"1\" y=\"2\" w=\"3\" h=\"4\">\n\n## Sub\n\n</Box>\n"));
        // A region's marker before it says where the rest goes.
        let src = e("<Region name=\"body\" />\n\n# T\n\n## Sub\n");
        let talk = parse_talk(&src);
        assert!(lift(&talk, 0, &tag_range(&talk, 0, "# T"), (1.0, 2.0, 3.0, 4.0)).is_some());
    });
}

#[test]
fn a_tag_value_that_cant_be_written_is_refused() {
    let src = "# T\n\n<Icon name=\"a\" x=\"1\" />\n";
    let talk = parse_talk(src);
    let tag = tag_range(&talk, 0, "<Icon");
    for value in ["a\"b'c", "a\nb", "a\r\nb", "a\u{7}b", "a\tb"] {
        let edit = [ValueEdit { target: Target::Tag(tag.clone()), name: "name", value: Some(value) }];
        assert_eq!(set_values(&talk, 0, &edit), None, "{value:?}");
    }
    // One kind of quote is written between the other.
    let edit = [ValueEdit { target: Target::Tag(tag.clone()), name: "name", value: Some("a\"b") }];
    assert_eq!(set_values(&talk, 0, &edit).unwrap().apply(src), "# T\n\n<Icon name='a\"b' x=\"1\" />\n");
    // And an attribute name that isn't one.
    let edit = [ValueEdit { target: Target::Tag(tag), name: "Name", value: Some("b") }];
    assert_eq!(set_values(&talk, 0, &edit), None);
}

const THREE: &str = "# T\n\nText\n\n<Icon name=\"a\" x=\"1\" />\n\n<Icon name=\"b\" x=\"2\" />\n\n<Icon name=\"c\" x=\"3\" />\n";

#[test]
fn front_of_three() {
    on_both(|e| {
        let src = e(THREE);
        let talk = parse_talk(&src);
        let a = tag_range(&talk, 0, "<Icon name=\"a\"");
        let c = tag_range(&talk, 0, "<Icon name=\"c\"");
        let front = reorder(&talk, 0, &a, Order::Front).unwrap();
        assert_eq!(
            front.apply(&src),
            e("# T\n\nText\n\n<Icon name=\"b\" x=\"2\" />\n\n<Icon name=\"c\" x=\"3\" />\n\n<Icon name=\"a\" x=\"1\" />\n")
        );
        assert_eq!(
            reorder(&talk, 0, &c, Order::Back).unwrap().apply(&src),
            e("# T\n\nText\n\n<Icon name=\"c\" x=\"3\" />\n\n<Icon name=\"a\" x=\"1\" />\n\n<Icon name=\"b\" x=\"2\" />\n")
        );
        assert_eq!(
            reorder(&talk, 0, &a, Order::Forward).unwrap().apply(&src),
            e("# T\n\nText\n\n<Icon name=\"b\" x=\"2\" />\n\n<Icon name=\"a\" x=\"1\" />\n\n<Icon name=\"c\" x=\"3\" />\n")
        );
        assert!(can_reorder(&talk, 0, &a, Order::Front));
        assert!(!can_reorder(&talk, 0, &c, Order::Forward));
        assert!(!can_reorder(&talk, 0, &a, Order::Back));
        assert!(reorder(&talk, 0, &c, Order::Front).unwrap().is_empty());
        // Text isn't a placed tag, and nor is a tag with no place.
        let text = tag_range(&talk, 0, "Text");
        assert_eq!(reorder(&talk, 0, &text, Order::Front), None);
        let src = e("# T\n\n<Icon name=\"a\" />\n\n<Icon name=\"b\" x=\"2\" />\n");
        let talk = parse_talk(&src);
        assert_eq!(reorder(&talk, 0, &tag_range(&talk, 0, "<Icon name=\"a\""), Order::Front), None);
    });
}

#[test]
fn reorder_does_not_cross_a_region_marker_wrongly() {
    on_both(|e| {
        let src = e("---\nlayout: two-columns\n---\n# T\n\nLeft words\n\n<Icon name=\"a\" x=\"1\" />\n\n<Region name=\"right\" />\n\n<Icon name=\"b\" x=\"2\" />\n\nRight words\n");
        let talk = parse_talk(&src);
        let a = tag_range(&talk, 0, "<Icon name=\"a\"");
        let new = reorder(&talk, 0, &a, Order::Forward).unwrap().apply(&src);
        assert_eq!(
            new,
            e("---\nlayout: two-columns\n---\n# T\n\nLeft words\n\n<Region name=\"right\" />\n\n<Icon name=\"b\" x=\"2\" />\n\n<Icon name=\"a\" x=\"1\" />\n\nRight words\n")
        );
        let after = parse_talk(&new);
        assert_eq!(flowing(&talk, 0), flowing(&after, 0));
        let b = tag_range(&after, 0, "<Icon name=\"a\"");
        assert_eq!(reorder(&after, 0, &b, Order::Backward).unwrap().apply(&new), e("---\nlayout: two-columns\n---\n# T\n\nLeft words\n\n<Region name=\"right\" />\n\n<Icon name=\"a\" x=\"1\" />\n\n<Icon name=\"b\" x=\"2\" />\n\nRight words\n"));
    });
}

#[test]
fn lifting_a_table_out_of_a_list_is_refused_or_exact() {
    on_both(|e| {
        let src = e("# T\n\n- item\n\n  | a | b |\n  | --- | --- |\n  | 1 | 2 |\n\nAfter\n");
        let talk = parse_talk(&src);
        let Block::List { items, .. } = &talk.slides[0].blocks[1] else { panic!("no list") };
        let table = items[0][1].range().clone();
        let change = lift(&talk, 0, &table, (10.0, 20.0, 300.0, 100.0)).expect("a table in a list can be lifted out");
        let new = change.apply(&src);
        assert_eq!(new, e("# T\n\n- item\n\nAfter\n\n<Box x=\"10\" y=\"20\" w=\"300\" h=\"100\">\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n\n</Box>\n"));
        let after = parse_talk(&new);
        let blocks = &after.slides[0].blocks;
        assert_eq!(blocks.len(), 4, "{new:?}");
        let Block::List { items, .. } = &blocks[1] else { panic!("no list in {new:?}") };
        assert_eq!(items[0].len(), 1);
        assert_eq!(text_of(&after, blocks[2].range()), "After");
        assert!(new.ends_with(&e("<Box x=\"10\" y=\"20\" w=\"300\" h=\"100\">\n\n| a | b |\n| --- | --- |\n| 1 | 2 |\n\n</Box>\n")), "{new:?}");
        assert_eq!(change.inverse(&src).apply(&new), src);

        // A picture at the top level.
        let src = e("# T\n\n![A cat](cat.png)\n\nAfter\n");
        let talk = parse_talk(&src);
        let picture = tag_range(&talk, 0, "![A cat]");
        let new = lift(&talk, 0, &picture, (1.0, 2.0, 3.0, 4.5)).unwrap().apply(&src);
        assert_eq!(new, e("# T\n\nAfter\n\n<Box x=\"1\" y=\"2\" w=\"3\" h=\"4.5\">\n\n![A cat](cat.png)\n\n</Box>\n"));
        // Nothing in a tag is lifted, nor a region's marker.
        let src = e("# T\n\n<Box>\n\nInside\n\n</Box>\n\n<Region name=\"body\" />\n");
        let talk = parse_talk(&src);
        let Block::Tag(boxed) = &talk.slides[0].blocks[1] else { panic!("no box") };
        assert_eq!(lift(&talk, 0, boxed.children[0].range(), (1.0, 2.0, 3.0, 4.0)), None);
        assert_eq!(lift(&talk, 0, talk.slides[0].blocks[2].range(), (1.0, 2.0, 3.0, 4.0)), None);
    });
}

#[test]
fn lifting_a_top_level_tag_writes_its_place() {
    on_both(|e| {
        let src = e("# T\n\n<Icon name=\"star\" />\n");
        let talk = parse_talk(&src);
        let star = tag_range(&talk, 0, "<Icon");
        let change = lift(&talk, 0, &star, (10.0, 20.0, 300.5, 100.0)).unwrap();
        assert_eq!(change.apply(&src), e("# T\n\n<Icon name=\"star\" x=\"10\" y=\"20\" w=\"300.5\" h=\"100\" />\n"));
        // One already placed lies in no region.
        let src = e("# T\n\n<Icon name=\"star\" x=\"1\" />\n");
        let talk = parse_talk(&src);
        assert_eq!(lift(&talk, 0, &tag_range(&talk, 0, "<Icon"), (10.0, 20.0, 300.5, 100.0)), None);
    });
}

#[test]
fn content_layout_removes_the_setting() {
    on_both(|e| {
        let src = e("---\nlayout: section\n---\n# T\n");
        let talk = parse_talk(&src);
        assert_eq!(set_layout(&talk, 0, "content").unwrap().apply(&src), e("# T\n"));
        assert_eq!(set_layout(&talk, 0, "two-columns").unwrap().apply(&src), e("---\nlayout: two-columns\n---\n# T\n"));
        assert!(set_layout(&talk, 0, "section").unwrap().is_empty());

        // The regions the new layout lacks keep their lines.
        let src = e("# A\n\n---\nlayout: two-columns\nregion.right: x=5\n---\n# T\n");
        let talk = parse_talk(&src);
        assert_eq!(set_layout(&talk, 1, "content").unwrap().apply(&src), e("# A\n\n---\nregion.right: x=5\n---\n# T\n"));
        let src = e("# A\n");
        let talk = parse_talk(&src);
        assert!(set_layout(&talk, 0, "content").unwrap().is_empty());
        assert_eq!(set_layout(&talk, 0, "blank").unwrap().apply(&src), e("---\nlayout: blank\n---\n# A\n"));
        assert_eq!(set_layout(&talk, 3, "blank"), None);
        assert_eq!(set_layout(&talk, 0, "two\ncolumns"), None);
    });
}

#[test]
fn a_transition_on_every_slide_is_one_change() {
    on_both(|e| {
        let src = e("---\ntitle: Talk\n---\n# One\n\n---\n# Two\n\n---\nhidden: true\ntransition: push\n---\n# Three\n");
        let talk = parse_talk(&src);
        let change = set_transition(&talk, &[0, 1, 2], Some("fade")).unwrap();
        assert_eq!(change.edits.len(), 3);
        let new = change.apply(&src);
        assert_eq!(new, e("---\ntitle: Talk\ntransition: fade\n---\n# One\n\n---\ntransition: fade\n---\n# Two\n\n---\nhidden: true\ntransition: fade\n---\n# Three\n"));
        assert_eq!(change.inverse(&src).apply(&new), src);
        let after = parse_talk(&new);
        assert!(set_transition(&after, &[0, 1, 2], Some("fade")).unwrap().is_empty());
        let none = set_transition(&after, &[2, 0, 1], None).unwrap();
        assert_eq!(none.apply(&new), e("---\ntitle: Talk\n---\n# One\n\n---\n# Two\n\n---\nhidden: true\n---\n# Three\n"));
        // Only the slides named.
        assert_eq!(set_transition(&talk, &[1], Some("push")).unwrap().apply(&src), e("---\ntitle: Talk\n---\n# One\n\n---\ntransition: push\n---\n# Two\n\n---\nhidden: true\ntransition: push\n---\n# Three\n"));
        assert_eq!(set_transition(&talk, &[1, 7], Some("push")), None);
    });
}

#[test]
fn crlf_talks_keep_crlf() {
    let src = "# A\r\n\r\n- item\r\n\r\n  ![A cat](cat.png)\r\n\r\n<Icon name=\"a\" x=\"1\" />\r\n\r\n<Icon name=\"b\" x=\"2\" />\r\n\r\n---\r\n# B\r\n";
    let talk = parse_talk(src);
    let placed = set_values(&talk, 1, &[ValueEdit { target: body("body"), name: "x", value: Some("3") }]).unwrap().apply(src);
    assert!(only_crlf(&placed), "{placed:?}");
    let a = tag_range(&talk, 0, "<Icon name=\"a\"");
    let moved = reorder(&talk, 0, &a, Order::Front).unwrap().apply(src);
    assert!(only_crlf(&moved), "{moved:?}");
    let Block::List { items, .. } = &talk.slides[0].blocks[1] else { panic!("no list") };
    let lifted = lift(&talk, 0, items[0][1].range(), (1.0, 2.0, 3.0, 4.0)).unwrap().apply(src);
    assert!(only_crlf(&lifted), "{lifted:?}");
    let laid = set_layout(&talk, 1, "blank").unwrap().apply(src);
    assert!(only_crlf(&laid), "{laid:?}");
    let all = set_transition(&talk, &[0, 1], Some("fade")).unwrap().apply(src);
    assert!(only_crlf(&all), "{all:?}");
}

#[test]
fn number_text_is_plain() {
    assert_eq!(number_text(0.0), "0");
    assert_eq!(number_text(-0.0), "0");
    assert_eq!(number_text(-0.04), "0");
    assert_eq!(number_text(120.0), "120");
    assert_eq!(number_text(120.5), "120.5");
    assert_eq!(number_text(12.25), "12.3");
    assert_eq!(number_text(-7.96), "-8");
    assert_eq!(number_text(1e9), "1000000000");
}
