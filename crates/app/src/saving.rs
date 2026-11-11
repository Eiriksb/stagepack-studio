//! Saving a talk, where it's kept or somewhere new. A [`SaveJob`] is made from the talk on the
//! window's thread and can be run on any other, so that writing never holds the window up.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use slides_engine::{
    FolderFiles, PackError, TalkFiles, copy_talk_folder, lies_within, open_packed,
    pack_folder_with_slides, read_talk_text, repack_with_slides, unpack_with_slides,
    write_atomically,
};

use crate::builds;
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

/// The talk's files, as a job hands them over from its own thread.
pub(crate) type SharedFiles = Arc<dyn TalkFiles + Send + Sync>;

/// What came of a save that worked, for [`crate::document::Document::saved`] to take in.
pub struct Saved {
    /// The text written.
    pub(crate) written: String,
    /// Where the talk is kept now, and its files there, after a Save As.
    pub(crate) moved_to: Option<(TalkPlace, SharedFiles)>,
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
    ) -> SaveJob {
        SaveJob {
            text,
            on_disk,
            from,
            to,
        }
    }

    /// Writes the talk out. A failure says why, in words for the person saving, and leaves
    /// what was there as it was.
    pub fn run(self) -> Result<Saved, SaveError> {
        let SaveJob {
            text,
            on_disk,
            from,
            to,
        } = self;
        if let (TalkPlace::Folder(folder), Some(SaveTarget::Folder(to) | SaveTarget::Packed(to))) =
            (&from, &to)
            && lies_within(to, folder)
        {
            return Err(failed(to, PackError::InsideTheTalk).into());
        }
        let (moved_to, notices) = match (&from, to) {
            (TalkPlace::Folder(folder), None) => {
                // A file that can't be read is written anyway: it may have been deleted. An
                // empty one is an editor halfway through a save, as a reload takes it, and
                // nothing would ever ask about it, so it's written too.
                if let Ok(found) = read_talk_text(&folder.join(SLIDES_FILE))
                    && !found.is_empty()
                    && found != on_disk
                    && found != text
                {
                    return Err(SaveError::ChangedOutside(found));
                }
                write_slides(folder, &text)?;
                (None, Vec::new())
            }
            (TalkPlace::Packed(file), None) => {
                repack_with_slides(file, file, &text, APP_VERSION).map_err(|e| failed(file, e))?;
                (None, Vec::new())
            }
            (TalkPlace::Folder(folder), Some(SaveTarget::Folder(to))) => {
                let left_out = copy_talk_folder(folder, &to, &text).map_err(|e| failed(&to, e))?;
                let notices = left_out.into_iter().map(|name| format!("{name} was left out")).collect();
                (Some(in_folder(to)), notices)
            }
            (TalkPlace::Folder(folder), Some(SaveTarget::Packed(out))) => {
                let report = pack_folder_with_slides(folder, &out, &text, APP_VERSION)
                    .map_err(|e| failed(&out, e))?;
                let left_out = report.skipped.into_iter().map(|name| format!("{name} was left out"));
                let outside = report
                    .outside_pictures
                    .into_iter()
                    .map(|name| format!("The picture {name} is outside assets/ and wasn't packed"));
                let behind = builds::behind(folder).into_iter().map(|tag| {
                    format!("{tag} couldn't be built just now, so the packed talk carries its last good build")
                });
                let notices = left_out.chain(outside).chain(behind).collect();
                (Some(in_pack(out)?), notices)
            }
            (TalkPlace::Packed(file), Some(SaveTarget::Packed(out))) => {
                repack_with_slides(file, &out, &text, APP_VERSION).map_err(|e| failed(&out, e))?;
                (Some(in_pack(out)?), Vec::new())
            }
            (TalkPlace::Packed(file), Some(SaveTarget::Folder(to))) => {
                unpack_with_slides(file, &to, &text).map_err(|e| failed(&to, e))?;
                (Some(in_folder(to)), Vec::new())
            }
        };
        Ok(Saved {
            written: text,
            moved_to,
            notices,
        })
    }
}

/// Writes only the talk's `slides.md` in `folder`, by way of a file beside it.
fn write_slides(folder: &Path, text: &str) -> Result<(), String> {
    let file = folder.join(SLIDES_FILE);
    write_atomically(&file, |out| out.write_all(text.as_bytes())).map_err(|e| failed(&file, e))
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
