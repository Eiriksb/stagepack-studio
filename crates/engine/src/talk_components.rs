//! A talk's own components: finding which it has and which of them are built, and running
//! the builds in the sandbox. A talk is untrusted, so whatever it says about its builds
//! is checked, and every file it names is read within limits.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

use slides_components::{
    Assets, BuildIndex, ComponentError, Diagnostic, INDEX_FILE, MAX_MODULE_BYTES, MEMORY_LIMIT_BYTES, WasmComponent, component_tag,
    safe_stem, source_hash,
};
use toolkit::{ComponentSchema, Registry, RenderInput, RenderOutput, asset_path_allowed};

use crate::TalkFiles;
use crate::flow::TEXT_TAG;

/// How long the components on one slide can take to render, together, each time it's laid out.
pub const SLIDE_RENDER_BUDGET: Duration = Duration::from_secs(1);

const COMPONENTS_FOLDER: &str = "components";
const SOURCE_EXTENSION: &str = ".rs";
const MAX_COMPONENTS: usize = 256;
const MAX_SOURCE_BYTES: u64 = 1024 * 1024;
/// Far more than an index of `MAX_COMPONENTS` entries needs.
const MAX_INDEX_BYTES: u64 = 1024 * 1024;
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

/// What a source said, kept with the stamp it had so that it isn't read again unchanged.
type SourceRead = Result<(String, String), String>;

/// What reading a file came to, and the stamp the file had then.
struct Seen<T> {
    stamp: (u64, SystemTime),
    value: T,
}

/// A module, loaded once for as long as its bytes stay the same.
struct Loaded {
    identity: String,
    component: Result<WasmComponent, ComponentError>,
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
    /// The time renders have taken since the slide being laid out was started.
    spent: Duration,
}

struct NoFiles;

impl TalkFiles for NoFiles {
    fn read(&self, _: &str) -> Option<Vec<u8>> {
        None
    }
}

fn problem(file: &str, message: impl Into<String>) -> Diagnostic {
    Diagnostic { file: file.to_string(), line: 0, message: message.into() }
}

/// The bytes of `path`, held to the rules for a render's reads: only from the talk's
/// assets folder, no file over the limit, and no more than `ASSET_BUDGET` in all, counted
/// in `used`. A file whose length is known is refused before it's read.
pub(crate) fn read_asset(files: &dyn TalkFiles, path: &str, used: &Cell<u64>) -> Option<Vec<u8>> {
    if !asset_path_allowed(path) {
        return None;
    }
    let left = ASSET_BUDGET.saturating_sub(used.get());
    if files.len(path).is_some_and(|len| len > left) {
        return None;
    }
    let bytes = files.read(path)?;
    let len = bytes.len() as u64;
    if len > left {
        return None;
    }
    used.set(used.get() + len);
    Some(bytes)
}

/// The assets one render of a talk's component can read.
fn assets_for_a_render(files: &Rc<dyn TalkFiles>) -> Assets {
    let files = files.clone();
    let used = Cell::new(0);
    Rc::new(move |path: &str| read_asset(&*files, path, &used))
}

/// Whether `tag` is already taken, by the format or by a component every talk has.
fn is_reserved(tag: &str, registry: &Registry) -> bool {
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
            spent: Duration::ZERO,
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
        let before = self.summary();
        self.infos.clear();
        self.problems.clear();
        self.runnable.clear();
        self.identities_this_scan.clear();
        let index = self.read_index();
        let named = self.add_sources(&index, registry);
        self.add_builds_with_no_source(&index, &named, registry);

        let runnable = &self.runnable;
        self.loaded.retain(|path, loaded| {
            runnable.values().filter_map(|runnable| runnable.build.as_ref()).any(|build| build.path == *path && build.identity == loaded.identity)
        });
        let files = self.files.clone();
        let still_there = |path: &String| files.never_changes() || files.stamp(path).is_some();
        self.sources_seen.retain(|path, _| still_there(path));
        self.builds_seen.retain(|path, _| still_there(path));
        self.index_seen.retain(|path, _| still_there(path));
        self.summary() != before
    }

    fn summary(&self) -> Summary {
        let mut runnable: Vec<(String, Runnable)> = self.runnable.iter().map(|(tag, runnable)| (tag.clone(), runnable.clone())).collect();
        runnable.sort_by(|a, b| a.0.cmp(&b.0));
        (self.infos.clone(), self.problems.clone(), runnable)
    }

    /// Restarts the budget, for the slide about to be laid out.
    pub(crate) fn new_layout(&mut self) {
        self.spent = Duration::ZERO;
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
        let assets = assets_for_a_render(&self.files);
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
            let component = self.load(&build.path);
            self.loaded.insert(build.path.clone(), Loaded { identity: build.identity.clone(), component });
        }
        let component = self.loaded[&build.path].component.as_ref().map_err(Clone::clone)?;
        let name = &component.schema().name;
        if name != tag {
            return Err(ComponentError::Malformed(format!("it says it is {name}, not {tag}")));
        }
        Ok(component)
    }

    fn load(&self, path: &str) -> Result<WasmComponent, ComponentError> {
        if self.files.len(path).is_some_and(|len| len > MAX_MODULE_BYTES as u64) {
            return Err(ComponentError::TooLarge);
        }
        let bytes = self.files.read(path).ok_or_else(|| ComponentError::Malformed("its build can't be read".into()))?;
        WasmComponent::load(&bytes)
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
            let too_large = || Err("It is over the limit of 1 MiB for a component's source".to_string());
            if files.len(path).is_some_and(|len| len > MAX_SOURCE_BYTES) {
                return too_large();
            }
            let bytes = files.read(path).ok_or("It can't be read")?;
            if bytes.len() as u64 > MAX_SOURCE_BYTES {
                return too_large();
            }
            let text = std::str::from_utf8(&bytes).map_err(|_| "It isn't UTF-8 text")?;
            let tag = component_tag(text).ok_or("No component was found in it")?;
            Ok((tag, source_hash(&bytes)))
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
            paths.truncate(MAX_COMPONENTS);
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
                Err(message) => {
                    self.problems.push(problem(&path, message));
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
            self.infos.push(ComponentInfo { tag: entry.tag.clone(), file: path.clone(), state: ComponentState::UpToDate });
            let build = Build { path, identity };
            self.runnable.insert(entry.tag.clone(), Runnable { state: ComponentState::UpToDate, build: Some(build) });
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
