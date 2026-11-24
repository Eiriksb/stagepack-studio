//! The licence notices: every crate in the app's and the helper's dependency graph, the fonts, the Rust toolchain and OpenH264,
//! then, under a heading of its own, every crate built into the Slidev add-on.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The two programs whose dependency graphs are shipped.
pub const ROOTS: [&str; 2] = ["stagepack-studio", "slides-terminal-helper"];
/// The platforms shipped.
pub const PLATFORMS: [&str; 2] = ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc"];
/// The Slidev add-on, whose dependency graph is shipped inside its `.spaddon`. The package holds
/// only its three files, so its notices are in the app's, under [`ADDON_HEADING`].
pub const ADDON_ROOT: &str = "slidev-addon";
/// The platform the add-on is built for.
pub const ADDON_PLATFORM: &str = "wasm32-unknown-unknown";
/// The heading the add-on's notices are under.
pub const ADDON_HEADING: &str = "The Slidev add-on";

/// A notice that is not a crate: a name, its licence and the licence texts.
#[derive(Clone, Debug)]
pub struct Notice {
    pub name: String,
    pub licence: String,
    pub texts: Vec<String>,
}

/// A package in a dependency graph.
#[derive(Clone, Debug, PartialEq)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub licence: Option<String>,
    pub dir: PathBuf,
    pub licence_file: Option<String>,
}

impl Package {
    pub fn label(&self) -> String {
        format!("{} {}", self.name, self.version)
    }
}

fn parse(json: &[u8]) -> Result<Value, String> {
    serde_json::from_slice(json).map_err(|e| format!("cargo metadata is not JSON: {e}"))
}

/// Every package reached from the roots over normal dependencies only.
pub fn graph(metadata_json: &[u8]) -> Result<Vec<Package>, String> {
    graph_of(metadata_json, &ROOTS)
}

/// Every package built into the Slidev add-on: of those reached from [`ADDON_ROOT`] in
/// `metadata_json`, the ones `built` names, as `name version`. `cargo metadata` gives the
/// workspace's features all at once, so its graph holds what other members turn on in the
/// add-on's crates too; [`run_addon_tree`] gives what the add-on alone is built with.
pub fn addon_graph(metadata_json: &[u8], built: &BTreeSet<String>) -> Result<Vec<Package>, String> {
    let graph = graph_of(metadata_json, &[ADDON_ROOT])?;
    if let Some(missing) = built.iter().find(|label| !graph.iter().any(|p| p.label() == **label)) {
        return Err(format!("{missing} is built into the add-on but isn't in its graph"));
    }
    Ok(graph.into_iter().filter(|p| built.contains(&p.label())).collect())
}

/// The packages the add-on alone is built with for [`ADDON_PLATFORM`], over normal
/// dependencies, as `name version`: what `cargo tree` resolves for it, features and all.
pub fn run_addon_tree() -> Result<BTreeSet<String>, String> {
    let out = Command::new(tool("CARGO", "cargo"))
        .args(["tree", "--locked", "-p", ADDON_ROOT, "--target", ADDON_PLATFORM, "-e", "normal", "--prefix", "none", "-f", "{p}"])
        .output()
        .map_err(|e| format!("can't run cargo tree: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo tree for {ADDON_ROOT} failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let built: BTreeSet<String> = text
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let (name, version) = (words.next()?, words.next()?.strip_prefix('v')?);
            Some(format!("{name} {version}"))
        })
        .collect();
    if built.is_empty() {
        return Err(format!("cargo tree gave no packages for {ADDON_ROOT}"));
    }
    Ok(built)
}

/// Every package reached from `roots` over normal dependencies only.
fn graph_of(metadata_json: &[u8], roots: &[&str]) -> Result<Vec<Package>, String> {
    let meta = parse(metadata_json)?;
    let packages = meta["packages"].as_array().ok_or("cargo metadata has no packages")?;
    let mut by_id: BTreeMap<&str, &Value> = BTreeMap::new();
    for p in packages {
        by_id.insert(p["id"].as_str().ok_or("a package has no id")?, p);
    }
    let nodes = meta["resolve"]["nodes"].as_array().ok_or("cargo metadata has no resolve graph")?;
    let mut deps: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for n in nodes {
        let id = n["id"].as_str().ok_or("a node has no id")?;
        let mut list = Vec::new();
        for d in n["deps"].as_array().into_iter().flatten() {
            // Only edges that are a normal dependency for some target: dev and build edges are not shipped.
            let normal = d["dep_kinds"].as_array().is_some_and(|ks| ks.iter().any(|k| k["kind"].is_null()));
            if normal {
                list.push(d["pkg"].as_str().ok_or("a dependency has no id")?);
            }
        }
        deps.insert(id, list);
    }
    // Proc-macro crates (and what they pull in: syn, quote, serde_derive) are kept on purpose: the compiler
    // bundle ships libserde_derive and libtoolkit_macros as .so files, so they are part of what is shipped.
    let mut todo: Vec<&str> = packages
        .iter()
        .filter(|p| roots.contains(&p["name"].as_str().unwrap_or("")))
        .filter_map(|p| p["id"].as_str())
        .collect();
    if todo.is_empty() {
        return Err(format!("none of {roots:?} is in the workspace"));
    }
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    while let Some(id) = todo.pop() {
        if !seen.insert(id) {
            continue;
        }
        todo.extend(deps.get(id).into_iter().flatten().copied());
    }
    let mut out = Vec::new();
    for id in seen {
        let p = by_id.get(id).ok_or_else(|| format!("{id} is in the graph but not in the packages"))?;
        let manifest = p["manifest_path"].as_str().ok_or("a package has no manifest path")?;
        out.push(Package {
            name: p["name"].as_str().unwrap_or_default().to_string(),
            version: p["version"].as_str().unwrap_or_default().to_string(),
            licence: p["license"].as_str().filter(|s| !s.trim().is_empty()).map(str::to_string),
            dir: Path::new(manifest).parent().unwrap_or(Path::new("")).to_path_buf(),
            licence_file: p["license_file"].as_str().map(str::to_string),
        });
    }
    out.sort_by(|a, b| (&a.name, &a.version).cmp(&(&b.name, &b.version)));
    Ok(out)
}

fn is_licence_name(name: &str) -> bool {
    let up = name.to_ascii_uppercase();
    ["LICENSE", "LICENCE", "COPYING", "NOTICE"].iter().any(|p| up.starts_with(p))
}

/// Folders that never hold a package's own licence.
fn skipped_dir(name: &str) -> bool {
    matches!(name, "target" | "tests" | "test" | "node_modules" | ".git")
}

fn collect_licence_files(dir: &Path, rel: &str, depth: u32, names: &mut BTreeSet<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let path = e.path();
        let shown = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
        if path.is_file() && is_licence_name(&name) {
            names.insert(shown);
        } else if path.is_dir() && depth > 0 && !skipped_dir(&name) {
            collect_licence_files(&path, &shown, depth - 1, names);
        }
    }
}

/// The licence files in a package's folder (a few levels deep, skipping test and build folders), as (path, text), in path order.
fn licence_files(p: &Package) -> Vec<(String, String)> {
    let mut names: BTreeSet<String> = BTreeSet::new();
    collect_licence_files(&p.dir, "", 2, &mut names);
    if let Some(f) = &p.licence_file
        && p.dir.join(f).is_file()
    {
        names.insert(f.clone());
    }
    names
        .into_iter()
        .filter_map(|n| std::fs::read(p.dir.join(&n)).ok().map(|b| (n, String::from_utf8_lossy(&b).into_owned())))
        .collect()
}

/// The SPDX ids in a licence expression (exceptions after WITH included).
fn spdx_ids(expr: &str) -> Vec<String> {
    let cleaned = expr.replace(['(', ')', '/'], " ");
    let mut ids: Vec<String> = Vec::new();
    for t in cleaned.split_whitespace() {
        if matches!(t, "AND" | "OR" | "WITH" | "and" | "or" | "with") {
            continue;
        }
        let id = t.trim_end_matches('+').to_string();
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

/// The toolchain's sysroot.
fn sysroot() -> Result<PathBuf, String> {
    let rustc = tool("RUSTC", "rustc");
    let out = Command::new(&rustc).args(["--print", "sysroot"]).output().map_err(|e| format!("can't run {rustc}: {e}"))?;
    if !out.status.success() {
        return Err(format!("{rustc} --print sysroot failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    Ok(PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
}

/// The folder of the toolchain's standard licence texts, if it can be found.
fn toolchain_licence_dir(sysroot: &Path) -> Option<PathBuf> {
    let mut dirs = vec![sysroot.join("share/doc/rust/licenses")];
    if let Ok(rd) = std::fs::read_dir(sysroot.join("lib/rustlib/src/rust")) {
        dirs.extend(rd.flatten().map(|e| e.path()));
    }
    dirs.into_iter().find(|d| d.join("Apache-2.0.txt").is_file() && d.join("MIT.txt").is_file())
}

/// The standard text of an SPDX id, from the toolchain's licence texts, when it has one.
fn standard_text(id: &str) -> Option<String> {
    let dir = toolchain_licence_dir(&sysroot().ok()?)?;
    std::fs::read(dir.join(format!("{id}.txt"))).ok().map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// The toolchain's full Rust, LLVM and third-party attributions, shipped beside the notices.
pub fn toolchain_copyright() -> Result<PathBuf, String> {
    let sysroot = sysroot()?;
    let file = sysroot.join("share/doc/rust/COPYRIGHT.html");
    if file.is_file() {
        Ok(file)
    } else {
        Err(format!("the Rust toolchain's COPYRIGHT.html is missing from {}", file.parent().unwrap_or(&sysroot).display()))
    }
}

/// The packages' notices, those with the same licence and the same texts listed together.
fn grouped(packages: impl IntoIterator<Item = Package>) -> Result<String, String> {
    let mut groups: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    {
        for p in packages {
            if !seen.insert(p.label()) {
                continue;
            }
            let files = licence_files(&p);
            if p.licence.is_none() && files.is_empty() {
                return Err(format!("{} has no licence field and no licence file", p.label()));
            }
            let expr = p.licence.clone().unwrap_or_else(|| "(no licence expression)".to_string());
            let mut body = String::new();
            if files.is_empty() {
                body.push_str("No licence file came with this package; the standard text of its licence follows where available.\n");
                for id in spdx_ids(&expr) {
                    if let Some(text) = standard_text(&id) {
                        body.push_str(&format!("\n--- {id} (standard text) ---\n{}\n", text.trim_end()));
                    }
                }
            }
            for (name, text) in files {
                body.push_str(&format!("\n--- {name} ---\n{}\n", text.trim_end()));
            }
            groups.entry((expr, body)).or_default().insert(p.label());
        }
    }
    let mut out = String::new();
    for ((expr, body), names) in &groups {
        out.push_str("\n================================================================\n");
        out.push_str(&format!("Packages: {}\nLicence: {expr}\n{body}", names.iter().cloned().collect::<Vec<_>>().join(", ")));
    }
    Ok(out)
}

/// The notices for crates in the given platforms' metadata, then `extra`.
pub fn notices_multi(metadata: &[&[u8]], extra: &[Notice]) -> Result<String, String> {
    notices_with_addon(metadata, None, extra)
}

/// The notices for crates in the given platforms' metadata, then `extra`, then, under
/// [`ADDON_HEADING`], `addon`, the crates built into the Slidev add-on.
pub fn notices_with_addon(metadata: &[&[u8]], addon: Option<Vec<Package>>, extra: &[Notice]) -> Result<String, String> {
    let mut packages = Vec::new();
    for m in metadata {
        packages.extend(graph(m)?);
    }
    let mut out = String::from(
        "Stagepack Studio: licence notices\n\nThe packages below are part of this program. Each is listed with its licence and the licence files that came with it.\n",
    );
    out.push_str(&grouped(packages)?);
    for n in extra {
        out.push_str("\n================================================================\n");
        out.push_str(&format!("{}\nLicence: {}\n", n.name, n.licence));
        for t in &n.texts {
            out.push_str(&format!("\n{}\n", t.trim_end()));
        }
    }
    if let Some(addon) = addon {
        out.push_str("\n################################################################\n");
        out.push_str(&format!(
            "{ADDON_HEADING}\n\nThe packages below are built into the Slidev add-on, the WebAssembly module in its .spaddon file, which runs inside this program.\n"
        ));
        out.push_str(&grouped(addon)?);
    }
    Ok(out)
}

/// The notices for one platform's metadata, then `extra`.
pub fn notices(metadata_json: &[u8], extra: &[Notice]) -> Result<String, String> {
    notices_multi(&[metadata_json], extra)
}

fn tool(var: &str, default: &str) -> String {
    std::env::var(var).ok().filter(|s| !s.is_empty()).unwrap_or_else(|| default.to_string())
}

/// `cargo metadata` for a platform, locked.
pub fn run_metadata(platform: &str) -> Result<Vec<u8>, String> {
    let out = Command::new(tool("CARGO", "cargo"))
        .args(["metadata", "--format-version", "1", "--locked", "--filter-platform", platform])
        .output()
        .map_err(|e| format!("can't run cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata for {platform} failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    Ok(out.stdout)
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned()).map_err(|e| format!("can't read {}: {e}", path.display()))
}

/// The fonts', the toolchain's (and LLVM's) and OpenH264's notices.
pub fn extra_notices(metadata_json: &[u8]) -> Result<Vec<Notice>, String> {
    let meta = parse(metadata_json)?;
    let root = PathBuf::from(meta["workspace_root"].as_str().ok_or("cargo metadata has no workspace root")?);
    let mut extra = Vec::new();

    extra.push(Notice {
        name: "Stagepack Studio".into(),
        licence: "MIT".into(),
        texts: vec![read(&root.join("LICENSE"))?],
    });

    let fonts = root.join("crates/engine/assets/fonts");
    extra.push(Notice {
        name: "Fonts: Inter, JetBrains Mono and Noto Color Emoji".into(),
        licence: "OFL-1.1".into(),
        texts: ["Inter-LICENSE.txt", "JetBrainsMono-OFL.txt", "NotoColorEmoji-OFL.txt"]
            .iter()
            .map(|f| read(&fonts.join(f)).map(|t| format!("--- {f} ---\n{t}")))
            .collect::<Result<_, _>>()?,
    });

    // The toolchain's own files: the standard library and the LLVM it was built with.
    let sysroot = sysroot()?;
    let dir = toolchain_licence_dir(&sysroot)
        .ok_or_else(|| format!("the Rust toolchain's licence files are not in {}/share/doc/rust/licenses", sysroot.display()))?;
    toolchain_copyright()?;
    let mut texts = Vec::new();
    for f in ["Apache-2.0.txt", "MIT.txt", "LLVM-exception.txt", "NCSA.txt", "BSD-2-Clause.txt", "ISC.txt", "Unicode-3.0.txt"] {
        let path = dir.join(f);
        if path.is_file() {
            texts.push(format!("--- {f} ---\n{}", read(&path)?));
        } else if matches!(f, "Apache-2.0.txt" | "MIT.txt" | "LLVM-exception.txt") {
            return Err(format!("the Rust toolchain's {f} is missing from {}", dir.display()));
        }
    }
    texts.insert(
        0,
        "Copyright (c) The Rust Project Contributors.\n\nThe compiler bundle contains the Rust compiler, the standard library and LLVM. Their full attributions, \nincluding the third-party code built into them, are in the file notices/rust-COPYRIGHT.html beside this one.".to_string(),
    );
    extra.push(Notice {
        name: "The Rust toolchain: the standard library and the LLVM it carries".into(),
        licence: "MIT OR Apache-2.0 (the standard library); Apache-2.0 WITH LLVM-exception (LLVM)".into(),
        texts,
    });

    // OpenH264's source, bundled by openh264-sys2.
    let sys2 = graph(metadata_json)?.into_iter().find(|p| p.name == "openh264-sys2").ok_or("openh264-sys2 is not in the dependency graph")?;
    let file = sys2.dir.join("upstream/LICENSE");
    extra.push(Notice {
        name: "OpenH264 (the H.264 decoder, built from source bundled in openh264-sys2)".into(),
        licence: "BSD-2-Clause".into(),
        texts: vec![read(&file)?],
    });
    Ok(extra)
}

/// Every notice, from this workspace as `cargo metadata` sees it for both platforms.
pub fn notices_for_workspace() -> Result<String, String> {
    let metas: Vec<Vec<u8>> = PLATFORMS.iter().map(|p| run_metadata(p)).collect::<Result<_, _>>()?;
    let addon = addon_graph(&run_metadata(ADDON_PLATFORM)?, &run_addon_tree()?)?;
    let extra = extra_notices(&metas[0])?;
    let refs: Vec<&[u8]> = metas.iter().map(Vec::as_slice).collect();
    notices_with_addon(&refs, Some(addon), &extra)
}

/// Writes the notices to `file` and the toolchain's COPYRIGHT.html to `notices/rust-COPYRIGHT.html` beside it.
pub fn write_notices(file: &Path) -> Result<(), String> {
    let text = notices_for_workspace()?;
    let html = toolchain_copyright()?;
    let parent = file.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let sub = parent.join("notices");
    std::fs::create_dir_all(&sub).map_err(|e| format!("can't create {}: {e}", sub.display()))?;
    std::fs::copy(&html, sub.join("rust-COPYRIGHT.html")).map_err(|e| format!("can't copy {}: {e}", html.display()))?;
    std::fs::write(file, text).map_err(|e| format!("can't write {}: {e}", file.display()))
}
