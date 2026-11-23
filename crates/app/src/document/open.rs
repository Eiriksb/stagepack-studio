//! Opening a talk from its folder or its packed file, with the fonts it carries.

use super::*;

/// Where a talk is kept: what saving it works from. It can be handed to a background task, as a
/// [`Document`] can't.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TalkPlace {
    Folder(PathBuf),
    Packed(PathBuf),
    /// Nowhere yet: a new presentation, held in memory until its first save gives it a place.
    Untitled(UntitledId),
}

/// What tells one untitled talk from another, wherever it's kept meanwhile: random, and written
/// as 32 hex digits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UntitledId(pub u128);

impl UntitledId {
    /// An id no other untitled talk has.
    pub fn random() -> UntitledId {
        use std::hash::{BuildHasher as _, Hasher as _};
        use std::sync::atomic::{AtomicU64, Ordering};
        // Each hasher is keyed afresh; the count and the time keep two made at once apart.
        static MADE: AtomicU64 = AtomicU64::new(0);
        let half = |salt: u64| {
            let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
            hasher.write_u64(salt);
            hasher.write_u64(MADE.fetch_add(1, Ordering::Relaxed));
            hasher.write_u32(std::process::id());
            if let Ok(since) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
                hasher.write_u128(since.as_nanos());
            }
            hasher.finish()
        };
        UntitledId((u128::from(half(1)) << 64) | u128::from(half(2)))
    }
}

impl std::fmt::Display for UntitledId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}

/// What an untitled talk is called until it's saved.
pub const UNTITLED_NAME: &str = "Untitled presentation";

impl TalkPlace {
    /// The folder this talk sits in, where anything saved from it starts out. An untitled talk
    /// sits in none, so it's the Documents folder, or else the home folder, or else the current
    /// one.
    pub fn directory(&self) -> PathBuf {
        match self {
            TalkPlace::Folder(path) | TalkPlace::Packed(path) => path.parent().unwrap_or(path).to_path_buf(),
            TalkPlace::Untitled(_) => documents_folder(),
        }
    }

    /// What to call a talk with no title setting.
    pub fn name(&self) -> String {
        match self {
            TalkPlace::Folder(folder) => folder_name(folder),
            TalkPlace::Packed(file) => file.file_stem().map_or_else(
                || file.display().to_string(),
                |stem| stem.to_string_lossy().into_owned(),
            ),
            TalkPlace::Untitled(_) => UNTITLED_NAME.to_string(),
        }
    }
}

/// The person's Documents folder, or else their home folder, or else the current one: where a
/// talk with no place of its own is first saved.
fn documents_folder() -> PathBuf {
    dirs::document_dir()
        .filter(|folder| folder.is_dir())
        .or_else(|| dirs::home_dir().filter(|folder| folder.is_dir()))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

impl Document {
    /// Opens the talk kept in `folder`, along with any fonts it carries.
    pub fn open(folder: &Path, fonts: Fonts) -> Result<Document, String> {
        let source = read_talk_text(&folder.join(SLIDES_FILE)).map_err(|error| {
            format!(
                "Could not read {SLIDES_FILE} in {}: {error}",
                folder.display()
            )
        })?;
        let files = FolderFiles::new(folder.to_path_buf());
        let talk_fonts = read_talk_fonts(font_names_in_folder(folder), &files);
        Document::new(
            TalkPlace::Folder(folder.to_path_buf()),
            Arc::new(files),
            talk_fonts,
            source,
            fonts,
        )
        .map_err(|_| {
            format!(
                "Could not read {SLIDES_FILE} in {}: something in it can't be read.",
                folder.display()
            )
        })
    }

    /// Opens the packed talk in the file at `path`, along with any fonts it carries.
    pub fn open_packed(path: &Path, fonts: Fonts) -> Result<Document, OpenProblem> {
        if !path.exists() {
            return Err(OpenProblem::Other(format!(
                "There's no talk at {}.",
                path.display()
            )));
        }
        let packed = open_packed(path)?;
        let source = packed.slides()?;
        let names = packed
            .names()
            .into_iter()
            .filter(|name| is_talk_font(name))
            .collect();
        let talk_fonts = read_talk_fonts(names, &packed);
        Document::new(
            TalkPlace::Packed(path.to_path_buf()),
            Arc::new(packed),
            talk_fonts,
            source,
            fonts,
        )
        .map_err(|_| {
            OpenProblem::Other(format!(
                "Could not read the talk in {}: something in it can't be read.",
                path.display()
            ))
        })
    }

    /// A new presentation made from `template`: its text, over its files held in memory. It's
    /// untitled until its first save, and has nothing unsaved until it's edited.
    pub fn new_untitled(template: &Template, id: UntitledId, fonts: Fonts) -> Result<Document, String> {
        let files = template.files.iter().map(|(path, bytes)| (path.to_string(), Arc::from(*bytes))).collect();
        Document::untitled(id, template.slides.to_string(), files, fonts)
    }

    /// An untitled talk brought back from its working copy: `text`, with `files` and `builds`,
    /// the files added to it and the builds entered with them, taken back as
    /// [`Self::recover_with_builds`] takes them. Nothing of it is saved anywhere, so it has
    /// unsaved changes from the start.
    pub fn untitled_with(
        id: UntitledId,
        text: String,
        files: Vec<(String, Arc<[u8]>)>,
        builds: Vec<BuildEntry>,
        fonts: Fonts,
    ) -> Result<Document, String> {
        let mut document = Document::untitled(id, text.clone(), Vec::new(), fonts)?;
        document.recover_with_builds(text, files, builds).map_err(|refused| refused.to_string())?;
        document.history.forget_saved();
        Ok(document)
    }

    /// An untitled talk of `source`, over `files` held in memory.
    fn untitled(id: UntitledId, source: String, files: Vec<(String, Arc<[u8]>)>, fonts: Fonts) -> Result<Document, String> {
        let memory = MemoryFiles::new(files.clone());
        let names = memory.names_in(FONTS_FOLDER).into_iter().filter(|name| is_talk_font(name)).collect();
        let talk_fonts = read_talk_fonts(names, &memory);
        let memory: Arc<dyn TalkFiles + Send + Sync> = Arc::new(memory);
        let mut document = Document::new(TalkPlace::Untitled(id), memory.clone(), talk_fonts, source, fonts)
            .map_err(|_| "The presentation couldn't be made: something in it can't be read.".to_string())?;
        // Only those the files hold: one whose name no talk could have is left out of both.
        document.untitled_files = files.into_iter().filter(|(path, _)| memory.len(path).is_some()).collect();
        Ok(document)
    }

    fn new(
        place: TalkPlace,
        files: Arc<dyn TalkFiles + Send + Sync>,
        talk_fonts: Vec<Arc<Vec<u8>>>,
        source: String,
        mut fonts: Fonts,
    ) -> Result<Document, EditRefused> {
        let talk = worked_out(|| parse_talk(&source))?;
        for font in &talk_fonts {
            fonts.load_talk_font(font.clone());
        }
        let added = Arc::new(AddedSet::new());
        let own_files = files.clone();
        let files: Arc<dyn TalkFiles + Send + Sync> = Arc::new(WithAdded::new(files, added.clone()));
        let mut document = Document {
            own_files,
            untitled_files: Vec::new(),
            on_disk: source,
            outside: None,
            deck: Deck::open_parsed(Box::new(files.clone()), fonts, talk),
            place_name: place.name(),
            place,
            files,
            added,
            pending: BTreeSet::new(),
            talk_fonts,
            current: 0,
            history: History::new(),
            typing_notes: None,
            editing: None,
            held_color: None,
            index_kept_in_pack: None,
            selection: None,
            selected: None,
            selection_slide: None,
            picks: 0,
            next_session: 0,
            message: None,
            files_to_save: Cell::new(None),
            files_to_save_walks: Cell::new(0),
            table_here: RefCell::new(None),
            palette: Vec::new(),
            palette_given_up: BTreeSet::new(),
            palette_longest_step: Duration::ZERO,
            large: Default::default(),
        };
        document.components_looked_at();
        // A still is never made on the thread that lays a slide out.
        document.leave_stills_aside();
        Ok(document)
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

/// Whether `name`, a path within a talk, is a font sitting right in its fonts folder.
fn is_talk_font(name: &str) -> bool {
    name.strip_prefix(FONTS_FOLDER)
        .and_then(|rest| rest.strip_prefix('/'))
        .is_some_and(|file| !file.contains('/') && is_font_file(file))
}

fn font_names_in_folder(folder: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(folder.join(FONTS_FOLDER)) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| is_font_file(name))
        .map(|name| format!("{FONTS_FOLDER}/{name}"))
        .collect()
}

/// Reads the talk's fonts in name order, as the order can decide between two similar fonts.
/// They're read through `files`, which stops a link from reaching outside the talk. Past
/// [`TALK_FONT_BYTES`] the rest are left out.
fn read_talk_fonts(mut names: Vec<String>, files: &dyn TalkFiles) -> Vec<Arc<Vec<u8>>> {
    names.sort();
    let mut fonts = Vec::new();
    let mut total = 0;
    for name in names {
        let Some(data) = files.read(&name) else {
            continue;
        };
        total += data.len();
        if total > TALK_FONT_BYTES {
            break;
        }
        fonts.push(Arc::new(data));
    }
    fonts
}

/// The fonts a talk in `folder` carries, read through `files`.
pub(crate) fn fonts_in_folder(folder: &Path, files: &dyn TalkFiles) -> Vec<Arc<Vec<u8>>> {
    read_talk_fonts(font_names_in_folder(folder), files)
}

/// The fonts a packed talk carries.
pub(crate) fn fonts_in_pack(packed: &slides_engine::PackedFiles) -> Vec<Arc<Vec<u8>>> {
    let names = packed.names().into_iter().filter(|name| is_talk_font(name)).collect();
    read_talk_fonts(names, packed)
}
