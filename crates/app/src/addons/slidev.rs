//! What the app gives the Slidev add-on and takes back from it: the files of a talk, each with a
//! name an add-on can be given, and for an import, a Slidev talk made for Stagepack read from
//! its zip. Nothing of the talk is run, and the file it's read from is never written to.

use std::collections::HashSet;
use std::io::Read as _;
use std::path::Path;

use addon_abi::{safe_name, Bundle, Notice, NoticeKind, Report, MAX_FILES};
use slides_components::addon::{Addon, MAX_BUNDLE_BYTES};
use slides_engine::{NewFile, STAGEPACK_MANIFEST};

/// The most names a notice of files left out lists, before it says how many more there are.
pub const LEFT_OUT_LISTED: usize = 20;
/// What an import says of Stagepack's own packages, which a talk made for it holds and the app
/// never reads.
pub const VENDOR_NOTICE: &str = "Stagepack's bundled packages (vendor/) aren't used.";
/// Where Stagepack keeps its packages in a talk.
const VENDOR: &str = "vendor/";
/// The talk's Markdown, which the add-on reads and writes.
pub(crate) const SLIDES: &str = "slides.md";
/// The most a talk's manifest is read to.
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

/// Holds the names of a talk's files to those an add-on can be given: each a safe name, and
/// none the same as another but for its letter case, which on Windows and macOS would be one
/// file. The rest are noted, to be said in one notice.
#[derive(Default)]
pub(crate) struct Names {
    seen: HashSet<String>,
    left_out: Vec<String>,
}

impl Names {
    /// Whether `name` can be given, noting it as left out if not.
    pub(crate) fn take(&mut self, name: &str) -> bool {
        // `unicode-normalization` isn't among the app's dependencies, so names are told apart
        // by their lower case alone.
        if safe_name(name) && self.seen.insert(name.to_lowercase()) {
            return true;
        }
        self.left_out.push(name.to_string());
        false
    }

    /// The one notice that names the files left out, if any were.
    pub(crate) fn notice(&self) -> Option<Notice> {
        let left = &self.left_out;
        if left.is_empty() {
            return None;
        }
        let mut names: Vec<String> = left.iter().take(LEFT_OUT_LISTED).map(|name| slides_components::shortened(&printable(name), 80)).collect();
        if left.len() > LEFT_OUT_LISTED {
            names.push(format!("and {} more", left.len() - LEFT_OUT_LISTED));
        }
        let text = if left.len() == 1 {
            format!("1 file was left out, as its name can't be given to the add-on: {}.", names.join(", "))
        } else {
            format!("{} files were left out, as their names can't be given to the add-on: {}.", left.len(), names.join(", "))
        };
        Some(Notice { slide: None, kind: NoticeKind::LeftOut, text })
    }
}

/// `name` with what can't be shown on a line as a `?`.
fn printable(name: &str) -> String {
    name.chars().map(|c| if c.is_control() { '?' } else { c }).collect()
}

/// Reads the Slidev talk made for Stagepack in `file` and has `addon` make a talk of it: its
/// Markdown, the files it shows (each under `assets/`), and what couldn't be carried over as it
/// was. The file is only read. Stagepack's manifest and its packages under `vendor/` aren't
/// given to the add-on, nor any file whose name can't be, and the report says so.
pub fn import_slidev(file: &Path, addon: &Addon) -> Result<(String, Vec<NewFile>, Report), String> {
    let (files, entry, notices) = read_talk(file)?;
    let args = serde_json::json!({ "entry": entry }).to_string();
    let (out, report) = addon.run("import", &args, files).map_err(|error| error.to_string())?;
    let mut slides = None;
    let mut assets = Vec::new();
    for (name, data) in out {
        if name == SLIDES {
            slides = Some(String::from_utf8(data).map_err(|_| "The add-on gave back slides that aren't text.".to_string())?);
        } else if name.starts_with("assets/") && safe_name(&name) {
            assets.push((name, data));
        }
    }
    let slides = slides.ok_or("The add-on gave back no slides.")?;
    let mut all = notices;
    all.extend(report.notices);
    Ok((slides, assets, Report { notices: all }))
}

/// The files of the talk in `file` to give the add-on, the Markdown its manifest names, and
/// what the app says itself of the files it leaves out.
fn read_talk(file: &Path) -> Result<(Bundle, String, Vec<Notice>), String> {
    let unreadable = || "The Slidev talk can't be read: it isn't a zip file, or it's damaged.".to_string();
    let opened = std::fs::File::open(file).map_err(|_| "The Slidev talk isn't there any more, or can't be read.".to_string())?;
    let mut zip = zip::ZipArchive::new(opened).map_err(|_| unreadable())?;
    // Room for the add-on's own two.
    if zip.len() > MAX_FILES - 2 {
        return Err(format!("The Slidev talk holds more than {} files, so it can't be imported.", MAX_FILES - 2));
    }
    let too_large = || format!("The Slidev talk's files come to more than {} MiB, so it can't be imported.", MAX_BUNDLE_BYTES >> 20);
    let mut names = Names::default();
    let mut vendor = false;
    let mut entry = SLIDES.to_string();
    let mut files = Bundle::new();
    let mut total: u64 = 0;
    for at in 0..zip.len() {
        let mut found = zip.by_index(at).map_err(|_| unreadable())?;
        let Ok(name) = std::str::from_utf8(found.name_raw()).map(str::to_string) else {
            names.left_out.push(String::from_utf8_lossy(found.name_raw()).into_owned());
            continue;
        };
        if found.is_dir() {
            continue;
        }
        if name == STAGEPACK_MANIFEST {
            entry = manifest_entry(&mut found).unwrap_or(entry);
            continue;
        }
        if name.starts_with(VENDOR) {
            vendor = true;
            continue;
        }
        if found.is_symlink() || found.encrypted() || !names.take(&name) {
            if found.is_symlink() || found.encrypted() {
                names.left_out.push(name);
            }
            continue;
        }
        let size = found.size();
        total = total.saturating_add(size).saturating_add(8 + name.len() as u64);
        if total > MAX_BUNDLE_BYTES as u64 {
            return Err(too_large());
        }
        let mut data = Vec::with_capacity(size as usize);
        // One byte past what it says it holds, so that a size the zip misstates is caught.
        (&mut found).take(size + 1).read_to_end(&mut data).map_err(|_| unreadable())?;
        if data.len() as u64 > size {
            return Err(unreadable());
        }
        files.push((name, data));
    }
    let mut notices = Vec::new();
    if vendor {
        notices.push(Notice { slide: None, kind: NoticeKind::LeftOut, text: VENDOR_NOTICE.to_string() });
    }
    notices.extend(names.notice());
    Ok((files, entry, notices))
}

/// The Markdown Stagepack's manifest names as the talk's, if it names one that can be.
fn manifest_entry(manifest: &mut impl std::io::Read) -> Option<String> {
    let mut bytes = Vec::new();
    manifest.take(MAX_MANIFEST_BYTES).read_to_end(&mut bytes).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let entry = value.get("entry")?.as_str()?;
    safe_name(entry).then(|| entry.to_string())
}
