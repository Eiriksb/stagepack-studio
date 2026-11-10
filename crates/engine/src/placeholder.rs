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
