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

#[derive(Clone)]
pub enum DisplayItem {
    Fill { path: tiny_skia::Path, color: Color },
    Stroke { path: tiny_skia::Path, color: Color, width: f32 },
    Text { text: ShapedText, x: f32, y: f32 },
    /// `src` is the file of the talk it was read from. An icon or a path has none.
    Picture { pixmap: Arc<tiny_skia::Pixmap>, src: Option<String>, rect: Rect },
}

/// One element's drawing. Item coordinates are relative to `rect`'s top-left corner,
/// and `rotate` (degrees) turns the whole layer around the centre of `rect`.
#[derive(Clone)]
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
    /// The video this layer is the still of, which plays over it when asked.
    pub video: Option<VideoOn>,
    /// The 3D model this layer is the still of, which is drawn live over it when it's turned.
    pub model: Option<ModelOn>,
}

/// A video among a slide's layers: the place of its layer among them, where its tag is written,
/// the click it appears on, and what it plays.
#[derive(Clone, Debug, PartialEq)]
pub struct VideoLayer {
    pub layer: usize,
    pub source: Option<Range<usize>>,
    pub click: Option<u32>,
    pub on: VideoOn,
}

/// A video on a slide: the file it plays, whether it goes round again at its end, and where it
/// plays, as its layer is placed.
#[derive(Clone, Debug, PartialEq)]
pub struct VideoOn {
    pub src: String,
    pub looped: bool,
    pub rect: Rect,
    pub rotate: f32,
}

/// A drawn element in device pixels, placed at (`x`, `y`) on the slide. It has no
/// source range: a cached layer can outlive the positions it was drawn from. A layer only
/// moved shows the same pixmap somewhere else.
pub struct Layer {
    pub pixmap: Arc<tiny_skia::Pixmap>,
    pub x: i32,
    pub y: i32,
}

/// A 3D model among a slide's layers: the place of its layer among them, where its tag is
/// written, the click it appears on, and what it shows.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelLayer {
    pub layer: usize,
    pub source: Option<Range<usize>>,
    pub click: Option<u32>,
    pub on: ModelOn,
}

/// A 3D model on a slide: the file it's drawn from, whether it turns slowly on its own, the
/// view it starts at, or `None` to be framed whole, the views each click goes on to, and where
/// it's drawn, as its layer is placed.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelOn {
    pub src: String,
    pub turn: bool,
    pub view: Option<slides_model::View>,
    pub views: Vec<slides_model::View>,
    pub rect: Rect,
    pub rotate: f32,
}
