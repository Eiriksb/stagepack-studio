use proptest::prelude::*;
use slides_format::*;

/// Runs `test` on text with `\n` line endings, and again with every `\n` made `\r\n`.
fn on_both(test: fn(fn(&str) -> String)) {
    test(|text| text.to_string());
    test(|text| text.replace('\n', "\r\n"));
}

fn applied(source: &str, change: &Change) -> String {
    change.apply(source)
}

#[test]
fn a_slide_is_added_after_another() {
    on_both(|e| {
        let src = e("# One\n\n---\n\n# Two\n");
        let talk = parse_talk(&src);
        assert_eq!(applied(&src, &insert_slide(&talk, Some(0), &e("# New\n"))), e("# One\n\n---\n# New\n---\n\n# Two\n"));
        assert_eq!(applied(&src, &insert_slide(&talk, Some(1), &e("# New\n"))), e("# One\n\n---\n\n# Two\n---\n# New\n"));
        // The new slide's text takes the talk's line ending.
        assert_eq!(applied(&src, &insert_slide(&talk, Some(1), "# New\n\nText\n")), e("# One\n\n---\n\n# Two\n---\n# New\n\nText\n"));
    });
}

#[test]
fn a_slide_is_added_first_and_the_talks_settings_stay_first() {
    on_both(|e| {
        let src = e("---\ntitle: Talk\nlayout: title\n---\n# One\n");
        let change = insert_slide(&parse_talk(&src), None, &e("# New\n"));
        assert_eq!(applied(&src, &change), e("---\ntitle: Talk\n---\n# New\n---\nlayout: title\n---\n# One\n"));

        let src = e("---\ntitle: Talk\n---\n# One\n");
        let change = insert_slide(&parse_talk(&src), None, &e("# New\n"));
        assert_eq!(applied(&src, &change), e("---\ntitle: Talk\n---\n# New\n---\n# One\n"));

        let src = e("# One\n");
        let change = insert_slide(&parse_talk(&src), None, &e("# New\n"));
        assert_eq!(applied(&src, &change), e("# New\n---\n# One\n"));
    });
}

#[test]
fn a_slide_is_added_to_a_talk_with_no_newline_at_its_end() {
    on_both(|e| {
        let src = e("# One\n---\n# Two").to_string();
        let talk = parse_talk(&src);
        assert_eq!(applied(&src, &insert_slide(&talk, Some(1), &e("# New\n"))), e("# One\n---\n# Two\n---\n# New"));
        assert_eq!(applied(&src, &insert_slide(&talk, Some(0), &e("# New\n"))), e("# One\n---\n# New\n---\n# Two"));
        assert_eq!(applied(&src, &insert_slide(&talk, None, &e("# New\n"))), e("# New\n---\n# One\n---\n# Two"));
    });
}

#[test]
fn a_slide_is_deleted() {
    on_both(|e| {
        let src = e("# One\n\n---\n# Two\n\n---\n# Three\n");
        let talk = parse_talk(&src);
        assert_eq!(applied(&src, &delete_slide(&talk, 1).unwrap()), e("# One\n\n---\n# Three\n"));
        assert_eq!(applied(&src, &delete_slide(&talk, 2).unwrap()), e("# One\n\n---\n# Two\n\n"));
        assert_eq!(applied(&src, &delete_slide(&talk, 0).unwrap()), e("# Two\n\n---\n# Three\n"));

        let src = e("# One\n---\n# Two");
        assert_eq!(applied(&src, &delete_slide(&parse_talk(&src), 1).unwrap()), e("# One\n"), "the slide before keeps its bytes");
    });
}

#[test]
fn the_only_slide_cannot_be_deleted() {
    on_both(|e| {
        assert_eq!(delete_slide(&parse_talk(&e("# One\n")), 0), None);
        assert_eq!(delete_slide(&parse_talk(&e("---\ntitle: Talk\n---\n# One\n")), 0), None);
    });
}

#[test]
fn deleting_the_first_slide_keeps_the_talks_settings() {
    on_both(|e| {
        let src = e("---\ntitle: Talk\nlayout: title\nfont: Inter\n---\n# One\n---\nlayout: section\n---\n# Two\n");
        let change = delete_slide(&parse_talk(&src), 0).unwrap();
        assert_eq!(applied(&src, &change), e("---\ntitle: Talk\nfont: Inter\nlayout: section\n---\n# Two\n"));

        let src = e("---\ntitle: Talk\n---\n# One\n---\n# Two\n");
        let change = delete_slide(&parse_talk(&src), 0).unwrap();
        assert_eq!(applied(&src, &change), e("---\ntitle: Talk\n---\n# Two\n"));
    });
}

#[test]
fn a_slide_is_duplicated_with_its_settings_and_notes() {
    on_both(|e| {
        let two = e("---\nlayout: x\n---\n# Two\n\n<Notes>\nSay hi\n</Notes>\n");
        let src = e("# One\n") + &two;
        let change = duplicate_slide(&parse_talk(&src), 1);
        assert_eq!(applied(&src, &change), src.clone() + &two);

        let src = e("# One\n") + &two + &e("---\n# Three\n");
        let change = duplicate_slide(&parse_talk(&src), 1);
        assert_eq!(applied(&src, &change), e("# One\n") + &two + &two + &e("---\n# Three\n"));
    });
}

#[test]
fn the_first_slides_copy_has_none_of_the_talks_settings() {
    on_both(|e| {
        let src = e("---\ntitle: Talk\nlayout: title\n---\n# One\n");
        let change = duplicate_slide(&parse_talk(&src), 0);
        assert_eq!(applied(&src, &change), e("---\ntitle: Talk\nlayout: title\n---\n# One\n---\nlayout: title\n---\n# One\n"));

        let src = e("---\ntitle: Talk\n---\n# One\n");
        let change = duplicate_slide(&parse_talk(&src), 0);
        assert_eq!(applied(&src, &change), e("---\ntitle: Talk\n---\n# One\n---\n# One\n"));

        let src = e("# One\n");
        assert_eq!(applied(&src, &duplicate_slide(&parse_talk(&src), 0)), e("# One\n---\n# One\n"));
    });
}

#[test]
fn a_slide_moves_down() {
    on_both(|e| {
        let src = e("# A\n---\n# B\n---\n# C\n");
        let talk = parse_talk(&src);
        assert_eq!(applied(&src, &move_slide(&talk, 0, 2)), e("# B\n---\n# C\n---\n# A\n"));
        assert_eq!(applied(&src, &move_slide(&talk, 1, 2)), e("# A\n---\n# C\n---\n# B\n"));
        assert_eq!(applied(&src, &move_slide(&talk, 0, 1)), e("# B\n---\n# A\n---\n# C\n"));
    });
}

#[test]
fn a_slide_moves_up() {
    on_both(|e| {
        let src = e("# A\n---\n# B\n---\n# C\n");
        let talk = parse_talk(&src);
        assert_eq!(applied(&src, &move_slide(&talk, 2, 0)), e("# C\n---\n# A\n---\n# B\n"));
        assert_eq!(applied(&src, &move_slide(&talk, 2, 1)), e("# A\n---\n# C\n---\n# B\n"));

        let src = e("# A\n---\n# B\n---\n# C");
        assert_eq!(applied(&src, &move_slide(&parse_talk(&src), 2, 1)), e("# A\n---\n# C\n---\n# B\n"));
    });
}

#[test]
fn a_slide_moves_to_where_it_is() {
    on_both(|e| {
        let talk = parse_talk(&e("# A\n---\n# B\n"));
        assert!(move_slide(&talk, 1, 1).is_empty());
        assert!(move_slide(&talk, 0, 0).is_empty());
    });
}

#[test]
fn the_talks_settings_stay_when_the_first_slide_moves() {
    on_both(|e| {
        let src = e("---\ntitle: Talk\nfont: Inter\nlayout: title\n---\n# A\n---\n# B\n");
        let change = move_slide(&parse_talk(&src), 0, 1);
        assert_eq!(applied(&src, &change), e("---\ntitle: Talk\nfont: Inter\n---\n# B\n---\nlayout: title\n---\n# A\n"));

        let src = e("---\ntitle: Talk\n---\n# A\n---\nlayout: x\n---\n# B\n");
        let change = move_slide(&parse_talk(&src), 1, 0);
        assert_eq!(applied(&src, &change), e("---\ntitle: Talk\nlayout: x\n---\n# B\n---\n# A\n"));
    });
}

#[test]
fn a_byte_order_mark_stays_at_the_start() {
    on_both(|e| {
        let src = e("\u{feff}---\ntitle: T\n---\n# A\n---\n# B\n");
        let change = move_slide(&parse_talk(&src), 1, 0);
        assert_eq!(applied(&src, &change), e("\u{feff}---\ntitle: T\n---\n# B\n---\n# A\n"));

        let src = e("\u{feff}# A\n---\n# B\n");
        let change = move_slide(&parse_talk(&src), 1, 0);
        assert_eq!(applied(&src, &change), e("\u{feff}# B\n---\n# A\n"));
    });
}

#[test]
fn a_slide_is_hidden_and_shown_again() {
    on_both(|e| {
        let src = e("# A\n---\nlayout: x\n---\n# B\n");
        let hidden = applied(&src, &set_hidden(&parse_talk(&src), 1, true));
        assert_eq!(hidden, e("# A\n---\nlayout: x\nhidden: true\n---\n# B\n"));
        assert!(is_hidden(&parse_talk(&hidden).slides[1]));
        let shown = applied(&hidden, &set_hidden(&parse_talk(&hidden), 1, false));
        assert_eq!(shown, src);
        assert!(!is_hidden(&parse_talk(&shown).slides[1]));

        let src = e("# A\n---\n# B\n");
        let hidden = applied(&src, &set_hidden(&parse_talk(&src), 0, true));
        assert_eq!(hidden, e("---\nhidden: true\n---\n# A\n---\n# B\n"));
        assert_eq!(applied(&hidden, &set_hidden(&parse_talk(&hidden), 0, false)), src);
        assert!(set_hidden(&parse_talk(&src), 0, false).is_empty());
    });
}

#[test]
fn hiding_adds_a_block_to_a_slide_with_none() {
    on_both(|e| {
        let src = e("# A\n---\n# B\n");
        let hidden = applied(&src, &set_hidden(&parse_talk(&src), 1, true));
        assert_eq!(hidden, e("# A\n---\nhidden: true\n---\n# B\n"));
        assert!(is_hidden(&parse_talk(&hidden).slides[1]));
        assert!(!is_hidden(&parse_talk(&hidden).slides[0]));
        assert_eq!(applied(&hidden, &set_hidden(&parse_talk(&hidden), 1, false)), src);
    });
}

#[test]
fn removing_a_setting_takes_its_whole_line() {
    on_both(|e| {
        let src = e("---\ntitle: T\nlayout: x\n---\n# A\n");
        let talk = parse_talk(&src);
        let edit = remove_setting(&src, &talk.slides[0], "layout").unwrap();
        assert_eq!(edit.apply(&src), e("---\ntitle: T\n---\n# A\n"));
        assert_eq!(remove_setting(&src, &talk.slides[0], "font"), None);
    });
}

#[test]
fn notes_are_added_changed_and_removed() {
    on_both(|e| {
        let src = e("# A\n---\n# B\n");
        let added = applied(&src, &set_notes(&parse_talk(&src), 0, "Say hi"));
        assert_eq!(added, e("# A\n\n<Notes>\nSay hi\n</Notes>\n---\n# B\n"));
        let changed = applied(&added, &set_notes(&parse_talk(&added), 0, "Say bye"));
        assert_eq!(changed, e("# A\n\n<Notes>\nSay bye\n</Notes>\n---\n# B\n"));
        assert_eq!(parse_talk(&changed).slides[0].notes.as_ref().unwrap().text, "Say bye");
        let removed = applied(&changed, &set_notes(&parse_talk(&changed), 0, ""));
        assert_eq!(removed, src);

        let src = e("# A\n---\n# B");
        let added = applied(&src, &set_notes(&parse_talk(&src), 1, "Say hi"));
        assert_eq!(added, e("# A\n---\n# B\n\n<Notes>\nSay hi\n</Notes>"));
        assert_eq!(applied(&added, &set_notes(&parse_talk(&added), 1, "")), src);

        let src = e("# A\n\n<Notes>\n\n  Old  \n\n</Notes>\n");
        let changed = applied(&src, &set_notes(&parse_talk(&src), 0, "New"));
        assert_eq!(changed, e("# A\n\n<Notes>\n\n  New  \n\n</Notes>\n"), "only the words change");
        assert!(set_notes(&parse_talk(&e("# A\n")), 0, "").is_empty());
    });
}

#[test]
fn notes_with_markdown_in_them_come_back_as_written() {
    on_both(|e| {
        let src = e("# A\n");
        let notes = "**Bold** and _this_\n\n- one\n- two\n\n`code` and > a quote and 2 < 3";
        let edited = applied(&src, &set_notes(&parse_talk(&src), 0, &e(notes)));
        let slide = &parse_talk(&edited).slides[0];
        assert_eq!(slide.error, None);
        assert_eq!(slide.notes.as_ref().unwrap().text, notes);
    });
}

#[test]
fn notes_cannot_end_their_own_element() {
    on_both(|e| {
        let src = e("# A\n---\n# B\n");
        for typed in [
            "Type </Notes> here",
            "</Notes>",
            "Already \\</Notes> escaped",
            "Two \\\\</Notes> slashes",
            "A <b>tag</b> and <Box> and {braces} and > this",
            "C:\\path\\to\\file",
            "Before\n---\nafter, and a rule with spaces:\n---  \nend",
            "```\nan open fence\n  ~~~\nand another",
            "- ```\n1. a list that trips the parser",
        ] {
            let edited = applied(&src, &set_notes(&parse_talk(&src), 0, typed));
            let talk = parse_talk(&edited);
            assert_eq!(talk.slides.len(), 2, "{typed}");
            assert_eq!(talk.slides[0].error, None, "{typed}");
            assert_eq!(talk.slides[0].notes.as_ref().unwrap().text, typed);
        }
    });
}

#[test]
fn hand_written_notes_read_as_before() {
    on_both(|e| {
        let src = e("# A\n\n<Notes>\nSee C:\\path and \\n and a\\b\n</Notes>\n");
        assert_eq!(parse_talk(&src).slides[0].notes.as_ref().unwrap().text, "See C:\\path and \\n and a\\b");
    });
}

#[test]
fn a_break_inside_a_code_fence_is_not_a_slide() {
    on_both(|e| {
        let src = e("# A\n\n```\n---\n```\n---\n# B\n");
        let talk = parse_talk(&src);
        assert_eq!(talk.slides.len(), 2);
        let moved = applied(&src, &move_slide(&talk, 0, 1));
        assert_eq!(moved, e("# B\n---\n# A\n\n```\n---\n```\n"));
        assert_eq!(parse_talk(&moved).slides.len(), 2);
        let copied = applied(&src, &duplicate_slide(&talk, 0));
        assert_eq!(parse_talk(&copied).slides.len(), 3);
    });
}

/// A talk, an operation on it, the text it should give, and how many slides that has.
type Case = (String, Box<dyn Fn(&Talk) -> Change>, String, usize);

#[test]
fn a_slide_of_key_value_lines_stays_a_slide() {
    on_both(|e| {
        let cases: Vec<Case> = vec![
            // An existing slide of such lines gets something after it.
            (e("# A\n---\nQuestions: ask me\n"), Box::new(|t| insert_slide(t, Some(1), "# New\n")), e("# A\n---\n\nQuestions: ask me\n---\n# New\n"), 3),
            (e("# A\n---\nQuestions: ask me\n"), Box::new(|t| duplicate_slide(t, 1)), e("# A\n---\n\nQuestions: ask me\n---\nQuestions: ask me\n"), 3),
            (e("# A\n---\n# B\n---\nQ: x\n"), Box::new(|t| move_slide(t, 2, 1)), e("# A\n---\n\nQ: x\n---\n# B\n"), 3),
            (e("# A\n---\nQ: x\n"), Box::new(|t| move_slide(t, 0, 1)), e("Q: x\n---\n# A\n"), 2),
            // A new slide of such lines.
            (e("# A\n---\n# B\n"), Box::new(|t| insert_slide(t, Some(0), "Q: x\nR: y\n")), e("# A\n---\n\nQ: x\nR: y\n---\n# B\n"), 3),
            // A first slide of such lines that moves away.
            (e("Q: x\n---\n# B\n---\n# C\n"), Box::new(|t| move_slide(t, 0, 1)), e("# B\n---\n\nQ: x\n---\n# C\n"), 3),
            (e("---\ntitle: T\n---\nQ: x\n---\n# B\n"), Box::new(|t| move_slide(t, 0, 1)), e("---\ntitle: T\n---\n# B\n---\nQ: x\n"), 2),
            (e("---\ntitle: T\n---\nQ: x\n---\n# B\n---\n# C\n"), Box::new(|t| move_slide(t, 0, 1)), e("---\ntitle: T\n---\n# B\n---\n\nQ: x\n---\n# C\n"), 3),
            // Deleting never needs it, but must not lose such a slide.
            (e("# A\n---\n\nQ: x\n---\n# C\n"), Box::new(|t| delete_slide(t, 0).unwrap()), e("\nQ: x\n---\n# C\n"), 2),
            (e("# A\n---\n# B\n---\nQ: x\n"), Box::new(|t| delete_slide(t, 1).unwrap()), e("# A\n---\nQ: x\n"), 2),
            // Taking away its settings block, or its notes.
            (e("# A\n---\nhidden: true\n---\nQ: x\n---\n# C\n"), Box::new(|t| set_hidden(t, 1, false)), e("# A\n---\n\nQ: x\n---\n# C\n"), 3),
            (e("# A\n---\nQ: x\n\n<Notes>\nhi\n</Notes>\n---\n# C\n"), Box::new(|t| set_notes(t, 1, "")), e("# A\n---\n\nQ: x\n---\n# C\n"), 3),
        ];
        for (src, op, want, count) in cases {
            let before = parse_talk(&src);
            let change = op(&before);
            let after = change.apply(&src);
            assert_eq!(after, want, "from {src:?}");
            assert_eq!(parse_talk(&after).slides.len(), count, "{after:?}");
            assert_eq!(change.inverse(&src).apply(&after), src);
        }
    });
}

#[test]
fn a_change_and_its_inverse_give_back_the_same_bytes() {
    let src = "abcdef";
    let change = Change {
        edits: vec![
            SourceEdit { range: 0..0, text: "<".into() },
            SourceEdit { range: 1..2, text: "XY".into() },
            SourceEdit { range: 4..4, text: "Z".into() },
            SourceEdit { range: 5..6, text: String::new() },
        ],
    };
    let changed = change.apply(src);
    assert_eq!(changed, "<aXYcdZe");
    assert_eq!(change.inverse(src).apply(&changed), src);
    assert!(!change.is_empty());
    assert!(Change { edits: vec![] }.is_empty());
    let single = Change::single(SourceEdit { range: 2..3, text: "Q".into() });
    assert_eq!(single.apply(src), "abQdef");

    on_both(|e| {
        let src = e("---\ntitle: Talk\nlayout: title\n---\n# A\n---\n# B\n---\n# C");
        let talk = parse_talk(&src);
        for change in [move_slide(&talk, 0, 2), move_slide(&talk, 2, 0), duplicate_slide(&talk, 0), delete_slide(&talk, 0).unwrap(), insert_slide(&talk, None, "# N\n"), set_notes(&talk, 2, "Hi")] {
            let changed = change.apply(&src);
            assert_eq!(change.inverse(&src).apply(&changed), src);
        }
    });
}

// Property tests.

#[derive(Clone, Debug)]
struct GenSlide {
    settings: Vec<(String, String)>,
    blocks: Vec<String>,
    notes: Option<String>,
    blank_after: bool,
    /// A slide of `key: value` lines only, which reads as a settings block if a break
    /// comes straight after a `---` and it.
    key_values: bool,
}

#[derive(Clone, Debug)]
struct GenTalk {
    slides: Vec<GenSlide>,
    crlf: bool,
    bom: bool,
    final_newline: bool,
    /// Whether a first slide with no settings starts with a `---` all the same.
    opening_rule: bool,
}

/// Every slide holds the marker `mk<n>x`, `n` its place in the generated talk.
fn marker(n: usize) -> String {
    format!("mk{n}x")
}

impl GenTalk {
    fn text(&self) -> String {
        let mut out = String::new();
        if self.bom {
            out.push('\u{feff}');
        }
        for (n, slide) in self.slides.iter().enumerate() {
            if !slide.settings.is_empty() {
                out.push_str("---\n");
                for (key, value) in &slide.settings {
                    out.push_str(&format!("{key}: {value}\n"));
                }
                out.push_str("---\n");
            } else if n > 0 || self.opening_rule {
                out.push_str("---\n");
            }
            if slide.key_values {
                let after_a_rule = slide.settings.is_empty() && (n > 0 || self.opening_rule);
                if after_a_rule && n + 1 < self.slides.len() {
                    out.push('\n');
                }
                out.push_str(&format!("Note: {}\nOther: thing", marker(n)));
            } else {
                let mut blocks = slide.blocks.clone();
                blocks.insert(0, format!("# Slide {}", marker(n)));
                if let Some(notes) = &slide.notes {
                    blocks.push(format!("<Notes>\n{notes}\n</Notes>"));
                }
                out.push_str(&blocks.join("\n\n"));
            }
            out.push('\n');
            if slide.blank_after {
                out.push('\n');
            }
        }
        if !self.final_newline {
            out.truncate(out.trim_end_matches('\n').len());
        }
        if self.crlf { out.replace('\n', "\r\n") } else { out }
    }
}

fn block() -> impl Strategy<Value = String> {
    prop::sample::select(vec![
        "## A heading".to_string(),
        "Some *text* with trailing spaces  ".to_string(),
        "A paragraph\nover two lines.".to_string(),
        "- one\n- two".to_string(),
        "1. first\n2. second".to_string(),
        "```\n---\ncode\n```".to_string(),
        "~~~md\n---\n~~~".to_string(),
        "> A quote".to_string(),
    ])
}

fn slide_settings() -> impl Strategy<Value = Vec<(String, String)>> {
    prop::sample::subsequence(vec![("layout", "section  "), ("hidden", "true"), ("transition", "fade")], 0..=3)
    .prop_map(|entries| entries.into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
}

fn talk_settings() -> impl Strategy<Value = Vec<(String, String)>> {
    prop::sample::subsequence(
        vec![("title", "My talk"), ("color.background", "#000"), ("font", "Inter"), ("font.mono", "Mono  ")],
        0..=4,
    )
    .prop_map(|entries| entries.into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect())
}

fn gen_slide() -> impl Strategy<Value = GenSlide> {
    (
        slide_settings(),
        prop::collection::vec(block(), 0..3),
        prop::option::of(prop::sample::select(vec!["Say **this**".to_string(), "- a point\n- another".to_string()])),
        any::<bool>(),
        prop::bool::weighted(0.25),
    )
        .prop_map(|(settings, blocks, notes, blank_after, key_values)| GenSlide { settings, blocks, notes, blank_after, key_values })
}

fn gen_talk() -> impl Strategy<Value = GenTalk> {
    (prop::collection::vec(gen_slide(), 1..=8), talk_settings(), any::<[bool; 4]>()).prop_map(
        |(mut slides, talk, [crlf, bom, final_newline, opening_rule])| {
            let own = std::mem::take(&mut slides[0].settings);
            slides[0].settings = talk.into_iter().chain(own).collect();
            GenTalk { slides, crlf, bom, final_newline, opening_rule }
        },
    )
}

#[derive(Clone, Debug)]
enum Op {
    /// After which slide, and whether the new one is of `key: value` lines only.
    Insert(Option<usize>, bool),
    Delete(usize),
    Duplicate(usize),
    Move(usize, usize),
    Hide(usize, bool),
    Notes(usize, String),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        (prop::option::of(0..8usize), any::<bool>()).prop_map(|(after, key_values)| Op::Insert(after, key_values)),
        (0..8usize).prop_map(Op::Delete),
        (0..8usize).prop_map(Op::Duplicate),
        (0..8usize, 0..8usize).prop_map(|(a, b)| Op::Move(a, b)),
        (0..8usize, any::<bool>()).prop_map(|(a, b)| Op::Hide(a, b)),
        (0..8usize, prop::sample::select(vec!["", "New words", "Watch </Notes> this", "**Bold**\n\n- list"]))
            .prop_map(|(a, s)| Op::Notes(a, s.to_string())),
    ]
}

/// The marker given to an inserted slide.
const NEW: usize = 99;

/// What `op` does to a talk of `n` slides: its change, the slides it should leave in order
/// (by marker), and the markers of the slides it names.
fn run(talk: &Talk, op: &Op) -> Option<(Change, Vec<usize>, Vec<usize>)> {
    let n = talk.slides.len();
    let order: Vec<usize> = (0..n).collect();
    Some(match op {
        Op::Insert(after, key_values) => {
            let after = after.map(|a| a % n);
            let mut expected = order;
            expected.insert(after.map_or(0, |a| a + 1), NEW);
            let text = if *key_values { format!("Note: {}\nOther: new\n", marker(NEW)) } else { format!("# Slide {}\n\nNew\n", marker(NEW)) };
            (insert_slide(talk, after, &text), expected, vec![NEW])
        }
        Op::Delete(i) => {
            let i = i % n;
            let mut expected = order;
            expected.remove(i);
            (delete_slide(talk, i)?, expected, vec![i])
        }
        Op::Duplicate(i) => {
            let i = i % n;
            let mut expected = order;
            expected.insert(i + 1, i);
            (duplicate_slide(talk, i), expected, vec![])
        }
        Op::Move(from, to) => {
            let (from, to) = (from % n, to % n);
            let mut expected = order;
            let moved = expected.remove(from);
            expected.insert(to, moved);
            (move_slide(talk, from, to), expected, vec![from])
        }
        Op::Hide(i, hidden) => (set_hidden(talk, i % n, *hidden), order, vec![i % n]),
        Op::Notes(i, notes) => (set_notes(talk, i % n, notes), order, vec![i % n]),
    })
}

fn markers_in(text: &str) -> Vec<usize> {
    text.match_indices("mk")
        .filter_map(|(at, _)| {
            let rest = &text[at + 2..];
            let end = rest.find('x')?;
            rest[..end].parse().ok()
        })
        .collect()
}

fn slide_markers(talk: &Talk) -> Vec<usize> {
    talk.slides.iter().map(|s| *markers_in(&talk.source[s.range.clone()]).first().unwrap_or(&usize::MAX)).collect()
}

fn talk_entries(talk: &Talk) -> Vec<(String, String)> {
    talk.slides[0]
        .settings
        .iter()
        .flat_map(|s| s.entries.iter())
        .filter(|e| TALK_SETTINGS.contains(&e.key.as_str()))
        .map(|e| (e.key.clone(), talk.source[e.line.clone()].to_string()))
        .collect()
}

/// A slide's bytes as compared before and after: for a slide that is first on either side,
/// what follows its settings and break.
fn comparable(talk: &Talk, index: usize, loose_head: bool) -> String {
    let raw = &split_slides(&talk.source)[index];
    let range = if loose_head { raw.body.clone() } else { raw.range.clone() };
    talk.source[range].trim_start_matches('\u{feff}').to_string()
}

/// The talk's line ending: the first in the file, or `\n` when there is none.
fn talk_eol(source: &str) -> &'static str {
    match source.find('\n') {
        Some(at) if source[..at].ends_with('\r') => "\r\n",
        _ => "\n",
    }
}

/// Text someone might type into the notes box, with everything the parser could take for
/// something else.
fn typed_notes() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop::sample::select(vec![
            "a", "Z", "é", "🎉", " ", "  ", ".", "*", "_", "#", "-", "&", ":", "|", "[x](y)", "\n", "\n\n", "<", ">", "{", "}",
            "\\", "`", "```", "~~~", "\n---\n", "\n---  \n", "\n  ```\n", "<b>", "</b>", "</Notes>", "<Notes>", "<Box x=\"1\">",
            "\\<", "\\-", "\n> ", "\n- ", "\n1. ", "key: value", "a```b", "````",
        ]),
        0..24,
    )
    .prop_map(|parts| parts.concat())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn anything_typed_in_notes_reads_back_as_typed(generated in gen_talk(), index in 0..8usize, typed in typed_notes()) {
        let source = generated.text();
        let before = parse_talk(&source);
        let index = index % before.slides.len();
        let after = parse_talk(&set_notes(&before, index, &typed).apply(&source));
        prop_assert_eq!(after.slides.len(), before.slides.len());
        prop_assert!(after.slides.iter().all(|s| s.error.is_none()), "{:?}", after.slides.iter().map(|s| &s.error).collect::<Vec<_>>());
        prop_assert_eq!(slide_markers(&after), slide_markers(&before));
        let read = after.slides[index].notes.as_ref().map(|n| n.text.as_str());
        let want = typed.trim();
        prop_assert_eq!(read, (!want.is_empty()).then_some(want));
    }

    #[test]
    fn slide_operations_leave_other_slides_bytes_alone(generated in gen_talk(), op in op()) {
        let source = generated.text();
        let before = parse_talk(&source);
        let Some((change, expected, named)) = run(&before, &op) else { return Ok(()) };
        let after = parse_talk(&change.apply(&source));
        prop_assert_eq!(slide_markers(&after), expected.clone());
        for (new_index, &old_index) in expected.iter().enumerate() {
            if old_index == NEW || named.contains(&old_index) {
                continue;
            }
            let loose = new_index == 0 || old_index == 0;
            let (now, was) = (comparable(&after, new_index, loose), comparable(&before, old_index, loose));
            let eol = talk_eol(&before.source);
            // What the slide may have become: the same bytes, or with the line ending a last
            // slide needs when it stops being last, or, for a slide of `key: value` lines,
            // with a blank line after its `---` that keeps it from reading as settings.
            let mut allowed = vec![was.clone()];
            if old_index == before.slides.len() - 1 && !before.source.ends_with('\n') {
                allowed.push(was.clone() + eol);
            }
            if was.contains("Note: mk") {
                let blanked: Vec<String> = allowed
                    .iter()
                    .map(|text| match (loose, text.find('\n')) {
                        (false, Some(at)) => format!("{}{eol}{}", &text[..=at], &text[at + 1..]),
                        _ => format!("{eol}{text}"),
                    })
                    .collect();
                allowed.extend(blanked);
            }
            prop_assert!(allowed.contains(&now), "slide {}: {:?} is none of {:?}", old_index, now, allowed);
        }
        prop_assert_eq!(talk_entries(&after), talk_entries(&before));
        prop_assert_eq!(after.source.starts_with('\u{feff}'), generated.bom);
        // A slide that stops being last gains the line ending it needs, and none loses one.
        prop_assert!(after.source.ends_with('\n') || !generated.final_newline);
    }

    #[test]
    fn every_operation_undoes_exactly(generated in gen_talk(), op in op()) {
        let source = generated.text();
        let talk = parse_talk(&source);
        let Some((change, _, _)) = run(&talk, &op) else { return Ok(()) };
        for pair in change.edits.windows(2) {
            prop_assert!(pair[0].range.end <= pair[1].range.start, "sorted, not overlapping");
        }
        let changed = change.apply(&source);
        prop_assert_eq!(change.inverse(&source).apply(&changed), source);
    }

    #[test]
    fn the_result_always_parses_to_the_slides_expected(generated in gen_talk(), op in op()) {
        let source = generated.text();
        let talk = parse_talk(&source);
        let Some((change, expected, _)) = run(&talk, &op) else { return Ok(()) };
        let after = parse_talk(&change.apply(&source));
        prop_assert_eq!(after.slides.len(), expected.len());
        prop_assert_eq!(slide_markers(&after), expected);
        prop_assert!(after.slides.iter().all(|s| s.error.is_none()), "{:?}", after.slides.iter().map(|s| &s.error).collect::<Vec<_>>());
        match op {
            Op::Hide(i, hidden) => prop_assert_eq!(is_hidden(&after.slides[i % talk.slides.len()]), hidden),
            Op::Notes(i, notes) => {
                let read = after.slides[i % talk.slides.len()].notes.as_ref().map(|n| n.text.clone());
                let want = (!notes.is_empty()).then(|| notes.clone());
                prop_assert_eq!(read, want);
            }
            _ => {}
        }
    }
}
