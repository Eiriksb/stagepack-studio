//! Reading a point cloud: a `.ply` file's header in words, then its points, written as words or
//! as numbers in binary. Only the points are read: each one's position, and its colour if the
//! file gives one.

use std::io::{BufRead, BufReader, SeekFrom};

use crate::limits::{MOST_PLY_HEADER, MOST_POINTS};
use crate::{CantDraw, Model, ModelFacts, ModelKind, Points, ReadSeek, bounds_of, read_at};

/// The longest number written in words that's read.
const MOST_WORD: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Words,
    Little,
    Big,
}

/// The kinds of number a property may be.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Number {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

impl Number {
    fn named(name: &str) -> Option<Number> {
        Some(match name {
            "char" | "int8" => Number::I8,
            "uchar" | "uint8" => Number::U8,
            "short" | "int16" => Number::I16,
            "ushort" | "uint16" => Number::U16,
            "int" | "int32" => Number::I32,
            "uint" | "uint32" => Number::U32,
            "float" | "float32" => Number::F32,
            "double" | "float64" => Number::F64,
            _ => return None,
        })
    }

    fn size(self) -> usize {
        match self {
            Number::I8 | Number::U8 => 1,
            Number::I16 | Number::U16 => 2,
            Number::I32 | Number::U32 | Number::F32 => 4,
            Number::F64 => 8,
        }
    }

    fn read(self, b: &[u8], format: Format) -> f64 {
        macro_rules! number {
            ($t:ty, $n:literal) => {{
                let bytes: [u8; $n] = std::array::from_fn(|i| b[i]);
                f64::from(if format == Format::Big { <$t>::from_be_bytes(bytes) } else { <$t>::from_le_bytes(bytes) })
            }};
        }
        match self {
            Number::I8 => f64::from(b[0] as i8),
            Number::U8 => f64::from(b[0]),
            Number::I16 => number!(i16, 2),
            Number::U16 => number!(u16, 2),
            Number::I32 => number!(i32, 4),
            Number::U32 => number!(u32, 4),
            Number::F32 => number!(f32, 4),
            Number::F64 => number!(f64, 8),
        }
    }

    /// A colour of this kind of number as a byte.
    fn colour(self, value: f64) -> u8 {
        if !value.is_finite() {
            return 0;
        }
        match self {
            Number::F32 | Number::F64 => (value.clamp(0.0, 1.0) * 255.0).round() as u8,
            Number::U16 => (value / 257.0).round().clamp(0.0, 255.0) as u8,
            _ => value.clamp(0.0, 255.0) as u8,
        }
    }
}

/// What a header says of the points.
struct Header {
    format: Format,
    count: u64,
    properties: Vec<Number>,
    position: [usize; 3],
    colour: Option<[usize; 3]>,
    alpha: Option<usize>,
    /// Where the points start.
    data: u64,
}

impl Header {
    fn stride(&self) -> u64 {
        self.properties.iter().map(|p| p.size() as u64).sum()
    }
}

/// Reads the header, of 64 KB at most, and checks what it says against the limits and against
/// the file's length.
fn header(reader: &mut dyn ReadSeek, len: u64) -> Result<Header, CantDraw> {
    let start = read_at(reader, 0, len.min(MOST_PLY_HEADER))?;
    let mut format = None;
    let mut count = None;
    let mut names: Vec<String> = Vec::new();
    let mut properties = Vec::new();
    let mut in_vertex = false;
    let mut at = 0;
    let mut data = None;
    let mut first = true;
    while at < start.len() {
        let Some(end) = start[at..].iter().position(|b| *b == b'\n') else { break };
        let line = std::str::from_utf8(&start[at..at + end]).map_err(|_| CantDraw::Damaged)?;
        at += end + 1;
        let words: Vec<&str> = line.split_ascii_whitespace().collect();
        if first {
            if words != ["ply"] {
                return Err(CantDraw::NotAModel);
            }
            first = false;
            continue;
        }
        match words.as_slice() {
            [] | ["comment", ..] | ["obj_info", ..] => {}
            ["format", kind, "1.0"] => {
                format = Some(match *kind {
                    "ascii" => Format::Words,
                    "binary_little_endian" => Format::Little,
                    "binary_big_endian" => Format::Big,
                    _ => return Err(CantDraw::Damaged),
                })
            }
            ["element", name, n] => {
                let n: u64 = n.parse().map_err(|_| CantDraw::Damaged)?;
                if count.is_none() {
                    if *name != "vertex" {
                        // Another kind of element before the points, which would have to be
                        // read through to reach them.
                        return Err(CantDraw::KindNotDrawn);
                    }
                    count = Some(n);
                    in_vertex = true;
                } else {
                    in_vertex = false;
                }
            }
            ["property", "list", ..] if in_vertex => return Err(CantDraw::KindNotDrawn),
            ["property", "list", _, _, _] => {}
            ["property", kind, name] => {
                let number = Number::named(kind).ok_or(CantDraw::Damaged)?;
                if in_vertex {
                    properties.push(number);
                    names.push((*name).to_owned());
                }
            }
            ["end_header"] => {
                data = Some(at as u64);
                break;
            }
            _ => return Err(CantDraw::Damaged),
        }
    }
    let (Some(format), Some(count), Some(data)) = (format, count, data) else { return Err(CantDraw::Damaged) };
    let find = |wanted: &[&str]| names.iter().position(|name| wanted.contains(&name.as_str()));
    let position = [find(&["x"]), find(&["y"]), find(&["z"])];
    let [Some(x), Some(y), Some(z)] = position else { return Err(CantDraw::Damaged) };
    let colour = match [find(&["red", "diffuse_red"]), find(&["green", "diffuse_green"]), find(&["blue", "diffuse_blue"])] {
        [Some(r), Some(g), Some(b)] => Some([r, g, b]),
        _ => None,
    };
    let alpha = find(&["alpha", "diffuse_alpha"]).filter(|_| colour.is_some());
    let header = Header { format, count, properties, position: [x, y, z], colour, alpha, data };
    if count == 0 {
        return Err(CantDraw::KindNotDrawn);
    }
    // Each point takes its stride in binary, and at least a figure and a space a number in words.
    let rest = len - data;
    // The last number in words needs no space after it.
    let (least, last) = match format {
        Format::Words => (header.properties.len() as u64 * 2, 1),
        _ => (header.stride(), 0),
    };
    if count.checked_mul(least).is_none_or(|needed| needed > rest + last) {
        return Err(CantDraw::Damaged);
    }
    if count > MOST_POINTS {
        return Err(CantDraw::TooLarge);
    }
    Ok(header)
}

pub(crate) fn facts(reader: &mut dyn ReadSeek, len: u64) -> Result<ModelFacts, CantDraw> {
    let header = header(reader, len)?;
    Ok(ModelFacts { kind: ModelKind::Points, triangles: 0, points: header.count, bounds: ([0.0; 3], [0.0; 3]) })
}

pub(crate) fn read(reader: &mut dyn ReadSeek, len: u64) -> Result<Model, CantDraw> {
    let header = header(reader, len)?;
    reader.seek(SeekFrom::Start(header.data)).map_err(|_| CantDraw::CouldNotRead)?;
    let mut input = BufReader::with_capacity(64 * 1024, reader);
    let count = header.count as usize;
    let mut positions = Vec::with_capacity(count);
    let mut colours = Vec::with_capacity(count);
    let mut values = vec![0.0f64; header.properties.len()];
    let mut record = vec![0u8; header.stride() as usize];
    let mut word = Vec::with_capacity(MOST_WORD);
    for _ in 0..count {
        match header.format {
            Format::Words => {
                for value in values.iter_mut() {
                    *value = next_word(&mut input, &mut word)?;
                }
            }
            format => {
                std::io::Read::read_exact(&mut input, &mut record).map_err(|_| CantDraw::Damaged)?;
                let mut at = 0;
                for (value, number) in values.iter_mut().zip(&header.properties) {
                    *value = number.read(&record[at..], format);
                    at += number.size();
                }
            }
        }
        let position = header.position.map(|i| values[i] as f32);
        if position.iter().any(|v| !v.is_finite()) {
            return Err(CantDraw::Damaged);
        }
        positions.push(position);
        colours.push(match header.colour {
            Some(rgb) => {
                let [r, g, b] = rgb.map(|i| header.properties[i].colour(values[i]));
                let a = header.alpha.map_or(255, |i| header.properties[i].colour(values[i]));
                [r, g, b, a]
            }
            None => [255; 4],
        });
    }
    let bounds = bounds_of(&positions);
    let facts = ModelFacts { kind: ModelKind::Points, triangles: 0, points: header.count, bounds };
    Ok(Model { meshes: Vec::new(), points: Some(Points { positions, colours }), texture: None, facts })
}

/// The next number written in words.
fn next_word(input: &mut impl BufRead, word: &mut Vec<u8>) -> Result<f64, CantDraw> {
    word.clear();
    loop {
        let buffer = input.fill_buf().map_err(|_| CantDraw::CouldNotRead)?;
        if buffer.is_empty() {
            break;
        }
        let mut used = 0;
        let mut ended = false;
        for byte in buffer {
            used += 1;
            if byte.is_ascii_whitespace() {
                if !word.is_empty() {
                    ended = true;
                    break;
                }
            } else {
                if word.len() == MOST_WORD {
                    return Err(CantDraw::Damaged);
                }
                word.push(*byte);
            }
        }
        input.consume(used);
        if ended {
            break;
        }
    }
    let text = std::str::from_utf8(word).map_err(|_| CantDraw::Damaged)?;
    text.parse().map_err(|_| CantDraw::Damaged)
}
