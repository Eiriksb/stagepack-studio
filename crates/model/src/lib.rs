//! Reading a 3D model: a binary glTF (`.glb`) with everything inside it, or a point cloud
//! (`.ply`), and, with the `draw` feature, drawing one off-screen into a picture on a thread of
//! its own. This knows nothing of windows: what it gives out is plain arrays.
//!
//! A model is data. Every size, count and place a file claims is checked against what holds it
//! and the file's own length before anything is allocated or read, and against the limits
//! before anything is decoded. Nothing outside the one file is read: a model naming another
//! file, a link or data written into its description is refused. A file that isn't a model that
//! can be drawn says why in plain words rather than crashing or hanging.

#[cfg(feature = "draw")]
mod device;
#[cfg(feature = "draw")]
mod draw;
mod glb;
mod image_size;
pub mod limits;
#[cfg(feature = "make")]
mod make;
mod maths;
mod ply;
mod view;

use std::fmt;
use std::time::Duration;
use std::io::{Read, Seek, SeekFrom};
use std::panic::{AssertUnwindSafe, catch_unwind};

#[cfg(feature = "draw")]
pub use device::{AdapterKind, adapter_kinds};
#[cfg(feature = "draw")]
pub use draw::{Answer, Drawer, DrawerOptions, MOST_RETRY, NotDrawn, Scene};
pub use limits::*;
#[cfg(feature = "make")]
pub use make::{BOX_COLOURS, GRID_COLOURS, MadeGlb, glb_from, glb_parts, make_test_glb, make_test_ply};
pub use view::View;

/// Something a model can be read from in pieces, from anywhere in it.
pub trait ReadSeek: Read + Seek + Send {}

impl<T: Read + Seek + Send> ReadSeek for T {}

/// Why a model can't be drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CantDraw {
    NotAModel,
    Damaged,
    NeedsOtherFiles,
    KindNotDrawn,
    TooLarge,
    CouldNotRead,
    NoAdapter,
    /// The graphics card refused the model though it's within the limits: a card with less
    /// memory, or smaller buffers, than the model needs.
    CardRefused,
}

impl fmt::Display for CantDraw {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            CantDraw::NotAModel => "This file isn't a 3D model.",
            CantDraw::Damaged => "This model is damaged, so it can't be drawn.",
            CantDraw::NeedsOtherFiles => {
                "This model needs other files kept beside it, so it can't be drawn. A model saved as one file (.glb) with everything inside it can."
            }
            CantDraw::KindNotDrawn => {
                "This kind of model can't be drawn here. A model of meshes saved as one file (.glb), or a point cloud (.ply), can."
            }
            CantDraw::TooLarge => {
                "This model is too large or has too many parts to draw here. Models of up to 256 MB, 4 million triangles and 20 million points can be drawn."
            }
            CantDraw::CouldNotRead => "This model couldn't be read.",
            CantDraw::NoAdapter => {
                "This computer can't draw 3D models here. The slide shows a still when it's presented somewhere that can."
            }
            CantDraw::CardRefused => {
                "This computer's graphics can't draw this model, though it's within what can be drawn. The slide shows a still when it's presented somewhere that can."
            }
        })
    }
}

impl std::error::Error for CantDraw {}

/// What a model is made of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelKind {
    Mesh,
    Points,
}

/// What a model is, as its headers say.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelFacts {
    pub kind: ModelKind,
    /// Counting each time a mesh is placed in the model.
    pub triangles: u64,
    pub points: u64,
    /// The lowest and highest corners of a box around the whole model, in its own units. A point
    /// cloud's header doesn't say where its points lie, so `facts` gives one a box of nothing at
    /// the origin: use `read(...).facts` for a cloud's.
    pub bounds: ([f32; 3], [f32; 3]),
}

/// A model, read whole and checked.
#[derive(Clone, Debug)]
pub struct Model {
    /// Each time a part of a mesh is placed in the model, in the model's own space.
    pub meshes: Vec<Mesh>,
    pub points: Option<Points>,
    /// The one picture the meshes are coloured from, if any is.
    pub texture: Option<Texture>,
    /// What the model is, with its bounds as its contents give them.
    pub facts: ModelFacts,
}

/// Triangles of one colour and material, placed where the model has them.
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub positions: Vec<[f32; 3]>,
    /// One for each position, of length one: the model's own, or made from its triangles.
    pub normals: Vec<[f32; 3]>,
    /// Where each position lies on the texture, from its top left, when `textured`; otherwise
    /// empty.
    pub texture_coords: Vec<[f32; 2]>,
    /// One for each position: its own colour times its material's, linear, straight alpha.
    pub colours: Vec<[f32; 4]>,
    /// Three for each triangle, each turning anticlockwise seen from the front.
    pub indices: Vec<u32>,
    /// Whether its colours are times the model's texture.
    pub textured: bool,
    /// Whether its triangles are seen from behind too.
    pub double_sided: bool,
}

/// A cloud of points.
#[derive(Clone, Debug, Default)]
pub struct Points {
    pub positions: Vec<[f32; 3]>,
    /// One for each position, as the file gives them: sRGB, straight alpha, white when it gives
    /// none.
    pub colours: Vec<[u8; 4]>,
}

/// A picture, as sRGB straight-alpha RGBA, rows from the top.
#[derive(Clone)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl fmt::Debug for Texture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Texture").field("width", &self.width).field("height", &self.height).finish()
    }
}

/// A model drawn: straight-alpha BGRA, rows from the top, opaque where the model is and clear
/// elsewhere.
#[derive(Clone)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
    /// How long the graphics card took to draw it.
    pub drawing: Duration,
    /// How long it took to bring the picture back from the card.
    pub reading_back: Duration,
}

impl fmt::Debug for Picture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Picture").field("width", &self.width).field("height", &self.height).finish()
    }
}

/// The two kinds of file read, told from how they start.
enum Kind {
    Glb,
    Ply,
}

/// How long the file is, checked against the most allowed, and which kind it is.
fn kind(reader: &mut dyn ReadSeek) -> Result<(Kind, u64), CantDraw> {
    let len = reader.seek(SeekFrom::End(0)).map_err(|_| CantDraw::CouldNotRead)?;
    let mut start = [0u8; 4];
    let have = len.min(4) as usize;
    reader.seek(SeekFrom::Start(0)).map_err(|_| CantDraw::CouldNotRead)?;
    reader.read_exact(&mut start[..have]).map_err(|_| CantDraw::CouldNotRead)?;
    let kind = match &start[..have] {
        b"glTF" => Kind::Glb,
        b"ply\n" | b"ply\r" => Kind::Ply,
        _ => return Err(CantDraw::NotAModel),
    };
    if len > MOST_BYTES {
        return Err(CantDraw::TooLarge);
    }
    Ok((kind, len))
}

/// Runs `work`, a panic in it said as the file being damaged.
fn guarded<T>(work: impl FnOnce() -> Result<T, CantDraw>) -> Result<T, CantDraw> {
    catch_unwind(AssertUnwindSafe(work)).unwrap_or(Err(CantDraw::Damaged))
}

/// Says what a model is from its headers alone: a glTF's description, of 8 MB at most, and its
/// texture's header; a point cloud's header. Every count is checked against the limits and the
/// file's length.
pub fn facts(mut reader: Box<dyn ReadSeek>) -> Result<ModelFacts, CantDraw> {
    guarded(|| {
        let (kind, len) = kind(&mut *reader)?;
        match kind {
            Kind::Glb => glb::facts(&mut *reader, len),
            Kind::Ply => ply::facts(&mut *reader, len),
        }
    })
}

/// Reads a whole model, checking all of it, or says why it can't be drawn.
pub fn read(mut reader: Box<dyn ReadSeek>) -> Result<Model, CantDraw> {
    guarded(|| {
        let (kind, len) = kind(&mut *reader)?;
        match kind {
            Kind::Glb => glb::read(&mut *reader, len),
            Kind::Ply => ply::read(&mut *reader, len),
        }
    })
}

/// Reads `len` bytes at `at`, which the caller has checked lie inside the file.
pub(crate) fn read_at(reader: &mut dyn ReadSeek, at: u64, len: u64) -> Result<Vec<u8>, CantDraw> {
    let len = usize::try_from(len).map_err(|_| CantDraw::TooLarge)?;
    let mut bytes = vec![0; len];
    reader.seek(SeekFrom::Start(at)).map_err(|_| CantDraw::CouldNotRead)?;
    reader.read_exact(&mut bytes).map_err(|_| CantDraw::CouldNotRead)?;
    Ok(bytes)
}

/// The box around `positions`, or a box of nothing at the origin if there are none.
pub(crate) fn bounds_of<'a>(positions: impl IntoIterator<Item = &'a [f32; 3]>) -> ([f32; 3], [f32; 3]) {
    let mut low = [f32::INFINITY; 3];
    let mut high = [f32::NEG_INFINITY; 3];
    for p in positions {
        for i in 0..3 {
            low[i] = low[i].min(p[i]);
            high[i] = high[i].max(p[i]);
        }
    }
    if low[0] > high[0] { ([0.0; 3], [0.0; 3]) } else { (low, high) }
}
