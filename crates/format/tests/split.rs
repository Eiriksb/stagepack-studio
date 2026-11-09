use slides_format::*;

const TWO: &str = "---\ntitle: Review\nformat: 1\n---\n\n# One\n\n---\nlayout: section\n---\n\n# Two\n";

#[test]
fn settings_blocks_belong_to_their_slides() {
    let slides = split_slides(TWO);
    assert_eq!(slides.len(), 2);
    assert_eq!(slides[0].settings.as_ref().unwrap().get("title"), Some("Review"));
    assert_eq!(slides[1].settings.as_ref().unwrap().get("layout"), Some("section"));
    assert_eq!(&TWO[slides[1].body.clone()], "\n# Two\n");
}

#[test]
fn plain_rule_starts_a_slide_with_no_settings() {
    let src = "# A\n\n---\n\n# B\n";
    let slides = split_slides(src);
    assert_eq!(slides.len(), 2);
    assert!(slides[1].settings.is_none());
    assert_eq!(&src[slides[1].body.clone()], "\n# B\n");
}

#[test]
fn rule_inside_code_fence_is_not_a_slide_break() {
    assert_eq!(split_slides("```\n---\n```\n").len(), 1);
    assert_eq!(split_slides("~~~\n---\n~~~\n").len(), 1);
}

#[test]
fn crlf_offsets_are_exact() {
    let src = "---\r\ntitle: A\r\n---\r\n# One\r\n---\r\n# Two\r\n";
    let slides = split_slides(src);
    assert_eq!(slides.len(), 2);
    assert_eq!(&src[slides[0].body.clone()], "# One\r\n");
    assert_eq!(slides[0].settings.as_ref().unwrap().get("title"), Some("A"));
    assert_eq!(&src[slides[1].body.clone()], "# Two\r\n");
}

#[test]
fn bom_is_skipped() {
    let slides = split_slides("\u{feff}---\ntitle: A\n---\n# One\n");
    assert_eq!(slides[0].settings.as_ref().unwrap().get("title"), Some("A"));
}

#[test]
fn value_range_points_at_the_value() {
    let src = "---\nlayout:   title  \n---\n";
    let settings = split_slides(src).remove(0).settings.unwrap();
    assert_eq!(&src[settings.entries[0].value_range.clone()], "title");
}

#[test]
fn every_byte_belongs_to_exactly_one_slide() {
    let slides = split_slides(TWO);
    assert_eq!(slides[0].range.start, 0);
    assert_eq!(slides[0].range.end, slides[1].range.start);
    assert_eq!(slides[1].range.end, TWO.len());
}
