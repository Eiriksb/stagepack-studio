//! Reading the files that belong to a talk.

use std::path::{Component, Path, PathBuf};
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

impl TalkFiles for FolderFiles {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        std::fs::read(self.resolve(path)?).ok()
    }

    fn stamp(&self, path: &str) -> Option<(u64, SystemTime)> {
        let metadata = std::fs::metadata(self.resolve(path)?).ok()?;
        Some((metadata.len(), metadata.modified().ok()?))
    }
}

impl TalkFiles for std::sync::Arc<dyn TalkFiles + Send + Sync> {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        (**self).read(path)
    }

    fn stamp(&self, path: &str) -> Option<(u64, SystemTime)> {
        (**self).stamp(path)
    }
}
