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
        if safe_name(name) && self.seen.insert(folded(name)) {
            return true;
        }
        self.left_out.push(name.to_string());
        false
    }

    /// The one notice that names the files left out, if any were.
    pub(crate) fn notice(&self) -> Option<Notice> {
        self.notice_saying(
            "1 file was left out, as its name can't be given to the add-on",
            "files were left out, as their names can't be given to the add-on",
        )
    }

    /// The one notice that names the files the add-on gave back and the talk can't hold, if any.
    pub(crate) fn given_back_notice(&self) -> Option<Notice> {
        self.notice_saying(
            "1 file the add-on gave back was left out, as its name can't be one of the talk's",
            "files the add-on gave back were left out, as their names can't be the talk's",
        )
    }

    /// The notice, saying `one` of one file and `many`, after the count, of more.
    fn notice_saying(&self, one: &str, many: &str) -> Option<Notice> {
        let left = &self.left_out;
        if left.is_empty() {
            return None;
        }
        let mut names: Vec<String> = left.iter().take(LEFT_OUT_LISTED).map(|name| slides_components::shortened(&printable(name), 80)).collect();
        if left.len() > LEFT_OUT_LISTED {
            names.push(format!("and {} more", left.len() - LEFT_OUT_LISTED));
        }
        let text = if left.len() == 1 { format!("{one}: {}.", names.join(", ")) } else { format!("{} {many}: {}.", left.len(), names.join(", ")) };
        Some(Notice { slide: None, kind: NoticeKind::LeftOut, text })
    }
}

/// `name` as a file system that ignores letter case and normalisation sees it, as macOS and
/// Windows do: composed (NFC), then in lower case. Two names with the same form are one file there.
pub(crate) fn folded(name: &str) -> String {
    icu_normalizer::ComposingNormalizerBorrowed::new_nfc().normalize(name).to_lowercase()
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
    // Two names the same but for letter case or normalisation would be one file once the talk
    // is saved on macOS or Windows, so the second is left out.
    let mut given = Names::default();
    for (name, data) in out {
        if name == SLIDES {
            slides = Some(String::from_utf8(data).map_err(|_| "The add-on gave back slides that aren't text.".to_string())?);
        } else if name.starts_with("assets/") && given.take(&name) {
            assets.push((name, data));
        }
    }
    let slides = slides.ok_or("The add-on gave back no slides.")?;
    let mut all = notices;
    all.extend(given.given_back_notice());
    all.extend(report.notices);
    Ok((slides, assets, Report { notices: all }))
}

/// The files of the talk in `file` to give the add-on, the Markdown its manifest names, and
/// what the app says itself of the files it leaves out.
fn read_talk(file: &Path) -> Result<(Bundle, String, Vec<Notice>), String> {
    let unreadable = || "The Slidev talk can't be read: it isn't a zip file, or it's damaged.".to_string();
    let mut opened = std::fs::File::open(file).map_err(|_| "The Slidev talk isn't there any more, or can't be read.".to_string())?;
    // What the zip says of its directory, before the zip reader reads all of it in.
    let listed = listed_entries(&mut opened).ok_or_else(unreadable)?;
    let too_many = || format!("The Slidev talk's zip lists more than {MAX_LISTED} entries, so it can't be imported.");
    if listed.entries > MAX_LISTED {
        return Err(too_many());
    }
    if listed.directory_size > MAX_DIRECTORY_BYTES {
        return Err(format!("The Slidev talk's zip says its list of entries takes more than {} MiB, so it can't be imported.", MAX_DIRECTORY_BYTES >> 20));
    }
    let mut zip = zip::ZipArchive::new(opened).map_err(|_| unreadable())?;
    if zip.len() as u64 > MAX_LISTED {
        return Err(too_many());
    }
    // Room for the add-on's own two, counting only what is given to it.
    let too_many_files = || format!("The Slidev talk holds more than {} files, so it can't be imported.", MAX_FILES - 2);
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
        // As a file system that ignores letter case would find them.
        let lower = name.to_ascii_lowercase();
        if lower == STAGEPACK_MANIFEST {
            entry = manifest_entry(&mut found).unwrap_or(entry);
            continue;
        }
        if lower.starts_with(VENDOR) {
            vendor = true;
            continue;
        }
        if found.is_symlink() || found.encrypted() {
            names.left_out.push(name);
            continue;
        }
        if !names.take(&name) {
            continue;
        }
        if files.len() >= MAX_FILES - 2 {
            return Err(too_many_files());
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

/// The most entries a Slidev talk's zip may list, counting directories and Stagepack's own
/// packages under `vendor/`, which aren't given to the add-on. Far more than a talk holds, so
/// only a zip made to make the reader work for ever is refused.
const MAX_LISTED: u64 = 200_000;
/// The most a Slidev talk's zip may say its central directory takes, for [`MAX_LISTED`] entries
/// of long names.
const MAX_DIRECTORY_BYTES: u64 = 64 * 1024 * 1024;

/// What the end of a zip says of its directory: the most entries and the largest directory any
/// of its end records gives, the zip64 one where there is one.
struct Listed {
    entries: u64,
    directory_size: u64,
}

/// What the zip in `file` says of its directory, from its end records, or `None` if it has none
/// or names a zip64 record that isn't there.
fn listed_entries(file: &mut std::fs::File) -> Option<Listed> {
    use std::io::{Seek as _, SeekFrom};

    let len = file.seek(SeekFrom::End(0)).ok()?;
    let start = len.saturating_sub(addon_abi::package::END_SEARCH_BYTES as u64);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut tail = Vec::new();
    (&mut *file).take(len - start).read_to_end(&mut tail).ok()?;
    let records = addon_abi::package::end_records(&tail);
    let mut listed = Listed { entries: 0, directory_size: 0 };
    for record in &records {
        let (entries, size) = match record.zip64_at {
            None => (record.entries, record.directory_size),
            Some(at) => {
                // The zip64 end record: its signature, then the two counts at 24 and 32, and the
                // directory's size at 40.
                let mut zip64 = [0; 56];
                file.seek(SeekFrom::Start(at)).ok()?;
                file.read_exact(&mut zip64).ok()?;
                if zip64[..4] != [0x50, 0x4b, 0x06, 0x06] {
                    return None;
                }
                let field = |at: usize| u64::from_le_bytes(zip64[at..at + 8].try_into().expect("eight bytes"));
                (field(24).max(field(32)), field(40))
            }
        };
        listed.entries = listed.entries.max(entries);
        listed.directory_size = listed.directory_size.max(size);
    }
    file.seek(SeekFrom::Start(0)).ok()?;
    (!records.is_empty()).then_some(listed)
}

/// The Markdown Stagepack's manifest names as the talk's, if it names one that can be.
fn manifest_entry(manifest: &mut impl std::io::Read) -> Option<String> {
    let mut bytes = Vec::new();
    manifest.take(MAX_MANIFEST_BYTES).read_to_end(&mut bytes).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let entry = value.get("entry")?.as_str()?;
    safe_name(entry).then(|| entry.to_string())
}
