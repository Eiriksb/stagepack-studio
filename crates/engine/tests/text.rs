use slides_engine::*;
use toolkit::Color;

fn run(text: &str) -> TextRun {
    TextRun { text: text.into(), size: 48.0, bold: false, italic: false, underline: false, color: Color { r: 0, g: 0, b: 0, a: 255 }, family: "Inter".into() }
}

#[test]
fn latin_and_emoji_need_no_notice() {
    let mut fonts = Fonts::bundled_only();
    assert!(shape_text(&mut fonts, &[run("blåbær 🎉 Ж")], None, Align::Left).notices.is_empty());
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
    let wrapped = shape_text(&mut fonts, &[long.clone()], Some(400.0), Align::Left);
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
