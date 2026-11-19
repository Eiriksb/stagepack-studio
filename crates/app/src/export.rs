//! Writing a talk out for other programs, and handing a file over to Stagepack.
//!
//! An export builds a deck of its own from [`ExportInput`], so it can run on a background
//! thread and share nothing with the window but the talk's files. Its 3D models are drawn as
//! stills there, by the app's drawer.

use std::io::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use slides_engine::{Deck, Fonts, TalkFiles, write_atomically};
use slides_format::Block;
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
    run_export_reading_with(input, kind, out, |make| make())
}

/// [`run_export`], reading the talk inside `reading`, as the talk's readers keep a save from
/// replacing its files while they're read: the output is written after.
pub fn run_export_reading_with(
    input: ExportInput,
    kind: ExportKind,
    out: &Path,
    reading: impl FnOnce(&mut dyn FnMut() -> Result<(Vec<u8>, Vec<String>), String>) -> Result<(Vec<u8>, Vec<String>), String>,
) -> Result<Vec<String>, String> {
    let mut input = Some(input);
    // Nothing in a talk should get this far, but the window is waiting to hear how it went.
    let mut make_it = || match input.take() {
        Some(input) => catch_unwind(AssertUnwindSafe(|| make(input, kind)))
            .unwrap_or_else(|_| Err("the exporter stopped on something in this talk".to_string())),
        None => Err("the exporter stopped on something in this talk".to_string()),
    };
    let (bytes, notices) = reading(&mut make_it).map_err(|problem| format!("Could not export {}: {problem}", out.display()))?;
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
    // Drawn here and now, on this thread, as each slide is laid out.
    deck.draw_models_with(Arc::new(crate::model_view::draw_still));
    match kind {
        ExportKind::PowerPoint => {
            let PptxOutput { bytes, mut notices } = write_pptx(&mut deck, &files)?;
            let every: Vec<usize> = (0..deck.slide_count()).collect();
            notices.extend(videos_as_pictures(videos_on(&mut deck, &every)));
            notices.extend(models_as_pictures(models_on(&mut deck, &every)));
            Ok((bytes, notices))
        }
        ExportKind::Pdf => {
            let bytes = write_pdf(&mut deck)?;
            let shown = deck.shown_slides().to_vec();
            let videos = videos_on(&mut deck, &shown);
            let models = models_on(&mut deck, &shown);
            let mut notices: Vec<String> = shown
                .into_iter()
                .flat_map(|index| {
                    deck.notices_whole(index)
                        .into_iter()
                        .map(move |notice| format!("Slide {}: {notice}", index + 1))
                })
                .collect();
            notices.extend(videos_as_pictures(videos));
            notices.extend(models_as_pictures(models));
            Ok((bytes, notices))
        }
    }
}

/// How many videos slides `slides` of `deck`'s talk hold, placed or in the text, that were drawn
/// as a picture, their first or the still they name: one that can't be played is its plain panel,
/// and says so in its own notice. Asked once the slides are laid out.
fn videos_on(deck: &mut Deck, slides: &[usize]) -> usize {
    fn videos(blocks: &[Block], found: &mut Vec<(String, Option<String>)>) {
        for block in blocks {
            if let Block::Tag(tag) = block {
                if tag.name == "Video" {
                    found.push((tag.attr("src").unwrap_or_default().to_string(), tag.attr("poster").map(str::to_string)));
                }
                videos(&tag.children, found);
            }
        }
    }
    let mut found = Vec::new();
    for slide in slides.iter().filter_map(|index| deck.talk().slides.get(*index)) {
        videos(&slide.blocks, &mut found);
    }
    found.into_iter().filter(|(src, poster)| deck.video_shows_a_picture(src, poster.as_deref())).count()
}

/// What an export says of the `videos` it holds, which PowerPoint and PDF get as their stills:
/// nothing, if there are none.
fn videos_as_pictures(videos: usize) -> Option<String> {
    match videos {
        0 => None,
        1 => Some("1 video was exported as a picture.".to_string()),
        videos => Some(format!("{videos} videos were exported as pictures.")),
    }
}

/// How many 3D models placed on slides `slides` of `deck`'s talk were drawn as a picture, their
/// still: one that can't be drawn is its plain panel, and says so in its own notice, and one in
/// the text is a panel saying it's drawn once it's placed. Asked once the slides are laid out.
fn models_on(deck: &mut Deck, slides: &[usize]) -> usize {
    let mut drawn = 0;
    for &index in slides {
        for model in deck.models(index) {
            let Some(source) = model.source else { continue };
            if deck.model_problem(index, &source).is_none() {
                drawn += 1;
            }
        }
    }
    drawn
}

/// What an export says of the `models` it holds, which PowerPoint and PDF get as their stills:
/// nothing, if there are none.
fn models_as_pictures(models: usize) -> Option<String> {
    match models {
        0 => None,
        1 => Some("1 model was exported as a picture.".to_string()),
        models => Some(format!("{models} models were exported as pictures.")),
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
