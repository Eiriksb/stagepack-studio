//! Reading the files that belong to a talk.

use std::path::PathBuf;

/// Where a talk's pictures and fonts come from.
pub trait TalkFiles {
    /// The bytes of `path`, relative to the talk, or `None` if it can't be read.
    fn read(&self, path: &str) -> Option<Vec<u8>>;
}

/// A talk kept in a folder on disk.
pub struct FolderFiles {
    root: PathBuf,
}

impl FolderFiles {
    pub fn new(root: PathBuf) -> FolderFiles {
        FolderFiles { root }
    }
}

impl TalkFiles for FolderFiles {
    /// A talk can't reach outside its folder, whether by `..`, an absolute path or a symlink.
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let root = self.root.canonicalize().ok()?;
        let file = root.join(path).canonicalize().ok()?;
        if !file.starts_with(&root) {
            return None;
        }
        std::fs::read(file).ok()
    }
}
