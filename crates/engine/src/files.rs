//! Reading the files that belong to a talk.

use std::fs::File;
use std::io::{self, Cursor, Read, Seek};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use slides_components::{BuildEntry, BuildIndex, INDEX_FILE};

use crate::MAX_INDEX_BYTES;

/// The biggest file a talk can use. Pictures and fonts are read whole, into memory.
pub const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;

/// The biggest file of a large kind, such as a video, a talk can hold. These are never read
/// whole: they're opened and read in pieces, and moved as files.
pub const MAX_LARGE_FILE: u64 = 4 * 1024 * 1024 * 1024;

/// The endings of the kinds of file that can be large, in `assets/`.
const LARGE_KINDS: [&str; 5] = ["mp4", "m4v", "mov", "glb", "ply"];

/// Whether `path` is a file of a kind that can be large, a video or a model, which is opened
/// rather than read and stored in a pack as it is: told by its ending, in `assets/`.
pub fn is_large_kind(path: &str) -> bool {
    let Some(path) = crate::packed::normalise(path) else {
        return false;
    };
    let Some(ending) = path.strip_prefix("assets/").and_then(|rest| rest.rsplit_once('.')).map(|(_, ending)| ending) else {
        return false;
    };
    LARGE_KINDS.iter().any(|kind| ending.eq_ignore_ascii_case(kind))
}

/// Something a talk's file can be read from in pieces, from anywhere in it.
pub trait ReadSeek: Read + Seek + Send {}

impl<T: Read + Seek + Send> ReadSeek for T {}

/// Where a talk's pictures and fonts come from.
pub trait TalkFiles {
    /// The bytes of `path`, relative to the talk, or `None` if it can't be read.
    fn read(&self, path: &str) -> Option<Vec<u8>>;

    /// `path`, to be read in pieces, or `None` if it can't be. Unless the files say otherwise,
    /// it's read whole first, so this is for what [`TalkFiles::read`] would read too.
    fn open(&self, path: &str) -> Option<Box<dyn ReadSeek>> {
        self.read(path).map(|bytes| Box::new(Cursor::new(bytes)) as Box<dyn ReadSeek>)
    }

    /// The file on disk `path` can be read from directly, when there is one.
    fn lies_at(&self, _path: &str) -> Option<PathBuf> {
        None
    }

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

    /// Whether these files stay as they are for as long as they're open, so that what was
    /// read of them once can be kept without asking again.
    fn never_changes(&self) -> bool {
        false
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

    /// The file itself, if it's a regular file: never a folder or a named pipe, which would wait
    /// for a writer.
    fn open(&self, path: &str) -> Option<Box<dyn ReadSeek>> {
        let file = File::open(self.lies_at(path)?).ok()?;
        // Asked again of what was opened, in case something else was put there in between.
        file.metadata().ok()?.is_file().then(|| Box::new(file) as Box<dyn ReadSeek>)
    }

    fn lies_at(&self, path: &str) -> Option<PathBuf> {
        let file = self.resolve(path)?;
        std::fs::metadata(&file).ok()?.is_file().then_some(file)
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

    fn open(&self, path: &str) -> Option<Box<dyn ReadSeek>> {
        (**self).open(path)
    }

    fn lies_at(&self, path: &str) -> Option<PathBuf> {
        (**self).lies_at(path)
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

    fn never_changes(&self) -> bool {
        (**self).never_changes()
    }
}

/// A talk's files held in memory, by their path in it: the files of a presentation that hasn't
/// been saved anywhere yet. They never change.
pub struct MemoryFiles {
    files: std::collections::BTreeMap<String, std::sync::Arc<[u8]>>,
}

impl MemoryFiles {
    /// Holds `files`. One whose path isn't a plain one inside the talk (a `..`, a leading `/`, a
    /// drive, a backslash or a NUL, or a name no talk could be saved with) is dropped, as it
    /// couldn't be read from a folder either. If a path is given twice, the last one is kept.
    pub fn new(files: Vec<(String, std::sync::Arc<[u8]>)>) -> MemoryFiles {
        let files = files
            .into_iter()
            .filter(|(path, _)| crate::packed::safe_name(path) && !path.ends_with('/'))
            .collect();
        MemoryFiles { files }
    }

    fn get(&self, path: &str) -> Option<&std::sync::Arc<[u8]>> {
        self.files.get(crate::packed::normalise(path)?.as_str())
    }
}

impl TalkFiles for MemoryFiles {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.get(path).map(|bytes| bytes.to_vec())
    }

    fn len(&self, path: &str) -> Option<u64> {
        self.get(path).map(|bytes| bytes.len() as u64)
    }

    fn names_in(&self, folder: &str) -> Vec<String> {
        let folder = crate::packed::normalise(folder.trim_end_matches('/')).unwrap_or_default();
        self.files
            .keys()
            .filter(|name| match name.rsplit_once('/') {
                Some((parent, _)) => parent == folder,
                None => folder.is_empty(),
            })
            .cloned()
            .collect()
    }

    fn never_changes(&self) -> bool {
        true
    }
}

/// Files added to a talk while it's open, by their path in it, kept until they're saved. Shared
/// between the talk and whatever saves it. A file added keeps its bytes, or the file on disk it
/// lies in, until a save has written it where the talk is read from and it's released, from when
/// it's read there, as the talk's own. A large file, such as a video, is never held in memory: it
/// lies in a file of its own, and is opened there.
///
/// It also holds builds entered in the talk's build index that no save has written yet, as
/// for a component pasted with its build: the index the talk's files give is the talk's own
/// with these added to it, each but one for a component or file the talk's own has an entry
/// for, which is never replaced.
#[derive(Default)]
pub struct AddedSet {
    files: Mutex<BTreeMap<String, AddedFile>>,
    /// How many files have been added or released, so that something holding on to what it
    /// read can tell when that's changed.
    added: AtomicU64,
    builds: Mutex<Vec<BuildEntry>>,
    /// Changes each time the builds entered change, so the index they give is read again.
    builds_changed: AtomicU64,
}

/// A file added to a talk: its bytes, held, or the file on disk it lies in, with its length.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AddedFile {
    Bytes(Arc<[u8]>),
    At(PathBuf, u64),
}

impl AddedFile {
    /// How many bytes it holds.
    pub fn len(&self) -> u64 {
        match self {
            AddedFile::Bytes(bytes) => bytes.len() as u64,
            AddedFile::At(_, len) => *len,
        }
    }

    /// Whether it holds nothing.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl AddedSet {
    pub fn new() -> AddedSet {
        AddedSet::default()
    }

    fn files(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, AddedFile>> {
        // A map is whole even if a thread panicked holding it.
        self.files.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Adds `bytes` as the file at `path`, unless there's one there already, which is kept:
    /// whether it was added.
    pub fn add(&self, path: &str, bytes: Arc<[u8]>) -> bool {
        self.add_file(path, AddedFile::Bytes(bytes))
    }

    /// Adds the file on disk at `file`, which holds `len` bytes, as the file at `path`, unless
    /// there's one there already, which is kept: whether it was added. It's read where it lies,
    /// in pieces, and never whole.
    pub fn add_at(&self, path: &str, file: PathBuf, len: u64) -> bool {
        self.add_file(path, AddedFile::At(file, len))
    }

    fn add_file(&self, path: &str, file: AddedFile) -> bool {
        let Some(path) = crate::packed::normalise(path) else {
            return false;
        };
        let mut files = self.files();
        if files.contains_key(&path) {
            return false;
        }
        files.insert(path, file);
        self.added.fetch_add(1, Ordering::SeqCst);
        true
    }

    /// Has the file added at `path` that lies on disk be read from `file` from now on, which holds
    /// the same bytes: whether it was one that lies on disk.
    pub fn move_at(&self, path: &str, file: PathBuf) -> bool {
        let Some(path) = crate::packed::normalise(path) else {
            return false;
        };
        let mut files = self.files();
        let Some(AddedFile::At(at, _)) = files.get_mut(&path) else {
            return false;
        };
        *at = file;
        self.added.fetch_add(1, Ordering::SeqCst);
        true
    }

    /// Lets go of the files added at `paths`, which a save has written where the talk is read
    /// from: from now on they're read there. Whether any was let go of.
    pub fn release(&self, paths: &[String]) -> bool {
        let mut files = self.files();
        let mut released = false;
        for path in paths.iter().filter_map(|path| crate::packed::normalise(path)) {
            released |= files.remove(&path).is_some();
        }
        if released {
            self.added.fetch_add(1, Ordering::SeqCst);
        }
        released
    }

    /// The bytes added as `path`, if they're held. A file that lies on disk has none here.
    pub fn get(&self, path: &str) -> Option<Arc<[u8]>> {
        match self.file(path)? {
            AddedFile::Bytes(bytes) => Some(bytes),
            AddedFile::At(..) => None,
        }
    }

    /// The file added as `path`, held or on disk.
    pub fn file(&self, path: &str) -> Option<AddedFile> {
        self.files().get(&crate::packed::normalise(path)?).cloned()
    }

    /// Where on disk the file added as `path` lies, if it lies anywhere.
    pub fn lies_at(&self, path: &str) -> Option<PathBuf> {
        match self.file(path)? {
            AddedFile::At(file, _) => Some(file),
            AddedFile::Bytes(_) => None,
        }
    }

    /// Every file on disk a file added lies in.
    pub fn files_on_disk(&self) -> Vec<PathBuf> {
        self.files()
            .values()
            .filter_map(|file| match file {
                AddedFile::At(file, _) => Some(file.clone()),
                AddedFile::Bytes(_) => None,
            })
            .collect()
    }

    /// The files added directly in `folder`, in name order.
    pub fn names_in(&self, folder: &str) -> Vec<String> {
        let Some(folder) = crate::packed::normalise(folder) else {
            return Vec::new();
        };
        let prefix = format!("{folder}/");
        self.files()
            .keys()
            .filter(|name| name.strip_prefix(&prefix).is_some_and(|rest| !rest.contains('/')))
            .cloned()
            .collect()
    }

    /// Changes each time a file is added or released.
    pub fn stamp(&self) -> u64 {
        self.added.load(Ordering::SeqCst)
    }

    fn builds(&self) -> std::sync::MutexGuard<'_, Vec<BuildEntry>> {
        self.builds.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Enters `entry` in the talk's build index, in place of any entered for the same component
    /// or file.
    pub fn enter_build(&self, entry: BuildEntry) {
        let mut builds = self.builds();
        builds.retain(|kept| kept.tag != entry.tag && kept.file != entry.file);
        builds.push(entry);
        self.builds_changed.fetch_add(1, Ordering::SeqCst);
        self.added.fetch_add(1, Ordering::SeqCst);
    }

    /// The builds entered that no save has written, in the order they were entered.
    pub fn builds_entered(&self) -> Vec<BuildEntry> {
        self.builds().clone()
    }

    /// Takes `written` out of the builds entered, once a save has entered them in the talk's own
    /// index.
    pub fn forget_builds(&self, written: &[BuildEntry]) {
        let mut builds = self.builds();
        let before = builds.len();
        builds.retain(|kept| !written.contains(kept));
        if builds.len() != before {
            self.builds_changed.fetch_add(1, Ordering::SeqCst);
            self.added.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// The index `under`'s own gives with the builds entered in it, or `None` if none are, or
    /// if its own can't be read as an index, which is then read as it is.
    fn index_over(&self, under: &dyn TalkFiles) -> Option<Vec<u8>> {
        let builds = self.builds_entered();
        if builds.is_empty() {
            return None;
        }
        let mut index = match under.len(INDEX_FILE) {
            Some(len) if len > MAX_INDEX_BYTES => return None,
            _ => match under.read(INDEX_FILE) {
                None => BuildIndex { format: 1, components: Vec::new() },
                Some(bytes) if bytes.len() as u64 > MAX_INDEX_BYTES => return None,
                Some(bytes) => serde_json::from_slice::<BuildIndex>(&bytes).ok()?,
            },
        };
        // Added beside the talk's own, never in place of one of them.
        for entry in builds {
            if !index.components.iter().any(|kept| kept.tag == entry.tag || kept.file == entry.file) {
                index.components.push(entry);
            }
        }
        index.format = index.format.max(1);
        serde_json::to_vec_pretty(&index).ok()
    }
}

/// A talk's files with those added to it since it opened, which are read first.
pub struct WithAdded {
    under: Arc<dyn TalkFiles + Send + Sync>,
    added: Arc<AddedSet>,
}

impl WithAdded {
    pub fn new(under: Arc<dyn TalkFiles + Send + Sync>, added: Arc<AddedSet>) -> WithAdded {
        WithAdded { under, added }
    }
}

impl WithAdded {
    /// The build index the talk's files give with the builds entered in it, if any are.
    fn entered_index(&self, path: &str) -> Option<Vec<u8>> {
        (crate::packed::normalise(path).as_deref() == Some(INDEX_FILE)).then(|| self.added.index_over(&*self.under)).flatten()
    }
}

impl TalkFiles for WithAdded {
    /// A file added that lies on disk is never read whole: it's opened.
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        if let Some(index) = self.entered_index(path) {
            return Some(index);
        }
        match self.added.file(path) {
            Some(AddedFile::Bytes(bytes)) => Some(bytes.to_vec()),
            Some(AddedFile::At(..)) => None,
            None => self.under.read(path),
        }
    }

    fn open(&self, path: &str) -> Option<Box<dyn ReadSeek>> {
        if let Some(index) = self.entered_index(path) {
            return Some(Box::new(Cursor::new(index)));
        }
        match self.added.file(path) {
            Some(AddedFile::Bytes(bytes)) => Some(Box::new(Cursor::new(bytes))),
            Some(AddedFile::At(file, _)) => {
                let file = File::open(file).ok()?;
                file.metadata().ok()?.is_file().then(|| Box::new(file) as Box<dyn ReadSeek>)
            }
            None => self.under.open(path),
        }
    }

    /// What's added is held in memory, or lies in a file of its own, and the build index with
    /// builds entered in it is made when it's read, so lies nowhere.
    fn lies_at(&self, path: &str) -> Option<PathBuf> {
        if self.entered_index(path).is_some() {
            return None;
        }
        match self.added.file(path) {
            Some(AddedFile::At(file, _)) => Some(file),
            Some(AddedFile::Bytes(_)) => None,
            None => self.under.lies_at(path),
        }
    }

    /// An added file never changes while it's held, so its length is its stamp; so is a file of a talk that
    /// never changes, such as a pack, as nothing under this can say that any more. The build
    /// index, with builds entered in it, changes with them as with its own file.
    fn stamp(&self, path: &str) -> Option<(u64, SystemTime)> {
        if let Some(index) = self.entered_index(path) {
            let own = if self.under.never_changes() { None } else { self.under.stamp(path) };
            let changed = std::time::Duration::from_nanos(self.added.builds_changed.load(Ordering::SeqCst));
            let at = own.map_or(SystemTime::UNIX_EPOCH, |(_, at)| at) + changed;
            return Some((index.len() as u64, at));
        }
        if let Some(file) = self.added.file(path) {
            return Some((file.len(), SystemTime::UNIX_EPOCH));
        }
        if self.under.never_changes() {
            return self.under.len(path).map(|len| (len, SystemTime::UNIX_EPOCH));
        }
        self.under.stamp(path)
    }

    fn len(&self, path: &str) -> Option<u64> {
        if let Some(index) = self.entered_index(path) {
            return Some(index.len() as u64);
        }
        match self.added.file(path) {
            Some(file) => Some(file.len()),
            None => self.under.len(path),
        }
    }

    fn names_in(&self, folder: &str) -> Vec<String> {
        let mut names = self.under.names_in(folder);
        names.extend(self.added.names_in(folder));
        if crate::packed::normalise(folder).as_deref() == Some("build") && !self.added.builds_entered().is_empty() {
            names.push(INDEX_FILE.to_string());
        }
        names.sort();
        names.dedup();
        names
    }

    /// Files can be added at any time.
    fn never_changes(&self) -> bool {
        false
    }
}
