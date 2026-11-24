//! What a font file of the talk's own says it is, for its `@font-face`: its family, how bold
//! it is and whether it is italic. Only the tables that say so are read, from a TrueType or
//! OpenType file; anything else is known by its file name.

/// A font's family, its weight from 100 to 900, and whether it is italic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Face {
    pub family: String,
    pub weight: u16,
    pub italic: bool,
}

/// What `data`, a font file named `stem` without its extension, says it is. One that can't
/// be read is the family `stem`, at the usual weight.
pub fn face(data: &[u8], stem: &str) -> Face {
    let fallback = Face { family: stem.to_string(), weight: 400, italic: false };
    let Some(tables) = tables(data) else { return fallback };
    let family = tables.iter().find(|(tag, _)| tag == b"name").and_then(|(_, table)| family(table));
    let (weight, italic) = tables.iter().find(|(tag, _)| tag == b"OS/2").and_then(|(_, table)| os2(table)).unwrap_or((400, false));
    Face { family: family.unwrap_or(fallback.family), weight, italic }
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

/// The font's tables, by tag.
fn tables(data: &[u8]) -> Option<Vec<([u8; 4], &[u8])>> {
    let version = u32_at(data, 0)?;
    if !matches!(version, 0x0001_0000 | 0x4F54_544F | 0x7472_7565) {
        return None;
    }
    let count = usize::from(u16_at(data, 4)?);
    (0..count)
        .map(|index| {
            let record = 12 + index * 16;
            let tag: [u8; 4] = data.get(record..record + 4)?.try_into().ok()?;
            let offset = u32_at(data, record + 8)? as usize;
            let length = u32_at(data, record + 12)? as usize;
            Some((tag, data.get(offset..offset.checked_add(length)?)?))
        })
        .collect()
}

/// The family the `name` table gives: the typographic family if it has one, or else the
/// family, in Windows' Unicode or the Mac's Roman letters.
fn family(table: &[u8]) -> Option<String> {
    let count = usize::from(u16_at(table, 2)?);
    let strings = usize::from(u16_at(table, 4)?);
    let mut found: Option<(u16, String)> = None;
    for index in 0..count {
        let record = 6 + index * 12;
        let (platform, encoding, name_id) = (u16_at(table, record)?, u16_at(table, record + 2)?, u16_at(table, record + 6)?);
        let length = usize::from(u16_at(table, record + 8)?);
        let offset = strings + usize::from(u16_at(table, record + 10)?);
        if !matches!(name_id, 1 | 16) {
            continue;
        }
        let Some(bytes) = table.get(offset..offset + length) else { continue };
        let text = match (platform, encoding) {
            (3, 0 | 1) | (0, _) => {
                let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|pair| u16::from_be_bytes(*pair)).collect();
                String::from_utf16(&units).ok()
            }
            (1, 0) => Some(bytes.iter().map(|byte| if byte.is_ascii() { char::from(*byte) } else { '?' }).collect()),
            _ => None,
        };
        let Some(text) = text.filter(|text| !text.trim().is_empty()) else { continue };
        // The typographic family is the one the styles are grouped under.
        if found.as_ref().is_none_or(|(id, _)| *id < name_id) {
            found = Some((name_id, text.trim().to_string()));
        }
    }
    found.map(|(_, text)| text)
}

/// The weight and whether it's italic, from the `OS/2` table.
fn os2(table: &[u8]) -> Option<(u16, bool)> {
    let weight = u16_at(table, 4)?;
    let selection = u16_at(table, 62)?;
    let weight = (weight.clamp(100, 900) / 100) * 100;
    Some((weight, selection & 1 != 0))
}
