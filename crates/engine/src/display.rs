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
    /// Shared, so a layer laid out again from what's kept, as a terminal recording's screen is,
    /// holds the same items rather than a copy of them.
    pub items: Arc<[DisplayItem]>,
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
    /// The terminal recording this layer is the last screen of, which plays over it when asked.
    pub terminal: Option<TerminalOn>,
    /// The button this layer draws, which a press in the show acts on.
    pub button: Option<ButtonOn>,
    /// The `id` its tag gives, by which a button names it: none for a region, or a tag that gives none.
    pub id: Option<String>,
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

/// So that layers held alone and layers shared with the deck are drawn by the same code.
impl AsRef<Layer> for Layer {
    fn as_ref(&self) -> &Layer {
        self
    }
}

/// A 3D model among a slide's layers: the place of its layer among them, where its tag is
/// written, the click it appears on, and what it shows.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelLayer {
    pub layer: usize,
    pub source: Option<Range<usize>>,
    pub click: Option<u32>,
    /// The `id` its tag gives.
    pub id: Option<String>,
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

/// A terminal recording among a slide's layers: the place of its layer among them, where its tag
/// is written, the click it appears on, and what it plays.
#[derive(Clone, Debug, PartialEq)]
pub struct TerminalLayer {
    pub layer: usize,
    pub source: Option<Range<usize>>,
    pub click: Option<u32>,
    /// The `id` its tag gives, by which a button's `in` names it.
    pub id: Option<String>,
    pub on: TerminalOn,
}

/// A terminal on a slide: the recording it plays, the command it runs, where it plays, as its layer is placed,
/// and how many times it stops until the next click, once its file has been read: none until
/// then, or if it can't be played. Two are the same recording, playing the same, whether or not
/// either's screen is still being shaped.
#[derive(Clone, Debug)]
pub struct TerminalOn {
    /// The recording it plays or is the last screen of: empty if it names none.
    pub src: String,
    /// The command it runs when it's run live, as it's written, if it names one. The engine only
    /// reads it: nothing here ever starts it.
    pub run: Option<String>,
    /// The columns and rows of its live screen: 100 by 30 if it doesn't say. A figure it wrote
    /// that isn't a number of columns or rows at all is 0.
    pub cols: u16,
    pub rows: u16,
    pub rect: Rect,
    pub rotate: f32,
    pub stops: usize,
    /// Whether its screen is still being shaped and placed aside: until it is, its layer is its
    /// panel alone, and what was drawn of it before is shown in its place.
    pub pending: bool,
}

impl PartialEq for TerminalOn {
    fn eq(&self, other: &TerminalOn) -> bool {
        (&self.src, &self.run, self.cols, self.rows, self.rect, self.rotate, self.stops) == (&other.src, &other.run, other.cols, other.rows, other.rect, other.rotate, other.stops)
    }
}

/// A button among a slide's layers: the place of its layer among them, where its tag is written,
/// the click it appears on, and what it is.
#[derive(Clone, Debug, PartialEq)]
pub struct ButtonLayer {
    pub layer: usize,
    pub source: Range<usize>,
    pub click: Option<u32>,
    pub on: ButtonOn,
}

/// A button on a slide: the words on it, how it looks, what pressing it does, or why it does
/// nothing, its own `id`, and where it is, as its layer is placed.
#[derive(Clone, Debug, PartialEq)]
pub struct ButtonOn {
    pub label: String,
    pub look: ButtonLook,
    pub action: Result<ButtonAction, ButtonFault>,
    pub id: Option<String>,
    pub rect: Rect,
    pub rotate: f32,
}

/// What pressing a button does: one of these, as its attributes say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ButtonAction {
    /// Types `text`, then Enter, into the live terminal on the slide whose `id` is `terminal`.
    Run { text: String, terminal: String },
    /// Sends the action `action`, with `value`, empty if it gives none, to the interactive thing
    /// on the slide whose `id` is `target`.
    Do { action: String, target: String, value: String },
    /// Goes to this click step of the slide.
    Step(u32),
}

/// Why a button does nothing when it's pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonFault {
    /// It gives none of `run`, `do` and `step`.
    NoAction,
    /// It gives more than one of them.
    ManyActions,
    /// Its `step` isn't a whole number from 0.
    BadStep,
}

/// How a button looks, coloured from the theme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonLook {
    /// Filled with the accent colour.
    #[default]
    Primary,
    /// Tinted with the accent colour.
    Secondary,
    /// Barely tinted, with the text colour.
    Quiet,
}
