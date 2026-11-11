//! Reading the files that belong to a talk.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

/// The biggest file a talk can use. Pictures and fonts are read whole, into memory.
pub const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;

/// Where a talk's pictures and fonts come from.
pub trait TalkFiles {
    /// The bytes of `path`, relative to the talk, or `None` if it can't be read.
    fn read(&self, path: &str) -> Option<Vec<u8>>;

    /// The length and modified time of `path`, for telling whether it has changed without
    /// reading it, or `None` if there's no such thing to go by.
    fn stamp(&self, _path: &str) -> Option<(u64, SystemTime)> {
        None
    }

    /// The length of `path` in bytes, found without reading it, or `None` if it can't be.
    fn len(&self, _path: &str) -> Option<u64> {
        None
    }

    /// The files directly in `folder`, as paths relative to the talk, in name order.
    fn names_in(&self, _folder: &str) -> Vec<String> {
        Vec::new()
    }
}

/// A talk kept in a folder on disk.
pub struct FolderFiles {
    root: PathBuf,
}

impl FolderFiles {
    pub fn new(root: PathBuf) -> FolderFiles {
        FolderFiles { root }
    }

    /// Where `path` really is. A talk can't reach outside its folder, whether by `..`, an
    /// absolute path or a symlink.
    fn resolve(&self, path: &str) -> Option<PathBuf> {
        // Judged by the text alone, before the disk is touched: on Windows even finding out
        // where a network path leads reaches out to the network. A path that is absolute, has a
        // Windows prefix or a backslash, or climbs out of the talk with `..`, is refused.
        let mut clean = PathBuf::new();
        if path.contains('\\') {
            return None;
        }
        for part in Path::new(path).components() {
            match part {
                Component::Normal(name) => clean.push(name),
                Component::CurDir => {}
                Component::ParentDir => {
                    if !clean.pop() {
                        return None;
                    }
                }
                Component::RootDir | Component::Prefix(_) => return None,
            }
        }
        let path = clean.as_path();
        let root = self.root.canonicalize().ok()?;
        let file = root.join(path).canonicalize().ok()?;
        file.starts_with(&root).then_some(file)
    }
}

/// The bytes of the regular file at `path`, if it's within `MAX_FILE_BYTES`. Its kind is asked
/// first, as opening a named pipe waits until something writes to it.
fn read_bounded(path: &Path) -> io::Result<Vec<u8>> {
    let too_big = || {
        let limit = MAX_FILE_BYTES / (1024 * 1024);
        io::Error::new(io::ErrorKind::InvalidData, format!("it's over the limit of {limit} MiB"))
    };
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "it isn't a regular file"));
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(too_big());
    }
    let mut bytes = Vec::new();
    // Bounded, in case the file grows while it is read.
    File::open(path)?.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(too_big());
    }
    Ok(bytes)
}

/// The text of a folder talk's own `slides.md`, held to the same rules as the files it uses.
pub fn read_talk_text(path: &Path) -> io::Result<String> {
    String::from_utf8(read_bounded(path)?)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "it isn't UTF-8 text"))
}

impl TalkFiles for FolderFiles {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        read_bounded(&self.resolve(path)?).ok()
    }

    fn stamp(&self, path: &str) -> Option<(u64, SystemTime)> {
        let metadata = std::fs::metadata(self.resolve(path)?).ok()?;
        Some((metadata.len(), metadata.modified().ok()?))
    }

    fn len(&self, path: &str) -> Option<u64> {
        let metadata = std::fs::metadata(self.resolve(path)?).ok()?;
        metadata.is_file().then_some(metadata.len())
    }

    /// Only regular files, and not links to them.
    fn names_in(&self, folder: &str) -> Vec<String> {
        let Some(entries) = self.resolve(folder).and_then(|dir| std::fs::read_dir(dir).ok()) else {
            return Vec::new();
        };
        let folder = folder.trim_end_matches('/');
        let mut names: Vec<String> = entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .filter_map(|entry| entry.file_name().into_string().ok())
            .map(|name| format!("{folder}/{name}"))
            .collect();
        names.sort();
        names
    }
}

impl TalkFiles for std::sync::Arc<dyn TalkFiles + Send + Sync> {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        (**self).read(path)
    }

    fn stamp(&self, path: &str) -> Option<(u64, SystemTime)> {
        (**self).stamp(path)
    }

    fn len(&self, path: &str) -> Option<u64> {
        (**self).len(path)
    }

    fn names_in(&self, folder: &str) -> Vec<String> {
        (**self).names_in(folder)
    }
}
