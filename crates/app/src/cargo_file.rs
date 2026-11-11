//! A Cargo project for a talk's components, written only when asked for, so that a code
//! editor with Rust can check them and offer completions. The app itself never uses it: it
//! builds with its own compiler and no Cargo.
//!
//! The project is written in the app's data folder, never in the talk. Cargo, rustup and
//! editors look for their settings from the project's folder upwards, so nothing in or above
//! a talk is ever consulted, whatever arrives in it later. Each `[[example]]` names a
//! component's file in the talk by its absolute path.

use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use slides_components::{Bundle, source_hash};
use slides_engine::{COMPONENTS_FOLDER, FolderFiles, TalkComponents, write_atomically};

const CARGO_FILE: &str = "Cargo.toml";
/// The folder of the app's data folder that holds a project for each talk set up.
const PROJECTS_FOLDER: &str = "editor-projects";
/// How many hex digits of the hash of a talk's path tell its project apart from another
/// talk's of the same folder name.
const PATH_HASH_DIGITS: usize = 12;

/// What the app says once it has written the project at `project`.
pub fn written(project: &Path) -> String {
    format!(
        "Open {} in your code editor to get completions for this talk's components.",
        project.display()
    )
}

/// Where the app keeps the projects it writes: in its own data folder, if the system has one.
pub fn default_projects_root() -> Option<PathBuf> {
    Some(crate::permissions::BuildPermissions::app_folder()?.join(PROJECTS_FOLDER))
}

/// Where the bundle keeps the toolkit's source, beside the workspace that stands in for ours.
fn toolkit_source(bundle: &Bundle) -> PathBuf {
    bundle.root.join("toolkit-src").join("toolkit")
}

/// `text` as a TOML string.
fn toml_string(text: &str) -> String {
    let mut quoted = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            c if c.is_control() => quoted.push_str(&format!("\\u{:04X}", c as u32)),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

/// The paths of the sources the talk in `folder` accepts, in `components`, by file name.
fn component_files(folder: &Path) -> Vec<(String, PathBuf)> {
    let scan = TalkComponents::scan(Rc::new(FolderFiles::new(folder.to_path_buf())));
    let mut files: Vec<(String, PathBuf)> = scan
        .infos()
        .iter()
        .filter_map(|info| {
            let stem = info
                .file
                .strip_prefix(&format!("{COMPONENTS_FOLDER}/"))?
                .strip_suffix(".rs")?;
            Some((stem.to_string(), folder.join(&info.file)))
        })
        .collect();
    files.sort();
    files
}

fn cargo_text(toolkit: &str, components: &[(String, String)]) -> String {
    let mut text = String::from(
        "# Written by Stagepack Studio for a code editor, so it can check a talk's components\n\
         # and offer completions. The app doesn't use it: it builds them itself. It's written\n\
         # again each time the talk is set up for a code editor.\n\
         [package]\n\
         name = \"talk-components\"\n\
         version = \"0.0.0\"\n\
         edition = \"2024\"\n\
         publish = false\n\
         # Nothing but the talk's components: a talk can come from anyone.\n\
         build = false\n\
         autolib = false\n\
         autobins = false\n\
         autoexamples = false\n\
         autotests = false\n\
         autobenches = false\n\
         \n\
         # A workspace of its own, so it isn't taken for part of one around it.\n\
         [workspace]\n\
         \n\
         [dependencies]\n",
    );
    text.push_str(&format!("toolkit = {{ path = {toolkit} }}\n"));
    for (name, path) in components {
        text.push_str(&format!(
            "\n[[example]]\nname = {}\npath = {}\ncrate-type = [\"cdylib\"]\n",
            toml_string(name),
            toml_string(path),
        ));
    }
    text
}

/// The folder of the project for the talk at `talk`, whose path is already canonical: the
/// talk's folder name and the start of the hash of its path, so that two talks of the same
/// name have projects of their own.
fn project_folder(projects_root: &Path, talk: &Path) -> PathBuf {
    let name = talk.file_name().map_or_else(|| "talk".into(), |name| name.to_string_lossy());
    let hash = source_hash(talk.as_os_str().as_encoded_bytes());
    projects_root.join(format!("{name}-{}", &hash[..PATH_HASH_DIGITS]))
}

fn as_text(path: &Path) -> Result<&str, String> {
    path.to_str()
        .ok_or_else(|| format!("{} is in a folder whose name a Cargo file can't hold.", path.display()))
}

/// Writes `<projects_root>/<name>/Cargo.toml` for the talk in `talk_folder`, naming the
/// bundle's toolkit source and each of the talk's components, by its path, as an example.
/// It's written afresh each time, so that it lists the components the talk has now. Nothing
/// is written in the talk. Gives the project's folder, or why nothing was written.
pub fn write_cargo_file(talk_folder: &Path, bundle: &Bundle, projects_root: &Path) -> Result<PathBuf, String> {
    let talk = talk_folder
        .canonicalize()
        .map_err(|error| format!("The talk's folder couldn't be found: {error}"))?;
    // Before anything is made, so that a refusal leaves nothing behind in the talk.
    if std::path::absolute(projects_root).is_ok_and(|root| root.starts_with(&talk) || root.starts_with(talk_folder)) {
        return Err("The folder for code editor projects is inside the talk, so nothing was written.".into());
    }
    std::fs::create_dir_all(projects_root)
        .map_err(|error| format!("The folder for code editor projects couldn't be made: {error}"))?;
    let root = projects_root
        .canonicalize()
        .map_err(|error| format!("The folder for code editor projects couldn't be found: {error}"))?;
    if root.starts_with(&talk) {
        return Err("The folder for code editor projects is inside the talk, so nothing was written.".into());
    }
    let toolkit = std::path::absolute(toolkit_source(bundle))
        .map_err(|error| format!("The toolkit's source couldn't be found: {error}"))?;
    if !toolkit.join(CARGO_FILE).is_file() {
        return Err("This copy of the app has no source for the toolkit to point a code editor at.".into());
    }
    let components = component_files(&talk)
        .into_iter()
        .map(|(name, path)| Ok((name, as_text(&path)?.to_string())))
        .collect::<Result<Vec<_>, String>>()?;
    let text = cargo_text(&toml_string(as_text(&toolkit)?), &components);
    let project = project_folder(&root, &talk);
    std::fs::create_dir_all(&project)
        .map_err(|error| format!("The project's folder couldn't be made: {error}"))?;
    write_atomically(&project.join(CARGO_FILE), |file| file.write_all(text.as_bytes()))
        .map_err(|error: io::Error| format!("{CARGO_FILE} couldn't be written: {error}"))?;
    Ok(project)
}
