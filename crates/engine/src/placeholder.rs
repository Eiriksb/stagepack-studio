//! What's drawn in place of something that can't be: a dashed outline and a label.

use tiny_skia::{PathBuilder, StrokeDash};
use toolkit::{Color, Theme};

use crate::{Align, DisplayItem, Fonts, Rect, TextRun, shape_text};

const GREY: Color = Color::rgb(0x94, 0xa3, 0xb8);
const OUTLINE_WIDTH: f32 = 2.0;
const DASH: f32 = 12.0;
const DASH_GAP: f32 = 8.0;
const LABEL_SIZE: f32 = 24.0;

fn dashed_outline(rect: Rect) -> Option<tiny_skia::Path> {
    // Inset by half the line, so that the line stays inside the rect.
    let inset = OUTLINE_WIDTH / 2.0;
    let outline = tiny_skia::Rect::from_xywh(rect.x + inset, rect.y + inset, rect.w - OUTLINE_WIDTH, rect.h - OUTLINE_WIDTH)?;
    PathBuilder::from_rect(outline).dash(&StrokeDash::new(vec![DASH, DASH_GAP], 0.0)?, 1.0)
}

/// How a placeholder's label looks.
pub(crate) fn placeholder_run(label: &str, theme: &Theme) -> TextRun {
    TextRun {
        text: label.to_string(),
        size: LABEL_SIZE,
        bold: false,
        italic: false,
        underline: false,
        color: GREY,
        family: theme.font.clone(),
    }
}

/// The placeholder for `rect`, with `label` centred in it. A label too big for the rect
/// runs past it, so that it can always be read.
pub(crate) fn placeholder_items(label: &str, rect: Rect, theme: &Theme, fonts: &mut Fonts) -> Vec<DisplayItem> {
    let text = shape_text(fonts, &[placeholder_run(label, theme)], Some(rect.w), Align::Center);
    let y = rect.y + (rect.h - text.height) / 2.0;
    let outline = dashed_outline(rect).map(|path| DisplayItem::Stroke { path, color: GREY, width: OUTLINE_WIDTH });
    outline.into_iter().chain([DisplayItem::Text { text, x: rect.x, y }]).collect()
}

/// How big the lines of a command's panel are.
const COMMAND_SIZE: f32 = 24.0;
/// How far from the panel's sides a command is kept, where it can be.
const COMMAND_MARGIN: f32 = 16.0;
/// The most times a command is cut again, as it's shaped wider than its box.
const MOST_CUTS: usize = 8;

/// `command` on one line, of what fits `room` wide in a monospaced font: cut with `…` where it's
/// longer. Anything that would end a line is a space.
fn one_line(command: &str, room: f32) -> String {
    let flat: String = command.chars().map(|c| if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') { ' ' } else { c }).collect();
    let fits = (room / (0.6 * COMMAND_SIZE)).floor().max(1.0) as usize;
    cut(&flat, fits)
}

/// The first `fits` characters of `text`, the last of them `…` if it's longer.
fn cut(text: &str, fits: usize) -> String {
    if text.chars().count() <= fits {
        return text.to_string();
    }
    text.chars().take(fits.saturating_sub(1)).chain(['\u{2026}']).collect()
}

/// The panel of a terminal that runs a command, as the placeholder of one that can't be played is
/// drawn: the words `label` over `command`, cut to one line, in the theme's monospaced font.
pub(crate) fn command_items(label: &str, command: &str, rect: Rect, theme: &Theme, fonts: &mut Fonts) -> Vec<DisplayItem> {
    let room = (rect.w - 2.0 * COMMAND_MARGIN).max(COMMAND_SIZE);
    let run = |text: &str| TextRun { family: theme.mono_font.clone(), size: COMMAND_SIZE, ..placeholder_run(text, theme) };
    let mut line = one_line(command, room);
    let mut command_text = shape_text(fonts, &[run(&line)], None, Align::Left);
    // A font that draws a character wider than a cell has it cut again, to what fits.
    for _ in 0..MOST_CUTS {
        if command_text.width <= room || line.chars().count() <= 1 {
            break;
        }
        let keep = ((line.chars().count() as f32) * room / command_text.width).floor() as usize;
        line = cut(&line, keep.clamp(1, line.chars().count() - 1));
        command_text = shape_text(fonts, &[run(&line)], None, Align::Left);
    }
    let label_text = shape_text(fonts, &[run(label)], None, Align::Left);
    let gap = COMMAND_SIZE / 2.0;
    let top = rect.y + (rect.h - label_text.height - gap - command_text.height) / 2.0;
    let centred = |width: f32| rect.x + (rect.w - width) / 2.0;
    let outline = dashed_outline(rect).map(|path| DisplayItem::Stroke { path, color: GREY, width: OUTLINE_WIDTH });
    let below = top + label_text.height + gap;
    let (label_x, command_x) = (centred(label_text.width), centred(command_text.width));
    outline
        .into_iter()
        .chain([DisplayItem::Text { text: label_text, x: label_x, y: top }, DisplayItem::Text { text: command_text, x: command_x, y: below }])
        .collect()
}
