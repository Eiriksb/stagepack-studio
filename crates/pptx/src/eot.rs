//! Fonts embedded in a `.pptx` (`/ppt/fonts/*.fntdata`) are Embedded OpenType: an EOT header (the
//! font's names, PANOSE, weight, Unicode and code page ranges, embedding rights) in front of the
//! TrueType file, uncompressed (version 0x00020001, no MTX compression, no XOR). This reads what
//! the header needs from the font's own tables, as `ooxml/eot.mjs` of `slidev-pptx-export` does.
//!
//! A font file comes from a talk and may be anything, so every read is checked: a short or
//! corrupt font is an `Err`, never a panic.

use std::collections::HashMap;

const EOT_VERSION: u32 = 0x0002_0001;
const MAGIC: u16 = 0x504c;
/// The fixed part of the header, up to and including Reserved4.
const FIXED_LEN: usize = 80;

/// What a font says about itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontInfo {
    pub family: String,
    pub style: String,
    /// `usWeightClass`; 600 and up is bold.
    pub weight: u16,
    pub fs_type: u16,
    pub italic: bool,
}

/// Bounds-checked big-endian reads of a font file.
struct Bytes<'a>(&'a [u8]);

impl<'a> Bytes<'a> {
    fn slice(&self, at: u64, len: u64) -> Result<&'a [u8], String> {
        let end = at.checked_add(len).ok_or("the font is damaged")?;
        match (usize::try_from(at), usize::try_from(end)) {
            (Ok(at), Ok(end)) if end <= self.0.len() => Ok(&self.0[at..end]),
            _ => Err("the font is cut short or damaged".to_string()),
        }
    }

    fn u16(&self, at: u64) -> Result<u16, String> {
        self.slice(at, 2).map(|b| u16::from_be_bytes([b[0], b[1]]))
    }

    fn u32(&self, at: u64) -> Result<u32, String> {
        self.slice(at, 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
}

struct Tables {
    name: u64,
    os2: u64,
    head: u64,
}

/// The table directory of the sfnt: offsets of the tables the header is made from.
fn sfnt_tables(font: &Bytes) -> Result<Tables, String> {
    let tag = font.slice(0, 4).map_err(|_| "not a TrueType font".to_string())?;
    if tag != [0, 1, 0, 0] && tag != b"true" {
        return Err(if tag == b"OTTO" {
            "CFF (PostScript) outlines: PowerPoint embeds TrueType fonts only".into()
        } else {
            "not a TrueType font".into()
        });
    }
    let count = u64::from(font.u16(4)?);
    let mut tables: HashMap<[u8; 4], u64> = HashMap::new();
    for i in 0..count {
        let entry = 12 + 16 * i;
        let tag = font.slice(entry, 4)?;
        tables.insert([tag[0], tag[1], tag[2], tag[3]], u64::from(font.u32(entry + 8)?));
    }
    let find = |tag: &[u8; 4]| {
        tables.get(tag).copied().ok_or_else(|| format!("no {} table", String::from_utf8_lossy(tag)))
    };
    let tables = Tables { name: find(b"name")?, os2: find(b"OS/2")?, head: find(b"head")? };
    find(b"glyf")?;
    Ok(tables)
}

/// A name (by nameID) from the name table: the Windows Unicode English record, else any Windows
/// Unicode one, as UTF-16LE bytes (what the EOT header holds). Empty if the font has none.
fn name_utf16le(font: &Bytes, table: u64, name_id: u16) -> Result<Vec<u8>, String> {
    let count = u64::from(font.u16(table + 2)?);
    let strings = table + u64::from(font.u16(table + 4)?);
    let mut found: Option<(u64, u64)> = None;
    for i in 0..count {
        let record = table + 6 + 12 * i;
        let (platform, encoding, language, id) =
            (font.u16(record)?, font.u16(record + 2)?, font.u16(record + 4)?, font.u16(record + 6)?);
        if id != name_id || platform != 3 || (encoding != 1 && encoding != 10) {
            continue;
        }
        let at = (u64::from(font.u16(record + 8)?), u64::from(font.u16(record + 10)?));
        if found.is_none() || language == 0x409 {
            found = Some(at);
        }
        if language == 0x409 {
            break;
        }
    }
    let Some((length, offset)) = found else { return Ok(Vec::new()) };
    let big_endian = font.slice(strings + offset, length)?;
    let mut little_endian = vec![0u8; big_endian.len()];
    for (out, pair) in little_endian.as_chunks_mut::<2>().0.iter_mut().zip(big_endian.as_chunks::<2>().0) {
        out[0] = pair[1];
        out[1] = pair[0];
    }
    Ok(little_endian)
}

fn decode(utf16le: &[u8]) -> String {
    let units: Vec<u16> = utf16le.as_chunks::<2>().0.iter().map(|p| u16::from_le_bytes(*p)).collect();
    String::from_utf16_lossy(&units)
}

/// What the font says about itself: family and style names, weight, italic, embedding rights.
pub fn font_info(data: &[u8]) -> Result<FontInfo, String> {
    let font = Bytes(data);
    let t = sfnt_tables(&font)?;
    let name = |id: u16| name_utf16le(&font, t.name, id).map(|b| decode(&b));
    let pick = |first: String, second: String| if first.is_empty() { second } else { first };
    Ok(FontInfo {
        family: pick(name(16)?, name(1)?),
        style: pick(name(17)?, name(2)?),
        weight: font.u16(t.os2 + 4)?,
        fs_type: font.u16(t.os2 + 8)?,
        italic: font.u16(t.os2 + 62)? & 1 == 1,
    })
}

/// `fsType` bits: 0x0002 restricted license embedding (may not be embedded at all); 0x0200 bitmap
/// embedding only. Everything else (installable, preview & print, editable) may be embedded.
pub fn embeddable(fs_type: u16) -> bool {
    fs_type & 0x000f != 0x0002 && fs_type & 0x0200 == 0
}

/// The font as Embedded OpenType, for `/ppt/fonts/fontN.fntdata`.
pub fn eot_from_ttf(data: &[u8]) -> Result<Vec<u8>, String> {
    let font = Bytes(data);
    let t = sfnt_tables(&font)?;
    let os2 = t.os2;
    let os2_version = font.u16(os2)?;
    let fs_type = font.u16(os2 + 8)?;
    if !embeddable(fs_type) {
        return Err(format!("the font's license does not allow embedding (fsType {fs_type})"));
    }
    // Family, style, version, full name.
    let names = [1, 2, 5, 4].map(|id| name_utf16le(&font, t.name, id));

    let mut header: Vec<u8> = Vec::with_capacity(FIXED_LEN + data.len() + 64);
    header.extend_from_slice(&[0; 8]); // EOTSize and FontDataSize are written last
    header.extend_from_slice(&EOT_VERSION.to_le_bytes());
    header.extend_from_slice(&0u32.to_le_bytes()); // Flags: not subsetted, not compressed, not XORed
    header.extend_from_slice(font.slice(os2 + 32, 10)?); // PANOSE
    header.push(1); // Charset: DEFAULT_CHARSET
    header.push((font.u16(os2 + 62)? & 1) as u8); // Italic: fsSelection bit 0
    header.extend_from_slice(&u32::from(font.u16(os2 + 4)?).to_le_bytes()); // Weight: usWeightClass
    header.extend_from_slice(&fs_type.to_le_bytes());
    header.extend_from_slice(&MAGIC.to_le_bytes());
    for i in 0..4 {
        header.extend_from_slice(&font.u32(os2 + 42 + 4 * i)?.to_le_bytes()); // ulUnicodeRange1-4
    }
    for i in 0..2 {
        let range = if os2_version >= 1 { font.u32(os2 + 78 + 4 * i)? } else { 0 }; // ulCodePageRange1-2
        header.extend_from_slice(&range.to_le_bytes());
    }
    header.extend_from_slice(&font.u32(t.head + 8)?.to_le_bytes()); // head.checkSumAdjustment
    header.extend_from_slice(&[0; 16]); // Reserved1-4
    debug_assert_eq!(header.len(), FIXED_LEN);
    for name in names {
        let name = name?;
        header.extend_from_slice(&0u16.to_le_bytes()); // Padding
        header.extend_from_slice(&(name.len() as u16).to_le_bytes()); // NameSize; a name record is at most 65535 long
        header.extend_from_slice(&name);
    }
    header.extend_from_slice(&[0; 4]); // Padding5, RootStringSize 0 (no root string: usable in any document)

    let total = u32::try_from(header.len() + data.len()).map_err(|_| "the font is too large".to_string())?;
    header[0..4].copy_from_slice(&total.to_le_bytes());
    header[4..8].copy_from_slice(&(data.len() as u32).to_le_bytes());
    header.extend_from_slice(data);
    Ok(header)
}
