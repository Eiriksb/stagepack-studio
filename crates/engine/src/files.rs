//! Reading the files that belong to a talk.

use std::path::PathBuf;
use std::time::SystemTime;

/// Where a talk's pictures and fonts come from.
pub trait TalkFiles {
    /// The bytes of `path`, relative to the talk, or `None` if it can't be read.
    fn read(&self, path: &str) -> Option<Vec<u8>>;

    /// The length and modified time of `path`, for telling whether it has changed without
    /// reading it, or `None` if there's no such thing to go by.
    fn stamp(&self, _path: &str) -> Option<(u64, SystemTime)> {
        None
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
        let root = self.root.canonicalize().ok()?;
        let file = root.join(path).canonicalize().ok()?;
        file.starts_with(&root).then_some(file)
    }
}

impl TalkFiles for FolderFiles {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        std::fs::read(self.resolve(path)?).ok()
    }

    fn stamp(&self, path: &str) -> Option<(u64, SystemTime)> {
        let metadata = std::fs::metadata(self.resolve(path)?).ok()?;
        Some((metadata.len(), metadata.modified().ok()?))
    }
}
