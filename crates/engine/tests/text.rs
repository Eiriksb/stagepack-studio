use slides_engine::*;
use toolkit::Color;

fn run(text: &str) -> TextRun {
    TextRun { text: text.into(), size: 48.0, bold: false, italic: false, underline: false, color: Color { r: 0, g: 0, b: 0, a: 255 }, family: "Inter".into() }
}

fn sized(text: &str, size: f32) -> TextRun {
    TextRun { size, ..run(text) }
}

/// How many pixels the text covers when drawn into a small target.
fn painted(text: &ShapedText, fonts: &mut Fonts) -> usize {
    let mut target = tiny_skia::Pixmap::new(400, 200).unwrap();
    draw_text(&mut target.as_mut(), text, fonts, tiny_skia::Transform::from_translate(10.0, 10.0));
    target.pixels().iter().filter(|p| p.alpha() > 0).count()
}

#[test]
fn size_zero_is_treated_as_the_minimum() {
    let mut fonts = Fonts::bundled_only();
    let zero = shape_text(&mut fonts, &[sized("hi", 0.0)], None, Align::Left);
    let one = shape_text(&mut fonts, &[sized("hi", 1.0)], None, Align::Left);
    assert!(zero.height <= one.height + 1.0, "height is {}", zero.height);
    let (zero_painted, one_painted) = (painted(&zero, &mut fonts), painted(&one, &mut fonts));
    assert!(zero_painted <= one_painted, "painted {zero_painted} pixels, against {one_painted} at size 1");
}

#[test]
fn a_size_that_is_not_finite_becomes_the_minimum() {
    let mut fonts = Fonts::bundled_only();
    let one = shape_text(&mut fonts, &[sized("hi", 1.0)], None, Align::Left);
    for size in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let odd = shape_text(&mut fonts, &[sized("hi", size)], None, Align::Left);
        assert!(odd.height <= one.height + 1.0, "height is {} for size {size}", odd.height);
        assert!(painted(&odd, &mut fonts) <= painted(&one, &mut fonts));
    }
}

#[test]
fn huge_sizes_are_limited() {
    let mut fonts = Fonts::bundled_only();
    for text in ["hi", "😀"] {
        let huge = shape_text(&mut fonts, &[sized(text, 100000.0)], None, Align::Left);
        let limit = shape_text(&mut fonts, &[sized(text, 512.0)], None, Align::Left);
        assert!(huge.height <= limit.height, "height is {} for {text}", huge.height);
        painted(&huge, &mut fonts);
    }
}

#[test]
fn latin_and_emoji_need_no_notice() {
    let mut fonts = Fonts::bundled_only();
    assert!(shape_text(&mut fonts, &[run("blåbær 🎉 Ж")], None, Align::Left).notices.is_empty());
}

#[test]
fn emoji_are_drawn_in_the_bundled_colour_font_before_any_of_this_computers() {
    // A computer's own fonts, such as DejaVu Sans, may have some emoji in one colour.
    let mut fonts = Fonts::with_system_fallback();
    for text in ["😀", "a 🎉 b", "👍🏽", "❤️", "🇳🇴"] {
        let notices = shape_text(&mut fonts, &[run(text)], None, Align::Left).notices;
        assert!(notices.is_empty(), "{text}: {notices:?}");
    }
}

#[test]
fn characters_nothing_covers_raise_a_notice() {
    let mut fonts = Fonts::bundled_only();
    assert!(shape_text(&mut fonts, &[run("日本語")], None, Align::Left).notices[0].0.contains("日"));
}

#[test]
fn wrapping_respects_the_width() {
    let mut fonts = Fonts::bundled_only();
    let long = run("word ".repeat(40).trim());
    let wrapped = shape_text(&mut fonts, std::slice::from_ref(&long), Some(400.0), Align::Left);
    let single = shape_text(&mut fonts, &[long], None, Align::Left);
    assert!(wrapped.height > single.height * 2.0);
    assert!(wrapped.width <= 400.0);
}

#[test]
fn drawing_twice_gives_the_same_pixels() {
    let mut fonts = Fonts::bundled_only();
    let text = shape_text(&mut fonts, &[run("Same 🎉")], None, Align::Left);
    let mut a = tiny_skia::Pixmap::new(400, 100).unwrap();
    let mut b = tiny_skia::Pixmap::new(400, 100).unwrap();
    draw_text(&mut a.as_mut(), &text, &mut fonts, tiny_skia::Transform::identity());
    draw_text(&mut b.as_mut(), &text, &mut fonts, tiny_skia::Transform::identity());
    assert_eq!(a.data(), b.data());
    assert!(a.pixels().iter().any(|p| p.alpha() > 0));
}

#[test]
fn an_underlined_run_gets_one_underline_per_line() {
    let mut fonts = Fonts::bundled_only();
    let mut under = run("word ".repeat(40).trim());
    under.underline = true;
    let shaped = shape_text(&mut fonts, &[run("plain "), under], Some(400.0), Align::Left);
    let lines = shaped.buffer.layout_runs().count();
    assert!(lines > 2);
    assert_eq!(shaped.underlines.len(), lines);
    assert!(shaped.underlines.iter().all(|u| u.2 > 0.0));
}
