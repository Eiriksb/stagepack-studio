//! An open talk, and which of its slides is showing.

use std::path::Path;
use std::sync::Arc;

use slides_engine::{Deck, FolderFiles, Fonts, Layer, TalkFiles};

pub(crate) const SLIDES_FILE: &str = "slides.md";
const FONTS_FOLDER: &str = "assets/fonts";

/// What came of reading the talk's file again.
pub enum Reload {
    /// The talk took the new text, or a picture it shows was replaced. These are the
    /// slides to show again.
    Changed(Vec<usize>),
    /// The file says what it said before, and its pictures are as they were.
    Unchanged,
    /// The read gave nothing to use, so the talk stays as it was.
    KeptLastGood,
}

pub struct Document {
    deck: Deck,
    folder_name: String,
    current: usize,
}

impl Document {
    /// Opens the talk kept in `folder`, along with any fonts it carries.
    pub fn open(folder: &Path, mut fonts: Fonts) -> Result<Document, String> {
        let source = std::fs::read_to_string(folder.join(SLIDES_FILE)).map_err(|error| {
            format!(
                "Could not read {SLIDES_FILE} in {}: {error}",
                folder.display()
            )
        })?;
        let files = FolderFiles::new(folder.to_path_buf());
        load_talk_fonts(folder, &files, &mut fonts);
        Ok(Document {
            deck: Deck::open(Box::new(files), fonts, source),
            folder_name: folder_name(folder),
            current: 0,
        })
    }

    pub fn current(&self) -> usize {
        self.current
    }

    pub fn slide_count(&self) -> usize {
        self.deck.slide_count()
    }

    pub fn next(&mut self) {
        if self.current + 1 < self.slide_count() {
            self.current += 1;
        }
    }

    pub fn previous(&mut self) {
        self.current = self.current.saturating_sub(1);
    }

    /// The talk's name and where you are in it.
    pub fn title(&self) -> String {
        let name = self
            .deck
            .title()
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| self.folder_name.clone());
        format!("{name} · {} / {}", self.current + 1, self.slide_count())
    }

    /// The current slide's layers, first to last, drawn at `scale`.
    pub fn layers(&mut self, scale: f32) -> Vec<Arc<Layer>> {
        self.deck.layers(self.current, scale)
    }

    /// What the user should hear about the current slide.
    pub fn notices(&mut self) -> Vec<String> {
        self.deck.notices(self.current)
    }

    pub fn source(&self) -> &str {
        self.deck.source()
    }

    /// Takes a new read of the talk's file: its text, or `None` if it couldn't be read. An
    /// empty read is an editor caught halfway through a save, not an empty talk.
    pub fn reload(&mut self, contents: Option<String>) -> Reload {
        let Some(source) = contents.filter(|source| !source.is_empty()) else {
            return Reload::KeptLastGood;
        };
        let same_text = source == self.source();
        // Even the same text goes through: a picture it shows may have been replaced.
        let mut changed = self.deck.update(source);
        if same_text && changed.is_empty() {
            return Reload::Unchanged;
        }
        let last = self.slide_count() - 1;
        if self.current > last {
            self.current = last;
            // Its text may be as it was, but this is now the slide to show.
            if !changed.contains(&last) {
                changed.push(last);
            }
        }
        Reload::Changed(changed)
    }
}

/// The folder's own name, even for a path like `talk/..`.
fn folder_name(folder: &Path) -> String {
    folder
        .canonicalize()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| folder.display().to_string())
}

fn is_font_file(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("ttf") || extension.eq_ignore_ascii_case("otf")
        })
}

/// Loads the talk's fonts in name order, as the order can decide between two similar fonts.
/// They're read through `files`, which stops a link from reaching outside the talk.
fn load_talk_fonts(folder: &Path, files: &FolderFiles, fonts: &mut Fonts) {
    let Ok(entries) = std::fs::read_dir(folder.join(FONTS_FOLDER)) else {
        return;
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| is_font_file(name))
        .collect();
    names.sort();
    for name in names {
        if let Some(data) = files.read(&format!("{FONTS_FOLDER}/{name}")) {
            fonts.load_talk_font(data);
        }
    }
}
