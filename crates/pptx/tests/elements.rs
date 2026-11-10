//! Text, shapes, tables and pictures must be byte-for-byte what the JavaScript exporter wrote for
//! the matching input. `tests/reference/README.md` says which call made each reference file and
//! which native value stands for each JavaScript object.

use slides_engine::{Align, Bullet, NativeContent, NativeParagraph, NativeRun, Rect};
use slides_pptx::shapes::{self, Ph};
use slides_pptx::table;
use slides_pptx::text::{self, Anchor, Autofit, BodyOptions, TextCtx};
use slides_pptx::xml;
use toolkit::{Color, ShapeKind};

const INK: Color = Color::rgb(0x1e, 0x29, 0x3b);
const BOX: Rect = Rect { x: 144.0, y: 288.0, w: 720.0, h: 216.0 };

fn ctx() -> TextCtx {
    TextCtx { lang: "en-US".into(), link: Box::new(|_| "rId7".into()) }
}

fn run(text: &str) -> NativeRun {
    NativeRun {
        text: text.into(),
        size: 48.0,
        bold: false,
        italic: false,
        underline: false,
        color: INK,
        family: "Inter".into(),
        link: None,
    }
}

fn para(runs: Vec<NativeRun>, align: Align, level: u8, bullet: Bullet) -> NativeParagraph {
    NativeParagraph { runs, align, level, bullet, line_height: None, space_before: None, bullet_color: None }
}

fn plain(text: &str) -> NativeParagraph {
    para(vec![run(text)], Align::Left, 0, Bullet::None)
}

fn rpr(run: &NativeRun) -> String {
    text::rpr_xml(run, &mut ctx(), "a:rPr")
}

fn run_xml(size: f32) -> String {
    rpr(&NativeRun { size, ..run("x") })
}

fn run_xml_of(text: &str) -> String {
    text::tx_body_xml(&[plain(text)], &mut ctx(), &BodyOptions::default(), "p:txBody")
}

/// A text box body as the reference cases write it: top anchor, 12 unit inset.
fn body(paragraphs: &[NativeParagraph], anchor: Anchor) -> String {
    let options = BodyOptions { anchor: Some(anchor), inset: Some(12.0), ..BodyOptions::default() };
    text::tx_body_xml(paragraphs, &mut ctx(), &options, "p:txBody")
}

fn text_content(paragraphs: Vec<NativeParagraph>) -> NativeContent {
    NativeContent::Text { paragraphs, fill: None, inset: 12.0 }
}

#[allow(clippy::too_many_arguments)]
fn shape(
    kind: ShapeKind,
    fill: Option<Color>,
    outline: Option<Color>,
    outline_width: f32,
    radius: f32,
    paragraphs: Vec<NativeParagraph>,
    inset: f32,
    centered: bool,
) -> NativeContent {
    NativeContent::Shape { kind, fill, outline, outline_width, radius, paragraphs, inset, centered }
}

fn shape_xml(content: &NativeContent, rect: Rect) -> String {
    shapes::shape_xml(4, rect, content, &mut ctx()).expect("a shape or a text box")
}

fn shape_of(kind: ShapeKind) -> String {
    shape_xml(&shape(kind, None, Some(INK), 4.0, 0.0, vec![], 0.0, false), BOX)
}

fn rounded(rect: Rect, radius: f32) -> String {
    shape_xml(&shape(ShapeKind::Rounded, None, Some(INK), 4.0, radius, vec![], 0.0, false), rect)
}

// ---- text.rs ----

#[test]
fn runs_match_the_reference() {
    assert_eq!(rpr(&run("x")), include_str!("reference/rpr-plain.xml"));
    let styled = NativeRun {
        size: 36.0,
        bold: true,
        italic: true,
        underline: true,
        color: Color::rgb(0x25, 0x63, 0xeb),
        family: "JetBrains Mono".into(),
        ..run("x")
    };
    assert_eq!(rpr(&styled), include_str!("reference/rpr-styled.xml"));
    let faint = NativeRun { color: Color { a: 128, ..INK }, ..run("x") };
    assert_eq!(rpr(&faint), include_str!("reference/rpr-alpha.xml"));
}

#[test]
fn a_link_asks_the_context_for_its_relationship() {
    let linked = NativeRun {
        underline: true,
        color: Color::rgb(0x25, 0x63, 0xeb),
        link: Some("https://example.com/a?b=1&c=2".into()),
        ..run("x")
    };
    let mut ctx = TextCtx {
        lang: "en-US".into(),
        link: Box::new(|url| {
            assert_eq!(url, "https://example.com/a?b=1&c=2");
            "rId7".into()
        }),
    };
    assert_eq!(text::rpr_xml(&linked, &mut ctx, "a:rPr"), include_str!("reference/rpr-link.xml"));
}

#[test]
fn paragraphs_match_the_reference() {
    let item = |bullet, level| para(vec![run("Item")], Align::Left, level, bullet);
    assert_eq!(body(&[item(Bullet::Dot, 0)], Anchor::Top), include_str!("reference/body-bullet.xml"));
    let nested = para(vec![run("Nested")], Align::Left, 1, Bullet::Dot);
    assert_eq!(body(&[nested], Anchor::Top), include_str!("reference/body-level1.xml"));
    let numbered = [
        para(vec![run("One")], Align::Left, 0, Bullet::Number(1)),
        para(vec![run("Two")], Align::Left, 0, Bullet::Number(2)),
    ];
    assert_eq!(body(&numbered, Anchor::Top), include_str!("reference/body-numbered.xml"));
    let three = para(vec![run("Three")], Align::Left, 0, Bullet::Number(3));
    assert_eq!(body(&[three], Anchor::Top), include_str!("reference/body-numbered-start.xml"));
    let middle = para(vec![run("Middle")], Align::Center, 0, Bullet::None);
    assert_eq!(body(&[middle], Anchor::Center), include_str!("reference/body-centered.xml"));
    let breaks = para(vec![NativeRun { bold: true, ..run("a\nb\n") }], Align::Left, 0, Bullet::None);
    assert_eq!(body(&[breaks], Anchor::Top), include_str!("reference/body-breaks.xml"));
    assert_eq!(body(&[], Anchor::Top), include_str!("reference/body-empty.xml"));
}

#[test]
fn body_properties_match_the_reference() {
    let placed = BodyOptions {
        placeholder: true,
        anchor: Some(Anchor::Bottom),
        autofit: Autofit::Shrink,
        ..BodyOptions::default()
    };
    let shrunk = BodyOptions { no_wrap: true, inset: Some(0.0), font_scale: Some(0.8), ..BodyOptions::default() };
    let both = format!("{}{}", text::body_pr_xml(&placed), text::body_pr_xml(&shrunk));
    assert_eq!(both, include_str!("reference/body-pr-autofit.xml"));
}

#[test]
fn a_number_that_does_not_follow_starts_at_itself() {
    let numbered = |n, level| para(vec![run("t")], Align::Left, level, Bullet::Number(n));
    let xml = body(&[numbered(1, 0), numbered(2, 0), numbered(5, 0), numbered(1, 0)], Anchor::Top);
    // 1 is the default start, 2 follows 1, 5 follows 2 no better than a restart does, and a
    // 1 after a numbered paragraph has to say so or PowerPoint carries on counting.
    assert_eq!(xml.matches("startAt").count(), 2, "{xml}");
    assert!(xml.contains("startAt=\"5\"") && xml.contains("startAt=\"1\""), "{xml}");
}

#[test]
fn levels_stop_at_the_eighth() {
    let deep = para(vec![run("t")], Align::Left, 200, Bullet::Dot);
    let xml = body(&[deep], Anchor::Top);
    assert!(xml.contains("lvl=\"8\"") && !xml.contains("lvl=\"200\""), "{xml}");
}

#[test]
fn sizes_and_angles_convert() {
    assert!(run_xml(48.0).contains("sz=\"2400\""));
    assert!(run_xml(0.2).contains("sz=\"100\""));
    assert!(run_xml(1.0e9).contains("sz=\"400000\""));
    assert!(!run_xml(f32::NAN).contains("sz="));
    assert!(shapes::xfrm_xml(BOX, 90.0).contains("rot=\"5400000\""));
    assert!(shapes::xfrm_xml(BOX, -90.0).contains("rot=\"16200000\""));
    assert!(!shapes::xfrm_xml(BOX, f32::INFINITY).contains("rot="));
}

#[test]
fn special_characters_stay_valid_xml() {
    let xml = run_xml_of("a & <b> \"c\" \u{0B} 🎉");
    let wrapped = format!("<r xmlns:a=\"x\">{xml}</r>");
    let mut reader = quick_xml::Reader::from_str(&wrapped);
    loop {
        if let quick_xml::events::Event::Eof = reader.read_event().unwrap() {
            break;
        }
    }
    assert!(xml.contains("a &amp; &lt;b&gt; &quot;c&quot;  🎉"), "{xml}");
}

#[test]
fn spacing_is_written_in_hundredths_of_a_point_ahead_of_the_bullet() {
    let spaced = NativeParagraph { line_height: Some(50.0), space_before: Some(24.0), ..para(vec![run("x")], Align::Left, 0, Bullet::Dot) };
    let xml = text::ppr_xml(&spaced, None, false);
    assert!(
        xml.contains("<a:lnSpc><a:spcPts val=\"2500\"/></a:lnSpc><a:spcBef><a:spcPts val=\"1200\"/></a:spcBef><a:buClr>"),
        "{xml}"
    );
}

#[test]
fn spacing_is_held_to_what_the_schema_allows() {
    let spaced = |line_height: f32, space_before: f32| {
        let paragraph = NativeParagraph { line_height: Some(line_height), space_before: Some(space_before), ..plain("x") };
        text::ppr_xml(&paragraph, None, false)
    };
    assert_eq!(
        spaced(1.0e6, 1.0e6),
        "<a:pPr><a:lnSpc><a:spcPts val=\"158400\"/></a:lnSpc><a:spcBef><a:spcPts val=\"158400\"/></a:spcBef></a:pPr>"
    );
    assert_eq!(
        spaced(f32::NAN, -5.0),
        "<a:pPr><a:lnSpc><a:spcPts val=\"0\"/></a:lnSpc><a:spcBef><a:spcPts val=\"0\"/></a:spcBef></a:pPr>"
    );
}

#[test]
fn a_bullet_is_written_in_the_colour_it_is_given() {
    let red = NativeParagraph { bullet_color: Some(Color::rgb(0xcc, 0, 0)), ..para(vec![run("x")], Align::Left, 0, Bullet::Dot) };
    let xml = text::ppr_xml(&red, None, false);
    assert!(xml.contains("<a:buClr><a:srgbClr val=\"CC0000\"/></a:buClr><a:buFont typeface=\"Arial\"/>"), "{xml}");
    assert!(!xml.contains("accent1"), "{xml}");
}

#[test]
fn a_bullet_with_no_colour_stays_the_accent() {
    let xml = text::ppr_xml(&para(vec![run("x")], Align::Left, 0, Bullet::Dot), None, false);
    assert!(xml.contains("<a:buClr><a:schemeClr val=\"accent1\"/></a:buClr>"), "{xml}");
}

#[test]
fn a_number_is_written_in_the_colour_it_is_given() {
    let red = NativeParagraph { bullet_color: Some(Color::rgb(0xcc, 0, 0)), ..para(vec![run("x")], Align::Left, 0, Bullet::Number(1)) };
    let xml = text::ppr_xml(&red, None, false);
    assert!(xml.contains("<a:buClr><a:srgbClr val=\"CC0000\"/></a:buClr><a:buFont typeface=\"+mj-lt\"/>"), "{xml}");
    let plain_number = text::ppr_xml(&para(vec![run("x")], Align::Left, 0, Bullet::Number(1)), None, false);
    assert!(!plain_number.contains("buClr"), "{plain_number}");
}

// ---- shapes.rs ----

#[test]
fn transforms_match_the_reference() {
    assert_eq!(shapes::xfrm_xml(BOX, 0.0), include_str!("reference/xfrm-plain.xml"));
    assert_eq!(shapes::xfrm_xml(BOX, 90.0), include_str!("reference/xfrm-rot90.xml"));
    assert_eq!(shapes::xfrm_xml(BOX, -90.0), include_str!("reference/xfrm-rot-minus90.xml"));
}

#[test]
fn shapes_and_text_boxes_match_the_reference() {
    assert_eq!(shape_xml(&text_content(vec![plain("Hello")]), BOX), include_str!("reference/text-box.xml"));
    let go = para(vec![NativeRun { color: Color::rgb(255, 255, 255), ..run("Go") }], Align::Center, 0, Bullet::None);
    let blue = Some(Color::rgb(0x25, 0x63, 0xeb));
    assert_eq!(
        shape_xml(&shape(ShapeKind::Rect, blue, None, 0.0, 0.0, vec![go], 12.0, true), BOX),
        include_str!("reference/shape-rect-text.xml")
    );
    let wide = Rect { x: 0.0, y: 0.0, w: 400.0, h: 200.0 };
    assert_eq!(rounded(wide, 24.0), include_str!("reference/shape-rounded.xml"));
    let violet = Some(Color { a: 128, ..Color::rgb(0x7c, 0x3a, 0xed) });
    let ellipse = shape(ShapeKind::Ellipse, violet, Some(Color::rgb(0, 0, 0)), 2.0, 0.0, vec![], 0.0, false);
    assert_eq!(shape_xml(&ellipse, BOX), include_str!("reference/shape-ellipse.xml"));
    assert_eq!(shape_of(ShapeKind::Line), include_str!("reference/shape-line.xml"));
}

#[test]
fn centred_text_is_centred_whatever_each_paragraph_says() {
    let left = plain("x");
    let xml = shape_xml(&shape(ShapeKind::Rect, Some(INK), None, 0.0, 0.0, vec![left], 0.0, true), BOX);
    assert!(xml.contains("anchor=\"ctr\"") && xml.contains("algn=\"ctr\""), "{xml}");
}

#[test]
fn an_unfilled_unlined_rectangle_is_a_text_box() {
    let xml = shape_xml(&shape(ShapeKind::Rect, None, None, 0.0, 0.0, vec![], 0.0, false), BOX);
    assert!(xml.contains("name=\"TextBox 4\"") && xml.contains("txBox=\"1\""), "{xml}");
}

#[test]
fn an_arrow_is_a_line_with_a_head() {
    let xml = shape_of(ShapeKind::Arrow);
    assert!(xml.contains("prst=\"line\"") && xml.contains("<a:tailEnd type=\"triangle\"/></a:ln>"), "{xml}");
    assert!(!shape_of(ShapeKind::Line).contains("tailEnd"));
}

#[test]
fn a_callout_is_a_rounded_callout() {
    assert!(shape_of(ShapeKind::Callout).contains("prst=\"wedgeRoundRectCallout\""));
}

#[test]
fn a_rounded_box_gets_its_radius() {
    let xml = rounded(Rect { x: 0.0, y: 0.0, w: 400.0, h: 200.0 }, 24.0);
    assert!(xml.contains("val 12000"), "{xml}");
    let capped = rounded(Rect { x: 0.0, y: 0.0, w: 400.0, h: 200.0 }, 5000.0);
    assert!(capped.contains("val 50000"), "{capped}");
    let flat = rounded(Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 }, 24.0);
    assert!(flat.contains("val 50000"), "{flat}");
}

#[test]
fn tables_and_pictures_are_not_shapes() {
    let table = NativeContent::Table { rows: vec![], column_widths: vec![], row_heights: vec![], line: INK };
    assert!(shapes::shape_xml(4, BOX, &table, &mut ctx()).is_none());
}

#[test]
fn pictures_match_the_reference() {
    assert_eq!(shapes::picture_xml(5, "Picture 5", "A \"cat\" & dog", "rId3", BOX), include_str!("reference/picture.xml"));
    assert_eq!(shapes::picture_xml(5, "Picture 5", "", "rId3", BOX), include_str!("reference/picture-no-alt.xml"));
}

#[test]
fn placeholders_match_the_reference() {
    let title = Ph { kind: Some("title"), idx: None };
    let mut words = run("Hi & bye");
    words.size = 52.0;
    words.family = String::new();
    let paragraphs = [para(vec![words], Align::Left, 0, Bullet::None)];
    let inherits = shapes::placeholder_xml(2, "Title 1", &title, None, &paragraphs, &mut ctx(), &BodyOptions::default());
    assert_eq!(inherits, include_str!("reference/placeholder-inherits.xml"));
    let placed = shapes::placeholder_xml(2, "Title 1", &title, Some(BOX), &paragraphs, &mut ctx(), &BodyOptions::default());
    assert_eq!(placed, include_str!("reference/placeholder-placed.xml"));
}

#[test]
fn backgrounds_and_slides_match_the_reference() {
    let white = Color::rgb(255, 255, 255);
    assert_eq!(shapes::background_xml(white, None), include_str!("reference/background-color.xml"));
    assert_eq!(shapes::background_xml(white, Some("rId2")), include_str!("reference/background-picture.xml"));
    let blue = Some(Color::rgb(0x25, 0x63, 0xeb));
    let one = shape_xml_with_id(2, &shape(ShapeKind::Rect, blue, None, 0.0, 0.0, vec![], 0.0, false));
    assert_eq!(shapes::slide_xml(&one, &shapes::background_xml(white, None), "", ""), include_str!("reference/slide-one-shape.xml"));
}

fn shape_xml_with_id(id: u32, content: &NativeContent) -> String {
    shapes::shape_xml(id, BOX, content, &mut ctx()).unwrap()
}

#[test]
fn graphic_frames_match_the_reference() {
    assert_eq!(
        shapes::graphic_frame_xml(6, "Table 6", BOX, table::TABLE_URI, "<x/>"),
        include_str!("reference/graphic-frame.xml")
    );
}

// ---- table.rs ----

fn cell(text: &str, bold: bool) -> Vec<NativeParagraph> {
    vec![para(vec![NativeRun { bold, ..run(text) }], Align::Left, 0, Bullet::None)]
}

#[test]
fn tables_match_the_reference() {
    let rows = vec![vec![cell("A", true), cell("B", true)], vec![cell("c", false), cell("d", false)]];
    let xml = table::table_xml(&rows, &[300.0, 420.0], &[100.0, 116.0], BOX, INK, &mut ctx());
    assert_eq!(xml, include_str!("reference/table-2x2.xml"));
    assert_eq!(table::table_styles_xml(), include_str!("reference/table-styles.xml"));
}

#[test]
fn a_short_row_is_filled_out_and_odd_sizes_are_shared_evenly() {
    let rows = vec![vec![cell("a", false), cell("b", false), cell("c", false)], vec![cell("d", false)]];
    let xml = table::table_xml(&rows, &[], &[1.0], BOX, INK, &mut ctx());
    assert_eq!(xml.matches("<a:tc>").count(), 6, "{xml}");
    assert_eq!(xml.matches("<a:gridCol w=\"1524000\"/>").count(), 3, "{xml}");
    assert_eq!(xml.matches("<a:tr h=\"685800\">").count(), 2, "{xml}");
}

// ---- xml.rs ----

#[test]
fn guids_match_the_reference() {
    let reference = include_str!("reference/guid.txt");
    let expected: Vec<&str> = reference.lines().collect();
    assert_eq!(xml::guid("deck-table-default"), expected[0]);
    assert_eq!(xml::guid("é🎉"), expected[1]);
}
