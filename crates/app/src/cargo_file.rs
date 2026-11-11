//! A `Cargo.toml` for a talk's components, written only when asked for, so that a code
//! editor with Rust can check them and offer completions. The app itself never uses it: it
//! builds with its own compiler and no Cargo.

use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use slides_components::Bundle;
use slides_engine::{COMPONENTS_FOLDER, FolderFiles, TalkComponents, write_atomically};

const CARGO_FILE: &str = "Cargo.toml";

/// What the app says once it has written the file. The checks here are made only then, so it
/// says plainly what they can't cover.
pub const WRITTEN: &str = "Wrote Cargo.toml in the talk's folder. Open the folder in a code editor to check its components as you type. Only use a code editor on a talk you trust: Cargo and your editor now treat this folder as a Rust project, and files added later can make them run code.";

/// What Cargo or an editor would find in a talk's folder and act on once it's a Rust project,
/// each with why it's refused. A talk can come from anyone, and an editor checks the project as
/// soon as it's opened, so with any of these the talk's own code or settings would run outside
/// the sandbox.
const REFUSED: [(&str, &str); 14] = [
    (
        "build.rs",
        "a build.rs, which Cargo would run on your computer",
    ),
    (
        ".cargo",
        "a .cargo folder, whose settings could make Cargo run other programs on your computer",
    ),
    (
        "rust-toolchain",
        "a rust-toolchain file, which could make Cargo use a compiler the talk chose",
    ),
    (
        "rust-toolchain.toml",
        "a rust-toolchain.toml, which could make Cargo use a compiler the talk chose",
    ),
    (
        "src",
        "a src folder, whose code Cargo would build and could run on your computer",
    ),
    (
        "Cargo.lock",
        "a Cargo.lock, which could make Cargo fetch libraries the talk chose",
    ),
    (
        "target",
        "a target folder, which could hold builds Cargo would use in place of its own",
    ),
    (
        ".vscode",
        "a .vscode folder, whose settings could make your editor run programs on your computer",
    ),
    (
        ".zed",
        "a .zed folder, whose settings could make your editor run programs on your computer",
    ),
    (
        ".helix",
        "a .helix folder, whose settings could make your editor run programs on your computer",
    ),
    (
        ".idea",
        "an .idea folder, whose settings could make your editor run programs on your computer",
    ),
    (
        ".fleet",
        "a .fleet folder, whose settings could make your editor run programs on your computer",
    ),
    (
        "rust-analyzer.toml",
        "a rust-analyzer.toml, whose settings could make your editor run programs on your computer",
    ),
    (
        ".rust-analyzer.toml",
        "a .rust-analyzer.toml, whose settings could make your editor run programs on your computer",
    ),
];

/// What Cargo and rustup also read from every folder above the project.
const REFUSED_ABOVE: [&str; 4] = [
    ".cargo/config.toml",
    ".cargo/config",
    "rust-toolchain",
    "rust-toolchain.toml",
];

/// The person's own Cargo home, which Cargo reads whatever folder it's in.
fn default_cargo_home() -> Option<PathBuf> {
    match std::env::var_os("CARGO_HOME") {
        Some(home) => Some(PathBuf::from(home)),
        None => dirs::home_dir().map(|home| home.join(".cargo")),
    }
}

/// The first of [`REFUSED_ABOVE`] in a folder above `folder`, other than in `cargo_home`.
fn found_above(folder: &Path, cargo_home: Option<&Path>) -> Result<Option<PathBuf>, String> {
    let folder = folder
        .canonicalize()
        .map_err(|error| format!("The talk's folder couldn't be found: {error}"))?;
    let cargo_home = cargo_home.and_then(|home| home.canonicalize().ok());
    for above in folder.ancestors().skip(1) {
        for name in REFUSED_ABOVE {
            let found = above.join(name);
            if found.symlink_metadata().is_err() {
                continue;
            }
            let in_cargo_home = name.starts_with(".cargo/")
                && cargo_home.as_deref().is_some_and(|home| {
                    above.join(".cargo").canonicalize().ok().as_deref() == Some(home)
                });
            if !in_cargo_home {
                return Ok(Some(found));
            }
        }
    }
    Ok(None)
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

/// The file names in `components`, without `.rs`, of the sources the talk's scan accepts.
fn component_stems(folder: &Path) -> Vec<String> {
    let scan = TalkComponents::scan(Rc::new(FolderFiles::new(folder.to_path_buf())));
    let mut stems: Vec<String> = scan
        .infos()
        .iter()
        .filter_map(|info| {
            info.file
                .strip_prefix(&format!("{COMPONENTS_FOLDER}/"))?
                .strip_suffix(".rs")
                .map(String::from)
        })
        .collect();
    stems.sort();
    stems
}

fn cargo_text(toolkit: &str, stems: &[String]) -> String {
    let mut text = String::from(
        "# Written by Stagepack Studio for a code editor, so it can check the components\n\
         # and offer completions. The app doesn't use it: it builds them itself.\n\
         [package]\n\
         name = \"talk-components\"\n\
         version = \"0.0.0\"\n\
         edition = \"2024\"\n\
         publish = false\n\
         # Nothing in the talk's folder but its components: a talk can come from anyone.\n\
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
    for stem in stems {
        text.push_str(&format!(
            "\n[[example]]\nname = {}\npath = {}\ncrate-type = [\"cdylib\"]\n",
            toml_string(stem),
            toml_string(&format!("{COMPONENTS_FOLDER}/{stem}.rs")),
        ));
    }
    text
}

/// Writes `Cargo.toml` at the top of the talk in `folder`, naming the bundle's toolkit source
/// and each of the talk's components as an example. A `Cargo.toml` already there is left
/// alone. Gives the path written, or why nothing was.
pub fn write_cargo_file(folder: &Path, bundle: &Bundle) -> Result<PathBuf, String> {
    write_cargo_file_with_cargo_home(folder, bundle, default_cargo_home().as_deref())
}

/// As [`write_cargo_file`], with `cargo_home` as the person's Cargo home, whose settings
/// don't count against a talk below it.
pub fn write_cargo_file_with_cargo_home(
    folder: &Path,
    bundle: &Bundle,
    cargo_home: Option<&Path>,
) -> Result<PathBuf, String> {
    let out = folder.join(CARGO_FILE);
    // A link counts as a file, as writing would replace it.
    if out.symlink_metadata().is_ok() {
        return Err(format!(
            "This talk already has a {CARGO_FILE}, so it has been left as it is."
        ));
    }
    // A link counts too, whether or not what it points at is there.
    if let Some((name, what)) = REFUSED
        .iter()
        .find(|(name, _)| folder.join(name).symlink_metadata().is_ok())
    {
        return Err(format!(
            "This talk has {what}, so it hasn't been set up for a code editor. Remove {name} first if you trust it isn't needed."
        ));
    }
    if let Some(found) = found_above(folder, cargo_home)? {
        return Err(format!(
            "There's a {} above this talk, at {}. Cargo would read it, and it could make Cargo run other programs on your computer, so this talk hasn't been set up for a code editor.",
            found.file_name().unwrap_or_default().to_string_lossy(),
            found.display()
        ));
    }
    let toolkit = std::path::absolute(toolkit_source(bundle))
        .map_err(|error| format!("The toolkit's source couldn't be found: {error}"))?;
    if !toolkit.join(CARGO_FILE).is_file() {
        return Err(
            "This copy of the app has no source for the toolkit to point a code editor at.".into(),
        );
    }
    let toolkit = toolkit
        .to_str()
        .ok_or("The toolkit's source is in a folder whose name a Cargo file can't hold.")?;
    let text = cargo_text(&toml_string(toolkit), &component_stems(folder));
    write_atomically(&out, |file| file.write_all(text.as_bytes()))
        .map_err(|error: io::Error| format!("{CARGO_FILE} couldn't be written: {error}"))?;
    Ok(out)
}
