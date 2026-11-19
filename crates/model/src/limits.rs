//! The most a model may be, as the 3D spike set them. Each is checked before anything it
//! counts is decoded.

/// The longest model file.
pub const MOST_BYTES: u64 = 256 * 1024 * 1024;
/// The most triangles, counting each time a mesh is placed.
pub const MOST_TRIANGLES: u64 = 4_000_000;
/// The most vertices, counting each time a mesh is placed.
pub const MOST_VERTICES: u64 = 4_000_000;
/// The most points in a cloud.
pub const MOST_POINTS: u64 = 20_000_000;
/// The widest or tallest texture.
pub const MOST_TEXTURE_SIDE: u32 = 8192;
/// The most texture pixels in all.
pub const MOST_TEXTURE_PIXELS: u64 = 64_000_000;
/// The longest glTF description.
pub const MOST_DESCRIPTION: u64 = 8 * 1024 * 1024;
/// The longest point cloud header.
pub const MOST_PLY_HEADER: u64 = 64 * 1024;

/// The most places a mesh is put, and nodes visited finding them, so nodes that each hold the
/// next many times over can't keep the reading going.
pub(crate) const MOST_PLACES: u64 = 100_000;
/// How deep nodes may hold each other. Deeper is taken as nodes holding themselves.
pub(crate) const MOST_DEPTH: usize = 64;
/// The most objects, and the most values of any kind, in a glTF description. Each object read
/// takes up to a few hundred bytes and each value a few tens, so these keep a description of 8 MB
/// from taking much more than 100 MB once read.
pub(crate) const MOST_OBJECTS: usize = 100_000;
pub(crate) const MOST_VALUES: usize = 1_000_000;
/// The most parts of a JPEG looked through for its size.
pub(crate) const MOST_JPEG_PARTS: u32 = 4096;
/// The most steps taken walking a JPEG, each a part or a piece of 64 KB: enough for its parts and
/// for a file of the most size read through in pieces twice over.
pub(crate) const MOST_JPEG_PASSES: u32 = 4 * MOST_JPEG_PARTS + 2 * (MOST_BYTES / (64 * 1024)) as u32;
/// The most memory decoding the texture may take at once.
pub const MOST_DECODING: u64 = 300 * 1024 * 1024;
