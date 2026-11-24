//! The add-on interface, version 1, shared by the app and the add-ons it runs.
//!
//! An add-on is a WebAssembly module with no imports at all. It exports `addon_abi() -> u32`,
//! `alloc(len: u32) -> u32`, `info() -> u64`, `run(ptr: u32, len: u32) -> u64` and `memory`.
//! A `u64` it returns is a pointer and a length packed as `(ptr << 32) | len`.
//!
//! `run` takes a bundle and gives one back. The bundle it takes holds [`OP`], [`ARGS`] and
//! the files; the one it gives back holds the files and [`REPORT`], or only [`ERROR`].

use serde::{Deserialize, Serialize};

/// The version of this interface. An add-on's `addon_abi` must return it.
pub const ABI_VERSION: u32 = 1;

/// The first four bytes of every bundle.
pub const MAGIC: [u8; 4] = *b"SPB1";

/// The name of the operation to do, `export` or `import`, in the bundle `run` takes.
pub const OP: &str = ".op";
/// The operation's arguments, as JSON, in the bundle `run` takes.
pub const ARGS: &str = ".args.json";
/// A [`Report`], as JSON, in the bundle `run` gives back.
pub const REPORT: &str = ".report.json";
/// One sentence on why the operation failed, the only file of a bundle that says so.
pub const ERROR: &str = ".error";
/// The bundle's own names, which are never files of a talk.
pub const OWN_NAMES: [&str; 4] = [OP, ARGS, REPORT, ERROR];

/// The most files a bundle can hold.
pub const MAX_FILES: usize = 10_000;

/// Files by name, in the order given.
pub type Bundle = Vec<(String, Vec<u8>)>;

/// What an operation couldn't carry over as it was.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub notices: Vec<Notice>,
}

/// One thing an operation couldn't carry over as it was, on a slide or on the whole talk.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    /// The slide it is on, counting from 1, or `None` for the whole talk.
    #[serde(default)]
    pub slide: Option<usize>,
    pub kind: NoticeKind,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    /// A labelled box holding the original text took its place.
    Placeholder,
    /// It was carried over, but doesn't look quite the same.
    Approximation,
    /// It was left out.
    LeftOut,
    /// It was carried over as a picture, so it can't be edited.
    Frozen,
}

/// The bytes of `bundle`: [`MAGIC`], a `u32` LE count, then for each file a `u32` LE name
/// length, the UTF-8 name, a `u32` LE data length and the data.
///
/// # Panics
///
/// If a name or a file is 4 GiB or more, which no bundle the app or an add-on makes holds.
pub fn encode(bundle: &Bundle) -> Vec<u8> {
    let size = 8 + bundle.iter().map(|(name, data)| 8 + name.len() + data.len()).sum::<usize>();
    let mut bytes = Vec::with_capacity(size);
    bytes.extend(MAGIC);
    bytes.extend(length(bundle.len()));
    for (name, data) in bundle {
        bytes.extend(length(name.len()));
        bytes.extend(name.as_bytes());
        bytes.extend(length(data.len()));
        bytes.extend(data);
    }
    bytes
}

fn length(len: usize) -> [u8; 4] {
    u32::try_from(len).expect("a bundle holds nothing of 4 GiB or more").to_le_bytes()
}

/// The files of a bundle, or a sentence naming what is wrong with it. Every name is either
/// one of [`OWN_NAMES`] or passes [`safe_name`], and no name is there twice, even in other
/// letter case: on Windows and macOS one of them would be written over the other.
pub fn decode(bytes: &[u8]) -> Result<Bundle, String> {
    let mut reader = Reader { bytes, at: 0 };
    if reader.take(4) != Some(&MAGIC[..]) {
        return Err("The bundle doesn't start with the bundle magic `SPB1`.".into());
    }
    let count = reader.u32().ok_or("The bundle ends before its file count.")? as usize;
    // Each file takes at least its two lengths, so a count the bytes can't hold is refused
    // before room is made for it.
    if count > MAX_FILES || count > reader.left() / 8 {
        return Err(format!("The bundle's file count, {count}, is more than it holds or than the {MAX_FILES} allowed."));
    }
    let mut bundle = Bundle::with_capacity(count);
    let mut names = std::collections::BTreeMap::new();
    for index in 1..=count {
        let len = reader.u32().ok_or_else(|| format!("The bundle ends before the name length of file {index}."))? as usize;
        let name = reader.take(len).ok_or_else(|| format!("The name of file {index} runs past the end of the bundle."))?;
        let name = std::str::from_utf8(name).map_err(|_| format!("The name of file {index} isn't UTF-8."))?;
        if !(safe_name(name) || OWN_NAMES.contains(&name)) {
            return Err(format!("The bundle has a file named {name:?}, which isn't a safe name."));
        }
        if let Some(earlier) = names.insert(name.to_lowercase(), name) {
            return Err(if earlier == name {
                format!("The bundle has the file {name:?} twice.")
            } else {
                format!("The bundle has the files {earlier:?} and {name:?}, whose names differ only in letter case.")
            });
        }
        let len = reader.u32().ok_or_else(|| format!("The bundle ends before the data length of {name:?}."))? as usize;
        let data = reader.take(len).ok_or_else(|| format!("The data of {name:?} runs past the end of the bundle."))?;
        bundle.push((name.to_string(), data.to_vec()));
    }
    if reader.left() > 0 {
        return Err(format!("The bundle has {} bytes after its last file.", reader.left()));
    }
    Ok(bundle)
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn left(&self) -> usize {
        self.bytes.len() - self.at
    }

    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let taken = self.bytes.get(self.at..self.at.checked_add(len)?)?;
        self.at += len;
        Some(taken)
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
}

/// Windows' device names, which can't be files there whatever their extension.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4",
    "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Whether `name`, a `/`-separated path, stays inside the talk and can be a file on every
/// platform. It is the rule a `.sptx` holds its entries to: no `..`, `.` or empty parts, no
/// absolute path, no `\`, no NUL or other control character, no drive and no hidden name.
pub fn safe_name(name: &str) -> bool {
    !name.is_empty() && name.split('/').all(safe_part)
}

fn safe_part(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or("").trim_end_matches(' ').to_ascii_uppercase();
    !part.is_empty()
        // Hidden names are how tools leave configuration that can run commands later, and
        // this covers `.` and `..` as well.
        && !part.starts_with('.')
        && !part.ends_with(['.', ' '])
        && !part.contains(['\\', ':', '<', '>', '"', '|', '?', '*'])
        && !part.chars().any(|c| c.is_control() || is_invisible(c))
        && !RESERVED.contains(&stem.as_str())
}

/// A format character that draws nothing, which could make one name pass for another.
fn is_invisible(c: char) -> bool {
    matches!(c, '\u{AD}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{206F}' | '\u{FEFF}')
}
