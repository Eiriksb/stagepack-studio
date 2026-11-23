//! Opening a talk without a window: from its folder or its packed file, with the fonts it
//! carries, and the 3D drawer's stills.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use slides_engine::{Deck, FolderFiles, TalkFiles, open_packed, read_talk_text};

use crate::document::{fonts_in_folder, fonts_in_pack};
use crate::export::fonts_with;

/// Where a talk is kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TalkPlace {
    Folder(PathBuf),
    Packed(PathBuf),
}

/// A talk opened for a command: its deck, its files and where it is.
pub struct OpenTalk {
    pub deck: Deck,
    pub files: Arc<dyn TalkFiles + Send + Sync>,
    pub place: TalkPlace,
}

/// Opens the talk at `path`, a folder with a `slides.md` or a packed `.sptx` file. The error is
/// one plain sentence naming the path.
pub fn open_talk(path: &Path) -> Result<OpenTalk, String> {
    let shown = path.display();
    if !path.exists() {
        return Err(format!("There's no talk at {shown}."));
    }
    let opened = catch_unwind(AssertUnwindSafe(|| {
        if path.is_dir() {
            open_folder(path)
        } else {
            open_file(path)
        }
    }));
    match opened {
        Ok(opened) => opened.map_err(|problem| one_line(&problem)),
        Err(_) => Err(format!("Something in {shown} stopped it being opened.")),
    }
}

fn open_folder(folder: &Path) -> Result<OpenTalk, String> {
    let shown = folder.display();
    let slides = folder.join("slides.md");
    if !slides.is_file() {
        return Err(format!("{shown} has no slides.md, so it isn't a talk."));
    }
    let source = read_talk_text(&slides).map_err(|error| format!("Could not read slides.md in {shown}: {error}."))?;
    let files = FolderFiles::new(folder.to_path_buf());
    let fonts = fonts_in_folder(folder, &files);
    Ok(made(source, Arc::new(files), fonts, TalkPlace::Folder(folder.to_path_buf())))
}

fn open_file(file: &Path) -> Result<OpenTalk, String> {
    let shown = file.display();
    let packed = open_packed(file).map_err(|error| format!("Could not open {shown} as a packed talk: {error}."))?;
    let source = packed.slides().map_err(|error| format!("Could not read the slides in {shown}: {error}."))?;
    let fonts = fonts_in_pack(&packed);
    Ok(made(source, Arc::new(packed), fonts, TalkPlace::Packed(file.to_path_buf())))
}

fn made(source: String, files: Arc<dyn TalkFiles + Send + Sync>, fonts: Vec<Arc<Vec<u8>>>, place: TalkPlace) -> OpenTalk {
    let mut deck = Deck::open(Box::new(files.clone()), fonts_with(fonts), source);
    // Drawn here and now, on this thread, as each slide is laid out.
    deck.draw_models_with(Arc::new(crate::model_view::draw_still));
    OpenTalk { deck, files, place }
}

/// `text` as one line, with the full stop it ends on kept to one.
fn one_line(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    line.trim_end_matches('.').to_string() + "."
}
