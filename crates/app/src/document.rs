//! An open talk, and which of its slides is showing.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use slides_components::Diagnostic;
use slides_engine::{
    ComponentInfo, Deck, FolderFiles, Fonts, Layer, LayerTiming, Moment, PackError, SlideTimeline,
    TalkFiles, Transition, layout_by_name, open_packed, pack_folder, read_talk_text,
    unpack_to_folder,
};
use slides_format::{Change, Talk, split_slides};

use crate::builds;
use crate::export::ExportInput;
use crate::history::History;
use crate::present::TalkShape;

pub(crate) const SLIDES_FILE: &str = "slides.md";
const FONTS_FOLDER: &str = "assets/fonts";

/// The most bytes of fonts a talk can bring. A small packed file can unpack to far more than
/// that, and the fonts of an open talk stay in memory.
const TALK_FONT_BYTES: usize = 256 * 1024 * 1024;

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

/// Why an edit wasn't made. Nothing of it was applied, and there's nothing to undo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditRefused {
    /// A slide that could be read would have become one that can't.
    WouldBreakASlide,
    /// The talk would have ended up with more or fewer slides than the edit was meant to give.
    WouldChangeTheSlides,
}

impl std::fmt::Display for EditRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            EditRefused::WouldBreakASlide => {
                "That change would have broken another slide, so it wasn't made."
            }
            EditRefused::WouldChangeTheSlides => {
                "That change would have joined or split slides, so it wasn't made."
            }
        })
    }
}

impl std::error::Error for EditRefused {}

/// What Undo shows for each kind of edit.
const ADD_SLIDE: &str = "Add slide";
const DUPLICATE_SLIDE: &str = "Duplicate slide";
const DELETE_SLIDE: &str = "Delete slide";
const MOVE_SLIDE: &str = "Move slide";
const HIDE_SLIDE: &str = "Hide slide";
const SHOW_SLIDE: &str = "Show slide";
const EDIT_NOTES: &str = "Edit notes";

/// The layout a slide has when its settings name none.
const DEFAULT_LAYOUT: &str = "content";

/// Why a packed file didn't open.
#[derive(Debug, PartialEq, Eq)]
pub enum OpenProblem {
    /// One of Stagepack's Slidev talks, which this app can't show yet.
    SlidevTalk,
    Other(String),
}

impl From<PackError> for OpenProblem {
    fn from(error: PackError) -> OpenProblem {
        match error {
            PackError::SlidevTalk => OpenProblem::SlidevTalk,
            other => OpenProblem::Other(other.to_string()),
        }
    }
}

/// Where a talk is kept: what saving it in the other form works from. It can be handed to a
/// background task, as a [`Document`] can't.
#[derive(Clone)]
pub enum TalkPlace {
    Folder(PathBuf),
    Packed(PathBuf),
}

impl TalkPlace {
    /// Packs a talk kept in a folder into the file `out`, and says what had to be left out
    /// and which components it carries an older build of.
    pub fn save_packed(&self, out: &Path) -> Result<Vec<String>, String> {
        let TalkPlace::Folder(folder) = self else {
            return Err("This talk is already packed.".to_string());
        };
        let report =
            pack_folder(folder, out, env!("CARGO_PKG_VERSION")).map_err(|e| e.to_string())?;
        let left_out = report
            .skipped
            .into_iter()
            .map(|name| format!("{name} was left out"));
        let outside = report
            .outside_pictures
            .into_iter()
            .map(|name| format!("The picture {name} is outside assets/ and wasn't packed"));
        let behind = builds::behind(folder).into_iter().map(|tag| {
            format!(
                "{tag} couldn't be built just now, so the packed talk carries its last good build"
            )
        });
        Ok(left_out.chain(outside).chain(behind).collect())
    }

    /// Unpacks a packed talk into `folder`, which must be missing or empty.
    pub fn save_folder(&self, folder: &Path) -> Result<(), String> {
        let TalkPlace::Packed(file) = self else {
            return Err("This talk is already a folder.".to_string());
        };
        unpack_to_folder(file, folder).map_err(|e| e.to_string())
    }

    /// The folder this talk sits in, where anything saved from it starts out.
    pub fn directory(&self) -> &Path {
        let (TalkPlace::Folder(path) | TalkPlace::Packed(path)) = self;
        path.parent().unwrap_or(path)
    }

    /// What to call a talk with no title setting.
    fn name(&self) -> String {
        match self {
            TalkPlace::Folder(folder) => folder_name(folder),
            TalkPlace::Packed(file) => file.file_stem().map_or_else(
                || file.display().to_string(),
                |stem| stem.to_string_lossy().into_owned(),
            ),
        }
    }
}

pub struct Document {
    deck: Deck,
    place: TalkPlace,
    /// The talk's files, which exports read from their own thread.
    files: Arc<dyn TalkFiles + Send + Sync>,
    /// The talk's own fonts, for exports to load into decks of their own.
    talk_fonts: Vec<Arc<Vec<u8>>>,
    /// What to call the talk if it has no title setting.
    place_name: String,
    current: usize,
    history: History,
    /// The slide whose notes the last step was typed into, while nothing else has happened
    /// since: more typing there joins that step.
    typing_notes: Option<usize>,
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
        Ok(Document::new(
            TalkPlace::Folder(folder.to_path_buf()),
            Arc::new(files),
            talk_fonts,
            source,
            fonts,
        ))
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
        Ok(Document::new(
            TalkPlace::Packed(path.to_path_buf()),
            Arc::new(packed),
            talk_fonts,
            source,
            fonts,
        ))
    }

    fn new(
        place: TalkPlace,
        files: Arc<dyn TalkFiles + Send + Sync>,
        talk_fonts: Vec<Arc<Vec<u8>>>,
        source: String,
        mut fonts: Fonts,
    ) -> Document {
        for font in &talk_fonts {
            fonts.load_talk_font(font.clone());
        }
        Document {
            deck: Deck::open(Box::new(files.clone()), fonts, source),
            place_name: place.name(),
            place,
            files,
            talk_fonts,
            current: 0,
            history: History::new(),
            typing_notes: None,
        }
    }

    pub fn current(&self) -> usize {
        self.current
    }

    pub fn slide_count(&self) -> usize {
        self.deck.slide_count()
    }

    /// How many slides a show goes through: all but the hidden ones.
    pub fn shown_count(&self) -> usize {
        self.deck.shown_slides().len()
    }

    /// The talk's own index of the slide a show calls `position`, counting only the slides it
    /// shows. This is the one place a show's slides are mapped to the talk's.
    pub fn shown_slide(&self, position: usize) -> usize {
        let shown = self.deck.shown_slides();
        shown.get(position).or(shown.last()).copied().unwrap_or(0)
    }

    pub fn next(&mut self) {
        if self.current + 1 < self.slide_count() {
            self.select(self.current + 1);
        }
    }

    pub fn previous(&mut self) {
        self.select(self.current.saturating_sub(1));
    }

    /// Makes slide `index` the current one, or the last if there's no such slide.
    pub fn select(&mut self, index: usize) {
        let index = index.min(self.slide_count() - 1);
        if index != self.current {
            self.current = index;
            self.typing_notes = None;
        }
    }

    /// What the talk is called: its title setting, or else the name of its folder or file.
    pub fn name(&self) -> String {
        self.deck
            .title()
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| self.place_name.clone())
    }

    /// The talk's name and where you are in it.
    pub fn title(&self) -> String {
        format!(
            "{} · {} / {}",
            self.name(),
            self.current + 1,
            self.slide_count()
        )
    }

    /// The current slide's layers, first to last, drawn at `scale`.
    pub fn layers(&mut self, scale: f32) -> Vec<Arc<Layer>> {
        self.layers_of(self.current, scale)
    }

    /// The layers of slide `index`, which must be one of the talk's, first to last, drawn at
    /// `scale`.
    pub fn layers_of(&mut self, index: usize, scale: f32) -> Vec<Arc<Layer>> {
        self.deck.layers(index, scale)
    }

    /// The layers of slide `index` at `moment`, first to last, drawn at `scale`.
    pub fn layers_at(&mut self, index: usize, moment: Moment, scale: f32) -> Vec<Arc<Layer>> {
        self.deck.layers_at(index, moment, scale)
    }

    /// When each layer of slide `index` at `moment` appears, in the order of [`Self::layers_at`]
    /// at that moment.
    pub fn timings_at(&mut self, index: usize, moment: Moment) -> Vec<LayerTiming> {
        self.deck.timings_at(index, moment)
    }

    /// When each layer of slide `index` appears, in the order of [`Self::layers_of`].
    pub fn timeline_of(&mut self, index: usize) -> SlideTimeline {
        self.deck.timeline(index)
    }

    /// The notes of slide `index`, if it has any.
    pub fn notes_of(&self, index: usize) -> Option<String> {
        self.deck.notes(index)
    }

    /// What the user should hear about the current slide.
    pub fn notices(&mut self) -> Vec<String> {
        // The window shows the slide in its whole state, so these are its notices.
        self.deck.notices_whole(self.current)
    }

    pub fn source(&self) -> &str {
        self.deck.source()
    }

    /// The talk as it is now: its slides, and where each sits in the text.
    pub fn talk(&self) -> &Talk {
        self.deck.talk()
    }

    /// Makes `change` to the talk's text, named `label` for Undo, and returns the slides to
    /// show again. `slides` is how the number of slides is meant to change: 0, 1 or -1.
    ///
    /// The change is refused, and nothing applied or recorded, if it would leave a slide that
    /// could be read as one that can't, or change the number of slides by anything else. An
    /// empty change does nothing.
    pub fn edit(
        &mut self,
        label: &'static str,
        change: Change,
        slides: isize,
    ) -> Result<Vec<usize>, EditRefused> {
        let current = self.current;
        self.make(label, change, slides, current, current)
    }

    /// Makes `change` as [`Self::edit`] does. `before` is the slide it's about in the text
    /// as it is, and `after` the one it's about once made, which becomes the current slide.
    fn make(
        &mut self,
        label: &'static str,
        change: Change,
        slides: isize,
        before: usize,
        after: usize,
    ) -> Result<Vec<usize>, EditRefused> {
        if change.is_empty() {
            return Ok(Vec::new());
        }
        let old = self.source().to_string();
        let old_count = self.slide_count();
        let broken_before = broken_bodies(self.talk());
        let undo = change.inverse(&old);
        let mut changed = self.deck.update(change.apply(&old));
        let refused = if old_count.checked_add_signed(slides) != Some(self.slide_count()) {
            Some(EditRefused::WouldChangeTheSlides)
        } else if broken_bodies(self.talk())
            .iter()
            .any(|body| !broken_before.contains(body))
        {
            Some(EditRefused::WouldBreakASlide)
        } else {
            None
        };
        if let Some(refused) = refused {
            self.deck.update(old);
            return Err(refused);
        }
        let joins_typing = label == EDIT_NOTES && self.typing_notes == Some(after);
        if joins_typing && self.history.can_undo() {
            self.history.extend_last(&old, self.deck.source());
        } else {
            self.history.record(label, undo, before, after);
        }
        self.typing_notes = (label == EDIT_NOTES).then_some(after);
        self.show(after, &mut changed);
        Ok(changed)
    }

    /// Makes `index` the current slide after an edit, and adds it to `changed` if it isn't
    /// there: its text may be as it was, but it's now the slide to show.
    fn show(&mut self, index: usize, changed: &mut Vec<usize>) {
        let index = index.min(self.slide_count() - 1);
        if index != self.current {
            self.current = index;
            if !changed.contains(&index) {
                changed.push(index);
            }
        }
    }

    /// Takes the last edit back, and returns the slides to show again: `None` if there was
    /// nothing to undo.
    pub fn undo(&mut self) -> Option<Vec<usize>> {
        let step = self.history.undo(self.deck.source())?;
        Some(self.take_step(step))
    }

    /// Makes the last undone edit again, and returns the slides to show again: `None` if
    /// there was nothing to redo.
    pub fn redo(&mut self) -> Option<Vec<usize>> {
        let step = self.history.redo(self.deck.source())?;
        Some(self.take_step(step))
    }

    fn take_step(&mut self, step: crate::history::Step) -> Vec<usize> {
        let source = step.change.apply(self.deck.source());
        let mut changed = self.deck.update(source);
        self.typing_notes = None;
        self.show(step.current, &mut changed);
        changed
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// What Undo would take back, such as "Add slide".
    pub fn undo_label(&self) -> Option<&'static str> {
        self.history.undo_label()
    }

    /// What Redo would make again.
    pub fn redo_label(&self) -> Option<&'static str> {
        self.history.redo_label()
    }

    /// Whether the talk has edits that its file doesn't.
    pub fn has_unsaved_changes(&self) -> bool {
        !self.history.is_saved()
    }

    /// Adds a slide with the layout called `layout` after the current one, with an empty
    /// heading, and makes it the current slide. A name that isn't a layout's is taken as the
    /// default one.
    pub fn add_slide(&mut self, layout: &str) -> Result<Vec<usize>, EditRefused> {
        let layout = layout_by_name(layout).map_or(DEFAULT_LAYOUT, |layout| layout.name);
        let text = if layout == DEFAULT_LAYOUT {
            "# \n".to_string()
        } else {
            format!("layout: {layout}\n---\n\n# \n")
        };
        let at = self.current;
        let change = slides_format::insert_slide(self.talk(), Some(at), &text);
        self.make(ADD_SLIDE, change, 1, at, at + 1)
    }

    /// Puts a copy of the current slide straight after it, and makes the copy current.
    pub fn duplicate_current(&mut self) -> Result<Vec<usize>, EditRefused> {
        let at = self.current;
        let change = slides_format::duplicate_slide(self.talk(), at);
        self.make(DUPLICATE_SLIDE, change, 1, at, at + 1)
    }

    /// Removes the current slide, unless it's the only one. The one after it becomes
    /// current, or the one before when it was last.
    pub fn delete_current(&mut self) -> Result<Vec<usize>, EditRefused> {
        let at = self.current;
        let Some(change) = slides_format::delete_slide(self.talk(), at) else {
            return Ok(Vec::new());
        };
        let after = at.min(self.slide_count() - 2);
        self.make(DELETE_SLIDE, change, -1, at, after)
    }

    /// Moves slide `from` so that `to` is its place afterwards, and makes it current.
    pub fn move_slide(&mut self, from: usize, to: usize) -> Result<Vec<usize>, EditRefused> {
        let change = slides_format::move_slide(self.talk(), from, to);
        self.make(MOVE_SLIDE, change, 0, from, to)
    }

    /// Leaves the current slide out of shows, or puts it back in.
    pub fn set_current_hidden(&mut self, hidden: bool) -> Result<Vec<usize>, EditRefused> {
        let at = self.current;
        let change = slides_format::set_hidden(self.talk(), at, hidden);
        let label = if hidden { HIDE_SLIDE } else { SHOW_SLIDE };
        self.make(label, change, 0, at, at)
    }

    /// Sets the notes of slide `index`, and makes it current. Notes typed into one slide
    /// with nothing else done in between are one step to undo.
    pub fn set_notes(&mut self, index: usize, notes: &str) -> Result<Vec<usize>, EditRefused> {
        let change = slides_format::set_notes(self.talk(), index, notes);
        self.make(EDIT_NOTES, change, 0, index, index)
    }

    /// The talk's own components, and whether each is built.
    pub fn components(&self) -> &[ComponentInfo] {
        self.deck.components()
    }

    /// What's wrong with the talk's own components, as found when they were last looked at.
    pub fn component_problems(&self) -> &[Diagnostic] {
        self.deck.component_problems()
    }

    /// Looks at the talk's components again, after a build or a change to one of them.
    pub fn reload_components(&mut self) -> Reload {
        let changed = self.deck.refresh_components();
        if changed.is_empty() {
            Reload::Unchanged
        } else {
            Reload::Changed(changed)
        }
    }

    /// Whether this talk came from a packed file, rather than a folder.
    pub fn is_packed(&self) -> bool {
        matches!(self.place, TalkPlace::Packed(_))
    }

    pub fn place(&self) -> &TalkPlace {
        &self.place
    }

    /// Packs a talk kept in a folder into the file `out`, and says what had to be left out.
    pub fn save_packed(&self, out: &Path) -> Result<Vec<String>, String> {
        self.place.save_packed(out)
    }

    /// Unpacks a packed talk into `folder`, which must be missing or empty.
    pub fn save_folder(&self, folder: &Path) -> Result<(), String> {
        self.place.save_folder(folder)
    }

    /// What an export needs to know about the talk as it is now.
    pub fn export_input(&self) -> ExportInput {
        ExportInput {
            source: self.source().to_string(),
            files: self.files.clone(),
            talk_fonts: self.talk_fonts.clone(),
        }
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
        if !same_text {
            // Its steps are ranges of the text it had, which no longer fit.
            self.history = History::new();
            self.typing_notes = None;
        }
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

/// A show's slides are the talk's shown ones, so `slide` here is a position among those.
impl TalkShape for Document {
    fn slide_count(&self) -> usize {
        self.shown_count()
    }

    fn steps(&mut self, slide: usize) -> u32 {
        self.timeline_of(self.shown_slide(slide)).steps
    }

    fn transition(&mut self, slide: usize) -> Transition {
        self.timeline_of(self.shown_slide(slide)).transition
    }
}

/// What the slides of `talk` that can't be read hold, past any settings, without the blank
/// lines around it. A slide like that moves, or is copied, as it is, and is still one of these.
fn broken_bodies(talk: &Talk) -> Vec<String> {
    if talk.slides.iter().all(|slide| slide.error.is_none()) {
        return Vec::new();
    }
    split_slides(&talk.source)
        .iter()
        .zip(&talk.slides)
        .filter(|(_, slide)| slide.error.is_some())
        .map(|(raw, _)| talk.source[raw.body.clone()].trim().to_string())
        .collect()
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
