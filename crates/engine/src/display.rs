//! What an element draws, in slide units, before any scale or rotation is applied.

use std::ops::Range;
use std::sync::Arc;

use toolkit::Color;

use crate::{Enter, NativeElement, ShapedText};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub enum DisplayItem {
    Fill { path: tiny_skia::Path, color: Color },
    Stroke { path: tiny_skia::Path, color: Color, width: f32 },
    Text { text: ShapedText, x: f32, y: f32 },
    /// `src` is the file of the talk it was read from. An icon or a path has none.
    Picture { pixmap: Arc<tiny_skia::Pixmap>, src: Option<String>, rect: Rect },
}

/// One element's drawing. Item coordinates are relative to `rect`'s top-left corner,
/// and `rotate` (degrees) turns the whole layer around the centre of `rect`.
pub struct LayerSpec {
    pub source: Option<Range<usize>>,
    pub rect: Rect,
    pub rotate: f32,
    pub items: Vec<DisplayItem>,
    /// The click this layer appears on, or `None` if it's there from the start.
    pub click: Option<u32>,
    pub enter: Enter,
    /// What the layer is made of, for export. The background has nothing here.
    pub native: Vec<NativeElement>,
    /// Whether this layer is the slide's title.
    pub is_title: bool,
    /// Whether something in this layer changes with the moment the slide is at.
    pub varies: bool,
}

/// A drawn element in device pixels, placed at (`x`, `y`) on the slide. It has no
/// source range: a cached layer can outlive the positions it was drawn from.
pub struct Layer {
    pub pixmap: tiny_skia::Pixmap,
    pub x: i32,
    pub y: i32,
}
