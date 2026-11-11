//! Building a talk's own components into its `build` folder.
//!
//! Only what the engine's scan accepts is built, read through [`FolderFiles`] so that a link
//! can't reach outside the talk, and nothing is written through a link either. A build that
//! fails changes nothing in `build`, so the talk keeps its last good one.

use std::io::{self, Write as _};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use slides_components::{
    BuildEntry, BuildIndex, Bundle, Diagnostic, INDEX_FILE, MAX_MODULE_BYTES, component_tag,
    source_hash,
};
use slides_engine::{
    COMPONENTS_FOLDER, ComponentInfo, ComponentState, FolderFiles, MAX_INDEX_BYTES,
    MAX_SOURCE_BYTES, TalkComponents, TalkFiles, write_atomically,
};

/// Turns a component's source into its module, or says what's wrong with it. It's given the
/// source's path within the talk, such as `components/score.rs`, and its text.
pub type Compile = Arc<dyn Fn(&str, &str) -> Result<Vec<u8>, Vec<Diagnostic>> + Send + Sync>;

/// What came of building a talk's components.
#[derive(Debug, Default)]
pub struct BuildOutcome {
    /// The tags of the components that were built.
    pub built: Vec<String>,
    /// What stopped the others.
    pub problems: Vec<Diagnostic>,
}

const BUILD_FOLDER: &str = "build";
const INDEX_FORMAT: u32 = 1;

/// The compiler that ships with the app, if this copy of it has one.
pub fn bundled_compiler() -> Option<Compile> {
    let bundle = Bundle::find()?;
    Some(Arc::new(move |file: &str, source: &str| {
        slides_components::compile(&bundle, file, source)
    }))
}

/// The talk's components as the engine's scan finds them: only sources it accepts, held to
/// its limits.
fn scan(folder: &Path) -> (Rc<dyn TalkFiles>, Vec<ComponentInfo>) {
    let files: Rc<dyn TalkFiles> = Rc::new(FolderFiles::new(folder.to_path_buf()));
    let infos = TalkComponents::scan(files.clone()).infos().to_vec();
    (files, infos)
}

/// Builds each of the talk's components in `folder` that is out of date or not built yet.
/// Each one that compiles is written to `build/<file>.wasm`, then entered in the index, which
/// keeps its other entries.
pub fn build_components(folder: &Path, compile: &Compile) -> BuildOutcome {
    let (files, infos) = scan(folder);
    let mut outcome = BuildOutcome::default();
    for info in infos {
        if info.state == ComponentState::UpToDate {
            continue;
        }
        // Only sources come with a state other than up to date.
        let Some(stem) = info
            .file
            .strip_prefix(&format!("{COMPONENTS_FOLDER}/"))
            .and_then(|name| name.strip_suffix(".rs"))
        else {
            continue;
        };
        match build_one(folder, &*files, &info, stem, compile) {
            Ok(true) => outcome.built.push(info.tag),
            Ok(false) => {}
            Err(problems) => outcome.problems.extend(problems),
        }
    }
    outcome
}

fn problem(file: &str, message: impl Into<String>) -> Vec<Diagnostic> {
    vec![Diagnostic::new(file, 0, message.into())]
}

fn source_too_large(file: &str) -> Vec<Diagnostic> {
    let limit = MAX_SOURCE_BYTES / (1024 * 1024);
    problem(
        file,
        format!("It is over the limit of {limit} MiB for a component's source"),
    )
}

/// Builds the component `info`, from `components/<stem>.rs`. Returns whether it was built: a
/// source that changed into something else since the scan is left for the next one.
fn build_one(
    folder: &Path,
    files: &dyn TalkFiles,
    info: &ComponentInfo,
    stem: &str,
    compile: &Compile,
) -> Result<bool, Vec<Diagnostic>> {
    let file = info.file.as_str();
    let too_large = || source_too_large(file);
    if files.len(file).is_some_and(|len| len > MAX_SOURCE_BYTES) {
        return Err(too_large());
    }
    let bytes = files
        .read(file)
        .ok_or_else(|| problem(file, "It can't be read"))?;
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        return Err(too_large());
    }
    let Ok(source) = std::str::from_utf8(&bytes) else {
        return Ok(false);
    };
    // The tag comes from the source as it's built, which must still be the one the scan
    // accepted: not a built-in's name, and not another source's.
    if component_tag(source).as_deref() != Some(info.tag.as_str()) {
        return Ok(false);
    }
    let module = compile(file, source)?;
    if module.len() > MAX_MODULE_BYTES {
        return Err(problem(
            file,
            format!(
                "Its build is over the limit of {} MiB, so it can't be used",
                MAX_MODULE_BYTES / (1024 * 1024)
            ),
        ));
    }
    let entry = BuildEntry {
        tag: info.tag.clone(),
        file: stem.to_string(),
        source_hash: source_hash(&bytes),
    };
    write_build(folder, &entry, &module).map_err(|error| {
        problem(
            file,
            format!("It was built, but the build couldn't be saved: {error}"),
        )
    })?;
    Ok(true)
}

/// Writes `module` as the build of `entry`, then enters it in the index in place of any
/// entry for the same component or the same file.
fn write_build(folder: &Path, entry: &BuildEntry, module: &[u8]) -> io::Result<()> {
    let build = folder.join(BUILD_FOLDER);
    match std::fs::symlink_metadata(&build) {
        Ok(found) if found.is_dir() => {}
        Ok(_) => return Err(not_followed("build isn't a folder of the talk's own")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => std::fs::create_dir(&build)?,
        Err(error) => return Err(error),
    }
    let wasm = build.join(format!("{}.wasm", entry.file));
    let index_file = folder.join(INDEX_FILE);
    refuse_a_link(&wasm)?;
    refuse_a_link(&index_file)?;

    let mut index = read_index(&index_file);
    index
        .components
        .retain(|kept| kept.tag != entry.tag && kept.file != entry.file);
    index.components.push(entry.clone());
    index.format = INDEX_FORMAT;
    let json = serde_json::to_vec_pretty(&index).map_err(io::Error::other)?;

    build_folder_is_the_talks(folder)?;
    write_atomically(&wasm, |out| out.write_all(module))?;
    build_folder_is_the_talks(folder)?;
    write_atomically(&index_file, |out| out.write_all(&json))
}

/// Fails unless the talk's `build` folder is where it seems to be, and not a link, even one to
/// somewhere else within the talk. Asked again right before each write, in case `build` was
/// swapped for a link since it was first looked at.
fn build_folder_is_the_talks(folder: &Path) -> io::Result<()> {
    let expected = folder.canonicalize()?.join(BUILD_FOLDER);
    if folder.join(BUILD_FOLDER).canonicalize()? == expected {
        Ok(())
    } else {
        Err(not_followed("build isn't a folder of the talk's own"))
    }
}

fn not_followed(why: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, why.to_string())
}

/// Fails if `path` is anything but a regular file or missing, so a link is never written
/// through, nor replaced.
fn refuse_a_link(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(found) if found.is_file() => Ok(()),
        Ok(_) => Err(not_followed(&format!(
            "{} isn't a file of the talk's own",
            path.file_name().unwrap_or_default().to_string_lossy()
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// The index as it is, or an empty one if it's missing, too large or can't be read, as the
/// scan already counts that as no builds at all. `file` is known not to be a link.
fn read_index(file: &Path) -> BuildIndex {
    std::fs::metadata(file)
        .ok()
        .filter(|found| found.is_file() && found.len() <= MAX_INDEX_BYTES)
        .and_then(|_| std::fs::read(file).ok())
        .filter(|bytes| bytes.len() as u64 <= MAX_INDEX_BYTES)
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or(BuildIndex {
            format: INDEX_FORMAT,
            components: Vec::new(),
        })
}

/// The tags of the talk's components in `folder` whose source no longer matches their build.
pub fn behind(folder: &Path) -> Vec<String> {
    scan(folder)
        .1
        .into_iter()
        .filter(|info| info.state == ComponentState::OutOfDate)
        .map(|info| info.tag)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_build_folder_must_be_the_talks_own() {
        let talk = tempfile::tempdir().unwrap();
        std::fs::create_dir(talk.path().join("build")).unwrap();
        assert!(build_folder_is_the_talks(talk.path()).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn a_build_folder_swapped_for_a_link_is_refused() {
        // A link made after the first check, even to a folder within the talk.
        for inside in [false, true] {
            let talk = tempfile::tempdir().unwrap();
            let elsewhere = tempfile::tempdir().unwrap();
            let target = if inside {
                std::fs::create_dir(talk.path().join("assets")).unwrap();
                talk.path().join("assets")
            } else {
                elsewhere.path().to_path_buf()
            };
            std::os::unix::fs::symlink(&target, talk.path().join("build")).unwrap();
            assert!(
                build_folder_is_the_talks(talk.path()).is_err(),
                "inside: {inside}"
            );
        }
    }

    #[test]
    fn the_limit_on_a_source_is_said_as_it_is() {
        assert_eq!(
            source_too_large("components/a.rs")[0].message,
            "It is over the limit of 1 MiB for a component's source"
        );
    }
}
