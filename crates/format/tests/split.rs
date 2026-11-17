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

#[test]
fn a_fence_closes_only_on_as_many_of_its_marks_and_nothing_else() {
    // Too few marks, or words after them, leave the fence open.
    assert_eq!(split_slides("````\n```\n---\n````\n").len(), 1);
    assert_eq!(split_slides("```\n```x\n---\n```\n").len(), 1);
    assert_eq!(split_slides("~~~\n~~~ \n---\n").len(), 2);
}

#[test]
fn a_fence_nested_in_a_list_hides_the_rules_in_it() {
    assert_eq!(split_slides("- a\n  - b\n\n    ```\n    ---\n    ```\n").len(), 1);
    assert_eq!(split_slides("10. a\n\n    ```yaml\n    ---\n    x: 1\n    ```\n").len(), 1);
}

/// The options the slide parser reads a slide with.
fn slide_options() -> markdown::ParseOptions {
    markdown::ParseOptions {
        constructs: markdown::Constructs {
            gfm_table: true,
            gfm_strikethrough: false,
            mdx_jsx_flow: true,
            mdx_jsx_text: true,
            html_flow: false,
            html_text: false,
            autolink: false,
            mdx_esm: false,
            mdx_expression_flow: false,
            mdx_expression_text: false,
            ..markdown::Constructs::default()
        },
        ..markdown::ParseOptions::default()
    }
}

/// Which lines of `source` are inside a block of code, as the slide parser's Markdown reads
/// it alone, or `None` where the `markdown` crate panics on it, as it does on `words`, `---`,
/// `---`, `words`, `---`, and the slide parser turns that into a refusal.
fn code_lines(source: &str) -> Option<Vec<bool>> {
    let root = std::panic::catch_unwind(|| markdown::to_mdast(source, &slide_options())).ok()?.expect("these lines are always read");
    let mut lines = vec![false; source.split('\n').count()];
    let mut to_visit = vec![&root];
    while let Some(node) = to_visit.pop() {
        if let (markdown::mdast::Node::Code(_), Some(at)) = (node, node.position()) {
            lines[at.start.line - 1..at.end.line].fill(true);
        }
        to_visit.extend(node.children().into_iter().flatten());
    }
    Some(lines)
}

#[test]
fn a_fence_opened_as_far_in_as_a_nested_or_wide_item_allows_ends_with_it() {
    // An item whose words start four columns in opens its fence there; a rule at the left edge
    // ends the item, and the fence with it.
    for source in ["- a\n  - b\n\n    ```\n---\nb", "10. a\n\n    ```\n    x\n---\nb"] {
        assert_eq!(split_slides(source).len(), 2, "{source:?}");
    }
}

#[test]
fn a_fence_in_a_list_item_closes_as_far_in_as_the_item_allows() {
    for source in ["- a\n\n  ```\n  x\n    ```\n---\nb", "1. a\n   ```\n   x\n     ```\n---\nb", "- a\n  ```\n  x\n\t```\n---\nb"] {
        assert_eq!(split_slides(source).len(), 2, "{source:?}");
    }
}

/// A line at the top level of a slide. The slide splitter can't see lists, so it takes a
/// line of backticks or tildes as a fence however far in it is, as in a list item it may be.
/// Markdown takes a top-level line four or more columns in as code by indentation instead,
/// so that one shape, which the splitter knowingly reads otherwise, isn't here.
fn top_level_line() -> impl Strategy<Value = String> {
    proptest::sample::select(vec![
        "```", "````", "```rust", "```a```b", "``` a ` b", "```x", "```  ", "``", " ```", "   ```", "~~~", "~~~~", "~~~a`b", "~~~ ```",
        "   ~~~", "~~~x", "---", "words", "", "`code` here",
    ])
    .prop_map(str::to_string)
}

/// A list item holding a fenced block, closed somewhere from the item's text column to three
/// columns past it, a tab counting as up to four, in an item nested or after a long number
/// too, so four to eight columns in; or something else at the top level.
fn fence_in_an_item() -> impl Strategy<Value = String> {
    proptest::sample::select(vec![
        "- a\n\n  ```\n  ---\n  ```",
        "- a\n\n  ```\n  x\n    ```",
        "- a\n  ```\n  x\n     ```",
        "- a\n  ```\n  x\n\t```",
        "- a\n  ~~~\n  x\n   ~~~~",
        "1. a\n   ```\n   x\n      ```",
        "1. a\n\n   ```rust\n   ---\n     ```",
        "- a\n  ``` x\n  ```",
        "- a\n  - b\n\n    ```\n    ---\n    ```",
        "- a\n  - b\n    ```\n    ---\n      ```",
        "- a\n  - b\n    - c\n\n      ~~~\n      ---\n      ~~~",
        "- a\n  - b\n    - c\n      ```\n      ---\n        ```",
        "10. a\n\n    ```yaml\n    ---\n    x: 1\n    ```",
        "10. a\n    ```\n    ---\n       ```",
        "100. a\n\n     ```\n     ---\n     ```",
        "1. a\n   1. b\n\n      ```\n      ---\n      ```",
        "- a\n  1. b\n     - c\n\n       ```\n       ---\n          ```",
        "---",
        "words",
        "",
    ])
    .prop_map(str::to_string)
}

/// Whether each `---` line of `pieces`, joined, starts a slide exactly where Markdown doesn't
/// read it as part of a block of code.
fn rules_break_outside_code(pieces: &[String]) -> Result<(), proptest::test_runner::TestCaseError> {
    let source = pieces.join("\n");
    let Some(in_code) = code_lines(&source) else { return Ok(()) };
    let slides = split_slides(&source);
    let mut start = 0;
    for (index, line) in source.split('\n').enumerate() {
        if line == "---" {
            let breaks = if index == 0 { slides[0].body.start > 0 } else { slides.iter().any(|slide| slide.range.start == start) };
            proptest::prop_assert_eq!(breaks, !in_code[index], "line {} of {:?}", index, source);
        }
        start += line.len() + 1;
    }
    Ok(())
}

use proptest::strategy::Strategy;

proptest::proptest! {
    /// A `---` line at the top level starts a slide exactly when Markdown doesn't read it as
    /// part of a block of code.
    #[test]
    fn a_rule_breaks_a_slide_unless_markdown_reads_it_as_code(lines in proptest::collection::vec(top_level_line(), 0..14)) {
        rules_break_outside_code(&lines)?;
    }

    /// The same, with fences in list items. A fence left open in a list item ends with its
    /// item, which the slide splitter can't see, so these come closed.
    #[test]
    fn a_rule_after_a_fence_in_a_list_item_breaks_a_slide(pieces in proptest::collection::vec(fence_in_an_item(), 0..10)) {
        rules_break_outside_code(&pieces)?;
    }
}

