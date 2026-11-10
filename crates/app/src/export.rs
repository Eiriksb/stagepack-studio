//! Writing a talk out for other programs, and handing a file over to Stagepack.
//!
//! An export builds a deck of its own from [`ExportInput`], so it can run on a background
//! thread and share nothing with the window but the talk's files.

use std::io::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use slides_engine::{Deck, Fonts, TalkFiles, write_atomically};
use slides_pdf::write_pdf;
use slides_pptx::{PptxOutput, write_pptx};

/// The longest a suggested file name's stem gets, in characters. Four bytes each at worst, and
/// file systems stop at 255 bytes with the extension still to come.
const LONGEST_STEM: usize = 60;

/// Everything an export needs, taken from the window's document.
pub struct ExportInput {
    pub source: String,
    pub files: Arc<dyn TalkFiles + Send + Sync>,
    pub talk_fonts: Vec<Arc<Vec<u8>>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportKind {
    PowerPoint,
    Pdf,
}

impl ExportKind {
    /// The extension of the file this kind of export writes.
    pub fn extension(self) -> &'static str {
        match self {
            ExportKind::PowerPoint => "pptx",
            ExportKind::Pdf => "pdf",
        }
    }
}

/// Writes the talk to `out`, replacing whatever is there only once it has all been made, and
/// returns what the user should hear about the slides. This blocks until it's done.
pub fn run_export(input: ExportInput, kind: ExportKind, out: &Path) -> Result<Vec<String>, String> {
    // Nothing in a talk should get this far, but the window is waiting to hear how it went.
    let (bytes, notices) = catch_unwind(AssertUnwindSafe(|| make(input, kind)))
        .unwrap_or_else(|_| Err("the exporter stopped on something in this talk".to_string()))
        .map_err(|problem| format!("Could not export {}: {problem}", out.display()))?;
    write_atomically(out, |file| file.write_all(&bytes))
        .map_err(|error| format!("Could not write {}: {error}", out.display()))?;
    Ok(notices)
}

/// The exported file's bytes, and what the user should hear about the slides.
fn make(input: ExportInput, kind: ExportKind) -> Result<(Vec<u8>, Vec<String>), String> {
    let ExportInput {
        source,
        files,
        talk_fonts,
    } = input;
    let mut fonts = Fonts::with_system_fallback();
    for font in talk_fonts {
        fonts.load_talk_font(font);
    }
    let mut deck = Deck::open(Box::new(files.clone()), fonts, source);
    match kind {
        ExportKind::PowerPoint => {
            let PptxOutput { bytes, notices } = write_pptx(&mut deck, &files)?;
            Ok((bytes, notices))
        }
        ExportKind::Pdf => {
            let bytes = write_pdf(&mut deck)?;
            let notices = (0..deck.slide_count())
                .flat_map(|index| {
                    deck.notices(index)
                        .into_iter()
                        .map(move |notice| format!("Slide {}: {notice}", index + 1))
                })
                .collect();
            Ok((bytes, notices))
        }
    }
}

/// A file name, without an extension, to suggest for a talk called `name`. The name comes from
/// inside the talk, so nothing in it can be allowed to look like a path.
pub fn file_stem_for(name: &str) -> String {
    let replaced: String = name
        .chars()
        .take(LONGEST_STEM)
        .map(|c| {
            if c.is_control() || r#"/\:*?"<>|"#.contains(c) {
                '-'
            } else {
                c
            }
        })
        .collect();
    // A leading dot hides a file, and Windows drops trailing dots and spaces.
    match replaced.trim_matches([' ', '.']) {
        "" => "talk".to_string(),
        stem => stem.to_string(),
    }
}

/// Where Stagepack is installed, if it is. `env` reads an environment variable and `exists`
/// says whether a file is there.
pub fn stagepack_executable(
    env: impl Fn(&str) -> Option<String>,
    exists: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    // A relative setting would run whatever happens to sit at that path under the current folder.
    let folder = |name: &str| {
        env(name)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    };
    let places = if cfg!(windows) {
        vec![folder("LOCALAPPDATA").map(|dir| dir.join("Stagepack").join("stagepack.exe"))]
    } else {
        vec![
            folder("XDG_DATA_HOME").map(|dir| dir.join("stagepack/stagepack")),
            folder("HOME").map(|dir| dir.join(".local/share/stagepack/stagepack")),
        ]
    };
    places.into_iter().flatten().find(|path| exists(path))
}

/// Runs `stagepack open <file>` and leaves it running.
pub fn open_in_stagepack(executable: &Path, file: &Path) -> std::io::Result<()> {
    let mut stagepack = Command::new(executable)
        .arg("open")
        .arg(file)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    // Waited for, or it would stay in the process table until this app exits.
    std::thread::spawn(move || stagepack.wait());
    Ok(())
}
