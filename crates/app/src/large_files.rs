//! Large files, such as videos: copied into a window's working copy a piece at a time, hashed as
//! they go, and never read whole into memory; the room a copy needs on the disk; the readers the
//! app holds on a talk's files, which a save that replaces a pack lets go of first; and the cache
//! a compressed video in a pack is unpacked to, to be read in pieces.

use std::cell::Cell;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::SystemTime;

use sha2::{Digest as _, Sha256};
use slides_engine::{PackError, ReadSeek, TalkFiles, open_packed, write_new_atomically_with};

use crate::added_files::{FileKind, MAX_VIDEO_BYTES, NotAdded};

/// A paste that would copy more than this into the talk asks first.
pub const ASK_BEFORE_COPYING: u64 = 256 * 1024 * 1024;
/// The most the cache of videos unpacked from packs holds.
pub const CACHE_LIMIT: u64 = 8 * 1024 * 1024 * 1024;
/// What a copy leaves free on the disk besides what it copies.
const ROOM_TO_SPARE: u64 = 16 * 1024 * 1024;
/// Bytes are copied and read this many at a time at most.
const PIECE: usize = 1024 * 1024;
/// What the status line says once every byte is copied, while the file is made safe on the disk
/// and put in place.
pub const FINISHING: &str = "Finishing…";
/// What a reader that was let go of says when it's read.
const LET_GO: &str = "the file was let go of, as the talk was being saved";

static ASK_OVER: AtomicU64 = AtomicU64::new(ASK_BEFORE_COPYING);

/// How much a paste copies before it asks.
pub fn ask_before_copying() -> u64 {
    ASK_OVER.load(Ordering::Relaxed)
}

/// Has a paste ask before copying more than `bytes`, in place of [`ASK_BEFORE_COPYING`]. For
/// tests, whose files are smaller.
#[doc(hidden)]
pub fn set_ask_before_copying_for_test(bytes: u64) {
    ASK_OVER.store(bytes, Ordering::Relaxed);
}

/// The room tests say the disk has, by the folder they say it of.
static FREE_FOR_TEST: Mutex<Vec<(PathBuf, u64)>> = Mutex::new(Vec::new());

/// Takes the disk under `folder` to have `bytes` free, whatever it has. For tests.
#[doc(hidden)]
pub fn set_free_space_for_test(folder: &Path, bytes: u64) {
    let mut free = FREE_FOR_TEST.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    free.retain(|(at, _)| at != folder);
    free.push((folder.to_path_buf(), bytes));
}

/// Folders whose disk tests say fills up once a copy into them has written so many bytes.
static FULL_AFTER_FOR_TEST: Mutex<Vec<(PathBuf, u64)>> = Mutex::new(Vec::new());

/// Has a copy into `folder` find the disk full once it has written `bytes`, as a full disk does:
/// for tests, which have no small disk to fill.
#[doc(hidden)]
pub fn set_full_after_for_test(folder: &Path, bytes: Option<u64>) {
    let mut full = FULL_AFTER_FOR_TEST.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    full.retain(|(at, _)| at != folder);
    if let Some(bytes) = bytes {
        full.push((folder.to_path_buf(), bytes));
    }
}

/// How many bytes a copy to `to` can write before tests say the disk is full.
fn full_after(to: &Path) -> Option<u64> {
    FULL_AFTER_FOR_TEST.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).iter().find(|(at, _)| to.starts_with(at)).map(|(_, bytes)| *bytes)
}

/// A writer that finds the disk full after so many bytes, for [`set_full_after_for_test`].
struct FillsUp<'a> {
    out: &'a mut dyn Write,
    left: Option<u64>,
}

impl Write for FillsUp<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Some(left) = self.left {
            if left < buf.len() as u64 {
                return Err(io::Error::new(io::ErrorKind::StorageFull, "the disk is full"));
            }
            self.left = Some(left - buf.len() as u64);
        }
        self.out.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

/// How many bytes can be written under `folder`, or `None` if the system won't say.
pub(crate) fn free_space(folder: &Path) -> Option<u64> {
    let said = FREE_FOR_TEST.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).iter().find(|(at, _)| folder.starts_with(at)).map(|(_, bytes)| *bytes);
    if said.is_some() {
        return said;
    }
    // The nearest folder that's there, as the file's folder may not be made yet.
    let mut at = folder;
    loop {
        if at.exists() {
            return system_free(at);
        }
        at = at.parent()?;
    }
}

/// Whether there's room under `folder` for `bytes` more, with some to spare. A disk that won't
/// say is taken to have room, and a copy that runs out says so then.
pub(crate) fn has_room(folder: &Path, bytes: u64) -> bool {
    free_space(folder).is_none_or(|free| free >= bytes.saturating_add(ROOM_TO_SPARE))
}

#[cfg(unix)]
fn system_free(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt as _;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: `statvfs` only writes into the struct it's given, which is plain data.
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `path` is a valid string ending in a NUL, and `stats` lives for the call.
    let done = unsafe { libc::statvfs(path.as_ptr(), &mut stats) };
    #[allow(clippy::unnecessary_cast, reason = "the fields' types differ between systems")]
    (done == 0).then(|| (stats.f_bavail as u64).saturating_mul(stats.f_frsize as u64))
}

#[cfg(windows)]
fn system_free(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt as _;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetDiskFreeSpaceExW(directory: *const u16, free_to_caller: *mut u64, total: *mut u64, free: *mut u64) -> i32;
    }
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free = 0u64;
    // SAFETY: `wide` ends in a NUL, and the one count asked for lives for the call.
    let done = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
    (done != 0).then_some(free)
}

#[cfg(not(any(unix, windows)))]
fn system_free(_path: &Path) -> Option<u64> {
    None
}

/// What the status line says of work that has done `done` bytes of `total`: *Copying video…
/// 40%*, and [`FINISHING`] once every byte is done.
pub fn progress_words(doing: &str, done: u64, total: u64) -> String {
    if total > 0 && done >= total {
        return FINISHING.to_string();
    }
    let percent = (u128::from(done) * 100).checked_div(u128::from(total)).unwrap_or(0);
    format!("{doing}… {percent}%")
}

/// What a paste asks before it copies `count` videos of `bytes` in all into the talk.
pub fn ask_to_copy(count: usize, bytes: u64) -> String {
    ask_to_copy_of(count, bytes, "video")
}

/// What a paste asks before it copies `count` files, called `noun`, `bytes` in all, into the talk.
pub fn ask_to_copy_of(count: usize, bytes: u64, noun: &str) -> String {
    let megabytes = bytes.saturating_add(500_000) / 1_000_000;
    let files = if count == 1 { format!("1 {noun}") } else { format!("{count} {noun}s") };
    format!("Copy {files}, {} MB, into this presentation?", megabytes.max(1))
}

/// What files of `kinds` are called, in one word: their kind's if they're all of one, otherwise a
/// plain "file".
pub fn noun_of_kinds(kinds: impl IntoIterator<Item = FileKind>) -> &'static str {
    let mut kinds = kinds.into_iter();
    match kinds.next() {
        Some(first) if kinds.all(|kind| kind == first) => first.noun(),
        _ => "file",
    }
}

/// Where the bytes of a large file to copy come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LargeSource {
    /// A file on disk.
    File(PathBuf),
    /// A file in a packed talk, by its path in the talk.
    InPack { pack: PathBuf, entry: String },
}

/// What a large file was found to be when it was looked at: how long, and when it last changed,
/// for telling whether it's the same file later without reading it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Looked {
    pub len: u64,
    pub modified: Option<SystemTime>,
}

impl LargeSource {
    /// Looks at it, without reading it: a regular file, or a file a pack holds, of no more than
    /// [`MAX_VIDEO_BYTES`]. For a file in a pack, it's when the pack last changed.
    pub(crate) fn look(&self) -> Result<Looked, NotAdded> {
        self.look_as(None)
    }

    /// [`Self::look`], where the file is to be a file of `kind`, which sets how large it can be.
    pub(crate) fn look_as(&self, kind: Option<FileKind>) -> Result<Looked, NotAdded> {
        let looked = match self {
            LargeSource::File(path) => {
                // Asked first, as opening a named pipe waits until something writes to it.
                let metadata = std::fs::metadata(path).map_err(|_| NotAdded::Unreadable)?;
                if !metadata.is_file() {
                    return Err(NotAdded::Unreadable);
                }
                Looked { len: metadata.len(), modified: metadata.modified().ok() }
            }
            LargeSource::InPack { pack, entry } => {
                let metadata = std::fs::metadata(pack).map_err(|_| NotAdded::Unreadable)?;
                let len = open_packed(pack).ok().and_then(|files| files.len(entry)).ok_or(NotAdded::Unreadable)?;
                Looked { len, modified: metadata.modified().ok() }
            }
        };
        let kind = kind.unwrap_or(FileKind::Video);
        if looked.len > kind.max_bytes().min(MAX_VIDEO_BYTES) {
            return Err(NotAdded::too_large(kind));
        }
        Ok(looked)
    }

    /// Whether it's as it was when it was `looked` at: it's there, as long, and last changed then.
    fn unchanged(&self, looked: Looked) -> bool {
        let at = match self {
            LargeSource::File(path) | LargeSource::InPack { pack: path, .. } => path,
        };
        let Ok(metadata) = std::fs::metadata(at) else { return false };
        let len_same = match self {
            LargeSource::File(_) => metadata.len() == looked.len,
            // The entry's own length is held to the pack's when it's copied.
            LargeSource::InPack { .. } => true,
        };
        len_same && metadata.modified().ok() == looked.modified
    }
}

/// `file`, opened to read without waiting on a pipe, if it's a regular file.
fn open_regular(path: &Path) -> Option<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path).ok()?;
    file.metadata().ok()?.is_file().then_some(file)
}

/// Whether the file `source` names, to be the talk's file at `path`, starts as a 3D model of the
/// kind its extension says. Only its first few bytes are read. A file that can't be opened, or a
/// compressed one in a pack, isn't judged here: the copy finds it changed, or the model's reader
/// refuses it.
pub(crate) fn starts_as_model(source: &LargeSource, path: &str) -> bool {
    let Some((_, ext)) = path.rsplit_once('.') else { return false };
    let mut head = [0u8; crate::added_files::MODEL_HEAD_BYTES];
    let read = match source {
        LargeSource::File(from) => open_regular(from).map(|file| read_head(file, &mut head)),
        LargeSource::InPack { pack, entry } => open_packed(pack).ok().and_then(|files| files.open(entry)).map(|from| read_head(from, &mut head)),
    };
    read.is_none_or(|got| crate::added_files::model_head_fits(ext, &head[..got]))
}

/// Reads up to the length of `head` from the start of `from`, and says how much there was.
fn read_head(mut from: impl Read, head: &mut [u8]) -> usize {
    let mut got = 0;
    while got < head.len() {
        match from.read(&mut head[got..]) {
            Ok(0) | Err(_) => break,
            Ok(n) => got += n,
        }
    }
    got
}

/// The plain file at `path`, opened to read, unless it's a link or anything but a file. Where the
/// system allows, a link put there after it's looked at isn't followed either.
pub(crate) fn open_plain(path: &Path) -> Option<std::fs::File> {
    if !std::fs::symlink_metadata(path).ok()?.is_file() {
        return None;
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path).ok()?;
    file.metadata().ok()?.is_file().then_some(file)
}

/// How far a copy has got, told as it goes, out of a whole that may be bigger than this one copy.
pub(crate) struct Told<'a> {
    pub(crate) progress: &'a dyn Fn(u64, u64),
    pub(crate) before: u64,
    pub(crate) total: u64,
}

impl Told<'_> {
    fn tell(&self, done: u64) {
        (self.progress)(self.before.saturating_add(done), self.total);
    }
}

/// A writer that hashes what it writes.
struct Hashing<'a> {
    out: &'a mut dyn Write,
    hash: Sha256,
}

impl Write for Hashing<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.out.write(buf)?;
        self.hash.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

/// Why a copy didn't finish, as it's said, for the error a writer passes along.
#[derive(Debug)]
struct Refused(NotAdded);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for Refused {}

fn refused(why: NotAdded) -> PackError {
    PackError::Io(io::Error::other(Refused(why)))
}

/// What `error`, from a copy, is said as.
fn said_of(error: &PackError) -> NotAdded {
    if error.is_changed_while_copied() {
        return NotAdded::Changed;
    }
    match error {
        PackError::Stopped => NotAdded::Cancelled,
        PackError::Io(error) => {
            if let Some(Refused(why)) = error.get_ref().and_then(|inner| inner.downcast_ref::<Refused>()) {
                return *why;
            }
            match error.kind() {
                io::ErrorKind::StorageFull => NotAdded::RanOutOfRoom,
                io::ErrorKind::AlreadyExists => NotAdded::NameTaken,
                #[cfg(unix)]
                _ if error.raw_os_error() == Some(libc::ENOSPC) => NotAdded::RanOutOfRoom,
                _ => NotAdded::Unreadable,
            }
        }
        // Damage in a pack it's copied from.
        _ => NotAdded::Unreadable,
    }
}

/// Copies what `from` holds, which is to be `len` bytes, to `to` a piece at a time, telling `told`
/// as it goes and stopping if `stop` is set. More or less than `len` is a file that changed.
fn copy_pieces(from: &mut dyn Read, to: &mut dyn Write, len: u64, told: &Told<'_>, stop: &AtomicBool) -> Result<(), PackError> {
    let mut piece = vec![0u8; PIECE];
    let mut copied = 0u64;
    loop {
        if stop.load(Ordering::SeqCst) {
            return Err(PackError::Stopped);
        }
        // One more than is left, to find one that holds more.
        let want = (len - copied).saturating_add(1).min(PIECE as u64) as usize;
        let n = match from.read(&mut piece[..want]) {
            Ok(n) => n,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            // Gone or cut short reads as less, which is a change; this is the disk failing.
            Err(_) => return Err(refused(NotAdded::Unreadable)),
        };
        if n == 0 {
            break;
        }
        copied += n as u64;
        if copied > len {
            return Err(refused(NotAdded::Changed));
        }
        to.write_all(&piece[..n])?;
        told.tell(copied);
    }
    if copied != len {
        return Err(refused(NotAdded::Changed));
    }
    Ok(())
}

/// Who a file copied in is for: the working copy, where it's someone's unsaved work and theirs
/// alone, or the talk itself, where it's like any other file a save writes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CopyFor {
    WorkingCopy,
    Talk,
}

/// Copies `source`, as it was when `looked` at, into the new file `to`, a piece at a time, and
/// gives the SHA-256 of what was copied. `to` is written beside and put in place only once it's
/// whole and safe on the disk, so a copy cut short, by a stop, a crash or a full disk, never
/// leaves a file there. Whatever is at `to` already is never replaced. If the source changes or
/// goes while it's copied, nothing is left either.
pub(crate) fn copy_to(
    source: &LargeSource,
    looked: Looked,
    to: &Path,
    told: &Told<'_>,
    stop: &AtomicBool,
    copy_for: CopyFor,
) -> Result<[u8; 32], NotAdded> {
    let hash = Cell::new([0u8; 32]);
    let written = write_new_atomically_with(to, |file| {
        // Someone's unsaved work: theirs alone.
        #[cfg(unix)]
        if copy_for == CopyFor::WorkingCopy {
            use std::os::unix::fs::PermissionsExt as _;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        #[cfg(not(unix))]
        let _ = copy_for;
        let mut fills_up = FillsUp { out: file, left: full_after(to) };
        let mut out = Hashing { out: &mut fills_up, hash: Sha256::new() };
        match source {
            LargeSource::File(path) => {
                let mut from = open_regular(path).ok_or_else(|| refused(NotAdded::Changed))?;
                copy_pieces(&mut from, &mut out, looked.len, told, stop)?;
            }
            LargeSource::InPack { pack, entry } => {
                let files = open_packed(pack).map_err(|_| refused(NotAdded::Changed))?;
                if !source.unchanged(looked) || files.len(entry) != Some(looked.len) {
                    return Err(refused(NotAdded::Changed));
                }
                match files.open(entry) {
                    Some(mut from) => copy_pieces(&mut from, &mut out, looked.len, told, stop)?,
                    None => files.copy_entry(entry, &mut out, &|done, _| told.tell(done), stop)?,
                }
            }
        }
        // Gone, or changed in place, since it was looked at: it isn't the file that was meant.
        if !source.unchanged(looked) {
            return Err(refused(NotAdded::Changed));
        }
        hash.set(out.hash.finalize().into());
        Ok(())
    });
    written.map_err(|error| said_of(&error))?;
    Ok(hash.get())
}

/// The SHA-256 of what `from` holds, read a piece at a time, telling `told` as it goes, or `None`
/// if it can't be read or `stop` is set.
pub(crate) fn hash_of(mut from: impl Read, told: &Told<'_>, stop: &AtomicBool) -> Option<[u8; 32]> {
    let mut hash = Sha256::new();
    let mut piece = vec![0u8; PIECE];
    let mut done = 0u64;
    loop {
        if stop.load(Ordering::SeqCst) {
            return None;
        }
        let n = match from.read(&mut piece) {
            Ok(0) => break,
            Ok(n) => n,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
        };
        hash.update(&piece[..n]);
        done += n as u64;
        told.tell(done);
    }
    Some(hash.finalize().into())
}

/// The SHA-256 of `source`, as it was when `looked` at, read a piece at a time.
pub(crate) fn hash_source(source: &LargeSource, looked: Looked, told: &Told<'_>, stop: &AtomicBool) -> Result<[u8; 32], NotAdded> {
    let hash = match source {
        LargeSource::File(path) => hash_of(open_regular(path).ok_or(NotAdded::Changed)?.take(looked.len.saturating_add(1)), told, stop),
        LargeSource::InPack { pack, entry } => {
            let files = open_packed(pack).map_err(|_| NotAdded::Changed)?;
            match files.open(entry) {
                Some(from) => hash_of(from, told, stop),
                None => {
                    let mut out = Hashing { out: &mut io::sink(), hash: Sha256::new() };
                    files.copy_entry(entry, &mut out, &|done, _| told.tell(done), stop).map_err(|error| said_of(&error))?;
                    Some(out.hash.finalize().into())
                }
            }
        }
    };
    if stop.load(Ordering::SeqCst) {
        return Err(NotAdded::Cancelled);
    }
    let hash = hash.ok_or(NotAdded::Changed)?;
    if !source.unchanged(looked) {
        return Err(NotAdded::Changed);
    }
    Ok(hash)
}

/// The SHA-256 of the talk's file `path` in `files`, read a piece at a time.
pub(crate) fn hash_in(files: &dyn TalkFiles, path: &str, stop: &AtomicBool) -> Option<[u8; 32]> {
    let quiet = |_: u64, _: u64| {};
    hash_of(files.open(path)?, &Told { progress: &quiet, before: 0, total: 0 }, stop)
}

/// A reader, until it's let go of, and whether a save let go of it. The flag is set first, so a
/// read going on as the save begins lets go of the reader itself as soon as it's done.
struct Slot {
    reader: Mutex<Option<Box<dyn ReadSeek>>>,
    let_go: Arc<AtomicBool>,
}

impl Slot {
    fn reader(&self) -> std::sync::MutexGuard<'_, Option<Box<dyn ReadSeek>>> {
        self.reader.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

type Held = Arc<Slot>;

/// Why a talk's file wasn't opened to be read in pieces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotOpened {
    /// A save is replacing the talk's pack: it's opened once that's done.
    Saving,
    /// It isn't there, or couldn't be read.
    Unreadable,
    /// A pack holds it compressed, and it hasn't been unpacked to play yet.
    NotUnpacked,
}

#[derive(Default)]
struct Gate {
    /// Whether a save replacing the talk's pack is running, when nothing may open it.
    saving: bool,
    /// How many files are being opened, which a save waits for before it lets go of them.
    opening: usize,
}

/// The readers the app holds on a talk's files, such as a video's while it plays, so that a save
/// that replaces the pack they read can let go of them first: on some systems a file that's open
/// can't be replaced, and on others it keeps the disk space of the file it replaced. While such a
/// save runs, nothing more is opened.
#[derive(Default)]
pub struct Readers {
    held: Mutex<Vec<Weak<Slot>>>,
    /// Whether any was let go of since the talk was last saved.
    let_go: AtomicBool,
    /// Those let go of while they were being read, which a save waits for before it replaces
    /// the pack.
    letting_go: Mutex<Vec<Weak<Slot>>>,
    gate: Mutex<Gate>,
    /// Told when the save is done, or no file is being opened any more.
    changed: std::sync::Condvar,
    /// Set while a save replaces the pack, for what reads the talk a piece at a time to stop.
    saving_now: Arc<AtomicBool>,
    /// How many files were refused as a save ran, for tests.
    refused: AtomicU64,
    /// What a test runs each time a file has been opened, before it's held.
    opened_hook: Mutex<Option<OpenedHook>>,
}

type OpenedHook = Box<dyn Fn() + Send + Sync>;

fn lock_hook(hook: &Mutex<Option<OpenedHook>>) -> std::sync::MutexGuard<'_, Option<OpenedHook>> {
    hook.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Readers {
    fn held(&self) -> std::sync::MutexGuard<'_, Vec<Weak<Slot>>> {
        self.held.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn gate(&self) -> std::sync::MutexGuard<'_, Gate> {
        self.gate.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Opens a reader with `open`, held among these until it's dropped or let go of, unless a
    /// save replacing the pack is running, or begins while it's being opened: then nothing is
    /// held. `open` can look at [`Self::saving_now`] between the pieces of a long open, and stop.
    pub(crate) fn open(&self, open: impl FnOnce() -> Result<Box<dyn ReadSeek>, NotOpened>) -> Result<HeldReader, NotOpened> {
        {
            let mut gate = self.gate();
            if gate.saving {
                self.refused.fetch_add(1, Ordering::SeqCst);
                return Err(NotOpened::Saving);
            }
            gate.opening += 1;
        }
        let opened = open();
        if let Some(hook) = lock_hook(&self.opened_hook).as_ref() {
            hook();
        }
        // Held, or refused, before it stops counting as being opened: a save beginning now finds
        // it held and lets go of it, or waits for this, and never misses it.
        let mut gate = self.gate();
        let held = if gate.saving {
            self.refused.fetch_add(1, Ordering::SeqCst);
            // The file is let go of at once, unread, and closed before the save waiting for it
            // goes on: closing may take a while, so not with the gate held.
            drop(gate);
            drop(opened);
            gate = self.gate();
            Err(NotOpened::Saving)
        } else {
            opened.map(|reader| self.hold(reader))
        };
        gate.opening -= 1;
        drop(gate);
        self.changed.notify_all();
        held
    }

    /// [`Self::open`], for tests.
    #[doc(hidden)]
    pub fn open_for_test(&self, open: impl FnOnce() -> Result<Box<dyn ReadSeek>, NotOpened>) -> Result<HeldReader, NotOpened> {
        self.open(open)
    }

    /// Has `hook` run each time a file has been opened, before it's held, for tests.
    #[doc(hidden)]
    pub fn when_opened_for_test(&self, hook: impl Fn() + Send + Sync + 'static) {
        *lock_hook(&self.opened_hook) = Some(Box::new(hook));
    }

    /// Runs `work`, which reads the talk's files, once no save replacing the pack is running, and
    /// keeps such a save from replacing it until `work` is done: for an export. It waits, so it's
    /// for another thread than the window's.
    pub fn while_reading<T>(&self, work: impl FnOnce() -> T) -> T {
        {
            let mut gate = self.gate();
            while gate.saving {
                gate = self.changed.wait(gate).unwrap_or_else(|poisoned| poisoned.into_inner());
            }
            gate.opening += 1;
        }
        let done = work();
        self.gate().opening -= 1;
        self.changed.notify_all();
        done
    }

    /// Set while a save replaces the pack: something reading the talk a piece at a time stops.
    pub(crate) fn saving_now(&self) -> &AtomicBool {
        &self.saving_now
    }

    /// `reader`, held among these until it's dropped or let go of.
    pub(crate) fn hold(&self, reader: Box<dyn ReadSeek>) -> HeldReader {
        let held: Held = Arc::new(Slot { reader: Mutex::new(Some(reader)), let_go: Arc::default() });
        let mut all = self.held();
        all.retain(|reader| reader.strong_count() > 0);
        all.push(Arc::downgrade(&held));
        HeldReader(held)
    }

    /// Lets go of every reader held: each closes what it reads, and reads nothing more. How many
    /// there were is given. It never waits: one being read now, which on a share that's gone may
    /// take for ever, lets go of itself once that read is done, and a save waits for that in its
    /// own work, with [`Self::wait_for_opens`].
    pub(crate) fn let_go(&self) -> usize {
        let all = std::mem::take(&mut *self.held());
        let mut let_go = 0;
        for slot in all.iter().filter_map(Weak::upgrade) {
            if slot.let_go.swap(true, Ordering::SeqCst) {
                continue;
            }
            let_go += 1;
            match slot.reader.try_lock() {
                Ok(mut reader) => drop(reader.take()),
                Err(std::sync::TryLockError::Poisoned(poisoned)) => drop(poisoned.into_inner().take()),
                Err(std::sync::TryLockError::WouldBlock) => self.letting_go().push(Arc::downgrade(&slot)),
            }
        }
        if let_go > 0 {
            self.let_go.store(true, Ordering::SeqCst);
        }
        let_go
    }

    /// A save that replaces the talk's pack begins: nothing more is opened until
    /// [`Self::save_ended`], what's being opened is waited for, and then every reader held is let
    /// go of. How many there were is given.
    pub(crate) fn begin_saving(&self) -> usize {
        self.gate().saving = true;
        self.saving_now.store(true, Ordering::SeqCst);
        self.let_go()
    }

    fn letting_go(&self) -> std::sync::MutexGuard<'_, Vec<Weak<Slot>>> {
        self.letting_go.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Waits until nothing is being opened, and every reader let go of while it was being read
    /// is closed: what was being opened as the save began is let go of as soon as it is. For the
    /// save's own work, before it replaces the pack, never for the window's thread.
    /// Stops waiting if `stop`, the save's own Cancel, is set.
    pub(crate) fn wait_for_opens(&self, stop: &AtomicBool) {
        let mut gate = self.gate();
        while gate.opening > 0 && !stop.load(Ordering::SeqCst) {
            gate = self.changed.wait_timeout(gate, std::time::Duration::from_millis(50)).unwrap_or_else(|poisoned| poisoned.into_inner()).0;
        }
        drop(gate);
        loop {
            // Each closed here if its read is done and it hasn't closed itself yet.
            self.letting_go().retain(|slot| {
                let Some(slot) = slot.upgrade() else { return false };
                match slot.reader.try_lock() {
                    Ok(mut reader) => drop(reader.take()),
                    Err(std::sync::TryLockError::Poisoned(poisoned)) => drop(poisoned.into_inner().take()),
                    Err(std::sync::TryLockError::WouldBlock) => return true,
                }
                false
            });
            if self.letting_go().is_empty() || stop.load(Ordering::SeqCst) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// [`Self::begin_saving`], for tests.
    #[doc(hidden)]
    pub fn begin_saving_for_test(&self) -> usize {
        self.begin_saving()
    }

    /// [`Self::wait_for_opens`], never stopped, for tests.
    #[doc(hidden)]
    pub fn wait_for_opens_for_test(&self) {
        self.wait_for_opens(&AtomicBool::new(false));
    }

    /// How many files are being opened now, for tests.
    pub(crate) fn opening(&self) -> usize {
        self.gate().opening
    }

    /// How many files were refused as a save ran, for tests.
    pub(crate) fn refused(&self) -> u64 {
        self.refused.load(Ordering::SeqCst)
    }

    /// The save that replaces the pack is done, whatever came of it: files can be opened again.
    pub(crate) fn save_ended(&self) {
        self.gate().saving = false;
        self.saving_now.store(false, Ordering::SeqCst);
        self.changed.notify_all();
    }

    /// Whether a save replacing the pack is running, when nothing is opened.
    pub(crate) fn saving(&self) -> bool {
        self.gate().saving
    }

    /// Whether any reader was let go of since [`Self::saved`].
    pub(crate) fn were_let_go(&self) -> bool {
        self.let_go.load(Ordering::SeqCst)
    }

    /// The talk was saved: what's let go of from now on is for the next save.
    pub(crate) fn saved(&self) {
        self.let_go.store(false, Ordering::SeqCst);
    }

    /// How many readers are held, and not let go of.
    pub(crate) fn count(&self) -> usize {
        self.held().iter().filter_map(Weak::upgrade).filter(|slot| !slot.let_go.load(Ordering::SeqCst)).count()
    }

    /// `reader`, held among these, for tests.
    #[doc(hidden)]
    pub fn hold_for_test(&self, reader: Box<dyn ReadSeek>) -> HeldReader {
        self.hold(reader)
    }
}

/// A reader of a talk's file, held among the talk's [`Readers`]: once they're let go of, it reads
/// nothing more, and whatever has it opens the file again.
pub struct HeldReader(Held);

impl HeldReader {
    /// Whether it was let go of.
    pub fn is_let_go(&self) -> bool {
        self.0.let_go.load(Ordering::SeqCst)
    }

    /// Something that tells whether a save let go of this reader, once it's been handed on.
    pub fn watch(&self) -> ReaderWatch {
        ReaderWatch(self.0.let_go.clone())
    }

    /// Runs `f` on the reader, unless it was let go of. One let go of while `f` ran, as a save
    /// began, is closed here once it's done, as the window's thread never waits for a read.
    fn with<T>(&mut self, f: impl FnOnce(&mut dyn ReadSeek) -> io::Result<T>) -> io::Result<T> {
        let mut reader = self.0.reader();
        if self.0.let_go.load(Ordering::SeqCst) {
            drop(reader.take());
            return Err(io::Error::other(LET_GO));
        }
        let Some(open) = &mut *reader else { return Err(io::Error::other(LET_GO)) };
        let done = f(&mut **open);
        if self.0.let_go.load(Ordering::SeqCst) {
            drop(reader.take());
        }
        done
    }
}

/// Tells whether a save let go of a [`HeldReader`] handed on. A reader dropped, as when what read
/// it failed, wasn't let go of: only a save lets go.
pub struct ReaderWatch(Arc<AtomicBool>);

impl ReaderWatch {
    /// Whether a save let go of it.
    pub fn is_let_go(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

impl Read for HeldReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.with(|reader| reader.read(buf))
    }
}

impl Seek for HeldReader {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        self.with(|reader| reader.seek(to))
    }
}

/// Where videos a pack holds compressed are unpacked to, to be read in pieces: such a pack is
/// made by another program, as this app stores a video as it is. It holds [`CACHE_LIMIT`] at
/// most, the files used longest ago going first.
#[derive(Clone, Debug)]
pub struct VideoCache {
    folder: PathBuf,
    limit: u64,
}

impl VideoCache {
    /// A cache in `folder`, holding `limit` bytes at most.
    pub fn new(folder: PathBuf, limit: u64) -> VideoCache {
        VideoCache { folder, limit }
    }

    /// The app's own, in the system's cache folder, if it has one.
    pub fn app() -> Option<VideoCache> {
        Some(VideoCache::new(dirs::cache_dir()?.join("stagepack-studio").join("videos"), CACHE_LIMIT))
    }

    /// The file the pack at `pack` holds as `entry`, unpacked here, a piece at a time, if it
    /// isn't already. The file is named for the pack, as it is now, and the entry, so a pack
    /// that changes is unpacked again.
    pub(crate) fn unpacked(&self, pack: &Path, entry: &str, stop: &AtomicBool) -> Option<PathBuf> {
        let (at, files, len) = self.place_for(pack, entry)?;
        if std::fs::symlink_metadata(&at).is_ok_and(|there| there.is_file() && there.len() == len) {
            // Used now: it goes after those used longer ago.
            let mut options = std::fs::OpenOptions::new();
            options.write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt as _;
                options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
            }
            if let Ok(file) = options.open(&at) {
                file.set_modified(SystemTime::now()).ok();
            }
            return Some(at);
        }
        make_private_folder(&self.folder).ok()?;
        if !has_room(&self.folder, len) {
            return None;
        }
        let written = write_new_atomically_with(&at, |file| files.copy_entry(entry, file, &|_, _| {}, stop));
        match written {
            Ok(()) => {}
            // Unpacked by another window meanwhile: taken only if it's a plain file, as long.
            Err(PackError::Io(error)) if error.kind() == io::ErrorKind::AlreadyExists => {
                if !std::fs::symlink_metadata(&at).is_ok_and(|there| there.is_file() && there.len() == len) {
                    return None;
                }
            }
            Err(_) => return None,
        }
        self.trim(&at);
        Some(at)
    }

    /// The file the pack at `pack` holds as `entry`, if it's unpacked here already: nothing is
    /// unpacked to find out.
    pub(crate) fn unpacked_already(&self, pack: &Path, entry: &str) -> Option<PathBuf> {
        let (at, _, len) = self.place_for(pack, entry)?;
        std::fs::symlink_metadata(&at).is_ok_and(|there| there.is_file() && there.len() == len).then_some(at)
    }

    /// Where the pack at `pack`'s `entry` is unpacked to, named for the pack as it is now and the
    /// entry, with the pack opened and the entry's length.
    fn place_for(&self, pack: &Path, entry: &str) -> Option<(PathBuf, slides_engine::PackedFiles, u64)> {
        let metadata = std::fs::metadata(pack).ok()?;
        let mut key = Sha256::new();
        key.update(pack.canonicalize().ok()?.as_os_str().as_encoded_bytes());
        key.update(metadata.len().to_le_bytes());
        let since = metadata.modified().ok()?.duration_since(SystemTime::UNIX_EPOCH).ok()?;
        key.update(since.as_nanos().to_le_bytes());
        key.update(entry.as_bytes());
        let key: String = key.finalize().iter().take(16).map(|byte| format!("{byte:02x}")).collect();
        let ext = entry.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase()).filter(|ext| ext.bytes().all(|b| b.is_ascii_alphanumeric()))?;
        let files = open_packed(pack).ok()?;
        let len = files.len(entry)?;
        Some((self.folder.join(format!("{key}.{ext}")), files, len))
    }

    /// Takes away the files used longest ago, but `keep`, until what's here is within the limit.
    fn trim(&self, keep: &Path) {
        let Ok(entries) = std::fs::read_dir(&self.folder) else { return };
        let mut held: Vec<(SystemTime, PathBuf, u64)> = entries
            .flatten()
            .filter(|entry| entry.file_name().to_str().is_some_and(|name| !name.starts_with('.')))
            .filter_map(|entry| {
                let metadata = std::fs::symlink_metadata(entry.path()).ok().filter(std::fs::Metadata::is_file)?;
                Some((metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH), entry.path(), metadata.len()))
            })
            .collect();
        held.sort();
        let mut total: u64 = held.iter().map(|(_, _, len)| len).sum();
        for (_, path, len) in held {
            if total <= self.limit {
                break;
            }
            // One being read can't be taken away on some systems: it stays, and goes later.
            if path != keep && std::fs::remove_file(&path).is_ok() {
                total -= len;
            }
        }
    }
}

/// Makes `folder`, and the folders it's in, readable by the user alone where it's made.
fn make_private_folder(folder: &Path) -> io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder.create(folder)
}
