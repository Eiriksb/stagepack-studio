use proptest::prelude::*;
use slides_format::*;

fn first_tag(talk: &Talk) -> &Tag {
    match &talk.slides[0].blocks[0] { Block::Tag(tag) => tag, _ => panic!() }
}

#[test]
fn changing_an_attribute_touches_only_its_value() {
    let src = "<Box   x=\"1\"    y=\"2\" />\n";
    let talk = parse_talk(src);
    assert_eq!(set_attr(src, first_tag(&talk), "y", "99").apply(src), "<Box   x=\"1\"    y=\"99\" />\n");
}

#[test]
fn a_new_attribute_goes_before_the_tag_ends() {
    let src = "<Box x=\"1\">\n\nHi\n\n</Box>\n";
    let talk = parse_talk(src);
    assert_eq!(set_attr(src, first_tag(&talk), "rotate", "5").apply(src), "<Box x=\"1\" rotate=\"5\">\n\nHi\n\n</Box>\n");
}

#[test]
fn a_new_attribute_on_a_self_closed_tag() {
    let src = "<Box x=\"1\" />\n";
    let talk = parse_talk(src);
    assert_eq!(set_attr(src, first_tag(&talk), "y", "2").apply(src), "<Box x=\"1\" y=\"2\" />\n");
}

#[test]
fn a_new_attribute_on_a_tag_with_none() {
    let src = "<Box />\n";
    let talk = parse_talk(src);
    assert_eq!(set_attr(src, first_tag(&talk), "y", "2").apply(src), "<Box y=\"2\" />\n");
}

#[test]
fn a_bare_attribute_gets_a_value() {
    let src = "<Box flag />\n";
    let talk = parse_talk(src);
    assert_eq!(set_attr(src, first_tag(&talk), "flag", "value").apply(src), "<Box flag=\"value\" />\n");
}

#[test]
fn a_quote_in_a_value_is_escaped() {
    let src = "<Box x=\"1\" />\n";
    let talk = parse_talk(src);
    assert_eq!(set_attr(src, first_tag(&talk), "x", "a\"b").apply(src), "<Box x=\"a&quot;b\" />\n");
}

#[test]
fn an_unquoted_value_is_replaced_with_a_quoted_one() {
    // The parser does not yet produce a tag for an unquoted value, so the tag is built by hand.
    let src = "<Box x=1 />\n";
    let tag = Tag {
        name: "Box".into(),
        attrs: vec![Attr { name: "x".into(), value: "1".into(), name_range: 5..6, value_range: 7..8 }],
        children: vec![],
        range: 0..11,
        open_end: 11,
    };
    assert_eq!(set_attr(src, &tag, "x", "a b").apply(src), "<Box x=\"a b\" />\n");
}

#[test]
fn changing_a_setting_touches_only_its_value() {
    let src = "---\nlayout:    title\n---\n\n# One\n";
    let talk = parse_talk(src);
    assert_eq!(set_setting(src, &talk.slides[0], "layout", "section").apply(src), "---\nlayout:    section\n---\n\n# One\n");
}

#[test]
fn a_new_setting_is_one_new_line_with_the_files_line_ending() {
    let src = "---\r\nlayout: title\r\n---\r\n# One\r\n";
    let talk = parse_talk(src);
    assert_eq!(set_setting(src, &talk.slides[0], "transition", "fade").apply(src), "---\r\nlayout: title\r\ntransition: fade\r\n---\r\n# One\r\n");
}

#[test]
fn a_slide_with_no_settings_gets_a_block() {
    let src = "# One\n";
    let talk = parse_talk(src);
    assert_eq!(set_setting(src, &talk.slides[0], "layout", "blank").apply(src), "---\nlayout: blank\n---\n# One\n");
}

#[test]
fn a_later_slide_with_a_bare_rule_gets_its_settings_after_the_rule() {
    let src = "# One\n\n---\n\n# Two\n";
    let talk = parse_talk(src);
    assert_eq!(talk.slides.len(), 2);
    assert_eq!(set_setting(src, &talk.slides[1], "layout", "blank").apply(src), "# One\n\n---\nlayout: blank\n---\n\n# Two\n");
}

#[test]
fn a_crlf_file_gets_a_crlf_block() {
    let src = "# One\r\n";
    let talk = parse_talk(src);
    assert_eq!(set_setting(src, &talk.slides[0], "layout", "blank").apply(src), "---\r\nlayout: blank\r\n---\r\n# One\r\n");
}

#[test]
fn an_empty_setting_value_gets_a_space() {
    let src = "---\nlayout:\n---\n# One\n";
    let talk = parse_talk(src);
    assert_eq!(set_setting(src, &talk.slides[0], "layout", "blank").apply(src), "---\nlayout: blank\n---\n# One\n");
}

proptest! {
    #[test]
    fn inverse_restores_the_source(prefix in ".{0,20}", old in ".{0,20}", suffix in ".{0,20}", new in ".{0,20}") {
        let source = format!("{prefix}{old}{suffix}");
        let edit = SourceEdit { range: prefix.len()..prefix.len() + old.len(), text: new };
        let changed = edit.apply(&source);
        prop_assert_eq!(edit.inverse(&source).apply(&changed), source);
    }
}
