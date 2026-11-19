//! Reading a binary glTF: a description in JSON and one block of data, in one file.
//!
//! The description is read into the glTF library's types, and nothing more of the library is
//! used: every index, size and place in it is checked here, against what holds it and against
//! the file's real length, before anything it names is read. Anything it names outside the file
//! is refused.

use std::collections::HashMap;
use std::io::Cursor;

use gltf::json::accessor::{ComponentType, Type};
use gltf::json::mesh::{Mode, Primitive, Semantic};
use gltf::json::validation::Checked;
use gltf::json::{self, Root};

use crate::image_size::{Picture, Size, checked_size, picture_kind};
use crate::limits::*;
use crate::maths::{self, Matrix};
use crate::{CantDraw, Mesh, Model, ModelFacts, ModelKind, ReadSeek, Texture, bounds_of, read_at};

/// Required extensions that change nothing about how a model is read or drawn here.
const KNOWN_REQUIRED: [&str; 1] = [
    // Colours not lit by the scene's lights; here they're lit simply, as every model is.
    "KHR_materials_unlit",
];

const JSON: u32 = u32::from_le_bytes(*b"JSON");
const BIN: u32 = u32::from_le_bytes(*b"BIN\0");

/// The file's description, and where its block of data lies, if it has one.
struct Glb {
    root: Root,
    bin: Option<(u64, u64)>,
}

/// One part of a mesh, placed.
struct Part {
    mesh: usize,
    primitive: usize,
    place: Matrix,
    textured: bool,
}

/// Where the one texture lies, and its size as its header says.
struct TextureAt {
    image: usize,
    at: u64,
    len: u64,
    kind: Picture,
    size: Size,
}

impl TextureAt {
    /// The most memory decoding it may take at once: its bytes as stored, the picture as RGBA,
    /// and for a JPEG, the decoder's own working, taken as two bytes for each number stored.
    fn decoding(&self) -> u64 {
        let pixels = u64::from(self.size.width) * u64::from(self.size.height);
        let working = match self.kind {
            Picture::Png => 0,
            Picture::Jpeg => pixels * u64::from(self.size.channels) * 2,
        };
        self.len + pixels * 4 + working
    }
}

/// What a description says is to be drawn, all of it checked.
struct Plan {
    glb: Glb,
    parts: Vec<Part>,
    texture: Option<TextureAt>,
    facts: ModelFacts,
}

/// Where an accessor's values lie in the file, checked to lie inside its view.
#[derive(Clone, Copy)]
struct Span {
    at: u64,
    count: u64,
    stride: u64,
    size: u64,
    component: ComponentType,
    kind: Type,
    normalized: bool,
}

impl Span {
    fn bytes(&self) -> u64 {
        if self.count == 0 { 0 } else { self.stride * (self.count - 1) + self.size }
    }
}

pub(crate) fn facts(reader: &mut dyn ReadSeek, len: u64) -> Result<ModelFacts, CantDraw> {
    Ok(plan(reader, len)?.facts)
}

fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// Reads the file's header and description, and finds where its data lies.
fn open(reader: &mut dyn ReadSeek, len: u64) -> Result<Glb, CantDraw> {
    if len < 20 {
        return Err(CantDraw::Damaged);
    }
    let head = read_at(reader, 0, 20)?;
    if u32_at(&head, 4) != 2 {
        // The first version of glTF, or something else.
        return Err(CantDraw::KindNotDrawn);
    }
    let end = u64::from(u32_at(&head, 8));
    if end < 20 || end > len {
        return Err(CantDraw::Damaged);
    }
    let described = u64::from(u32_at(&head, 12));
    if u32_at(&head, 16) != JSON {
        return Err(CantDraw::Damaged);
    }
    if described > end - 20 {
        return Err(CantDraw::Damaged);
    }
    if described > MOST_DESCRIPTION {
        return Err(CantDraw::TooLarge);
    }
    let text = read_at(reader, 20, described)?;
    let next = 20 + described;
    let mut bin = None;
    if end - next >= 8 {
        let chunk = read_at(reader, next, 8)?;
        if u32_at(&chunk, 4) == BIN {
            let bin_len = u64::from(u32_at(&chunk, 0));
            if bin_len > end - next - 8 {
                return Err(CantDraw::Damaged);
            }
            bin = Some((next + 8, bin_len));
        }
    }
    count_values(&text)?;
    let root: Root = json::deserialize::from_slice(&text).map_err(|_| CantDraw::Damaged)?;
    Ok(Glb { root, bin })
}

/// Counts the objects and values in a description before it's read, so one of a great many small
/// things can't take far more memory read than it does written.
fn count_values(text: &[u8]) -> Result<(), CantDraw> {
    let mut objects = 0;
    let mut values = 0;
    let mut in_string = false;
    let mut escaped = false;
    for byte in text {
        if in_string {
            match byte {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' => {
                objects += 1;
                values += 1;
            }
            b'[' | b',' => values += 1,
            _ => {}
        }
    }
    if objects > MOST_OBJECTS || values > MOST_VALUES { Err(CantDraw::TooLarge) } else { Ok(()) }
}

fn component_size(component: ComponentType) -> u64 {
    match component {
        ComponentType::I8 | ComponentType::U8 => 1,
        ComponentType::I16 | ComponentType::U16 => 2,
        ComponentType::U32 | ComponentType::F32 => 4,
    }
}

fn components(kind: Type) -> u64 {
    match kind {
        Type::Scalar => 1,
        Type::Vec2 => 2,
        Type::Vec3 => 3,
        Type::Vec4 | Type::Mat2 => 4,
        Type::Mat3 => 9,
        Type::Mat4 => 16,
    }
}

/// Where accessor `index`'s values lie, or `None` if it has none of its own in the file.
fn span(glb: &Glb, index: usize) -> Result<Option<Span>, CantDraw> {
    let root = &glb.root;
    let accessor = root.accessors.get(index).ok_or(CantDraw::Damaged)?;
    let Checked::Valid(component) = accessor.component_type else { return Err(CantDraw::Damaged) };
    let Checked::Valid(kind) = accessor.type_ else { return Err(CantDraw::Damaged) };
    let component = component.0;
    let Some(view) = accessor.buffer_view else { return Ok(None) };
    let view = root.buffer_views.get(view.value()).ok_or(CantDraw::Damaged)?;
    let size = component_size(component) * components(kind);
    let stride = view.byte_stride.map_or(size, |stride| stride.0 as u64);
    if stride < size {
        return Err(CantDraw::Damaged);
    }
    let count = accessor.count.0;
    let offset = accessor.byte_offset.map_or(0, |offset| offset.0);
    let span = Span { at: 0, count, stride, size, component, kind, normalized: accessor.normalized };
    let needed = (count.checked_sub(1))
        .map_or(Some(0), |last| stride.checked_mul(last).and_then(|bytes| bytes.checked_add(size)))
        .and_then(|bytes| bytes.checked_add(offset))
        .ok_or(CantDraw::Damaged)?;
    let (start, _) = view_at(glb, view)?;
    if needed > view.byte_length.0 {
        return Err(CantDraw::Damaged);
    }
    Ok(Some(Span { at: start + offset, ..span }))
}

/// Where a view lies in the file, checked to lie inside its buffer, which is checked to lie
/// inside the file's block of data.
fn view_at(glb: &Glb, view: &json::buffer::View) -> Result<(u64, u64), CantDraw> {
    let (bin_at, _) = buffer_at(glb, view.buffer.value())?;
    let buffer = &glb.root.buffers[view.buffer.value()];
    let offset = view.byte_offset.map_or(0, |offset| offset.0);
    let end = offset.checked_add(view.byte_length.0).ok_or(CantDraw::Damaged)?;
    if end > buffer.byte_length.0 {
        return Err(CantDraw::Damaged);
    }
    Ok((bin_at + offset, view.byte_length.0))
}

fn buffer_at(glb: &Glb, index: usize) -> Result<(u64, u64), CantDraw> {
    let buffer = glb.root.buffers.get(index).ok_or(CantDraw::Damaged)?;
    // Only the first buffer can be the file's own block of data; any other would be outside it.
    let (at, len) = glb.bin.filter(|_| index == 0).ok_or(CantDraw::Damaged)?;
    if buffer.uri.is_some() {
        return Err(CantDraw::NeedsOtherFiles);
    }
    if buffer.byte_length.0 > len {
        return Err(CantDraw::Damaged);
    }
    Ok((at, len))
}

/// Checks the whole description, works out every part to be drawn and where, and finds the one
/// texture and its size.
fn plan(reader: &mut dyn ReadSeek, len: u64) -> Result<Plan, CantDraw> {
    let glb = open(reader, len)?;
    let root = &glb.root;
    // Anything outside the file is refused before anything else is looked at.
    if root.buffers.iter().any(|buffer| buffer.uri.is_some()) || root.images.iter().any(|image| image.uri.is_some()) {
        return Err(CantDraw::NeedsOtherFiles);
    }
    if root.extensions_required.iter().any(|name| !KNOWN_REQUIRED.contains(&name.as_str())) {
        return Err(CantDraw::KindNotDrawn);
    }
    for index in 0..root.buffers.len() {
        buffer_at(&glb, index)?;
    }
    for view in &root.buffer_views {
        view_at(&glb, view)?;
    }
    for index in 0..root.accessors.len() {
        span(&glb, index)?;
    }

    let mut parts = placed(root)?;
    let mut triangles = 0u64;
    let mut vertices = 0u64;
    let mut corners = Vec::new();
    // The one texture, once the first part coloured from one has been looked at.
    let mut texture: Option<Option<TextureAt>> = None;
    let mut drawn = Vec::with_capacity(parts.len());
    for mut part in parts.drain(..) {
        let primitive = &root.meshes[part.mesh].primitives[part.primitive];
        let Some(counted) = check_primitive(&glb, primitive)? else { continue };
        vertices = vertices.checked_add(counted.vertices).ok_or(CantDraw::TooLarge)?;
        triangles = triangles.checked_add(counted.triangles).ok_or(CantDraw::TooLarge)?;
        if vertices > MOST_VERTICES || triangles > MOST_TRIANGLES {
            return Err(CantDraw::TooLarge);
        }
        let (low, high) = counted.bounds;
        for corner in 0..8 {
            let pick = |axis: usize| if corner & (1 << axis) == 0 { low[axis] } else { high[axis] };
            corners.push(maths::place(&part.place, [pick(0), pick(1), pick(2)]));
        }
        if let Some((image, tex_coord)) = base_colour(root, primitive)? {
            if texture.is_none() {
                texture = Some(texture_at(reader, &glb, image)?);
            }
            let set = Checked::Valid(Semantic::TexCoords(tex_coord));
            part.textured = texture.as_ref().is_some_and(|t| t.as_ref().is_some_and(|t| t.image == image))
                && primitive.attributes.contains_key(&set);
        }
        drawn.push(part);
    }
    if triangles == 0 {
        return Err(CantDraw::KindNotDrawn);
    }
    if corners.iter().flatten().any(|v| !v.is_finite()) {
        return Err(CantDraw::Damaged);
    }
    let facts = ModelFacts { kind: ModelKind::Mesh, triangles, points: 0, bounds: bounds_of(&corners) };
    Ok(Plan { glb, parts: drawn, texture: texture.flatten(), facts })
}

/// Every part of every mesh the model's scene places, and where.
fn placed(root: &Root) -> Result<Vec<Part>, CantDraw> {
    let starts: Vec<usize> = match root.scene {
        Some(scene) => root.scenes.get(scene.value()).ok_or(CantDraw::Damaged)?.nodes.iter().map(|n| n.value()).collect(),
        None if !root.scenes.is_empty() => root.scenes[0].nodes.iter().map(|n| n.value()).collect(),
        None => {
            // No scene: every node nothing else holds.
            let mut held = vec![false; root.nodes.len()];
            for node in &root.nodes {
                for child in node.children.iter().flatten() {
                    if let Some(held) = held.get_mut(child.value()) {
                        *held = true;
                    }
                }
            }
            (0..root.nodes.len()).filter(|i| !held[*i]).collect()
        }
    };
    let mut parts = Vec::new();
    // Counted as nodes are put on the stack, not as they're taken off, so a node holding many
    // others can't fill memory before the count is checked.
    let mut pushed = starts.len() as u64;
    if pushed > MOST_PLACES {
        return Err(CantDraw::TooLarge);
    }
    // The nodes from the scene down to the one being looked at, to find a node holding itself.
    let mut path: Vec<usize> = Vec::with_capacity(MOST_DEPTH + 1);
    let mut stack: Vec<(usize, Matrix, usize)> = starts.into_iter().rev().map(|node| (node, maths::IDENTITY, 0)).collect();
    while let Some((index, above, depth)) = stack.pop() {
        path.truncate(depth);
        path.push(index);
        let node = root.nodes.get(index).ok_or(CantDraw::Damaged)?;
        let own = match node.matrix {
            Some(matrix) => matrix,
            None => maths::from_parts(
                node.translation.unwrap_or([0.0; 3]),
                node.rotation.map_or([0.0, 0.0, 0.0, 1.0], |r| r.0),
                node.scale.unwrap_or([1.0; 3]),
            ),
        };
        if own.iter().any(|v| !v.is_finite()) {
            return Err(CantDraw::Damaged);
        }
        let place = maths::multiply(&above, &own);
        if let Some(mesh) = node.mesh {
            let primitives = root.meshes.get(mesh.value()).ok_or(CantDraw::Damaged)?.primitives.len();
            if parts.len() as u64 + primitives as u64 > MOST_PLACES {
                return Err(CantDraw::TooLarge);
            }
            parts.extend((0..primitives).map(|primitive| Part { mesh: mesh.value(), primitive, place, textured: false }));
        }
        let children = node.children.as_deref().unwrap_or_default();
        if children.is_empty() {
            continue;
        }
        if children.iter().any(|child| path.contains(&child.value())) {
            // A node holding itself, or one that holds it.
            return Err(CantDraw::Damaged);
        }
        if depth + 1 > MOST_DEPTH {
            return Err(CantDraw::Damaged);
        }
        pushed += children.len() as u64;
        if pushed > MOST_PLACES {
            return Err(CantDraw::TooLarge);
        }
        for child in children.iter().rev() {
            stack.push((child.value(), place, depth + 1));
        }
    }
    Ok(parts)
}

/// What a part counts, and the box its own description says its positions lie in.
struct Counted {
    vertices: u64,
    triangles: u64,
    bounds: ([f32; 3], [f32; 3]),
}

/// A part's positions and indices, checked; `None` if it's of a kind not drawn, as lines are.
fn check_primitive(glb: &Glb, primitive: &Primitive) -> Result<Option<Counted>, CantDraw> {
    let mode = match primitive.mode {
        Checked::Valid(mode) => mode,
        Checked::Invalid => return Err(CantDraw::Damaged),
    };
    if !matches!(mode, Mode::Triangles | Mode::TriangleStrip | Mode::TriangleFan) {
        return Ok(None);
    }
    let Some(position) = primitive.attributes.get(&Checked::Valid(Semantic::Positions)) else { return Ok(None) };
    let positions = drawn_span(glb, position.value())?;
    if positions.component != ComponentType::F32 || positions.kind != Type::Vec3 {
        // Positions stored small, as some compressing tools do.
        return Err(CantDraw::KindNotDrawn);
    }
    let vertices = positions.count;
    for (semantic, accessor) in &primitive.attributes {
        let wanted: &[Type] = match semantic {
            Checked::Valid(Semantic::Normals) => &[Type::Vec3],
            Checked::Valid(Semantic::TexCoords(_)) => &[Type::Vec2],
            Checked::Valid(Semantic::Colors(0)) => &[Type::Vec3, Type::Vec4],
            _ => continue,
        };
        let span = drawn_span(glb, accessor.value())?;
        if span.count != vertices || !wanted.contains(&span.kind) {
            return Err(CantDraw::Damaged);
        }
        if matches!(semantic, Checked::Valid(Semantic::Normals)) && span.component != ComponentType::F32 {
            return Err(CantDraw::KindNotDrawn);
        }
    }
    let indexed = match primitive.indices {
        Some(indices) => {
            let span = drawn_span(glb, indices.value())?;
            if span.kind != Type::Scalar || !matches!(span.component, ComponentType::U8 | ComponentType::U16 | ComponentType::U32) {
                return Err(CantDraw::Damaged);
            }
            span.count
        }
        None => vertices,
    };
    let triangles = match mode {
        Mode::Triangles => indexed / 3,
        _ => indexed.saturating_sub(2),
    };
    let accessor = &glb.root.accessors[position.value()];
    let corner = |value: &Option<json::Value>| -> Result<[f32; 3], CantDraw> {
        let numbers = value.as_ref().and_then(|v| v.as_array()).filter(|v| v.len() == 3).ok_or(CantDraw::Damaged)?;
        let mut corner = [0.0; 3];
        for (to, from) in corner.iter_mut().zip(numbers) {
            *to = from.as_f64().ok_or(CantDraw::Damaged)? as f32;
        }
        Ok(corner)
    };
    let bounds = (corner(&accessor.min)?, corner(&accessor.max)?);
    Ok(Some(Counted { vertices, triangles, bounds }))
}

/// An accessor's span when it's to be drawn: it must have its own values, not sparse ones.
fn drawn_span(glb: &Glb, index: usize) -> Result<Span, CantDraw> {
    let accessor = glb.root.accessors.get(index).ok_or(CantDraw::Damaged)?;
    if accessor.sparse.is_some() {
        return Err(CantDraw::KindNotDrawn);
    }
    span(glb, index)?.ok_or(CantDraw::KindNotDrawn)
}

/// The image a part's material colours it from, and which texture coordinates it uses.
fn base_colour(root: &Root, primitive: &Primitive) -> Result<Option<(usize, u32)>, CantDraw> {
    let Some(material) = primitive.material else { return Ok(None) };
    let material = root.materials.get(material.value()).ok_or(CantDraw::Damaged)?;
    let Some(info) = &material.pbr_metallic_roughness.base_color_texture else { return Ok(None) };
    let texture = root.textures.get(info.index.value()).ok_or(CantDraw::Damaged)?;
    let image = texture.source.value();
    root.images.get(image).ok_or(CantDraw::Damaged)?;
    Ok(Some((image, info.tex_coord)))
}

/// Where image `index` lies and its size, checked; `None` if it's a kind of picture not drawn.
fn texture_at(reader: &mut dyn ReadSeek, glb: &Glb, index: usize) -> Result<Option<TextureAt>, CantDraw> {
    let image = &glb.root.images[index];
    let view = image.buffer_view.ok_or(CantDraw::Damaged)?;
    let view = glb.root.buffer_views.get(view.value()).ok_or(CantDraw::Damaged)?;
    let (at, len) = view_at(glb, view)?;
    let start = read_at(reader, at, len.min(8))?;
    let Some(kind) = picture_kind(&start) else { return Ok(None) };
    let mut peek = |from: u64, n: u64| {
        if from.checked_add(n).is_none_or(|end| end > len) {
            return Err(CantDraw::Damaged);
        }
        read_at(reader, at + from, n)
    };
    let size = checked_size(kind, len, &mut peek)?;
    let texture = TextureAt { image: index, at, len, kind, size };
    if texture.decoding() > MOST_DECODING {
        return Err(CantDraw::TooLarge);
    }
    Ok(Some(texture))
}

pub(crate) fn read(reader: &mut dyn ReadSeek, len: u64) -> Result<Model, CantDraw> {
    let plan = plan(reader, len)?;
    let glb = &plan.glb;
    let mut meshes: Vec<Mesh> = Vec::with_capacity(plan.parts.len());
    // Each part is read once, however many times it's placed.
    let mut order: Vec<(usize, usize)> = Vec::new();
    let mut places: HashMap<(usize, usize), Vec<&Part>> = HashMap::new();
    for part in &plan.parts {
        let key = (part.mesh, part.primitive);
        places
            .entry(key)
            .or_insert_with(|| {
                order.push(key);
                Vec::new()
            })
            .push(part);
    }
    for key in order {
        let primitive = &glb.root.meshes[key.0].primitives[key.1];
        let own = read_primitive(reader, glb, primitive)?;
        for part in &places[&key] {
            meshes.push(place(&own, part)?);
        }
    }
    let texture = match &plan.texture {
        Some(at) => Some(decode(reader, at)?),
        None => None,
    };
    let triangles = meshes.iter().map(|m| m.indices.len() as u64 / 3).sum();
    let bounds = bounds_of(meshes.iter().flat_map(|m| &m.positions));
    Ok(Model { meshes, points: None, texture, facts: ModelFacts { triangles, bounds, ..plan.facts } })
}

/// A part as its description gives it, before it's placed.
struct Own {
    positions: Vec<[f32; 3]>,
    normals: Option<Vec<[f32; 3]>>,
    texture_coords: Option<Vec<[f32; 2]>>,
    colours: Vec<[f32; 4]>,
    indices: Vec<u32>,
    double_sided: bool,
}

fn read_primitive(reader: &mut dyn ReadSeek, glb: &Glb, primitive: &Primitive) -> Result<Own, CantDraw> {
    let attribute = |semantic: Semantic| primitive.attributes.get(&Checked::Valid(semantic)).map(|a| a.value());
    let position = attribute(Semantic::Positions).ok_or(CantDraw::Damaged)?;
    let positions: Vec<[f32; 3]> = arrays(&values(reader, drawn_span(glb, position)?)?);
    if positions.iter().flatten().any(|v| !v.is_finite()) {
        return Err(CantDraw::Damaged);
    }
    let vertices = positions.len();
    let normals = match attribute(Semantic::Normals) {
        Some(normals) => {
            let normals: Vec<[f32; 3]> = arrays(&values(reader, drawn_span(glb, normals)?)?);
            // Normals that aren't numbers are made again from the triangles.
            normals.iter().flatten().all(|v| v.is_finite()).then_some(normals)
        }
        None => None,
    };
    let (factor, tex_coord, double_sided) = match primitive.material {
        Some(material) => {
            let material = glb.root.materials.get(material.value()).ok_or(CantDraw::Damaged)?;
            let pbr = &material.pbr_metallic_roughness;
            (pbr.base_color_factor.0, pbr.base_color_texture.as_ref().map(|t| t.tex_coord), material.double_sided)
        }
        None => ([1.0; 4], None, false),
    };
    let texture_coords = match tex_coord.and_then(|set| attribute(Semantic::TexCoords(set))) {
        Some(coords) => Some(arrays::<2>(&values(reader, drawn_span(glb, coords)?)?).into_iter().map(|c| c.map(finite)).collect()),
        None => None,
    };
    let factor = factor.map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 });
    let colours = match attribute(Semantic::Colors(0)) {
        Some(colours) => {
            let span = drawn_span(glb, colours)?;
            let values = values(reader, span)?;
            let own: Vec<[f32; 4]> = if span.kind == Type::Vec3 {
                arrays::<3>(&values).into_iter().map(|[r, g, b]| [r, g, b, 1.0]).collect()
            } else {
                arrays(&values)
            };
            own.into_iter().map(|c| [0, 1, 2, 3].map(|i| finite(c[i]).clamp(0.0, 1.0) * factor[i])).collect()
        }
        None => vec![factor; vertices],
    };
    let mode = match primitive.mode {
        Checked::Valid(mode) => mode,
        Checked::Invalid => return Err(CantDraw::Damaged),
    };
    let listed = match primitive.indices {
        Some(indices) => {
            let indices = read_indices(reader, drawn_span(glb, indices.value())?)?;
            if indices.iter().any(|i| *i as usize >= vertices) {
                return Err(CantDraw::Damaged);
            }
            indices
        }
        None => (0..u32::try_from(vertices).map_err(|_| CantDraw::TooLarge)?).collect(),
    };
    let indices = match mode {
        Mode::Triangles => listed[..listed.len() / 3 * 3].to_vec(),
        Mode::TriangleStrip => (0..listed.len().saturating_sub(2))
            .flat_map(|i| if i % 2 == 0 { [listed[i], listed[i + 1], listed[i + 2]] } else { [listed[i + 1], listed[i], listed[i + 2]] })
            .collect(),
        _ => (0..listed.len().saturating_sub(2)).flat_map(|i| [listed[0], listed[i + 1], listed[i + 2]]).collect(),
    };
    Ok(Own { positions, normals, texture_coords, colours, indices, double_sided })
}

fn finite(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

fn arrays<const N: usize>(values: &[f32]) -> Vec<[f32; N]> {
    values.as_chunks::<N>().0.to_vec()
}

/// An accessor's values as numbers, each component of each value in turn.
fn values(reader: &mut dyn ReadSeek, span: Span) -> Result<Vec<f32>, CantDraw> {
    let bytes = read_at(reader, span.at, span.bytes())?;
    let n = components(span.kind) as usize;
    let size = component_size(span.component) as usize;
    if span.component != ComponentType::F32 && !span.normalized {
        // Whole numbers standing for positions or directions, as some compressing tools store.
        return Err(CantDraw::KindNotDrawn);
    }
    let mut out = Vec::with_capacity(span.count as usize * n);
    for index in 0..span.count as usize {
        let start = index * span.stride as usize;
        for c in 0..n {
            let at = start + c * size;
            let b = &bytes[at..at + size];
            out.push(match span.component {
                ComponentType::F32 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                ComponentType::U8 => f32::from(b[0]) / 255.0,
                ComponentType::I8 => (f32::from(b[0] as i8) / 127.0).max(-1.0),
                ComponentType::U16 => f32::from(u16::from_le_bytes([b[0], b[1]])) / 65535.0,
                ComponentType::I16 => (f32::from(i16::from_le_bytes([b[0], b[1]])) / 32767.0).max(-1.0),
                ComponentType::U32 => return Err(CantDraw::Damaged),
            });
        }
    }
    Ok(out)
}

fn read_indices(reader: &mut dyn ReadSeek, span: Span) -> Result<Vec<u32>, CantDraw> {
    let bytes = read_at(reader, span.at, span.bytes())?;
    let size = component_size(span.component) as usize;
    let mut out = Vec::with_capacity(span.count as usize);
    for index in 0..span.count as usize {
        let at = index * span.stride as usize;
        let b = &bytes[at..at + size];
        out.push(match span.component {
            ComponentType::U8 => u32::from(b[0]),
            ComponentType::U16 => u32::from(u16::from_le_bytes([b[0], b[1]])),
            ComponentType::U32 => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            _ => return Err(CantDraw::Damaged),
        });
    }
    Ok(out)
}

/// A part put where the model places it.
fn place(own: &Own, part: &Part) -> Result<Mesh, CantDraw> {
    let positions: Vec<[f32; 3]> = own.positions.iter().map(|p| maths::place(&part.place, *p)).collect();
    if positions.iter().flatten().any(|v| !v.is_finite()) {
        // Placed so far away it can't be said where.
        return Err(CantDraw::Damaged);
    }
    let (turning, determinant) = maths::normal_matrix(&part.place);
    let mut indices = own.indices.clone();
    if determinant < 0.0 {
        // Placed as in a mirror, so its triangles turn the other way.
        for triangle in indices.as_chunks_mut::<3>().0 {
            triangle.swap(1, 2);
        }
    }
    let normals = match &own.normals {
        Some(normals) => normals.iter().map(|n| maths::unit(maths::turn(&turning, *n)).unwrap_or([0.0, 0.0, 1.0])).collect(),
        None => made_normals(&positions, &indices),
    };
    let textured = part.textured && own.texture_coords.is_some();
    Ok(Mesh {
        positions,
        normals,
        texture_coords: if textured { own.texture_coords.clone().unwrap_or_default() } else { Vec::new() },
        colours: own.colours.clone(),
        indices,
        textured,
        double_sided: own.double_sided,
    })
}

/// Normals made from the triangles each position is a corner of, the larger counting more.
fn made_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut sums = vec![[0.0f32; 3]; positions.len()];
    for triangle in indices.as_chunks::<3>().0 {
        let [a, b, c] = triangle.map(|i| i as usize);
        let face = maths::cross(maths::minus(positions[b], positions[a]), maths::minus(positions[c], positions[a]));
        for corner in [a, b, c] {
            for i in 0..3 {
                sums[corner][i] += face[i];
            }
        }
    }
    sums.into_iter().map(|sum| maths::unit(sum).unwrap_or([0.0, 0.0, 1.0])).collect()
}

/// Decodes the one texture, its size from its header already checked against the limits and
/// the memory decoding it takes. The picture is decoded straight into the RGBA it's kept as,
/// with fewer numbers to a pixel at first, then spread out in place, so no second copy is made.
fn decode(reader: &mut dyn ReadSeek, at: &TextureAt) -> Result<Texture, CantDraw> {
    let bytes = read_at(reader, at.at, at.len)?;
    let (width, height) = (at.size.width, at.size.height);
    let pixels = width as usize * height as usize;
    let (mut rgba, channels) = match at.kind {
        Picture::Png => decode_png(&bytes, at.size)?,
        Picture::Jpeg => decode_jpeg(&bytes, at.size, at.decoding())?,
    };
    spread(&mut rgba[..pixels * 4], channels);
    Ok(Texture { width, height, rgba })
}

/// A PNG as 8 bits a number, at the start of a buffer large enough for it as RGBA, and how many
/// numbers it has to a pixel. Sixteen bits a number are made eight as each row is read, so a
/// picture of 16 bits never takes more than one of 8.
fn decode_png(bytes: &[u8], size: Size) -> Result<(Vec<u8>, usize), CantDraw> {
    let mut decoder = png::Decoder::new_with_limits(Cursor::new(bytes), png::Limits { bytes: 32 * 1024 * 1024 });
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(png_refusal)?;
    let info = reader.info();
    if (info.width, info.height) != (size.width, size.height) {
        return Err(CantDraw::Damaged);
    }
    let (colour, depth) = reader.output_color_type();
    if depth != png::BitDepth::Eight {
        return Err(CantDraw::Damaged);
    }
    let channels = colour.samples();
    let needed = reader.output_buffer_size().ok_or(CantDraw::TooLarge)?;
    let mut rgba = vec![0; size.width as usize * size.height as usize * 4];
    if channels > 4 || needed > rgba.len() {
        return Err(CantDraw::Damaged);
    }
    reader.next_frame(&mut rgba[..needed]).map_err(png_refusal)?;
    Ok((rgba, channels))
}

fn png_refusal(why: png::DecodingError) -> CantDraw {
    match why {
        png::DecodingError::LimitsExceeded => CantDraw::TooLarge,
        _ => CantDraw::Damaged,
    }
}

/// A JPEG, as `decode_png` gives a PNG.
fn decode_jpeg(bytes: &[u8], size: Size, most: u64) -> Result<(Vec<u8>, usize), CantDraw> {
    use image::ImageDecoder;
    let mut decoder = image::codecs::jpeg::JpegDecoder::new(Cursor::new(bytes)).map_err(|_| CantDraw::Damaged)?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MOST_TEXTURE_SIDE);
    limits.max_image_height = Some(MOST_TEXTURE_SIDE);
    limits.max_alloc = Some(most);
    decoder.set_limits(limits).map_err(|_| CantDraw::TooLarge)?;
    if decoder.dimensions() != (size.width, size.height) {
        return Err(CantDraw::Damaged);
    }
    let channels = match decoder.color_type() {
        image::ColorType::L8 => 1,
        image::ColorType::La8 => 2,
        image::ColorType::Rgb8 => 3,
        image::ColorType::Rgba8 => 4,
        _ => return Err(CantDraw::KindNotDrawn),
    };
    let needed = usize::try_from(decoder.total_bytes()).map_err(|_| CantDraw::TooLarge)?;
    let mut rgba = vec![0; size.width as usize * size.height as usize * 4];
    if needed != rgba.len() / 4 * channels {
        return Err(CantDraw::Damaged);
    }
    decoder.read_image(&mut rgba[..needed]).map_err(|why| match why {
        image::ImageError::Limits(_) => CantDraw::TooLarge,
        _ => CantDraw::Damaged,
    })?;
    Ok((rgba, channels))
}

/// Spreads pixels of `channels` numbers each, packed at the start of `rgba`, out to four each,
/// working from the end so nothing is written over before it's read.
fn spread(rgba: &mut [u8], channels: usize) {
    let pixels = rgba.len() / 4;
    for pixel in (0..pixels).rev() {
        let from = pixel * channels;
        let to = pixel * 4;
        let colour = match channels {
            1 => [rgba[from], rgba[from], rgba[from], 255],
            2 => [rgba[from], rgba[from], rgba[from], rgba[from + 1]],
            3 => [rgba[from], rgba[from + 1], rgba[from + 2], 255],
            _ => [rgba[from], rgba[from + 1], rgba[from + 2], rgba[from + 3]],
        };
        rgba[to..to + 4].copy_from_slice(&colour);
    }
}
