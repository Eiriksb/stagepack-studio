//! Large files added to the talk, such as videos: copied into the window's working copy as
//! files, a piece at a time, hashed as they go, held there by their path until a save writes
//! them, read in pieces from wherever they lie, and carried by where they lie when copied.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use slides_engine::{AddedFile, MadeStill, StillJob, StillWanted, VideoLayer, is_large_kind};

use super::*;
use crate::clipboard::{CopiedLarge, PastedLarge};
use crate::large_files::{self, HeldReader, LargeSource, Looked, Readers, Told, VideoCache};
use crate::recovery::{Keeping, Recovery};

/// The files in the working copy that a copy is being made to, or has been made to and not yet
/// taken in, which a slot that's cleared keeps.
type SpokenFor = Arc<Mutex<BTreeSet<PathBuf>>>;

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
    /// What's told when a video's still is wanted, once stills are made aside.
    stills_wanted: Option<StillWanted>,
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

    /// Whether it copies nothing at all.
    pub fn is_empty(&self) -> bool {
        self.looked.len == 0
    }

    /// Copies the file in, a piece at a time, telling `progress` the bytes done and the bytes in
    /// all, and stopping if `stop` is set. If the talk has a file of the same length, the source
    /// is hashed first, and one with the same hash is the file: nothing is copied. Room on the
    /// disk is looked for before anything is copied. Stopped, or failing, nothing is left.
    pub fn run(self, progress: &dyn Fn(u64, u64), stop: &AtomicBool) -> CopiedIn {
        let LargeCopy { source, looked, path, folder, candidates, files, mut claim, recovery, place } = self;
        let hashing = !candidates.is_empty();
        let total = if hashing { looked.len.saturating_mul(2) } else { looked.len };
        let outcome = (|| {
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
pub struct LargeOpen {
    files: Arc<dyn TalkFiles + Send + Sync>,
    path: String,
    pack: Option<PathBuf>,
    cache: Option<VideoCache>,
    readers: Arc<Readers>,
}

impl LargeOpen {
    /// The file, to read in pieces, held among the talk's readers until it's dropped, or a save
    /// lets go of it. A file a pack holds compressed is unpacked to the cache, and read there.
    pub fn open(self) -> Option<HeldReader> {
        let reader = match self.files.open(&self.path) {
            Some(reader) => reader,
            None => {
                let pack = self.pack.as_ref().filter(|_| is_large_kind(&self.path))?;
                let cache = self.cache.or_else(VideoCache::app)?;
                let at = cache.unpacked(pack, &self.path)?;
                let file = large_files::open_plain(&at)?;
                Box::new(file) as Box<dyn slides_engine::ReadSeek>
            }
        };
        Some(self.readers.hold(reader))
    }
}

impl Document {
    /// Copies large files added to the talk, such as videos, into `recovery`, the window's
    /// working copies, in its slot for the talk: until the talk is saved, that copy is the only
    /// one there is.
    pub fn keep_large_files_in(&mut self, recovery: Recovery) {
        self.large.recovery = Some(recovery);
    }

    /// Unpacks a video a pack holds compressed into `cache`, in place of the app's own.
    pub fn use_video_cache(&mut self, cache: VideoCache) {
        self.large.cache = Some(cache);
        self.open_stills_aside();
    }

    /// Leaves the stills of the talk's videos to be made aside, as a window's thread mustn't
    /// decode a video: `wanted` is told, on the window's thread, each time one is wanted, and
    /// [`Self::still_jobs`] then gives it.
    pub fn make_stills_aside(&mut self, wanted: StillWanted) {
        self.large.stills_wanted = Some(wanted);
        self.open_stills_aside();
    }

    /// Tells the deck stills are made aside, if they are: after the talk's files are new too.
    pub(crate) fn open_stills_aside(&mut self) {
        if let Some(wanted) = self.large.stills_wanted.clone() {
            self.deck.make_stills_aside(wanted);
        }
    }

    /// The stills wanted to be made aside since last asked, each with its video's file to read
    /// it from, held among the talk's readers while it's read, on another thread.
    pub fn still_jobs(&mut self) -> Vec<(StillJob, Option<LargeOpen>)> {
        let jobs = self.deck.still_jobs();
        jobs.into_iter()
            .map(|job| {
                let open = self.open_large(job.path());
                (job, open)
            })
            .collect()
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
        let looked = from.look()?;
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
        Ok(LargeCopy { source: from, looked, path, folder, candidates, files: self.files.clone(), claim, recovery, place })
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
    /// can, under a video's name, at the path the copy gives it: in a talk's folder, beside its
    /// `slides.md`; in the working copies, in a slot's folder of files; or in a pack, under that
    /// path. A file on disk is copied from where the system resolves it to, links and all.
    fn may_bring(&self, large: &PastedLarge) -> Option<LargeSource> {
        let named = large.kind.is_large() && added_files::is_added_path(&large.path) && is_large_kind(&large.path);
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
        files
            .names_in(FileKind::Video.folder())
            .into_iter()
            .filter(|path| is_large_kind(path) && crate::clipboard::pasted_kind(path) == Some(FileKind::Video))
            .filter(|path| guarded(|| talk_uses(&talk, files, path)).unwrap_or(false))
            .filter_map(|path| {
                let source = match (files.lies_at(&path), &self.place) {
                    (Some(at), _) => LargeSource::File(std::path::absolute(&at).unwrap_or(at)),
                    (None, TalkPlace::Packed(pack)) => {
                        LargeSource::InPack { pack: std::path::absolute(pack).unwrap_or_else(|_| pack.clone()), entry: path.clone() }
                    }
                    (None, _) => return None,
                };
                let looked = source.look().ok()?;
                Some(CopiedLarge { path, source, looked })
            })
            .collect()
    }

    /// The talk's file at `path`, to read in pieces, wherever it lies: see [`LargeOpen::open`].
    pub fn open_large(&self, path: &str) -> Option<LargeOpen> {
        let path = slides_engine::normalise(path)?;
        self.files.len(&path)?;
        let pack = self.pack();
        Some(LargeOpen { files: self.files.clone(), path, pack, cache: self.large.cache.clone(), readers: self.large.readers.clone() })
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
            if !added_files::is_added_path(&path) || !is_large_kind(&path) {
                continue;
            }
            let Some(len) = std::fs::symlink_metadata(&at).ok().filter(std::fs::Metadata::is_file).map(|meta| meta.len()) else {
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
        added || self.own_files.names_in(FileKind::Video.folder()).iter().any(|path| is_large_kind(path))
    }
}
