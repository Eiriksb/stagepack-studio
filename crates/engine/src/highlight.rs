//! Colours code by its language.

use std::sync::OnceLock;

use syntect::easy::HighlightLines;
use syntect::highlighting::{self, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
use toolkit::Color;

fn assets() -> &'static (SyntaxSet, highlighting::Theme) {
    static ASSETS: OnceLock<(SyntaxSet, highlighting::Theme)> = OnceLock::new();
    ASSETS.get_or_init(|| {
        let theme = ThemeSet::load_defaults().themes.remove("InspiredGitHub").expect("syntect ships this theme");
        (SyntaxSet::load_defaults_newlines(), theme)
    })
}

/// One list of coloured pieces per line, each line keeping its line ending. A language
/// syntect doesn't know comes back as plain text.
pub fn highlight(code: &str, language: Option<&str>) -> Vec<Vec<(Color, String)>> {
    let (syntaxes, theme) = assets();
    let syntax = language
        .and_then(|name| syntaxes.find_syntax_by_token(name))
        .unwrap_or_else(|| syntaxes.find_syntax_plain_text());
    let plain = theme.settings.foreground.map_or(Color::rgb(0, 0, 0), to_color);
    let mut lines = HighlightLines::new(syntax, theme);
    LinesWithEndings::from(code)
        .map(|line| match lines.highlight_line(line, syntaxes) {
            Ok(pieces) => pieces.into_iter().map(|(style, text)| (to_color(style.foreground), text.to_string())).collect(),
            Err(_) => vec![(plain, line.to_string())],
        })
        .collect()
}

fn to_color(color: highlighting::Color) -> Color {
    Color { r: color.r, g: color.g, b: color.b, a: color.a }
}
