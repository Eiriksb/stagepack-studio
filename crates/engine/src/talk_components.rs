//! A talk's own components: finding which it has and which of them are built, and running
//! the builds in the sandbox. A talk is untrusted, so whatever it says about its builds
//! is checked, and every file it names is read within limits.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Range;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

use slides_components::{
    Assets, BuildIndex, ComponentError, Diagnostic, INDEX_FILE, MAX_ASSET_PATH_BYTES, MAX_MODULE_BYTES, MEMORY_LIMIT_BYTES, WasmComponent, check_source, component_tag,
    safe_stem, source_hash,
};
use toolkit::{ComponentSchema, Event, Registry, RenderInput, RenderOutput, asset_path_allowed};

use crate::TalkFiles;
use crate::flow::TEXT_TAG;
use crate::pictures::FileVersion;

/// How long the components on one slide can take to render, together, each time it's laid out.
pub const SLIDE_RENDER_BUDGET: Duration = Duration::from_secs(1);

/// Where a talk keeps its components' sources: each a `.rs` file directly in this folder.
pub const COMPONENTS_FOLDER: &str = "components";
const SOURCE_EXTENSION: &str = ".rs";
/// The most components a talk can have.
pub const MAX_COMPONENTS: usize = 256;
/// The most bytes a component's source can have.
pub const MAX_SOURCE_BYTES: u64 = 1024 * 1024;
/// The most bytes a talk's index of builds can have: far more than `MAX_COMPONENTS` entries need.
pub const MAX_INDEX_BYTES: u64 = 1024 * 1024;
/// The most bytes of modules a talk can have loaded at once. Loading takes time on the
/// thread that asks, about 12 s for this much.
pub const MAX_TALK_MODULE_BYTES: u64 = 16 * 1024 * 1024;
/// The most nodes the layout of one slide can make, across all its layers.
pub const MAX_LAYOUT_NODES: usize = 50_000;
/// What one render can read of the talk's assets, in one file and in all of them together.
const ASSET_BUDGET: u64 = MEMORY_LIMIT_BYTES as u64;
/// The tags the format itself uses, which a component can't have either.
const FORMAT_TAGS: [&str; 3] = [TEXT_TAG, "Region", "Notes"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComponentState {
    /// Its build was made from the source as it is now.
    UpToDate,
    /// It has a build, made from an older source.
    OutOfDate,
    /// It has no build.
    NeedsBuilding,
}

/// One of the talk's own components. `file` is its source, or its build if the talk has no source for it.
#[derive(Clone, Debug, PartialEq)]
pub struct ComponentInfo {
    pub tag: String,
    pub file: String,
    pub state: ComponentState,
}

/// What's known of a component's schema without loading anything.
#[derive(Clone, Debug, PartialEq)]
pub enum SchemaLoad {
    /// Its build hasn't been loaded yet, or it has none.
    NotLoaded,
    /// Its build was loaded and doesn't work.
    Failed,
    Loaded(ComponentSchema),
}

/// What an interactive component on the slide shown is given when it's next drawn: the state it
/// kept and the events since, and a number told from every other feed's, which is the state the
/// layer it's drawn on is drawn from.
#[derive(Clone, Debug, PartialEq)]
pub struct ComponentFeed {
    pub state: Option<serde_json::Value>,
    pub events: Vec<Event>,
    pub hash: u64,
}

/// What came of drawing an interactive component from a feed: the state it keeps, or why it
/// couldn't be drawn. `source` is where its tag is written, and `hash` the feed's.
#[derive(Clone, Debug, PartialEq)]
pub struct ComponentAnswer {
    pub source: Range<usize>,
    pub hash: u64,
    pub state: Result<Option<serde_json::Value>, String>,
}

/// The most answers kept until they're taken: a show takes them as it draws.
const MAX_ANSWERS: usize = 64;

/// What came of asking a talk's component to render.
pub(crate) enum Rendered {
    Output(RenderOutput),
    NeedsBuilding,
    Failed(ComponentError),
    /// The slide's components had already used up its budget, so this one didn't run.
    OutOfTime,
}

/// A build a tag runs, and what its bytes were when the talk was scanned.
#[derive(Clone, PartialEq)]
struct Build {
    path: String,
    identity: String,
}

#[derive(Clone, PartialEq)]
struct Runnable {
    state: ComponentState,
    build: Option<Build>,
}

/// What the index of builds came to: nothing if there's none, or why it can't be used.
type IndexRead = Result<BuildIndex, String>;

/// What a source said, or why it can't be used, kept with the stamp it had so that it isn't
/// read again unchanged.
type SourceRead = Result<(String, String), Diagnostic>;

/// What reading a file came to, and the stamp the file had then.
struct Seen<T> {
    stamp: (u64, SystemTime),
    value: T,
}

/// A module, loaded once for as long as its bytes stay the same.
struct Loaded {
    identity: String,
    component: Result<WasmComponent, ComponentError>,
    /// The bytes it counts against `MAX_TALK_MODULE_BYTES`, whether or not it loaded, as
    /// trying took time too. None if it was never tried.
    bytes: u64,
}

pub struct TalkComponents {
    files: Rc<dyn TalkFiles>,
    infos: Vec<ComponentInfo>,
    problems: Vec<Diagnostic>,
    runnable: HashMap<String, Runnable>,
    loaded: HashMap<String, Loaded>,
    sources_seen: HashMap<String, Seen<SourceRead>>,
    builds_seen: HashMap<String, Seen<Option<String>>>,
    index_seen: HashMap<String, Seen<Option<IndexRead>>>,
    /// What each build was found to be in the scan under way.
    identities_this_scan: HashMap<String, Option<String>>,
    /// The sources the scan under way refused, by file name without `.rs`, and their paths.
    refused_sources: HashMap<String, String>,
    /// The sources past `MAX_COMPONENTS` in the scan under way, by file name without `.rs`.
    left_out_sources: HashSet<String>,
    /// The time renders have taken since the slide being laid out was started.
    spent: Duration,
    /// The nodes made since the slide being laid out was started.
    nodes: usize,
    assets_read: AssetsRead,
    /// How many modules have been loaded, or tried.
    loads: usize,
    /// Changes whenever a module is loaded or let go of, and so whenever what takes content can.
    loaded_generation: u64,
    /// What each interactive component, by where its tag is written, is given when it's drawn at
    /// a moment of some state.
    feeds: HashMap<Range<usize>, ComponentFeed>,
    /// What came of drawing them so, oldest first, until it's taken.
    answers: Vec<ComponentAnswer>,
    /// The last render of each fed component, by where its tag is written: laid out again from
    /// the same input, as both of a show's windows and the next click's preview do, it isn't run
    /// again.
    fed_renders: HashMap<Range<usize>, FedRender>,
}

/// A fed component's last render: the input it was drawn from, what came of it, and the feed
/// that was last answered with it. A new feed is always answered, even with the same input.
struct FedRender {
    input: u64,
    result: Result<RenderOutput, ComponentError>,
    answered: u64,
}

struct NoFiles;

impl TalkFiles for NoFiles {
    fn read(&self, _: &str) -> Option<Vec<u8>> {
        None
    }
}

fn problem(file: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::new(file, 0, message.into())
}

/// The most asset paths remembered as read, for a talk.
pub const MAX_ASSETS_REMEMBERED: usize = 256;

/// The assets components have read since the talk's files were last looked at, and what
/// each file was then: `None` if there was no such file.
#[derive(Default)]
pub(crate) struct AssetRecord {
    paths: HashMap<String, Option<FileVersion>>,
    /// Whether a path was read when the record was full, so that what it was isn't known.
    overflowed: bool,
}

impl AssetRecord {
    /// The first version seen is the one kept: a slide laid out since may have read the file as
    /// it has become, and the slides laid out before it still show what it was.
    fn note(&mut self, path: &str, version: Option<FileVersion>) {
        if self.paths.contains_key(path) {
            return;
        }
        if self.paths.len() < MAX_ASSETS_REMEMBERED {
            self.paths.insert(path.to_string(), version);
        } else {
            self.overflowed = true;
        }
    }
}

pub(crate) type AssetsRead = Rc<RefCell<AssetRecord>>;

/// The bytes of `path`, held to the rules for a render's reads: only from the talk's
/// assets folder, no file over the limit, and no more than `ASSET_BUDGET` in all, counted
/// in `used`. A file whose length is known is refused before it's read. What the file was
/// is noted in `seen`, so that a change to it is found, unless the talk never changes.
pub(crate) fn read_asset(files: &dyn TalkFiles, path: &str, used: &Cell<u64>, seen: &AssetsRead) -> Option<Vec<u8>> {
    if path.len() > MAX_ASSET_PATH_BYTES || !asset_path_allowed(path) {
        return None;
    }
    let note = |version: Option<FileVersion>| {
        if !files.never_changes() {
            seen.borrow_mut().note(path, version);
        }
    };
    let stamp = files.stamp(path).map(|(length, modified)| FileVersion::Stamp(length, modified));
    let left = ASSET_BUDGET.saturating_sub(used.get());
    if files.len(path).is_some_and(|len| len > left) {
        if stamp.is_some() {
            note(stamp);
        }
        return None;
    }
    let Some(bytes) = files.read(path) else {
        note(stamp);
        return None;
    };
    note(stamp.or_else(|| Some(FileVersion::of_bytes(&bytes))));
    let len = bytes.len() as u64;
    if len > left {
        return None;
    }
    used.set(used.get() + len);
    Some(bytes)
}

/// The assets one render of a talk's component can read.
fn assets_for_a_render(files: &Rc<dyn TalkFiles>, seen: &AssetsRead) -> Assets {
    let (files, seen) = (files.clone(), seen.clone());
    let used = Cell::new(0);
    Rc::new(move |path: &str| read_asset(&*files, path, &used, &seen))
}

/// Whether `tag` is already taken, by the format or by a component every talk has.
pub(crate) fn is_reserved(tag: &str, registry: &Registry) -> bool {
    FORMAT_TAGS.contains(&tag) || registry.get(tag).is_some()
}

fn build_path(file: &str) -> String {
    format!("build/{file}.wasm")
}

/// What a scan found, for telling whether a later one found anything different.
type Summary = (Vec<ComponentInfo>, Vec<Diagnostic>, Vec<(String, Runnable)>);

impl TalkComponents {
    /// The components of the talk in `files`, as its sources and its `build` folder say.
    pub fn scan(files: Rc<dyn TalkFiles>) -> TalkComponents {
        TalkComponents::scan_for(files, &crate::builtin_registry())
    }

    /// As `scan`, with `registry` holding the names a talk's component can't take.
    pub(crate) fn scan_for(files: Rc<dyn TalkFiles>, registry: &Registry) -> TalkComponents {
        let mut components = TalkComponents {
            files,
            infos: Vec::new(),
            problems: Vec::new(),
            runnable: HashMap::new(),
            loaded: HashMap::new(),
            sources_seen: HashMap::new(),
            builds_seen: HashMap::new(),
            index_seen: HashMap::new(),
            identities_this_scan: HashMap::new(),
            refused_sources: HashMap::new(),
            left_out_sources: HashSet::new(),
            spent: Duration::ZERO,
            nodes: 0,
            assets_read: AssetsRead::default(),
            loads: 0,
            loaded_generation: 0,
            feeds: HashMap::new(),
            answers: Vec::new(),
            fed_renders: HashMap::new(),
        };
        components.rescan(registry);
        components
    }

    /// A talk with no components of its own.
    pub fn none() -> TalkComponents {
        TalkComponents::scan_for(Rc::new(NoFiles), &Registry::default())
    }

    pub fn infos(&self) -> &[ComponentInfo] {
        &self.infos
    }

    pub fn problems(&self) -> &[Diagnostic] {
        &self.problems
    }

    /// What `tag` takes, for a form, if the talk has a working build of it.
    pub fn schema(&mut self, tag: &str) -> Option<ComponentSchema> {
        let build = self.runnable.get(tag)?.build.clone()?;
        self.component(tag, &build).ok().map(|component| component.schema().clone())
    }

    /// Looks at the talk's components again, keeping each module already loaded whose bytes
    /// are the same. Returns whether anything is different.
    pub(crate) fn rescan(&mut self, registry: &Registry) -> bool {
        let assets_changed = self.assets_changed();
        // A module or an asset may have changed since.
        self.fed_renders.clear();
        let before = self.summary();
        self.infos.clear();
        self.problems.clear();
        self.runnable.clear();
        self.identities_this_scan.clear();
        self.refused_sources.clear();
        self.left_out_sources.clear();
        let index = self.read_index();
        let named = self.add_sources(&index, registry);
        self.add_builds_with_no_source(&index, &named, registry);

        let runnable = &self.runnable;
        let loaded_before = self.loaded.len();
        self.loaded.retain(|path, loaded| {
            runnable.values().filter_map(|runnable| runnable.build.as_ref()).any(|build| build.path == *path && build.identity == loaded.identity)
        });
        let files = self.files.clone();
        let still_there = |path: &String| files.never_changes() || files.stamp(path).is_some();
        self.sources_seen.retain(|path, _| still_there(path));
        self.builds_seen.retain(|path, _| still_there(path));
        self.index_seen.retain(|path, _| still_there(path));
        if self.loaded.len() != loaded_before {
            self.loaded_generation += 1;
        }
        self.summary() != before || assets_changed
    }

    /// What `tag` takes, if its build is loaded already: `None` if it isn't loaded, or there's
    /// no build of it to load; `Some(None)` if it was tried and doesn't work. Nothing is loaded.
    pub(crate) fn loaded_schema(&self, tag: &str) -> Option<Option<&ComponentSchema>> {
        let build = self.runnable.get(tag)?.build.as_ref()?;
        let loaded = self.loaded.get(&build.path).filter(|loaded| loaded.identity == build.identity)?;
        Some(loaded.component.as_ref().ok().map(WasmComponent::schema).filter(|schema| schema.name == tag))
    }

    /// Whether `tag` is a component of the talk's whose build is loaded and working and says it
    /// takes content: what's written between its tags is its to draw. Nothing is loaded.
    pub(crate) fn takes_content(&self, tag: &str) -> bool {
        self.loaded_schema(tag).flatten().is_some_and(|schema| schema.children)
    }

    /// Whether `tag` is a component of the talk's whose build is loaded and working and says it's
    /// interactive. Nothing is loaded.
    pub(crate) fn is_interactive(&self, tag: &str) -> bool {
        self.loaded_schema(tag).flatten().is_some_and(|schema| schema.interactive)
    }

    pub(crate) fn set_feeds(&mut self, feeds: HashMap<Range<usize>, ComponentFeed>) {
        self.fed_renders.retain(|source, _| feeds.contains_key(source));
        self.feeds = feeds;
    }

    /// Renders `tag`, written at `source`, from `input`, which holds its feed, told from every other
    /// by `feed`: once for each input, as the last is kept. Gives whether what came of it is for
    /// that feed to be answered with: the first time each feed is drawn, whether or not the module
    /// ran, so a feed giving what the one before gave is answered too.
    pub(crate) fn render_fed(&mut self, tag: &str, source: &Range<usize>, input: &RenderInput, feed: u64) -> Option<(Rendered, bool)> {
        let mut hasher = DefaultHasher::new();
        (tag, serde_json::to_string(input).unwrap_or_default()).hash(&mut hasher);
        let key = hasher.finish();
        if let Some(kept) = self.fed_renders.get_mut(source)
            && kept.input == key
        {
            let rendered = match &kept.result {
                Ok(output) => Rendered::Output(output.clone()),
                Err(error) => Rendered::Failed(error.clone()),
            };
            let answer = std::mem::replace(&mut kept.answered, feed) != feed;
            return Some((rendered, answer));
        }
        let rendered = self.render(tag, input)?;
        let result = match &rendered {
            Rendered::Output(output) => Some(Ok(output.clone())),
            Rendered::Failed(error) => Some(Err(error.clone())),
            Rendered::NeedsBuilding | Rendered::OutOfTime => None,
        };
        match result {
            Some(result) => {
                self.fed_renders.insert(source.clone(), FedRender { input: key, result, answered: feed });
            }
            None => {
                self.fed_renders.remove(source);
            }
        }
        Some((rendered, true))
    }

    /// What the interactive component whose tag is written at `source` is given, if anything.
    pub(crate) fn feed(&self, source: &Range<usize>) -> Option<ComponentFeed> {
        self.feeds.get(source).cloned()
    }

    /// Notes what came of drawing a component from a feed, forgetting the oldest past the most kept.
    pub(crate) fn answer(&mut self, answer: ComponentAnswer) {
        if self.answers.len() >= MAX_ANSWERS {
            self.answers.remove(0);
        }
        self.answers.push(answer);
    }

    pub(crate) fn take_answers(&mut self) -> Vec<ComponentAnswer> {
        std::mem::take(&mut self.answers)
    }

    /// Changes whenever a module is loaded or let go of.
    pub(crate) fn loaded_generation(&self) -> u64 {
        self.loaded_generation
    }

    /// How many modules have been loaded, or tried.
    pub(crate) fn loads(&self) -> usize {
        self.loads
    }

    pub(crate) fn assets_remembered(&self) -> usize {
        self.assets_read.borrow().paths.len()
    }

    /// The record of what components read, for a built-in's render to add to.
    pub(crate) fn assets_read(&self) -> &AssetsRead {
        &self.assets_read
    }

    /// Whether a file a component read is different now, like a replaced picture. Then
    /// what was read is forgotten, as every slide is laid out again and reads it afresh.
    /// A record that overflowed can't tell, so it counts as changed every time.
    fn assets_changed(&mut self) -> bool {
        let mut seen = self.assets_read.borrow_mut();
        let changed =
            seen.overflowed || seen.paths.iter().any(|(path, version)| FileVersion::of_file(path, &*self.files) != *version);
        if changed {
            *seen = AssetRecord::default();
        }
        changed
    }

    fn summary(&self) -> Summary {
        let mut runnable: Vec<(String, Runnable)> = self.runnable.iter().map(|(tag, runnable)| (tag.clone(), runnable.clone())).collect();
        runnable.sort_by(|a, b| a.0.cmp(&b.0));
        (self.infos.clone(), self.problems.clone(), runnable)
    }

    /// Restarts the budget, for the slide about to be laid out.
    pub(crate) fn new_layout(&mut self) {
        self.spent = Duration::ZERO;
        self.nodes = 0;
    }

    /// Counts one more node made in the slide's layout.
    pub(crate) fn count_node(&mut self) {
        self.nodes += 1;
    }

    /// Whether the slide's layout has made more nodes than it can have.
    pub(crate) fn too_many_nodes(&self) -> bool {
        self.nodes > MAX_LAYOUT_NODES
    }

    pub(crate) fn is_out_of_date(&self, tag: &str) -> bool {
        self.runnable.get(tag).is_some_and(|runnable| runnable.state == ComponentState::OutOfDate)
    }

    /// Renders `tag` with the talk's component of that name, or `None` if it has none.
    pub(crate) fn render(&mut self, tag: &str, input: &RenderInput) -> Option<Rendered> {
        let Some(build) = self.runnable.get(tag)?.build.clone() else {
            return Some(Rendered::NeedsBuilding);
        };
        if self.spent >= SLIDE_RENDER_BUDGET {
            return Some(Rendered::OutOfTime);
        }
        let assets = assets_for_a_render(&self.files, &self.assets_read);
        let component = match self.component(tag, &build) {
            Ok(component) => component,
            Err(error) => return Some(Rendered::Failed(error)),
        };
        let started = Instant::now();
        let rendered = component.render(input, assets);
        self.spent += started.elapsed();
        Some(match rendered {
            Ok(output) => Rendered::Output(output),
            Err(error) => Rendered::Failed(error),
        })
    }

    /// The loaded module of `build`, loading it if it isn't already, and only if it says it is `tag`.
    fn component(&mut self, tag: &str, build: &Build) -> Result<&WasmComponent, ComponentError> {
        if !self.loaded.contains_key(&build.path) {
            let (component, bytes) = self.load(&build.path);
            self.loads += 1;
            self.loaded_generation += 1;
            self.loaded.insert(build.path.clone(), Loaded { identity: build.identity.clone(), component, bytes });
        }
        let component = self.loaded[&build.path].component.as_ref().map_err(Clone::clone)?;
        let name = &component.schema().name;
        if name != tag {
            return Err(ComponentError::Malformed(format!("it says it is {name}, not {tag}")));
        }
        Ok(component)
    }

    /// The module at `path`, if it fits in what the talk has left to load, and the bytes it
    /// counts against that: its length once it's been tried, whether or not it loaded.
    fn load(&self, path: &str) -> (Result<WasmComponent, ComponentError>, u64) {
        let left = MAX_TALK_MODULE_BYTES.saturating_sub(self.loaded.values().map(|loaded| loaded.bytes).sum());
        let limit = left.min(MAX_MODULE_BYTES as u64);
        if self.files.len(path).is_some_and(|len| len > limit) {
            return (Err(ComponentError::TooLarge), 0);
        }
        let Some(bytes) = self.files.read(path) else {
            return (Err(ComponentError::Malformed("its build can't be read".into())), 0);
        };
        if bytes.len() as u64 > limit {
            return (Err(ComponentError::TooLarge), 0);
        }
        (WasmComponent::load(&bytes), bytes.len() as u64)
    }

    /// The talk's index of builds, or an empty one if it has none or it can't be read.
    fn read_index(&mut self) -> BuildIndex {
        let empty = BuildIndex { format: 0, components: Vec::new() };
        let files = self.files.clone();
        let index = remembered(&mut self.index_seen, &*files, INDEX_FILE, || {
            if files.len(INDEX_FILE).is_some_and(|len| len > MAX_INDEX_BYTES) {
                return Some(Err("it is too large to read".to_string()));
            }
            let bytes = files.read(INDEX_FILE)?;
            if bytes.len() as u64 > MAX_INDEX_BYTES {
                return Some(Err("it is too large to read".to_string()));
            }
            Some(serde_json::from_slice::<BuildIndex>(&bytes).map_err(|error| error.to_string()))
        });
        let mut index = match index {
            None => return empty,
            Some(Ok(index)) => index,
            Some(Err(reason)) => {
                self.problems.push(problem(INDEX_FILE, format!("It can't be read, so no component counts as built: {reason}")));
                return empty;
            }
        };
        let mut unsafe_names = Vec::new();
        index.components.retain(|entry| {
            let safe = safe_stem(&entry.file);
            if !safe {
                unsafe_names.push(entry.file.clone());
            }
            safe
        });
        for name in unsafe_names {
            self.problems.push(problem(INDEX_FILE, format!("It names a build {name:?}, which isn't a name a build can have")));
        }
        if index.components.len() > MAX_COMPONENTS {
            let left_out = index.components.len() - MAX_COMPONENTS;
            self.problems.push(problem(
                INDEX_FILE,
                format!("A talk can have at most {MAX_COMPONENTS} components, so the last {left_out} builds it lists are left out"),
            ));
            index.components.truncate(MAX_COMPONENTS);
        }
        index
    }

    /// What the build at `path` is, to tell whether it changes, or `None` if there's no such file.
    /// Each build is looked at once in a scan, however many entries name it.
    fn build_identity(&mut self, path: &str) -> Option<String> {
        if let Some(identity) = self.identities_this_scan.get(path) {
            return identity.clone();
        }
        let files = self.files.clone();
        let identity = remembered(&mut self.builds_seen, &*files, path, || {
            if let Some(len) = files.len(path).filter(|len| *len > MAX_MODULE_BYTES as u64) {
                return Some(format!("{len} bytes, too large to run"));
            }
            files.read(path).map(|bytes| source_hash(&bytes))
        });
        self.identities_this_scan.insert(path.to_string(), identity.clone());
        identity
    }

    /// The tag and hash of the source at `path`, or why it can't be used.
    fn read_source(&mut self, path: &str) -> SourceRead {
        let files = self.files.clone();
        remembered(&mut self.sources_seen, &*files, path, || {
            let too_large = || Err(problem(path, "It is over the limit of 1 MiB for a component's source"));
            if files.len(path).is_some_and(|len| len > MAX_SOURCE_BYTES) {
                return too_large();
            }
            let bytes = files.read(path).ok_or_else(|| problem(path, "It can't be read"))?;
            if bytes.len() as u64 > MAX_SOURCE_BYTES {
                return too_large();
            }
            let text = std::str::from_utf8(&bytes).map_err(|_| problem(path, "It isn't UTF-8 text"))?;
            match component_tag(text) {
                Some(tag) => Ok((tag, source_hash(&bytes))),
                // Most often it can't be read as Rust, which the check says, and where.
                None => Err(check_source(path, text).err().and_then(|found| found.into_iter().next()).unwrap_or_else(|| problem(path, "No component was found in it"))),
            }
        })
    }

    /// The talk's sources, in name order, each with the build that best matches it. Returns
    /// the tags the sources name, whether or not they can be used.
    fn add_sources(&mut self, index: &BuildIndex, registry: &Registry) -> HashSet<String> {
        let mut paths: Vec<String> =
            self.files.names_in(COMPONENTS_FOLDER).into_iter().filter(|path| path.ends_with(SOURCE_EXTENSION)).collect();
        if paths.len() > MAX_COMPONENTS {
            let left_out = paths.len() - MAX_COMPONENTS;
            self.problems.push(problem(
                COMPONENTS_FOLDER,
                format!("A talk can have at most {MAX_COMPONENTS} components, so the last {left_out} are left out"),
            ));
            for left_out in paths.drain(MAX_COMPONENTS..) {
                if let Some(stem) = left_out.strip_prefix(&format!("{COMPONENTS_FOLDER}/")).and_then(|name| name.strip_suffix(SOURCE_EXTENSION)) {
                    self.left_out_sources.insert(stem.to_string());
                }
            }
        }
        let mut claimed: HashMap<String, String> = HashMap::new();
        let mut named = HashSet::new();
        for path in paths {
            let stem = path.strip_prefix(&format!("{COMPONENTS_FOLDER}/")).and_then(|name| name.strip_suffix(SOURCE_EXTENSION));
            if !stem.is_some_and(safe_stem) {
                self.problems.push(problem(&path, "A component's file name can have only letters, digits, _ and -, and at most 64 of them"));
                continue;
            }
            let (tag, hash) = match self.read_source(&path) {
                Ok(read) => read,
                Err(refused) => {
                    self.problems.push(refused);
                    // Its build, if it has one, still runs, out of date.
                    if let Some(stem) = stem {
                        self.refused_sources.insert(stem.to_string(), path.clone());
                    }
                    continue;
                }
            };
            named.insert(tag.clone());
            if is_reserved(&tag, registry) {
                self.problems.push(problem(&path, format!("{tag} is the name of a component every talk has, so this one can't be used")));
                continue;
            }
            if let Some(first) = claimed.get(&tag) {
                self.problems.push(problem(&path, format!("{tag} is already the component in {first}, so this one is left out")));
                continue;
            }
            let built: Vec<(&str, Build)> = index
                .components
                .iter()
                .filter(|entry| entry.tag == tag)
                .filter_map(|entry| {
                    let path = build_path(&entry.file);
                    let identity = self.build_identity(&path)?;
                    Some((entry.source_hash.as_str(), Build { path, identity }))
                })
                .collect();
            let (state, build) = match built.iter().find(|(built_from, _)| *built_from == hash) {
                Some((_, build)) => (ComponentState::UpToDate, Some(build.clone())),
                None => match built.into_iter().next() {
                    Some((_, build)) => (ComponentState::OutOfDate, Some(build)),
                    None => (ComponentState::NeedsBuilding, None),
                },
            };
            claimed.insert(tag.clone(), path.clone());
            self.infos.push(ComponentInfo { tag: tag.clone(), file: path, state });
            self.runnable.insert(tag, Runnable { state, build });
        }
        named
    }

    /// The builds the talk has no source for, which run as they are.
    fn add_builds_with_no_source(&mut self, index: &BuildIndex, named: &HashSet<String>, registry: &Registry) {
        for entry in &index.components {
            if named.contains(&entry.tag) || self.runnable.contains_key(&entry.tag) {
                continue;
            }
            let path = build_path(&entry.file);
            let Some(identity) = self.build_identity(&path) else {
                continue;
            };
            if is_reserved(&entry.tag, registry) {
                let message = format!("{} is the name of a component every talk has, so this one can't be used", entry.tag);
                self.problems.push(problem(&path, message));
                continue;
            }
            if self.infos.len() == MAX_COMPONENTS {
                self.problems.push(problem(INDEX_FILE, format!("A talk can have at most {MAX_COMPONENTS} components, so some builds are left out")));
                return;
            }
            // A build whose source is there but refused is from an older source, as far as can be told.
            let (state, file) = match self.refused_sources.get(&entry.file) {
                Some(source) => (ComponentState::OutOfDate, source.clone()),
                None if self.left_out_sources.contains(&entry.file) => (ComponentState::OutOfDate, path.clone()),
                None => (ComponentState::UpToDate, path.clone()),
            };
            self.infos.push(ComponentInfo { tag: entry.tag.clone(), file, state });
            let build = Build { path, identity };
            self.runnable.insert(entry.tag.clone(), Runnable { state, build: Some(build) });
        }
    }
}

/// What `read` makes of the file at `path`, from `seen` if the file's stamp is the one it
/// had then. Files that never change are read once, and any other file with no stamp is
/// read every time.
fn remembered<T: Clone>(seen: &mut HashMap<String, Seen<T>>, files: &dyn TalkFiles, path: &str, read: impl FnOnce() -> T) -> T {
    let stamp = if files.never_changes() { Some((0, SystemTime::UNIX_EPOCH)) } else { files.stamp(path) };
    let Some(stamp) = stamp else {
        seen.remove(path);
        return read();
    };
    if let Some(earlier) = seen.get(path).filter(|earlier| earlier.stamp == stamp) {
        return earlier.value.clone();
    }
    let value = read();
    seen.insert(path.to_string(), Seen { stamp, value: value.clone() });
    value
}
