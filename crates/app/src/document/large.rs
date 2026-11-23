//! Large files added to the talk, such as videos: copied into the window's working copy as
//! files, a piece at a time, hashed as they go, held there by their path until a save writes
//! them, read in pieces from wherever they lie, and carried by where they lie when copied.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use slides_engine::{AddedFile, MadeStill, MeasureVideo, ModelLayer, StillJob, StillWanted, TerminalLayer, VideoLayer, is_large_kind};

use super::*;
use crate::clipboard::{CopiedLarge, PastedLarge};
use crate::large_files::{self, HeldReader, LargeSource, Looked, NotOpened, Readers, Told, VideoCache};
use crate::recovery::{Keeping, Recovery};

/// The files in the working copy that a copy is being made to, or has been made to and not yet
/// taken in, which a slot that's cleared keeps.
type SpokenFor = Arc<Mutex<BTreeSet<PathBuf>>>;

fn lock_any<T>(held: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    held.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn lock(spoken_for: &SpokenFor) -> std::sync::MutexGuard<'_, BTreeSet<PathBuf>> {
    spoken_for.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Where the talk's large files are copied to, and what it knows of them.
#[derive(Default)]
pub(crate) struct LargeHome {
    /// The window's working copies, whose slot a large file is copied to as it's added.
    recovery: Option<Recovery>,
    spoken_for: SpokenFor,
    /// The SHA-256 of each large file copied in, by where it lies, for telling the same file
    /// added again without reading it.
    hashes: BTreeMap<PathBuf, [u8; 32]>,
    /// The large files brought back from last time that couldn't be taken into this window's
    /// working copy, and lie in last time's still, which is kept until they're taken in.
    left_in_last: LeftInLast,
    readers: Arc<Readers>,
    /// Where a video a pack holds compressed is unpacked to; the app's own if none is given.
    cache: Option<VideoCache>,
    /// Whether a large file was ever to be copied into the working copy, which made its folder.
    copied_in: std::cell::Cell<bool>,
    /// What's told when a video's still is wanted, once something makes them.
    stills_wanted: Arc<Mutex<Option<StillWanted>>>,
    /// Whether one was wanted before anything made them.
    still_wanted_early: Arc<AtomicBool>,
    /// The stills that couldn't be made yet: while a save replaced the pack, or for a video a
    /// pack holds compressed that hasn't been unpacked to play.
    stills_waiting: Vec<StillJob>,
    /// What a talk's file is read from, as things are now, for what reads off the window's thread.
    opening: Arc<Mutex<Opening>>,
    /// What's known of each video, 3D model and terminal recording, by its path, and the version
    /// of its file it was read from, at most [`MOST_FACTS`] of them, each numbered as it was asked
    /// for.
    facts: BTreeMap<String, (Option<FileStamp>, Facts, u64)>,
    facts_asked: u64,
}

/// The most videos, models and recordings whose facts are kept: the one asked for longest ago
/// goes first.
const MOST_FACTS: usize = 256;

/// A file's length and when it was last changed, which tell a new version of it. A file the
/// talk's files give no time for is one that doesn't change while it's open, as one in a pack
/// or one added and kept in the working copy, and is told by its length alone.
type FileStamp = (u64, std::time::SystemTime);

/// What's known of a video's, a model's or a recording's facts.
enum Facts {
    /// They're being read.
    Reading,
    /// They couldn't be read yet: while a save replaced the pack, or before a video a pack holds
    /// compressed is unpacked to play.
    Waiting,
    Known(Known),
}

/// A video's facts, a model's or a recording's, or why there are none.
#[derive(Clone, Copy)]
enum Known {
    Video(Result<VideoFacts, slides_video::CantPlay>),
    Model(Result<slides_model::ModelFacts, slides_model::CantDraw>),
    Recording(Result<slides_terminal::RecordingFacts, slides_terminal::CantPlay>),
}

/// The kind of file whose facts are read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FactsOf {
    Video,
    Model,
    Recording,
}

impl Known {
    fn of(&self) -> FactsOf {
        match self {
            Known::Video(_) => FactsOf::Video,
            Known::Model(_) => FactsOf::Model,
            Known::Recording(_) => FactsOf::Recording,
        }
    }
}

/// What the form says of a video: how long it is, in seconds, and its size, in pixels, as it's shown.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoFacts {
    pub seconds: f64,
    pub size: (u32, u32),
}

/// A video's facts, a model's or a recording's, to be read off the window's thread, as the whole
/// of a video's index is read, a model's description and a recording from end to end: see
/// [`Document::video_facts_to_read`], [`Document::model_facts_to_read`] and
/// [`Document::recording_facts_to_read`].
pub struct FactsToRead {
    path: String,
    version: Option<FileStamp>,
    from: ReadFrom,
}

/// Where facts to be read are read from, and what kind of file they're of.
enum ReadFrom {
    Video(Option<LargeOpen>),
    Model(Option<LargeOpen>),
    /// Read whole, as a small data file of the talk's is.
    Recording(Option<LargeOpen>),
}

/// What came of reading a video's or a model's facts, for [`Document::take_video_facts`] or
/// [`Document::take_model_facts`] to take in.
pub struct FactsRead {
    path: String,
    version: Option<FileStamp>,
    /// `None` if they couldn't be read yet.
    read: Option<Known>,
}

impl FactsToRead {
    /// Reads them, held among the talk's readers, within the limits a model is read with. A video
    /// a pack holds compressed is read only once it's been unpacked, never unpacked just for this;
    /// a model, never more than 256 MB, is unpacked to read them, as it is to draw its still. A
    /// recording is read as its still reads it, unpacked if a pack holds it compressed.
    pub fn read(self) -> FactsRead {
        let (open, model) = match self.from {
            ReadFrom::Recording(open) => {
                let read = match open.map(LargeOpen::read_recording) {
                    None => Some(Err(slides_terminal::CantPlay::CouldNotRead)),
                    Some(Ok(bytes)) => Some(bytes.and_then(|bytes| slides_terminal::facts(&bytes))),
                    // While a save replaces the pack: read again after it.
                    Some(Err(_)) => None,
                };
                return FactsRead { path: self.path, version: self.version, read: read.map(Known::Recording) };
            }
            ReadFrom::Video(open) => (open, false),
            ReadFrom::Model(open) => (open, true),
        };
        let cant_read = if model { Known::Model(Err(slides_model::CantDraw::CouldNotRead)) } else { Known::Video(Err(slides_video::CantPlay::CouldNotRead)) };
        let opened = if model { LargeOpen::open_unless_saving } else { LargeOpen::open_for_still };
        let read = match open.map(opened) {
            None | Some(Err(NotOpened::Unreadable)) => Some(cant_read),
            Some(Err(NotOpened::Saving | NotOpened::NotUnpacked)) => None,
            Some(Ok(reader)) => {
                let watch = reader.watch();
                let read = if model { Known::Model(slides_model::facts(Box::new(reader))) } else { Known::Video(facts_of(reader)) };
                match read {
                    // A save let go of its file: they're read again after it.
                    Known::Video(Err(_)) | Known::Model(Err(_)) if watch.is_let_go() => None,
                    read => Some(read),
                }
            }
        };
        FactsRead { path: self.path, version: self.version, read }
    }
}

/// The facts of the video `reader` reads: its size from its headers, and its length from its
/// index, which is read whole.
fn facts_of(mut reader: HeldReader) -> Result<VideoFacts, slides_video::CantPlay> {
    let size = slides_video::video_size(&mut reader);
    let info = slides_video::open(Box::new(reader))?.info();
    Ok(VideoFacts { seconds: info.seconds, size: size.unwrap_or((info.width, info.height)) })
}

/// What a talk's file is opened from: see [`LargeOpen`].
#[derive(Clone, Default)]
struct Opening {
    files: Option<Arc<dyn TalkFiles + Send + Sync>>,
    pack: Option<PathBuf>,
    cache: Option<VideoCache>,
}

/// What a working copy that's cleared keeps of its slot: every file a large file added to the
/// talk lies in, and those a copy is being made to. It can be handed to a background task.
#[derive(Clone)]
pub struct LargeKeep {
    added: Arc<AddedSet>,
    spoken_for: SpokenFor,
    left_in_last: LeftInLast,
}

impl LargeKeep {
    /// The files to keep, by where they lie, as things are now.
    pub fn held(&self) -> BTreeSet<PathBuf> {
        self.held_with(&lock(&self.spoken_for))
    }

    /// Takes each large file brought back that still lies in last time's working copy into
    /// `recovery`, this window's, for the talk at `place`, and reads it from there from then on:
    /// once none lie there, last time's copy can go. Done by a background job, as it may copy the
    /// whole file; one that can't be taken in waits for the next save, or the next start.
    pub fn take_in_left_in_last(&self, recovery: &Recovery, place: &TalkPlace) {
        if !self.left_in_last.again.swap(false, Ordering::SeqCst) {
            return;
        }
        let left: Vec<String> = self.left_in_last.paths().iter().cloned().collect();
        for path in left {
            let Some(at) = self.added.lies_at(&path) else {
                self.left_in_last.paths().remove(&path);
                continue;
            };
            if let Ok(here) = recovery.take_in(place, &path, &at) {
                self.added.move_at(&path, here);
                self.left_in_last.paths().remove(&path);
            }
        }
    }

    fn held_with(&self, spoken_for: &BTreeSet<PathBuf>) -> BTreeSet<PathBuf> {
        let mut held: BTreeSet<PathBuf> = self.added.files_on_disk().into_iter().collect();
        held.extend(spoken_for.iter().cloned());
        held
    }
}

/// What's to stay as it was at one moment.
struct HeldNow(BTreeSet<PathBuf>);

impl Keeping for HeldNow {
    fn keeps(&self, file: &Path) -> bool {
        self.0.iter().any(|kept| is_or_becomes(file, kept))
    }

    fn keeps_any_under(&self, folder: &Path) -> bool {
        self.0.iter().any(|kept| kept.starts_with(folder))
    }

    fn while_held(&self, f: &mut dyn FnMut(&dyn Keeping)) {
        f(self);
    }
}

/// Whether `file` is `kept`, or the file it's being written as beside it, which is named for it
/// and starts with a dot, until it's whole and put in place.
fn is_or_becomes(file: &Path, kept: &Path) -> bool {
    if file == kept {
        return true;
    }
    let (Some(kept_name), Some(name)) = (kept.file_name(), file.file_name()) else { return false };
    kept.parent() == file.parent() && name.to_string_lossy().starts_with(&format!(".{}.", kept_name.to_string_lossy()))
}

/// Asked as each file would go, so what's held is what's held then.
impl Keeping for LargeKeep {
    fn keeps(&self, file: &Path) -> bool {
        self.held().iter().any(|kept| is_or_becomes(file, kept))
    }

    fn keeps_any_under(&self, folder: &Path) -> bool {
        self.held().iter().any(|kept| kept.starts_with(folder))
    }

    /// No copy is given room while `f` runs: room is given under the same lock.
    fn while_held(&self, f: &mut dyn FnMut(&dyn Keeping)) {
        let spoken_for = lock(&self.spoken_for);
        f(&HeldNow(self.held_with(&spoken_for)));
    }
}

/// The large files brought back from last time that couldn't be taken into this window's
/// working copy, shared with the working copies' background jobs, which try again.
#[derive(Clone, Default)]
pub(crate) struct LeftInLast {
    paths: Arc<Mutex<BTreeSet<String>>>,
    /// Whether to try again: once, after they're brought back, and again after each save. A
    /// try that fails waits for one of those, as on a disk that can't link it copies the whole
    /// file each time.
    again: Arc<AtomicBool>,
}

impl LeftInLast {
    fn paths(&self) -> std::sync::MutexGuard<'_, BTreeSet<String>> {
        self.paths.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// A file the talk's working copy has room kept for while it's copied in. Let go of, it's
/// taken away, if it was made, and its room is free again.
struct Claim {
    to: PathBuf,
    spoken_for: SpokenFor,
    made: bool,
}

impl Drop for Claim {
    fn drop(&mut self) {
        if self.made {
            std::fs::remove_file(&self.to).ok();
        }
        lock(&self.spoken_for).remove(&self.to);
    }
}

/// A large file to copy into the talk, made on the window's thread and run on any other, as the
/// copy can take a while: see [`Document::add_large_file`].
pub struct LargeCopy {
    source: LargeSource,
    looked: Looked,
    /// Its path in the talk.
    path: String,
    /// What kind of file it is.
    kind: FileKind,
    /// The working copy's folder of files.
    folder: PathBuf,
    /// The files of the talk with as many bytes, and their hashes if they're known.
    candidates: Vec<(String, Option<[u8; 32]>)>,
    files: Arc<dyn TalkFiles + Send + Sync>,
    claim: Claim,
    /// The window's working copies and the talk's place, to make the folder again with, if a
    /// clear took it away meanwhile.
    recovery: Recovery,
    place: TalkPlace,
}

enum Copied {
    /// The talk has it already, at this path.
    There(String),
    /// Copied, with this hash.
    New([u8; 32]),
}

/// What came of a [`LargeCopy`], for [`Document::take_large_copy`] to take in. If it's dropped
/// instead, what it copied is taken away.
pub struct CopiedIn {
    path: String,
    len: u64,
    outcome: Result<Copied, NotAdded>,
    claim: Claim,
}

impl CopiedIn {
    /// Why it wasn't copied, if it wasn't.
    pub fn refused(&self) -> Option<NotAdded> {
        self.outcome.as_ref().err().copied()
    }
}

impl LargeCopy {
    /// How many bytes it copies.
    pub fn len(&self) -> u64 {
        self.looked.len
    }

    /// What kind of file it copies.
    pub fn kind(&self) -> FileKind {
        self.kind
    }

    /// Whether it copies nothing at all.
    pub fn is_empty(&self) -> bool {
        self.looked.len == 0
    }

    /// Copies the file in, a piece at a time, telling `progress` the bytes done and the bytes in
    /// all, and stopping if `stop` is set. If the talk has a file of the same length, the source
    /// is hashed first, and one with the same hash is the file: nothing is copied. Room on the
    /// disk is looked for before anything is copied. Stopped, or failing, nothing is left.
    pub fn run(self, progress: &dyn Fn(u64, u64), stop: &AtomicBool) -> CopiedIn {
        let LargeCopy { source, looked, path, kind, folder, candidates, files, mut claim, recovery, place } = self;
        let hashing = !candidates.is_empty();
        let total = if hashing { looked.len.saturating_mul(2) } else { looked.len };
        let outcome = (|| {
            // A model that doesn't start as one does is refused before a byte of it is copied.
            if kind == FileKind::Model && !large_files::starts_as_model(&source, &path) {
                return Err(NotAdded::NotAModel);
            }
            if hashing {
                let hash = large_files::hash_source(&source, looked, &Told { progress, before: 0, total }, stop)?;
                for (there, known) in &candidates {
                    let theirs = known.or_else(|| large_files::hash_in(&*files, there, stop));
                    if theirs == Some(hash) {
                        return Ok(Copied::There(there.clone()));
                    }
                }
            }
            if stop.load(Ordering::SeqCst) {
                return Err(NotAdded::Cancelled);
            }
            // A clear may have taken the slot's folders away before this had room kept: they're
            // made, and held, again.
            recovery.large_folder(&place).map_err(|_| NotAdded::NowhereToKeep)?;
            if !large_files::has_room(&folder, looked.len) {
                return Err(NotAdded::NoRoom);
            }
            let before = if hashing { looked.len } else { 0 };
            let hash = large_files::copy_to(&source, looked, &claim.to, &Told { progress, before, total }, stop, large_files::CopyFor::WorkingCopy)?;
            claim.made = true;
            Ok(Copied::New(hash))
        })();
        CopiedIn { path, len: looked.len, outcome, claim }
    }
}

/// A file of the talk's to read in pieces, found on the window's thread and opened on any other,
/// as a video that a pack holds compressed is unpacked first.
#[derive(Clone)]
pub struct LargeOpen {
    files: Arc<dyn TalkFiles + Send + Sync>,
    path: String,
    pack: Option<PathBuf>,
    cache: Option<VideoCache>,
    readers: Arc<Readers>,
}

impl LargeOpen {
    /// `path` of the talk's files as `opening` says they're read, if it's one of them.
    fn of(opening: &Opening, path: &str, readers: &Arc<Readers>) -> Option<LargeOpen> {
        let files = opening.files.clone()?;
        let path = slides_engine::normalise(path)?;
        files.len(&path)?;
        Some(LargeOpen { files, path, pack: opening.pack.clone(), cache: opening.cache.clone(), readers: readers.clone() })
    }

    /// The file, to read in pieces, held among the talk's readers until it's dropped, or a save
    /// lets go of it. A file a pack holds compressed is unpacked to the cache, and read there.
    /// `None` while a save replaces the talk's pack, as well as for a file that can't be read.
    pub fn open(self) -> Option<HeldReader> {
        self.open_unless_saving().ok()
    }

    /// [`Self::open`], saying why not.
    pub fn open_unless_saving(self) -> Result<HeldReader, NotOpened> {
        self.open_as(true)
    }

    /// [`Self::open`] for a still: a file a pack holds compressed is read from the cache only if
    /// it's unpacked there already, as it is once the video has been played, and is never
    /// unpacked whole just for its first picture.
    pub fn open_for_still(self) -> Result<HeldReader, NotOpened> {
        self.open_as(false)
    }

    /// What tells this file from another and one version of it from another, while it's open: the
    /// talk's files it's read from, its path in them, and its stamp, if they give one.
    pub(crate) fn identity(&self) -> (usize, String, Option<(u64, std::time::SystemTime)>) {
        (Arc::as_ptr(&self.files) as *const () as usize, self.path.clone(), self.files.stamp(&self.path))
    }

    /// The file read whole, as a terminal recording is: a small data file of the talk, read as any
    /// is, from a pack compressed or not, while held among the talk's readers. Only a byte more than
    /// a recording can be is read of one larger. What can't be read says why, as a recording's
    /// still says it; while a save replaces the pack, it isn't read.
    pub fn read_recording(self) -> Result<Result<Vec<u8>, slides_terminal::CantPlay>, NotOpened> {
        let LargeOpen { files, path, readers, .. } = self;
        let mut read = Err(slides_terminal::CantPlay::CouldNotRead);
        // Held while it's read, and let go of at once: what's read is all that's wanted of it.
        let held = readers.open(|| {
            read = slides_engine::recording_bytes(&*files, &path);
            Ok(Box::new(std::io::Cursor::new(Vec::new())) as Box<dyn slides_engine::ReadSeek>)
        })?;
        drop(held);
        Ok(read)
    }

    /// [`Self::open`] for reading a few headers on the window's thread: only a file read where it
    /// lies, never one from the cache, whose pack's list of files would have to be read first.
    pub(crate) fn open_in_place(self) -> Result<HeldReader, NotOpened> {
        let LargeOpen { files, path, readers, .. } = self;
        readers.open(|| files.open(&path).ok_or(NotOpened::NotUnpacked))
    }

    fn open_as(self, unpacking: bool) -> Result<HeldReader, NotOpened> {
        let LargeOpen { files, path, pack, cache, readers } = self;
        let stop = readers.clone();
        readers.open(|| match files.open(&path) {
            Some(reader) => Ok(reader),
            None => {
                let unreadable = NotOpened::Unreadable;
                let pack = pack.as_ref().filter(|_| is_large_kind(&path)).ok_or(unreadable)?;
                let cache = cache.or_else(VideoCache::app).ok_or(unreadable)?;
                let at = if unpacking { cache.unpacked(pack, &path, stop.saving_now()).ok_or(unreadable)? } else { cache.unpacked_already(pack, &path).ok_or(NotOpened::NotUnpacked)? };
                let file = large_files::open_plain(&at).ok_or(unreadable)?;
                Ok(Box::new(file) as Box<dyn slides_engine::ReadSeek>)
            }
        })
    }
}

/// The talk's files as an export reads them: a 3D model a pack holds compressed is unpacked to
/// the cache to draw its still, as it is to turn it, as a model is never more than 256 MB.
/// Everything else is read as the talk reads it.
struct ModelsUnpacked {
    files: Arc<dyn TalkFiles + Send + Sync>,
    pack: PathBuf,
    cache: Option<VideoCache>,
    readers: Arc<Readers>,
}

impl ModelsUnpacked {
    /// The model at `path`, unpacked from the pack to the cache, if it is one the pack holds.
    fn unpacked(&self, path: &str) -> Option<Box<dyn slides_engine::ReadSeek>> {
        let path = slides_engine::normalise(path).filter(|path| is_large_kind(path))?;
        let ext = path.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase())?;
        if !FileKind::Model.extensions().contains(&ext.as_str()) {
            return None;
        }
        let cache = self.cache.clone().or_else(VideoCache::app)?;
        // An export reads while no save replaces the pack: this stops only if one begins anyway.
        let at = cache.unpacked(&self.pack, &path, self.readers.saving_now())?;
        let file = large_files::open_plain(&at)?;
        Some(Box::new(file))
    }
}

impl TalkFiles for ModelsUnpacked {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        self.files.read(path)
    }

    fn open(&self, path: &str) -> Option<Box<dyn slides_engine::ReadSeek>> {
        self.files.open(path).or_else(|| self.unpacked(path))
    }

    fn read_prefix(&self, path: &str, most: usize) -> Option<Vec<u8>> {
        self.files.read_prefix(path, most)
    }

    fn lies_at(&self, path: &str) -> Option<PathBuf> {
        self.files.lies_at(path)
    }

    fn stamp(&self, path: &str) -> Option<(u64, std::time::SystemTime)> {
        self.files.stamp(path)
    }

    fn len(&self, path: &str) -> Option<u64> {
        self.files.len(path)
    }

    fn names_in(&self, folder: &str) -> Vec<String> {
        self.files.names_in(folder)
    }

    fn never_changes(&self) -> bool {
        self.files.never_changes()
    }
}

impl Document {
    /// The talk's files as an export reads them: in a pack, a 3D model the pack holds compressed
    /// is unpacked to draw its still.
    pub(crate) fn files_for_export(&self) -> Arc<dyn TalkFiles + Send + Sync> {
        match self.pack() {
            Some(pack) => Arc::new(ModelsUnpacked { files: self.files.clone(), pack, cache: self.large.cache.clone(), readers: self.large.readers.clone() }),
            None => self.files.clone(),
        }
    }

    /// Copies large files added to the talk, such as videos, into `recovery`, the window's
    /// working copies, in its slot for the talk: until the talk is saved, that copy is the only
    /// one there is.
    pub fn keep_large_files_in(&mut self, recovery: Recovery) {
        self.large.recovery = Some(recovery);
    }

    /// Unpacks a video a pack holds compressed into `cache`, in place of the app's own.
    pub fn use_video_cache(&mut self, cache: VideoCache) {
        self.large.cache = Some(cache);
        self.follow_files();
    }

    /// Has the deck leave every video's still to be made aside, from the moment the talk is
    /// opened, as a window's thread mustn't decode a video: until something makes them, with
    /// [`Self::make_stills_aside`], a video shows a plain panel. A video's own size is read from
    /// its file, gated as every reader is.
    pub(crate) fn leave_stills_aside(&mut self) {
        self.follow_files();
        let (slot, early) = (self.large.stills_wanted.clone(), self.large.still_wanted_early.clone());
        let wanted: StillWanted = Arc::new(move || match lock_any(&slot).clone() {
            Some(wanted) => wanted(),
            None => early.store(true, Ordering::SeqCst),
        });
        let (opening, readers) = (self.large.opening.clone(), self.large.readers.clone());
        let measure: MeasureVideo = Arc::new(move |path: &str| {
            // Its headers alone, 64 KB at most: this is on the thread laying the slide out.
            let mut reader = LargeOpen::of(&lock_any(&opening), path, &readers)?.open_in_place().ok()?;
            slides_video::video_size(&mut reader).ok()
        });
        self.deck.make_stills_aside(wanted, measure);
        // A model's still is drawn by the app's drawer, on what makes the stills.
        self.deck.draw_models_with(Arc::new(crate::model_view::draw_still));
    }

    /// Notes what the talk's files are read from now, for what reads them off the window's thread.
    pub(crate) fn follow_files(&mut self) {
        *lock_any(&self.large.opening) = Opening { files: Some(self.files.clone()), pack: self.pack(), cache: self.large.cache.clone() };
    }

    /// Makes the talk's videos' stills from now on: `wanted` is told, on the window's thread,
    /// each time one is wanted, and [`Self::still_jobs`] then gives it. It's told now if any was
    /// wanted before.
    pub fn make_stills_aside(&mut self, wanted: StillWanted) {
        *lock_any(&self.large.stills_wanted) = Some(wanted.clone());
        if self.large.still_wanted_early.swap(false, Ordering::SeqCst) {
            wanted();
        }
    }

    /// Whether something makes the talk's videos' stills.
    pub fn stills_made_aside(&self) -> bool {
        lock_any(&self.large.stills_wanted).is_some()
    }

    /// The stills wanted to be made since last asked, each with its video's file to read it from,
    /// on another thread, held among the talk's readers: with [`LargeOpen::open_for_still`].
    /// While a save replaces the pack there are none: they're given once it's done.
    pub fn still_jobs(&mut self) -> Vec<(StillJob, Option<LargeOpen>)> {
        if self.saving_the_pack() {
            return Vec::new();
        }
        let mut jobs = std::mem::take(&mut self.large.stills_waiting);
        jobs.extend(self.deck.still_jobs());
        jobs.into_iter()
            .map(|job| {
                let open = self.open_large(job.path());
                (job, open)
            })
            .collect()
    }

    /// Keeps `job`, a still that couldn't be made yet, to be given again: after a save, or once
    /// a video has been opened to play, which may have unpacked it.
    pub fn still_waits(&mut self, job: StillJob) {
        self.large.stills_waiting.push(job);
    }

    /// How many stills couldn't be made yet, and wait to be given again: for tests.
    #[doc(hidden)]
    pub fn stills_waiting(&self) -> usize {
        self.large.stills_waiting.len()
    }

    /// Gives again the stills that couldn't be made yet, telling what makes them, and lets the
    /// videos' facts that couldn't be read yet be read: whether any had waited is given, for the
    /// form to be drawn again, and read them.
    pub fn try_waiting_stills_again(&mut self) -> bool {
        let facts = self.read_waiting_facts_again();
        if !self.large.stills_waiting.is_empty() {
            self.ask_for_stills();
        }
        facts
    }

    /// What's known of the video at `path`, a file of the talk's, as its file is now: its facts,
    /// or why it can't be played. `None` until they've been read, with
    /// [`Self::video_facts_to_read`].
    pub fn video_facts(&self, path: &str) -> Option<Result<VideoFacts, slides_video::CantPlay>> {
        match self.known_facts(path)? {
            Known::Video(known) => Some(known),
            Known::Model(_) | Known::Recording(_) => None,
        }
    }

    /// What's known of the 3D model at `path`, a file of the talk's, as its file is now: what its
    /// headers say it is, or why it can't be drawn. `None` until they've been read, with
    /// [`Self::model_facts_to_read`].
    pub fn model_facts(&self, path: &str) -> Option<Result<slides_model::ModelFacts, slides_model::CantDraw>> {
        match self.known_facts(path)? {
            Known::Model(known) => Some(known),
            Known::Video(_) | Known::Recording(_) => None,
        }
    }

    /// What's known of the terminal recording at `path`, a file of the talk's, as its file is
    /// now: its screen's size, how long it plays and how many times it stops, or why it can't be
    /// played. `None` until they've been read, with [`Self::recording_facts_to_read`].
    pub fn recording_facts(&self, path: &str) -> Option<Result<slides_terminal::RecordingFacts, slides_terminal::CantPlay>> {
        match self.known_facts(path)? {
            Known::Recording(known) => Some(known),
            Known::Video(_) | Known::Model(_) => None,
        }
    }

    /// What's known of the file at `path`, as it is now.
    fn known_facts(&self, path: &str) -> Option<Known> {
        let key = slides_engine::normalise(path).unwrap_or_else(|| path.to_string());
        match self.large.facts.get(&key)? {
            (version, Facts::Known(known), _) if *version == self.file_stamp(&key) => Some(*known),
            _ => None,
        }
    }

    /// The facts of the video at `path` to be read, on another thread, if they're neither known
    /// for its file as it is now nor being read or waiting to be: they're read once a version.
    pub fn video_facts_to_read(&mut self, path: &str) -> Option<FactsToRead> {
        self.facts_to_read(path, FactsOf::Video)
    }

    /// The facts of the 3D model at `path` to be read, on another thread, from its headers alone,
    /// as [`Self::video_facts_to_read`] gives a video's.
    pub fn model_facts_to_read(&mut self, path: &str) -> Option<FactsToRead> {
        self.facts_to_read(path, FactsOf::Model)
    }

    /// The facts of the terminal recording at `path` to be read, on another thread, from end to
    /// end, keeping none of its output, as [`Self::video_facts_to_read`] gives a video's. It's
    /// read from the talk's files as any of its data files is, unpacked if a pack holds it
    /// compressed.
    pub fn recording_facts_to_read(&mut self, path: &str) -> Option<FactsToRead> {
        self.facts_to_read(path, FactsOf::Recording)
    }

    /// Takes in a recording's facts, read off the window's thread, as [`Self::take_video_facts`]
    /// takes in a video's.
    pub fn take_recording_facts(&mut self, read: FactsRead) -> bool {
        self.take_video_facts(read)
    }

    /// The facts of the file of kind `of` at `path` to be read. Those known of another kind, as
    /// of a file named by a video and a model both, are read again as this kind.
    fn facts_to_read(&mut self, path: &str, of: FactsOf) -> Option<FactsToRead> {
        let key = slides_engine::normalise(path).unwrap_or_else(|| path.to_string());
        let version = self.file_stamp(&key);
        let asked = self.large.facts.get(&key).is_some_and(|(held, facts, _)| match facts {
            Facts::Reading | Facts::Waiting => true,
            Facts::Known(known) => *held == version && known.of() == of,
        });
        if asked {
            return None;
        }
        self.keep_facts(key.clone(), version, Facts::Reading);
        let from = match of {
            FactsOf::Video => ReadFrom::Video(self.open_large(&key)),
            FactsOf::Model => ReadFrom::Model(self.open_large(&key)),
            FactsOf::Recording => ReadFrom::Recording(self.open_large(&key)),
        };
        Some(FactsToRead { path: key, version, from })
    }

    /// Takes in a model's facts, read off the window's thread, as [`Self::take_video_facts`]
    /// takes in a video's.
    pub fn take_model_facts(&mut self, read: FactsRead) -> bool {
        self.take_video_facts(read)
    }

    /// Takes in a video's facts, read off the window's thread, and says whether they're known
    /// now. Those that couldn't be read yet wait for a save to end, or the video to be played.
    pub fn take_video_facts(&mut self, read: FactsRead) -> bool {
        let FactsRead { path, version, read } = read;
        let known = read.is_some();
        let facts = read.map_or(Facts::Waiting, Facts::Known);
        // One let go of meanwhile, to keep within the most kept, is asked for again if wanted.
        if self.large.facts.contains_key(&path) {
            self.keep_facts(path, version, facts);
        }
        known
    }

    /// Keeps `facts` of the video at `path`, read from `version` of its file, letting go of the
    /// one asked for longest ago once there are more than [`MOST_FACTS`].
    fn keep_facts(&mut self, path: String, version: Option<FileStamp>, facts: Facts) {
        self.large.facts_asked += 1;
        self.large.facts.insert(path, (version, facts, self.large.facts_asked));
        while self.large.facts.len() > MOST_FACTS {
            let oldest = self.large.facts.iter().min_by_key(|(_, (_, _, asked))| *asked).map(|(path, _)| path.clone());
            let Some(oldest) = oldest else { break };
            self.large.facts.remove(&oldest);
        }
    }

    /// Lets the videos' facts that waited be read again, and says whether any had.
    fn read_waiting_facts_again(&mut self) -> bool {
        let before = self.large.facts.len();
        self.large.facts.retain(|_, (_, facts, _)| !matches!(facts, Facts::Waiting));
        self.large.facts.len() != before
    }

    /// The length and last change of the talk's file at `path`, found without reading it, or
    /// its length alone.
    fn file_stamp(&self, path: &str) -> Option<FileStamp> {
        self.files.stamp(path).or_else(|| self.files.len(path).map(|len| (len, std::time::SystemTime::UNIX_EPOCH)))
    }

    /// Tells what makes the stills to look for any wanted.
    fn ask_for_stills(&self) {
        let wanted = lock_any(&self.large.stills_wanted).clone();
        if let Some(wanted) = wanted {
            wanted();
        }
    }

    /// The layers wanted drawn aside since last asked: terminal recordings' screens too long to
    /// draw on the window's thread. Each is drawn with [`slides_engine::DrawJob::draw`] on another.
    pub fn draw_jobs(&mut self) -> Vec<slides_engine::DrawJob> {
        self.deck.draw_jobs()
    }

    /// Takes in a layer drawn aside, and says whether a slide shows it, to be drawn again.
    pub fn take_drawn(&mut self, drawn: slides_engine::DrawnLayer) -> bool {
        !self.deck.take_drawn(drawn).is_empty()
    }

    /// Takes in a still made aside, and says whether a slide shows it: it's drawn again when
    /// next asked for.
    pub fn take_still(&mut self, made: MadeStill) -> bool {
        !self.deck.take_still(made).is_empty()
    }

    /// Whether any video's still is still being made aside.
    pub fn stills_coming(&self) -> bool {
        self.deck.stills_coming()
    }

    /// The videos on the current slide, among the layers [`Self::editor_layers`] gives.
    pub fn videos_here(&mut self) -> Vec<VideoLayer> {
        let current = self.current();
        guarded(|| self.deck.videos(current)).unwrap_or_default()
    }

    /// The videos on slide `index`, of those a show goes through, at `moment`, among the layers
    /// [`Self::layers_at`] gives then.
    pub fn videos_at(&mut self, index: usize, moment: slides_engine::Moment) -> Vec<VideoLayer> {
        guarded(|| self.deck.videos_at(index, moment)).unwrap_or_default()
    }

    /// What's said of the one thing selected, if it's a video that can't be played: why not, that
    /// it has no file, or that one in the text plays only once it's placed.
    pub fn selected_video_said(&mut self) -> Option<String> {
        if let Some(video) = self.selected_video() {
            if video.on.src.is_empty() {
                return Some(slides_engine::VIDEO_NO_FILE.to_string());
            }
            return self.video_problem(video.source.as_ref()?);
        }
        let selected = self.selected()?.clone();
        let one = self.selection().is_some_and(|selection| selection.things.len() == 1);
        let tag = self.selected_tag().filter(|_| one && selected.slide == self.current())?;
        (tag.name == "Video" && !slides_engine::is_positioned(tag)).then(|| slides_engine::VIDEO_IN_TEXT.to_string())
    }

    /// The one thing selected on the slide shown, if it's a video.
    pub fn selected_video(&mut self) -> Option<VideoLayer> {
        let selected = self.selected()?.clone();
        if selected.slide != self.current() || self.selection().is_none_or(|selection| selection.things.len() != 1) {
            return None;
        }
        self.videos_here().into_iter().find(|video| video.source.as_ref() == Some(&selected.source))
    }

    /// Why the video at `source` on the current slide can't be played, in a plain sentence, if it
    /// can't.
    pub fn video_problem(&mut self, source: &std::ops::Range<usize>) -> Option<String> {
        let current = self.current();
        guarded(|| self.deck.video_problem(current, source)).ok().flatten()
    }

    /// The 3D models on the current slide, among the layers [`Self::editor_layers`] gives.
    pub fn models_here(&mut self) -> Vec<ModelLayer> {
        let current = self.current();
        guarded(|| self.deck.models(current)).unwrap_or_default()
    }

    /// The 3D models on slide `index`, of those a show goes through, at `moment`, among the
    /// layers [`Self::layers_at`] gives then.
    pub fn models_at(&mut self, index: usize, moment: slides_engine::Moment) -> Vec<ModelLayer> {
        guarded(|| self.deck.models_at(index, moment)).unwrap_or_default()
    }

    /// The one thing selected on the slide shown, if it's a 3D model placed on it.
    pub fn selected_model(&mut self) -> Option<ModelLayer> {
        let selected = self.selected()?.clone();
        if selected.slide != self.current() || self.selection().is_none_or(|selection| selection.things.len() != 1) {
            return None;
        }
        self.models_here().into_iter().find(|model| model.source.as_ref() == Some(&selected.source))
    }

    /// What's said of the one thing selected, if it's a 3D model that can't be drawn: why not,
    /// that it has no file, or that one in the text shows only once it's placed.
    pub fn selected_model_said(&mut self) -> Option<String> {
        if let Some(model) = self.selected_model() {
            let current = self.current();
            let source = model.source?;
            return guarded(|| self.deck.model_problem(current, &source)).ok().flatten();
        }
        let selected = self.selected()?.clone();
        let one = self.selection().is_some_and(|selection| selection.things.len() == 1);
        let tag = self.selected_tag().filter(|_| one && selected.slide == self.current())?;
        (tag.name == "Model" && !slides_engine::is_positioned(tag)).then(|| slides_engine::MODEL_IN_TEXT.to_string())
    }

    /// Why the 3D model at `source` on slide `index` can't be drawn, in a plain sentence, if its
    /// still has been tried and it can't.
    pub fn model_problem_on(&mut self, index: usize, source: &std::ops::Range<usize>) -> Option<String> {
        guarded(|| self.deck.model_problem(index, source)).ok().flatten()
    }

    /// Why the terminal recording at `source` on slide `index` can't be played, in a plain
    /// sentence, if it's been read and can't be, or has no file.
    pub fn terminal_problem_on(&mut self, index: usize, source: &std::ops::Range<usize>) -> Option<String> {
        guarded(|| self.deck.terminal_problem(index, source)).ok().flatten()
    }

    /// Why the command or screen of the terminal at `source` on slide `index` is wrong, if it is:
    /// one that is never starts.
    pub fn terminal_run_problem_on(&mut self, index: usize, source: &std::ops::Range<usize>) -> Option<String> {
        guarded(|| self.deck.terminal_run_problem(index, source)).ok().flatten()
    }

    /// Whether a terminal recording on slide `index` is still being read aside.
    pub fn terminal_pending(&mut self, index: usize) -> bool {
        guarded(|| self.deck.terminal_pending(index)).unwrap_or(false)
    }

    /// The terminal recordings on the current slide, among the layers [`Self::editor_layers`]
    /// gives.
    pub fn terminals_here(&mut self) -> Vec<TerminalLayer> {
        let current = self.current();
        guarded(|| self.deck.terminals(current)).unwrap_or_default()
    }

    /// Every terminal on slide `index` of the talk, with its layer, tag and what it names.
    pub fn terminals_on(&mut self, index: usize) -> Vec<TerminalLayer> {
        guarded(|| self.deck.terminals(index)).unwrap_or_default()
    }

    /// The terminal recordings on slide `index`, of those a show goes through, at `moment`, among
    /// the layers [`Self::layers_at`] gives then.
    pub fn terminals_at(&mut self, index: usize, moment: slides_engine::Moment) -> Vec<TerminalLayer> {
        guarded(|| self.deck.terminals_at(index, moment)).unwrap_or_default()
    }

    /// The one thing selected on the slide shown, if it's a terminal recording placed on it.
    pub fn selected_terminal(&mut self) -> Option<TerminalLayer> {
        let selected = self.selected()?.clone();
        if selected.slide != self.current() || self.selection().is_none_or(|selection| selection.things.len() != 1) {
            return None;
        }
        self.terminals_here().into_iter().find(|terminal| terminal.source.as_ref() == Some(&selected.source))
    }

    /// Whether the terminal recording selected can be played: it's been read, and can be.
    pub fn selected_terminal_plays(&mut self) -> bool {
        let Some((source, src)) = self.selected_terminal().and_then(|terminal| Some((terminal.source?, terminal.on.src))) else { return false };
        // One that only runs a command has no recording to play.
        if src.is_empty() {
            return false;
        }
        let current = self.current();
        !self.terminal_pending(current) && self.terminal_problem_on(current, &source).is_none()
    }

    /// Keeps `bytes`, what a live terminal showed as a recording, as a file of the talk named as like
    /// `stem` as is safe, and sets `src` of the terminal at `source` on the slide shown to it, as one
    /// step named "Change setting". The file goes in as any added file does, so a save writes it only
    /// while the text uses it: undone, the `src` goes and the file isn't saved. Returns its path.
    pub fn keep_terminal_recording(&mut self, source: std::ops::Range<usize>, stem: &str, bytes: Vec<u8>) -> Result<String, InsertFailed> {
        let thing = crate::selection::Selected { slide: self.current(), source };
        let path = self.add_file(FileKind::Recording, stem, bytes).map_err(InsertFailed::File)?;
        self.set_attr_of(&thing, "src", Some(&path)).map_err(InsertFailed::Edit)?;
        Ok(path)
    }

    /// The talk's fonts, to draw with on another thread, as a recording played is.
    pub fn fonts_seed(&mut self) -> slides_engine::FontsSeed {
        self.deck.fonts_mut().seed()
    }

    /// Writes `view` as the starting view of the 3D model at `source` on the current slide, as
    /// one step, with as few figures as a view put on a slide has: only its `view` changes.
    pub fn set_model_view(&mut self, source: std::ops::Range<usize>, view: slides_model::View) -> Result<Vec<usize>, EditRefused> {
        let thing = crate::selection::Selected { slide: self.current(), source };
        self.set_attr_of(&thing, "view", Some(&crate::insert::written_view(view).to_string()))
    }

    /// Adds `view` to the views of the 3D model at `source` on the current slide, after those it
    /// has, as one step: only its `views` changes, and what's written there stays as it was,
    /// with `; ` and the view after it, with as few figures as a view put on a slide has. One
    /// with as many as a model can have takes no more.
    pub fn add_model_view(&mut self, source: std::ops::Range<usize>, view: slides_model::View) -> Result<Vec<usize>, EditRefused> {
        let thing = crate::selection::Selected { slide: self.current(), source };
        let view = crate::insert::written_view(view);
        let written = slides_format::find_tag(self.talk(), thing.slide, &thing.source)
            .ok_or(EditRefused::CouldNotBeMade)?
            .attr("views")
            .map(str::to_string)
            .unwrap_or_default();
        let (views, _) = slides_engine::read_model_views(Some(&written));
        if views.len() >= slides_engine::MOST_MODEL_VIEWS {
            return Err(EditRefused::TooManyViews);
        }
        let kept = written.trim_end();
        let added = match kept.chars().last() {
            None => view.to_string(),
            Some(';' | ',') => format!("{kept} {view}"),
            Some(_) => format!("{kept}; {view}"),
        };
        self.set_attr_of(&thing, "views", Some(&added))
    }

    /// Where the talk's pack is, if it's kept in one.
    fn pack(&self) -> Option<PathBuf> {
        match &self.place {
            TalkPlace::Packed(pack) => Some(pack.clone()),
            TalkPlace::Folder(_) | TalkPlace::Untitled(_) => None,
        }
    }

    /// Adds the file at `at`, outside the talk, to it as a file of `kind`, a video, named as like
    /// `suggested` as is safe, as a picture is, and returns its path in the talk. It's copied a
    /// piece at a time into the window's working copy, hashed as it goes, telling `progress` the
    /// bytes done and the bytes in all and stopping if `stop` is set, and only then becomes one
    /// of the talk's files, lying there, never read whole. The same bytes already in the talk,
    /// found by their length and hash, are that file, and nothing is copied. It's refused if it's
    /// too large, if the disk hasn't room for it, as is seen before copying, or runs out, or if
    /// it changed or went while it was copied; and a copy stopped or refused leaves nothing.
    ///
    /// The window runs this in its parts, on its own thread and on another: see
    /// [`Self::large_copy`].
    pub fn add_large_file(
        &mut self,
        kind: FileKind,
        suggested: &str,
        at: &Path,
        progress: &dyn Fn(u64, u64),
        stop: &AtomicBool,
    ) -> Result<String, NotAdded> {
        let copy = self.large_copy(kind, suggested, LargeSource::File(at.to_path_buf()), None)?;
        let copied = copy.run(progress, stop);
        self.take_large_copy(copied)
    }

    /// The first part of [`Self::add_large_file`], quick, on the window's thread: what's copied
    /// from where, to where, under what name. `expected` is what the file was when it was looked
    /// at before, if it was, which it must still be. The copy is run on any other thread, and
    /// what comes of it is taken in with [`Self::take_large_copy`].
    pub fn large_copy(&self, kind: FileKind, suggested: &str, from: LargeSource, expected: Option<Looked>) -> Result<LargeCopy, NotAdded> {
        if !kind.is_large() {
            return Err(NotAdded::NotThatKind);
        }
        let looked = from.look_as(Some(kind))?;
        if expected.is_some_and(|expected| expected != looked) {
            return Err(NotAdded::Changed);
        }
        let recovery = self.large.recovery.as_ref().ok_or(NotAdded::NowhereToKeep)?;
        let folder = recovery.large_folder(&self.place).map_err(|_| NotAdded::NowhereToKeep)?;
        self.large.copied_in.set(true);
        let mut spoken_for = lock(&self.large.spoken_for);
        let taken = |path: &str| {
            let at = folder.join(path);
            self.on_disk(path) || spoken_for.contains(&at) || std::fs::symlink_metadata(&at).is_ok()
        };
        let path = added_files::large_path(&*self.files, kind, suggested, taken)?;
        let to = folder.join(&path);
        let candidates = added_files::same_length(&*self.files, kind, looked.len)
            .into_iter()
            .map(|there| {
                let known = self.files.lies_at(&there).and_then(|at| self.large.hashes.get(&at).copied());
                (there, known)
            })
            .collect();
        spoken_for.insert(to.clone());
        drop(spoken_for);
        let claim = Claim { to, spoken_for: self.large.spoken_for.clone(), made: false };
        let (recovery, place) = (recovery.clone(), self.place.clone());
        Ok(LargeCopy { source: from, looked, path, kind, folder, candidates, files: self.files.clone(), claim, recovery, place })
    }

    /// The last part of [`Self::add_large_file`], on the window's thread: the file copied becomes
    /// one of the talk's, lying where it was copied to, and its path in the talk is given. Nothing
    /// is written until a save, which writes it if the text uses it then.
    pub fn take_large_copy(&mut self, mut copied: CopiedIn) -> Result<String, NotAdded> {
        match std::mem::replace(&mut copied.outcome, Err(NotAdded::Cancelled)) {
            Err(why) => Err(why),
            Ok(Copied::There(path)) => Ok(path),
            Ok(Copied::New(hash)) => {
                let to = copied.claim.to.clone();
                // Taken away since, which nothing should do: never taken in as if it were there.
                if !std::fs::symlink_metadata(&to).is_ok_and(|meta| meta.is_file() && meta.len() == copied.len) {
                    return Err(NotAdded::Unreadable);
                }
                let free = self.files.len(&copied.path).is_none() && !self.on_disk(&copied.path);
                // Taken since, as by a paste: the copy is let go of with the claim.
                if !free || !self.added.add_at(&copied.path, to.clone(), copied.len) {
                    return Err(NotAdded::NameTaken);
                }
                // The talk's now, and kept as its file is.
                copied.claim.made = false;
                self.pending.insert(copied.path.clone());
                self.large.hashes.insert(to, hash);
                if talk_uses(self.talk(), &*self.files, &copied.path) {
                    self.deck.files_added();
                }
                Ok(copied.path.clone())
            }
        }
    }

    /// Copies `large`, a large file a paste carries by where it lies, into the talk, as
    /// [`Self::add_large_file`] does, and gives its path in this talk.
    pub fn bring_large(&mut self, large: &PastedLarge, progress: &dyn Fn(u64, u64), stop: &AtomicBool) -> Result<String, NotAdded> {
        let copy = self.large_copy_for_paste(large)?;
        let copied = copy.run(progress, stop);
        self.take_large_copy(copied)
    }

    /// The first part of [`Self::bring_large`], as [`Self::large_copy`] is of adding one. What a
    /// clipboard says is trusted no more than a dropped file is: it's copied only from a talk's
    /// own files, or from the working copies the app keeps, and only if it's as long, and was
    /// last changed when, the copy says.
    pub fn large_copy_for_paste(&self, large: &PastedLarge) -> Result<LargeCopy, NotAdded> {
        let source = self.may_bring(large).ok_or(NotAdded::Unreadable)?;
        let name = large.path.split_once('/').map_or(large.path.as_str(), |(_, name)| name);
        let expected = Looked { len: large.len, modified: large.modified };
        self.large_copy(large.kind, name, source, Some(expected))
    }

    /// Where `large`, as a clipboard says, is copied from, if it lies where a large file of a talk
    /// can, under a large file's name, at the path the copy gives it: in a talk's folder, beside its
    /// `slides.md`; in the working copies, in a slot's folder of files; or in a pack, under that
    /// path. A file on disk is copied from where the system resolves it to, links and all.
    fn may_bring(&self, large: &PastedLarge) -> Option<LargeSource> {
        let named = large.kind.is_large() && added_files::kind_of_path(&large.path) == Some(large.kind) && is_large_kind(&large.path);
        if !named {
            return None;
        }
        match &large.source {
            LargeSource::File(at) => {
                let at = at.canonicalize().ok()?;
                let parts = large.path.split('/').count();
                let above = at.ancestors().nth(parts).filter(|_| at.ends_with(&large.path))?;
                let in_working_copies = above.file_name().is_some_and(|name| name == "files")
                    && self.large.recovery.as_ref().is_some_and(|recovery| recovery.holds(&at));
                let in_a_talk = above.join(SLIDES_FILE).is_file();
                (in_working_copies || in_a_talk).then_some(LargeSource::File(at))
            }
            LargeSource::InPack { pack, entry } => {
                let pack = pack.canonicalize().ok()?;
                (*entry == large.path && open_packed(pack.as_path()).is_ok()).then(|| LargeSource::InPack { pack, entry: entry.clone() })
            }
        }
    }

    /// The large files of the talk's that `text`, a slide's own text or a thing's, uses, for a
    /// copy to carry by where they lie: a file of the talk's folder or of the working copy, or a
    /// file the talk's pack holds.
    pub fn large_for_copy(&self, text: &str) -> Vec<CopiedLarge> {
        let Ok(talk) = worked_out(|| parse_talk(text)) else {
            return Vec::new();
        };
        let files = &*self.files;
        added_files::large_folders()
            .into_iter()
            .flat_map(|folder| files.names_in(folder))
            .filter(|path| is_large_kind(path) && crate::clipboard::pasted_kind(path).is_some_and(FileKind::is_large))
            .filter(|path| guarded(|| talk_uses(&talk, files, path)).unwrap_or(false))
            .filter_map(|path| {
                let source = match (files.lies_at(&path), &self.place) {
                    (Some(at), _) => LargeSource::File(std::path::absolute(&at).unwrap_or(at)),
                    (None, TalkPlace::Packed(pack)) => {
                        LargeSource::InPack { pack: std::path::absolute(pack).unwrap_or_else(|_| pack.clone()), entry: path.clone() }
                    }
                    (None, _) => return None,
                };
                let looked = source.look_as(crate::clipboard::pasted_kind(&path)).ok()?;
                Some(CopiedLarge { path, source, looked })
            })
            .collect()
    }

    /// The talk's file at `path`, to read in pieces, wherever it lies: see [`LargeOpen::open`].
    pub fn open_large(&self, path: &str) -> Option<LargeOpen> {
        let opening = Opening { files: Some(self.files.clone()), pack: self.pack(), cache: self.large.cache.clone() };
        LargeOpen::of(&opening, path, &self.large.readers)
    }

    /// A save that replaces the talk's pack begins: nothing of the talk is opened until
    /// [`Self::pack_save_ended`], and every reader held is let go of first. How many there were
    /// is given.
    pub fn begin_saving_the_pack(&self) -> usize {
        self.large.readers.begin_saving()
    }

    /// The save that replaced the pack is done, whatever came of it: the talk's files can be
    /// opened again, and the stills that waited for it are made.
    pub fn pack_save_ended(&mut self) {
        self.large.readers.save_ended();
        self.read_waiting_facts_again();
        // Those wanted meanwhile, as well as those that waited.
        self.ask_for_stills();
    }

    /// The talk's readers, for a save to wait on what's being opened as it begins.
    #[doc(hidden)]
    pub fn readers(&self) -> Arc<Readers> {
        self.large.readers.clone()
    }

    /// How many of the talk's files are being opened now, and how many were refused while a save
    /// ran, for tests.
    #[doc(hidden)]
    pub fn opens_for_test(&self) -> (usize, u64) {
        (self.large.readers.opening(), self.large.readers.refused())
    }

    /// Whether a save replacing the talk's pack is running, when nothing of it is opened.
    pub fn saving_the_pack(&self) -> bool {
        self.large.readers.saving()
    }

    /// Lets go of every reader of the talk's files held: done before a save that replaces the
    /// talk's pack, as a file that's open can't be replaced on some systems, and on others keeps
    /// the disk space of what was replaced. Whatever was reading opens the file again.
    pub fn let_go_of_readers(&self) -> usize {
        self.large.readers.let_go()
    }

    /// Whether a reader was let go of since the talk was last saved: a save in place of its pack
    /// then opens the pack again, so nothing holds the pack it replaced.
    pub(crate) fn readers_were_let_go(&self) -> bool {
        self.large.readers.were_let_go()
    }

    /// The talk was saved.
    pub(crate) fn readers_saved(&self) {
        self.large.readers.saved();
    }

    /// How many readers of the talk's files are held.
    pub fn readers_held(&self) -> usize {
        self.large.readers.count()
    }

    /// Whether the talk's working copy holds, or held, anything of a large file: a closing
    /// window clears it then, whatever else it holds.
    pub fn kept_large_files(&self) -> bool {
        self.large.copied_in.get() || !self.added.files_on_disk().is_empty()
    }

    /// What the working copies keep of their slot when they clear it.
    pub fn large_keep(&self) -> LargeKeep {
        LargeKeep { added: self.added.clone(), spoken_for: self.large.spoken_for.clone(), left_in_last: self.large.left_in_last.clone() }
    }

    /// Takes back `large`, the large files kept with a working copy from last time, by their
    /// path in the talk and where they lie, as added again: each is taken into this window's own
    /// working copy first, linked or else copied, so that it's kept whatever becomes of last
    /// time's. One the talk has already is left as it is. One whose name the talk now gives a
    /// different file is left out, and the window says so, once.
    pub fn bring_back_large(&mut self, large: Vec<(String, PathBuf)>) {
        let mut any = false;
        let mut clashed = false;
        for (path, at) in large {
            let Some(kind) = added_files::kind_of_path(&path).filter(|kind| kind.is_large()) else {
                continue;
            };
            if !is_large_kind(&path) {
                continue;
            }
            let Some(len) =
                std::fs::symlink_metadata(&at).ok().filter(std::fs::Metadata::is_file).map(|meta| meta.len()).filter(|len| *len <= kind.max_bytes())
            else {
                continue;
            };
            if self.added.lies_at(&path).is_some_and(|lies| lies == at) {
                continue;
            }
            if self.files.len(&path).is_some() || self.on_disk(&path) {
                clashed = true;
                continue;
            }
            // If it can't be taken in, it's read where it lies, and last time's slot is kept
            // until it is: see `LargeKeep::take_in_left_in_last`.
            let taken = self.large.recovery.as_ref().and_then(|recovery| recovery.take_in(&self.place, &path, &at).ok());
            let left = taken.is_none();
            if self.added.add_at(&path, taken.unwrap_or(at), len) {
                if left {
                    self.large.left_in_last.paths().insert(path.clone());
                    self.large.left_in_last.again.store(true, Ordering::SeqCst);
                }
                self.pending.insert(path);
                any = true;
            }
        }
        if clashed {
            self.message = Some(added_files::KEPT_FILE_CLASH.to_string());
        }
        if any {
            self.deck.files_added();
            self.components_looked_at();
        }
    }

    /// The talk was saved: a large file brought back that couldn't be taken into this window's
    /// working copy is tried again.
    pub(crate) fn large_saved(&self) {
        if !self.large.left_in_last.paths().is_empty() {
            self.large.left_in_last.again.store(true, Ordering::SeqCst);
        }
    }

    /// Whether the talk holds or adds any large file: what a save of it moves may then take a
    /// while, and the window shows how far it's got.
    pub(crate) fn moves_large(&self) -> bool {
        let added = self.files_to_write().iter().any(|(_, file)| matches!(file, AddedFile::At(..)));
        added || self.holds_large()
    }

    /// Whether the talk's own files, as last saved, hold a large file.
    pub(crate) fn holds_large(&self) -> bool {
        added_files::large_folders().into_iter().any(|folder| self.own_files.names_in(folder).iter().any(|path| is_large_kind(path)))
    }
}
