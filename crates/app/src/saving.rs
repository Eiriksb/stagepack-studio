//! Saving a talk, where it's kept or somewhere new. A [`SaveJob`] is made from the talk on the
//! window's thread and can be run on any other, so that writing never holds the window up.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use slides_engine::{
    FolderFiles, NewFile, PackError, TalkFiles, copy_talk_folder, lies_within, open_packed,
    pack_folder_with_slides, pack_new, read_talk_text, repack_with_slides, unpack_with_slides,
    write_atomically, write_new_atomically, write_new_folder,
};

use slides_components::{BuildEntry, INDEX_FILE};

use crate::added_files::{NotAdded, is_added_path};
use crate::builds::{self, Unreadable};
use crate::document::{SLIDES_FILE, TalkPlace};

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where Save As puts a talk, and in which form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveTarget {
    /// A folder, which must be missing or empty.
    Folder(PathBuf),
    /// A packed `.sptx` file, which is replaced if it's there.
    Packed(PathBuf),
}

/// A save of the talk as it was when the job was made.
pub struct SaveJob {
    text: String,
    /// What the talk's file held when the app last wrote or read it.
    on_disk: String,
    from: TalkPlace,
    to: Option<SaveTarget>,
    /// The files to write besides the text, by their path in the talk: those added to it that the
    /// text uses and aren't saved yet, and for an untitled talk the files it was made with that
    /// the text uses.
    added: Vec<(String, Arc<[u8]>)>,
    /// The builds entered in the talk's index, of builds the text uses, that no save has
    /// written into the index on disk.
    builds: Vec<BuildEntry>,
    /// The talk's build index with `builds` entered in it, for a save to anywhere but its own
    /// folder, which takes it in place of the talk's; `None` if the talk's own can't be read.
    index: Option<Vec<u8>>,
}

/// Why a save didn't write anything.
#[derive(Debug)]
pub enum SaveError {
    /// It couldn't, for the reason given, in words for the person saving.
    Failed(String),
    /// The talk's file has been changed outside the app since it was last read, and holds
    /// this. It's asked about before anything is written over it.
    ChangedOutside(String),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::Failed(problem) => f.write_str(problem),
            SaveError::ChangedOutside(_) => {
                f.write_str("The talk wasn't saved, as it was changed outside the app.")
            }
        }
    }
}

impl std::error::Error for SaveError {}

impl From<String> for SaveError {
    fn from(problem: String) -> SaveError {
        SaveError::Failed(problem)
    }
}

/// What a save of an untitled talk to where it's kept says: it's kept nowhere yet.
pub const NO_PLACE_YET: &str = "This presentation hasn't been saved anywhere yet, so it needs Save As.";

/// What a save says when the pack it saved in place in couldn't be opened again afterwards.
pub const NOT_READ_BACK: &str = "The talk was saved, but couldn't be read back. Close it and open it again to be sure of it.";

/// The talk's files, as a job hands them over from its own thread.
pub(crate) type SharedFiles = Arc<dyn TalkFiles + Send + Sync>;

/// What came of a save that worked, for [`crate::document::Document::saved`] to take in.
pub struct Saved {
    /// The text written.
    pub(crate) written: String,
    /// Where the talk is kept now, and its files there, after a Save As.
    pub(crate) moved_to: Option<(TalkPlace, SharedFiles)>,
    /// The files added to the talk that were written, by their path in it.
    pub(crate) files_written: Vec<String>,
    /// The builds entered in the index of the place the talk is read from from now on, which
    /// no longer need entering.
    pub(crate) builds_written: Vec<BuildEntry>,
    /// The pack the talk was saved in place in, opened again, to read the talk from now on.
    pub(crate) reopened: Option<SharedFiles>,
    /// The build index written into the pack the talk was saved in place in, when the pack
    /// couldn't be opened again: what the pack's index is now, for the next save to start from.
    pub(crate) index_kept_in_pack: Option<Vec<u8>>,
    notices: Vec<String>,
}

impl Saved {
    /// What the save had to leave out, or carries an old build of, one sentence each.
    pub fn notices(&self) -> &[String] {
        &self.notices
    }

    /// Whether the talk is kept somewhere new from now on, after a Save As.
    pub fn moved(&self) -> bool {
        self.moved_to.is_some()
    }
}

impl SaveJob {
    pub(crate) fn new(
        text: String,
        on_disk: String,
        from: TalkPlace,
        to: Option<SaveTarget>,
        added: Vec<(String, Arc<[u8]>)>,
        (builds, index): (Vec<BuildEntry>, Option<Vec<u8>>),
    ) -> SaveJob {
        SaveJob {
            text,
            on_disk,
            from,
            to,
            added,
            builds,
            index,
        }
    }

    /// Writes the talk out, with the files added to it that its text uses, and the builds entered
    /// in its index: merged into the index in its own folder, one write at a time with any build,
    /// or, anywhere else, as the merged index in place of the talk's. If the talk's own index
    /// can't be read, it's left as it is, the builds stay to be entered, and that's said. A
    /// failure says why,
    /// in words for the person saving, and leaves what was there as it was: a folder may have
    /// some of those files in it, which nothing uses until a save writes its text.
    pub fn run(self) -> Result<Saved, SaveError> {
        self.run_reading_back_with(|file| in_pack(file.to_path_buf()).map(|(_, files)| files))
    }

    /// [`Self::run`], opening the pack saved in place in again with `read_back`, as a test can
    /// make fail.
    #[doc(hidden)]
    pub fn run_reading_back_with(
        self,
        read_back: impl FnOnce(&Path) -> Result<Arc<dyn TalkFiles + Send + Sync>, String>,
    ) -> Result<Saved, SaveError> {
        let SaveJob {
            text,
            on_disk,
            from,
            to,
            added,
            builds,
            index,
        } = self;
        let mut builds_written = Vec::new();
        let mut reopened = None;
        let mut index_kept_in_pack = None;
        let files_written: Vec<String> = added.iter().map(|(path, _)| path.clone()).collect();
        let index_kept = |place: &Path| failed(&place.join(INDEX_FILE), "it can't be read, so a pasted component needs building.");
        // The merged index goes with the files, in place of the talk's own.
        let index_written = !builds.is_empty() && index.is_some();
        let extra = || -> Vec<NewFile> {
            let index = index.iter().filter(|_| !builds.is_empty()).map(|index| (INDEX_FILE.to_string(), index.clone()));
            added.iter().map(|(path, bytes)| (path.clone(), bytes.to_vec())).chain(index).collect()
        };
        let index_notice = |place: &Path| (!builds.is_empty() && index.is_none()).then(|| index_kept(place));
        if let (TalkPlace::Folder(folder), Some(SaveTarget::Folder(to) | SaveTarget::Packed(to))) =
            (&from, &to)
            && lies_within(to, folder)
        {
            return Err(failed(to, PackError::InsideTheTalk).into());
        }
        let (moved_to, notices) = match (&from, to) {
            (TalkPlace::Folder(folder), None) => {
                // A file that's gone is written without asking. One that's there but can't be
                // read is left alone: it may be someone else's that the app can't make out.
                let found = match read_talk_text(&folder.join(SLIDES_FILE)) {
                    Ok(found) => Some(found),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                    Err(error) => return Err(failed(folder, unreadable(&error)).into()),
                };
                if let Some(found) = found {
                    changed_outside(found, &on_disk, &text)?;
                }
                // Before the text, so that a talk cut short never names a file it hasn't got.
                for (path, bytes) in &added {
                    write_added(folder, path, bytes)?;
                }
                let entered = builds::merge_into_index(folder, &builds, Unreadable::Keep).map_err(|e| failed(&folder.join(INDEX_FILE), e))?;
                let notices = entered.is_none().then(|| index_kept(folder)).into_iter().collect();
                builds_written = entered.unwrap_or_default();
                write_slides(folder, &text)?;
                (None, notices)
            }
            (TalkPlace::Packed(file), None) => {
                // A pack that can't be read is refused by the repack below.
                if let Ok(found) = open_packed(file).and_then(|packed| packed.slides()) {
                    changed_outside(found, &on_disk, &text)?;
                }
                repack_with_slides(file, file, &text, APP_VERSION, &extra()).map_err(|e| failed(file, e))?;
                // The talk is read from the pack as it's written from now on, so what it holds,
                // files and entries, is the talk's, as after a save in a folder.
                // If it can't be read back, the save has still been made: that's said, the talk
                // goes on being read as it was, and the next save starts from the index written.
                let mut notices: Vec<String> = index_notice(file).into_iter().collect();
                if index_written || !files_written.is_empty() {
                    match read_back(file) {
                        Ok(files) => reopened = Some(files),
                        Err(_) => {
                            notices.push(NOT_READ_BACK.to_string());
                            index_kept_in_pack = index.clone().filter(|_| index_written);
                        }
                    }
                }
                (None, notices)
            }
            (TalkPlace::Folder(folder), Some(SaveTarget::Folder(to))) => {
                let report = copy_talk_folder(folder, &to, &text, &extra()).map_err(|e| failed(&to, e))?;
                let left_out = report.skipped.into_iter().map(|name| format!("{name} was left out"));
                let outside = report
                    .outside_pictures
                    .into_iter()
                    .map(|name| format!("The picture {name} is outside assets/ and wasn't copied"));
                let kept = index_notice(&to);
                (Some(in_folder(to)), left_out.chain(outside).chain(kept).collect())
            }
            (TalkPlace::Folder(folder), Some(SaveTarget::Packed(out))) => {
                let report = pack_folder_with_slides(folder, &out, &text, APP_VERSION, &extra())
                    .map_err(|e| failed(&out, e))?;
                let left_out = report.skipped.into_iter().map(|name| format!("{name} was left out"));
                let outside = report
                    .outside_pictures
                    .into_iter()
                    .map(|name| format!("The picture {name} is outside assets/ and wasn't packed"));
                let behind = builds::behind(folder).into_iter().map(|tag| {
                    format!("{tag} couldn't be built just now, so the packed talk carries its last good build")
                });
                let kept = index_notice(&out);
                let notices = left_out.chain(outside).chain(behind).chain(kept).collect();
                (Some(in_pack(out)?), notices)
            }
            (TalkPlace::Packed(file), Some(SaveTarget::Packed(out))) => {
                repack_with_slides(file, &out, &text, APP_VERSION, &extra()).map_err(|e| failed(&out, e))?;
                let kept = index_notice(&out).into_iter().collect();
                (Some(in_pack(out)?), kept)
            }
            (TalkPlace::Packed(file), Some(SaveTarget::Folder(to))) => {
                unpack_with_slides(file, &to, &text, &extra()).map_err(|e| failed(&to, e))?;
                let kept = index_notice(&to).into_iter().collect();
                (Some(in_folder(to)), kept)
            }
            // Never asked for by the window, which asks where first.
            (TalkPlace::Untitled(_), None) => return Err(SaveError::Failed(NO_PLACE_YET.to_string())),
            // A first save: everything is in memory, so all of it is written, fresh. A failure
            // leaves nothing at the target, and the talk as it was.
            (TalkPlace::Untitled(_), Some(SaveTarget::Folder(to))) => {
                write_new_folder(&to, &text, &extra()).map_err(|e| failed(&to, e))?;
                let kept = index_notice(&to).into_iter().collect();
                (Some(in_folder(to)), kept)
            }
            (TalkPlace::Untitled(_), Some(SaveTarget::Packed(out))) => {
                pack_new(&out, &text, APP_VERSION, &extra()).map_err(|e| failed(&out, e))?;
                let kept = index_notice(&out).into_iter().collect();
                (Some(in_pack(out)?), kept)
            }
        };
        if (moved_to.is_some() || reopened.is_some()) && index_written {
            builds_written = builds;
        }
        Ok(Saved {
            written: text,
            moved_to,
            files_written,
            builds_written,
            reopened,
            index_kept_in_pack,
            notices,
        })
    }
}

/// Refuses the save if `found`, what the talk's file holds, is neither what the app last
/// wrote or read nor what it's about to write. An empty file is an editor halfway through a
/// save, as a reload takes it, and nothing would ever ask about it, so it's written over.
fn changed_outside(found: String, on_disk: &str, text: &str) -> Result<(), SaveError> {
    if !found.is_empty() && found != on_disk && found != text {
        return Err(SaveError::ChangedOutside(found));
    }
    Ok(())
}

/// Why a `slides.md` that's there wasn't written over, in words for the person saving.
fn unreadable(error: &std::io::Error) -> String {
    let why = match error.kind() {
        std::io::ErrorKind::PermissionDenied => "the app isn't allowed to read it".to_string(),
        // The reader's own words, such as that it isn't UTF-8 text or is too big.
        std::io::ErrorKind::InvalidData | std::io::ErrorKind::InvalidInput => error.to_string(),
        _ => "something went wrong reading it".to_string(),
    };
    format!("its {SLIDES_FILE} can't be read, as {why}, so it was left as it is.")
}

/// Writes only the talk's `slides.md` in `folder`, by way of a file beside it.
fn write_slides(folder: &Path, text: &str) -> Result<(), String> {
    let file = folder.join(SLIDES_FILE);
    write_atomically(&file, |out| out.write_all(text.as_bytes())).map_err(|e| failed(&file, e))
}

/// Writes `bytes`, a file added to the talk, at `path` in its `folder`, by way of a file beside
/// it. Nothing is written through a link, and a file that's there is never written over: one
/// with the same bytes is the file, and one with others means the save can't go ahead.
fn write_added(folder: &Path, path: &str, bytes: &[u8]) -> Result<(), String> {
    let Some((place, name)) = path.split_once('/').filter(|_| is_added_path(path)) else {
        return Err(failed(folder, NotAdded::NameTaken));
    };
    let place = folder.join(place);
    match std::fs::symlink_metadata(&place) {
        // Not a link: a link's own kind is asked.
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Err(failed(&place, "something that isn't a folder is where its files go.")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(&place).map_err(|e| failed(&place, e))?
        }
        Err(error) => return Err(failed(&place, error)),
    }
    let target = place.join(name);
    if holds(&target, bytes).map_err(|e| failed(&target, e))? {
        return Ok(());
    }
    match write_new_atomically(&target, bytes) {
        Ok(()) => Ok(()),
        // Put there since it was looked at.
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => match holds(&target, bytes) {
            Ok(true) => Ok(()),
            _ => Err(failed(folder, NotAdded::NameTaken)),
        },
        Err(error) => Err(failed(&target, error)),
    }
}

/// Whether `target` is a file holding `bytes`: `false` if nothing is there, and an error if
/// something else is, which mustn't be written over.
fn holds(target: &Path, bytes: &[u8]) -> Result<bool, NotAdded> {
    use std::io::Read as _;
    let meta = match std::fs::symlink_metadata(target) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err(NotAdded::Unreadable),
    };
    if !meta.is_file() || meta.len() != bytes.len() as u64 {
        return Err(NotAdded::NameTaken);
    }
    let mut there = Vec::new();
    std::fs::File::open(target)
        .and_then(|file| file.take(bytes.len() as u64 + 1).read_to_end(&mut there))
        .map_err(|_| NotAdded::Unreadable)?;
    if there != bytes {
        return Err(NotAdded::NameTaken);
    }
    Ok(true)
}

fn in_folder(folder: PathBuf) -> (TalkPlace, SharedFiles) {
    let files: SharedFiles = Arc::new(FolderFiles::new(folder.clone()));
    (TalkPlace::Folder(folder), files)
}

fn in_pack(file: PathBuf) -> Result<(TalkPlace, SharedFiles), String> {
    let files: SharedFiles = Arc::new(open_packed(&file).map_err(|e| failed(&file, e))?);
    Ok((TalkPlace::Packed(file), files))
}

fn failed(place: &Path, problem: impl std::fmt::Display) -> String {
    let name = place.file_name().map_or_else(
        || place.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    format!("Couldn't save {name}: {problem}")
}
