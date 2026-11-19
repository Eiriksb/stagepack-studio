//! Making small models, for tests here and in the other crates.

use std::io::Cursor;

use serde_json::{Value, json};

/// The colours of the four quarters of a made grid's texture: top left, top right, bottom left,
/// bottom right. sRGB, opaque.
pub const GRID_COLOURS: [[u8; 4]; 4] = [[220, 40, 40, 255], [40, 180, 60, 255], [40, 80, 220, 255], [240, 240, 240, 255]];

/// The colours of a made box's three materials, linear: the faces looking along x, along y and
/// along z.
pub const BOX_COLOURS: [[f32; 4]; 3] = [[0.8, 0.1, 0.1, 1.0], [0.1, 0.7, 0.2, 1.0], [0.1, 0.3, 0.9, 1.0]];

/// Which model to make.
#[derive(Clone, Copy, Debug)]
pub enum MadeGlb {
    /// A flat square grid from -1 to 1 across and up, facing the front (along z), of `cells` by
    /// `cells` squares, each two triangles, with no normals of its own. Coloured from a texture
    /// of `texture` pixels a side, in four quarters of `GRID_COLOURS`, as a PNG or a JPEG.
    Grid { cells: u32, texture: u32, jpeg: bool },
    /// A box a unit a side standing on the ground, from -0.5 to 0.5 across and deep and 0 to 1
    /// up, placed by a node. Three parts of two faces each, one for each of `BOX_COLOURS`; the
    /// last has no normals of its own.
    Boxes,
}

/// A binary glTF of the model asked for.
pub fn make_test_glb(made: MadeGlb) -> Vec<u8> {
    match made {
        MadeGlb::Grid { cells, texture, jpeg } => grid(cells, texture, jpeg),
        MadeGlb::Boxes => boxes(),
    }
}

/// Values written into a block of data, each set in a view of its own.
#[derive(Default)]
struct Data {
    bin: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Data {
    fn view(&mut self, bytes: &[u8]) -> usize {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        self.views.push(json!({ "buffer": 0, "byteOffset": self.bin.len(), "byteLength": bytes.len() }));
        self.bin.extend_from_slice(bytes);
        self.views.len() - 1
    }

    fn floats<const N: usize>(&mut self, values: &[[f32; N]], bounds: bool) -> usize {
        let bytes: Vec<u8> = values.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
        let view = self.view(&bytes);
        let kind = ["SCALAR", "VEC2", "VEC3", "VEC4"][N - 1];
        let mut accessor = json!({ "bufferView": view, "componentType": 5126, "count": values.len(), "type": kind });
        if bounds {
            let low: Vec<f32> = (0..N).map(|i| values.iter().map(|v| v[i]).fold(f32::INFINITY, f32::min)).collect();
            let high: Vec<f32> = (0..N).map(|i| values.iter().map(|v| v[i]).fold(f32::NEG_INFINITY, f32::max)).collect();
            accessor["min"] = json!(low);
            accessor["max"] = json!(high);
        }
        self.accessors.push(accessor);
        self.accessors.len() - 1
    }

    fn indices_u32(&mut self, indices: &[u32]) -> usize {
        let bytes: Vec<u8> = indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let view = self.view(&bytes);
        self.accessors.push(json!({ "bufferView": view, "componentType": 5125, "count": indices.len(), "type": "SCALAR" }));
        self.accessors.len() - 1
    }

    fn indices_u16(&mut self, indices: &[u16]) -> usize {
        let bytes: Vec<u8> = indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let view = self.view(&bytes);
        self.accessors.push(json!({ "bufferView": view, "componentType": 5123, "count": indices.len(), "type": "SCALAR" }));
        self.accessors.len() - 1
    }

    /// The description around these values, with `rest` added to it.
    fn finish(mut self, rest: Value) -> Vec<u8> {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let mut json = json!({
            "asset": { "version": "2.0", "generator": "slides-model tests" },
            "scene": 0,
            "buffers": [{ "byteLength": self.bin.len() }],
            "bufferViews": self.views,
            "accessors": self.accessors,
        });
        if let (Value::Object(json), Value::Object(rest)) = (&mut json, rest) {
            json.extend(rest);
        }
        glb_from(&json.to_string(), &self.bin)
    }
}

fn grid(cells: u32, side: u32, jpeg: bool) -> Vec<u8> {
    let cells = cells.max(1);
    let across = cells + 1;
    let mut positions = Vec::new();
    let mut coords = Vec::new();
    for row in 0..across {
        for column in 0..across {
            let (u, v) = (column as f32 / cells as f32, row as f32 / cells as f32);
            positions.push([u * 2.0 - 1.0, 1.0 - v * 2.0, 0.0]);
            coords.push([u, v]);
        }
    }
    let mut indices = Vec::new();
    for row in 0..cells {
        for column in 0..cells {
            let corner = |r: u32, c: u32| (row + r) * across + column + c;
            let (top_left, top_right, bottom_left, bottom_right) = (corner(0, 0), corner(0, 1), corner(1, 0), corner(1, 1));
            indices.extend([top_left, bottom_left, bottom_right, top_left, bottom_right, top_right]);
        }
    }
    let mut data = Data::default();
    let position = data.floats(&positions, true);
    let coord = data.floats(&coords, false);
    let index = data.indices_u32(&indices);
    let picture = data.view(&texture(side, jpeg));
    data.finish(json!({
        "scenes": [{ "nodes": [0] }],
        "nodes": [{ "mesh": 0 }],
        "meshes": [{ "primitives": [{ "attributes": { "POSITION": position, "TEXCOORD_0": coord }, "indices": index, "material": 0 }] }],
        "materials": [{ "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 }, "metallicFactor": 0.0 } }],
        "textures": [{ "source": 0 }],
        "images": [{ "bufferView": picture, "mimeType": if jpeg { "image/jpeg" } else { "image/png" } }],
    }))
}

/// A picture `side` pixels a side in four quarters of `GRID_COLOURS`.
fn texture(side: u32, jpeg: bool) -> Vec<u8> {
    let side = side.max(2);
    let picture = image::RgbaImage::from_fn(side, side, |x, y| {
        let quarter = usize::from(x >= side / 2) + 2 * usize::from(y >= side / 2);
        image::Rgba(GRID_COLOURS[quarter])
    });
    let mut bytes = Cursor::new(Vec::new());
    if jpeg {
        let picture = image::DynamicImage::ImageRgba8(picture).into_rgb8();
        picture.write_to(&mut bytes, image::ImageFormat::Jpeg).unwrap();
    } else {
        picture.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    }
    bytes.into_inner()
}

fn boxes() -> Vec<u8> {
    let mut data = Data::default();
    let mut primitives = Vec::new();
    for axis in 0..3 {
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        for sign in [1.0f32, -1.0] {
            let mut normal = [0.0; 3];
            normal[axis] = sign;
            let (mut u, mut v) = ([0.0f32; 3], [0.0f32; 3]);
            u[(axis + 1) % 3] = 1.0;
            v[(axis + 2) % 3] = 1.0;
            if sign < 0.0 {
                std::mem::swap(&mut u, &mut v);
            }
            for (a, b) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                positions.push([0, 1, 2].map(|i| (normal[i] + u[i] * a + v[i] * b) * 0.5));
                normals.push(normal);
            }
        }
        let position = data.floats(&positions, true);
        let mut attributes = json!({ "POSITION": position });
        if axis < 2 {
            attributes["NORMAL"] = json!(data.floats(&normals, false));
        }
        let index = data.indices_u16(&[0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7]);
        primitives.push(json!({ "attributes": attributes, "indices": index, "material": axis }));
    }
    let materials: Vec<Value> = BOX_COLOURS.iter().map(|c| json!({ "pbrMetallicRoughness": { "baseColorFactor": c } })).collect();
    data.finish(json!({
        "scenes": [{ "nodes": [0] }],
        "nodes": [{ "mesh": 0, "translation": [0.0, 0.5, 0.0] }],
        "meshes": [{ "primitives": primitives }],
        "materials": materials,
    }))
}

/// A binary glTF of the description `json` and the block of data `bin`, each padded as the
/// format asks.
pub fn glb_from(json: &str, bin: &[u8]) -> Vec<u8> {
    let mut text = json.as_bytes().to_vec();
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    let mut bin = bin.to_vec();
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let total = 12 + 8 + text.len() + if bin.is_empty() { 0 } else { 8 + bin.len() };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&text);
    if !bin.is_empty() {
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
    }
    out
}

/// A well made binary glTF's description and block of data.
pub fn glb_parts(glb: &[u8]) -> Option<(String, Vec<u8>)> {
    let number = |at: usize| Some(u32::from_le_bytes(glb.get(at..at + 4)?.try_into().ok()?) as usize);
    let described = number(12)?;
    let text = String::from_utf8(glb.get(20..20 + described)?.to_vec()).ok()?;
    let bin = match number(20 + described) {
        Some(len) => glb.get(28 + described..28 + described + len)?.to_vec(),
        None => Vec::new(),
    };
    Some((text, bin))
}

/// A point cloud of `points` points spread over a sphere of radius 1 around the origin, each
/// coloured by where it is, as binary or as words.
pub fn make_test_ply(points: u32, binary: bool) -> Vec<u8> {
    let format = if binary { "binary_little_endian" } else { "ascii" };
    let mut out = format!(
        "ply\nformat {format} 1.0\ncomment made for tests\nelement vertex {points}\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nend_header\n"
    )
    .into_bytes();
    let golden = std::f32::consts::PI * (3.0 - 5.0f32.sqrt());
    for i in 0..points {
        let y = 1.0 - 2.0 * (i as f32 + 0.5) / points as f32;
        let across = (1.0 - y * y).max(0.0).sqrt();
        let turn = golden * i as f32;
        let position = [across * turn.cos(), y, across * turn.sin()];
        let colour = position.map(|v| ((v + 1.0) * 127.5).round().clamp(0.0, 255.0) as u8);
        if binary {
            out.extend(position.iter().flat_map(|v| v.to_le_bytes()));
            out.extend_from_slice(&colour);
        } else {
            let [x, y, z] = position;
            let [r, g, b] = colour;
            out.extend_from_slice(format!("{x} {y} {z} {r} {g} {b}\n").as_bytes());
        }
    }
    out
}
