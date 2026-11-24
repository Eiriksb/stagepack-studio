//! Add-ons: programs the app runs from a signed `.spaddon` file, installed by the owner. There
//! is one, the Slidev add-on, kept in `<app data folder>/addons/slidev/` as the three files its
//! package held: `addon.json`, `addon.wasm` and `addon.sig`. Its signature is checked when it's
//! installed and again each time it's loaded, so a file changed since shows it as damaged.

pub mod bake;
pub mod dialog;
pub mod fetch;
pub mod key;
mod slidev;

use std::io::Read as _;
use std::path::{Path, PathBuf};

use addon_abi::package::{read_package, verify_parts, MANIFEST_ENTRY, MAX_MANIFEST_BYTES, MAX_PACKAGE_BYTES, MAX_WASM_BYTES, SIG_BYTES, SIG_ENTRY, WASM_ENTRY};
use slides_components::addon::Addon;

pub use slidev::{import_slidev, LEFT_OUT_LISTED, VENDOR_NOTICE};
pub(crate) use slidev::{Names, SLIDES as SLIDES_FILE};

/// Where add-ons are kept in the app's data folder.
const ADDONS_FOLDER: &str = "addons";
/// The one add-on's folder among them, by its name.
const SLIDEV_FOLDER: &str = "slidev";
/// The version of Slidev the app writes Stagepack's talks for: the one Stagepack bundles. An
/// add-on writing for another isn't installed.
pub const SLIDEV_VERSION: &str = "53.0.0";
/// What the Slidev add-on is, in the Add-ons dialog.
pub const SLIDEV_DESCRIPTION: &str = "Slidev — export to and import from Slidev (for Stagepack)";

/// The Slidev add-on as it's installed: its version, and whether it can be run.
#[derive(Debug)]
pub struct Installed {
    pub version: String,
    pub state: InstalledState,
}

#[derive(Debug)]
pub enum InstalledState {
    /// Checked and loaded, ready to run.
    Ready(Addon),
    /// It can't be used, for the reason the sentence gives. It can only be removed.
    Damaged(String),
}

/// What the Add-ons dialog and the menus show of the add-on, found without loading it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    NotInstalled,
    /// Its signature holds; this is its version.
    Installed(String),
    /// It can't be used, for the reason the sentence gives.
    Damaged(String),
}

/// The folder the Slidev add-on is kept in, in the app's data folder `app_folder`.
pub fn addon_folder(app_folder: &Path) -> PathBuf {
    app_folder.join(ADDONS_FOLDER).join(SLIDEV_FOLDER)
}

/// The three files of an installed add-on: `addon.json`, `addon.wasm` and `addon.sig`.
type Parts = (Vec<u8>, Vec<u8>, Vec<u8>);

/// The three files of the add-on installed under `app_folder`, read within the limits a
/// package holds them to, or why they can't be.
fn read_installed(app_folder: &Path) -> Result<Parts, String> {
    let folder = addon_folder(app_folder);
    let read = |name: &str, limit: u64| -> Result<Vec<u8>, String> {
        let file = std::fs::File::open(folder.join(name)).map_err(|_| format!("Its {name} is missing or can't be read."))?;
        let mut bytes = Vec::new();
        file.take(limit + 1).read_to_end(&mut bytes).map_err(|_| format!("Its {name} can't be read."))?;
        if bytes.len() as u64 > limit {
            return Err(format!("Its {name} is larger than an add-on's may be."));
        }
        Ok(bytes)
    };
    Ok((read(MANIFEST_ENTRY, MAX_MANIFEST_BYTES)?, read(WASM_ENTRY, MAX_WASM_BYTES)?, read(SIG_ENTRY, SIG_BYTES as u64)?))
}

/// The version `addon.json` names, read loosely, for a damaged add-on that can't be trusted.
fn named_version(manifest: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(manifest).ok()?;
    let version = value.get("version")?.as_str()?;
    Some(slides_components::shortened(version, 40))
}

/// The installed add-on's version, if its files are there and its signature holds, checked
/// with the key the app was built with, and the Slidev it writes for is the app's.
///
/// The files are read under the add-ons folder's lock, shared, so that an install swapping the
/// folder isn't seen half done, as `shared` says.
fn verified(app_folder: &Path, shared: Shared) -> Result<(String, Vec<u8>), (Option<String>, String)> {
    let addons = app_folder.join(ADDONS_FOLDER);
    let _shared = match shared {
        Shared::Wait => Lock::shared(&addons),
        Shared::Try => Lock::try_shared(&addons),
        Shared::Held => None,
    };
    let (manifest, wasm, sig) = read_installed(app_folder).map_err(|sentence| (None, sentence))?;
    drop(_shared);
    let version = named_version(&manifest);
    let checked = verify_parts(&manifest, &wasm, &sig, &key::addon_key()).map_err(|sentence| (version.clone(), sentence))?;
    if checked.slidev != SLIDEV_VERSION {
        return Err((version, wrong_slidev(&checked.slidev)));
    }
    Ok((checked.version, wasm))
}

/// How [`verified`] reads the installed files under the lock.
#[derive(Clone, Copy)]
enum Shared {
    /// Waits for an install going on to finish.
    Wait,
    /// Reads anyway if one is going on, for a look that is checked again once the files change.
    Try,
    /// The caller holds the lock already, shared or not, so it isn't taken again.
    Held,
}

/// What status and load say of an add-ons folder that is a link.
const LINKED_READ: &str = "The add-ons folder, or the Slidev add-on's, is a link to somewhere else, so the app won't use it.";

/// Whether the add-ons folder under `app_folder`, or the Slidev add-on's, is a link.
fn is_linked(app_folder: &Path) -> bool {
    refuse_links(&app_folder.join(ADDONS_FOLDER)).is_err()
}

fn wrong_slidev(slidev: &str) -> String {
    format!("The add-on writes for Slidev {}, where Stagepack's talks are for Slidev {SLIDEV_VERSION}.", slides_components::shortened(slidev, 40))
}

/// Whether anything of the add-on is installed under `app_folder`. An install cut short between
/// moving the earlier add-on aside and putting the new one in place is put back first.
fn is_there(app_folder: &Path) -> bool {
    let folder = addon_folder(app_folder);
    if std::fs::symlink_metadata(&folder).is_err() {
        put_back_cut_short(app_folder);
    }
    folder.exists()
}

/// When an install stopped after moving the earlier add-on aside, and before putting the new one
/// in its place, puts the earlier one back, if nothing else is installing now.
fn put_back_cut_short(app_folder: &Path) {
    let addons = app_folder.join(ADDONS_FOLDER);
    if !addons.is_dir() || refuse_links(&addons).is_err() {
        return;
    }
    let Ok(_lock) = Lock::try_take(&addons) else { return };
    put_back_cut_short_locked(app_folder);
}

/// What a look at the installed files went by: each one's length and when it was last changed,
/// and the key it was checked with.
type Stamps = (Vec<Option<(u64, std::time::SystemTime)>>, [u8; 32]);

/// The status found last for each app folder, with what it went by, so that a window opening
/// doesn't read and check the add-on again when nothing of it has changed.
static LOOKED_AT: std::sync::Mutex<Vec<(PathBuf, Stamps, Status)>> = std::sync::Mutex::new(Vec::new());

fn stamps(app_folder: &Path) -> Stamps {
    let folder = addon_folder(app_folder);
    let files = [MANIFEST_ENTRY, WASM_ENTRY, SIG_ENTRY]
        .iter()
        .map(|name| std::fs::metadata(folder.join(name)).ok().and_then(|metadata| Some((metadata.len(), metadata.modified().ok()?))))
        .collect();
    (files, key::addon_key())
}

/// What's installed under `app_folder`, found by checking its signature but without loading it,
/// which takes a moment longer. It's checked again only once one of its files has changed.
pub fn status(app_folder: &Path) -> Status {
    if is_linked(app_folder) {
        return Status::Damaged(LINKED_READ.to_string());
    }
    if !is_there(app_folder) {
        return Status::NotInstalled;
    }
    let stamped = stamps(app_folder);
    let mut looked_at = LOOKED_AT.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((_, _, status)) = looked_at.iter().find(|(folder, was, _)| folder == app_folder && *was == stamped) {
        return status.clone();
    }
    let status = match verified(app_folder, Shared::Try) {
        Ok((version, _)) => Status::Installed(version),
        Err((_, sentence)) => Status::Damaged(sentence),
    };
    looked_at.retain(|(folder, _, _)| folder != app_folder);
    looked_at.push((app_folder.to_path_buf(), stamped, status.clone()));
    status
}

/// The add-on installed under `app_folder`, checked and loaded, or `None` if there's none.
pub fn load(app_folder: &Path) -> Option<Installed> {
    load_with(app_folder, Shared::Wait)
}

/// The add-on installed under `app_folder`, checked and loaded, held in use until the [`InUse`]
/// is dropped, as an export or an import holds it while it runs. An install cut short is put back
/// first, and the lock is taken once only.
pub fn load_in_use(app_folder: &Path) -> (InUse, Option<Installed>) {
    if is_linked(app_folder) {
        return (InUse { _shared: None }, load(app_folder));
    }
    let held = in_use(app_folder);
    let loaded = load_with(app_folder, Shared::Held);
    (held, loaded)
}

/// [`load`], reading the files as `shared` says.
fn load_with(app_folder: &Path, shared: Shared) -> Option<Installed> {
    if is_linked(app_folder) {
        return Some(Installed { version: String::new(), state: InstalledState::Damaged(LINKED_READ.to_string()) });
    }
    if !is_there(app_folder) {
        return None;
    }
    Some(match verified(app_folder, shared) {
        Ok((version, wasm)) => match loaded(&wasm, &version) {
            Ok(addon) => Installed { version, state: InstalledState::Ready(addon) },
            Err(sentence) => Installed { version, state: InstalledState::Damaged(sentence) },
        },
        Err((version, sentence)) => Installed { version: version.unwrap_or_default(), state: InstalledState::Damaged(sentence) },
    })
}

/// The add-on `wasm`, loaded, if it is the Slidev add-on at `version`, as its signed `addon.json`
/// says, and does both its operations.
fn loaded(wasm: &[u8], version: &str) -> Result<Addon, String> {
    let addon = Addon::load(wasm).map_err(|error| error.to_string())?;
    let info = addon.info();
    if info.name != addon_abi::package::ADDON_NAME {
        return Err(format!("The add-on says it is {:?}, not the Slidev add-on.", slides_components::shortened(&info.name, 40)));
    }
    if info.version != version {
        return Err(format!(
            "The add-on says it is version {}, where its addon.json says {}.",
            slides_components::shortened(&info.version, 40),
            slides_components::shortened(version, 40)
        ));
    }
    for operation in ["export", "import"] {
        if !info.operations.iter().any(|known| known == operation) {
            return Err(format!("The add-on can't {operation}."));
        }
    }
    Ok(addon)
}

/// Installs the add-on in the package `file` under `app_folder`, in place of any earlier
/// version, and gives it back loaded. What's installed is what the package's signature was
/// checked over, written as it was checked. If anything is wrong, the sentence says what, and
/// whatever was installed before stays as it was.
pub fn install_from_file(app_folder: &Path, file: &Path) -> Result<Installed, String> {
    install_from_bytes(app_folder, &read_package_file(file)?)
}

/// Installs the add-on in the package `bytes` under `app_folder`, as [`install_from_file`]
/// does: its signature is checked first, and whatever was installed before stays as it was if
/// anything is wrong.
pub fn install_from_bytes(app_folder: &Path, bytes: &[u8]) -> Result<Installed, String> {
    install(app_folder, bytes, true)
}

/// [`install_from_bytes`], for *Get* and *Update*: unless an export, an import or another
/// install holds the add-on just now, when it says [`BUSY`] rather than wait; and only if the
/// package's signed version is later than what is installed by then, so that it never goes back.
/// Both are found under the lock, so no install from another window slips in between.
pub fn update_from_bytes(app_folder: &Path, bytes: &[u8]) -> Result<Installed, String> {
    install(app_folder, bytes, false)
}

/// Whether an export, an import, an install or a removal holds the add-on under `app_folder`
/// just now, so that an install would have to wait for it.
pub fn is_busy(app_folder: &Path) -> bool {
    let addons = app_folder.join(ADDONS_FOLDER);
    // A lock file that can't be opened isn't one held: the install says what's wrong.
    let Ok(file) = Lock::open(&addons) else { return false };
    addons.is_dir() && file.try_lock().is_err()
}

/// What an export or an import holds while it runs the add-on, so that no install or removal
/// swaps its files meanwhile: they wait for it, or say [`BUSY`]. It goes when it's dropped.
pub struct InUse {
    /// Held only so that the lock is; `None` if nothing was ever installed.
    _shared: Option<Lock>,
}

/// Holds the add-on under `app_folder` in use, once no install is going on, as an export or an
/// import does while it runs.
pub fn in_use(app_folder: &Path) -> InUse {
    // A linked folder is never loaded from, so nothing of it is held either.
    if is_linked(app_folder) {
        return InUse { _shared: None };
    }
    // Put back before it's held, as it can't be while it is.
    is_there(app_folder);
    InUse { _shared: Lock::shared(&app_folder.join(ADDONS_FOLDER)) }
}

/// [`install_from_bytes`], waiting for the lock if `wait`; or, if not, as [`update_from_bytes`]:
/// saying [`BUSY`] rather than wait, and installing only a later version.
fn install(app_folder: &Path, bytes: &[u8], wait: bool) -> Result<Installed, String> {
    let package = read_package(bytes, &key::addon_key())?;
    if package.manifest.slidev != SLIDEV_VERSION {
        return Err(wrong_slidev(&package.manifest.slidev));
    }
    let addon = loaded(&package.wasm, &package.manifest.version)?;
    let addons = app_folder.join(ADDONS_FOLDER);
    std::fs::create_dir_all(&addons).map_err(|error| format!("The add-on couldn't be installed: the app's folder can't be written to ({error})."))?;
    refuse_links(&addons)?;
    // One install or removal at a time, from whichever window.
    let _lock = if wait { Lock::take(&addons)? } else { Lock::take_now(&addons)? };
    put_back_cut_short_locked(app_folder);
    if !wait
        && let Ok((current, _)) = verified(app_folder, Shared::Held)
        && !fetch::newer(&current, &package.manifest.version)
    {
        return Err(format!(
            "Version {current} is installed already, which is no older than the {} downloaded, so it was kept.",
            slides_components::shortened(&package.manifest.version, 40)
        ));
    }
    sweep_leftovers(&addons);
    let fresh = addons.join(format!(".{SLIDEV_FOLDER}-new-{}", unique()));
    let written = (|| -> std::io::Result<()> {
        std::fs::create_dir(&fresh)?;
        for (name, data) in [(MANIFEST_ENTRY, &package.manifest_bytes[..]), (WASM_ENTRY, &package.wasm[..]), (SIG_ENTRY, &package.sig[..])] {
            use std::io::Write as _;
            let mut out = std::fs::File::create(fresh.join(name))?;
            out.write_all(data)?;
            out.sync_all()?;
        }
        sync_folder(&fresh);
        Ok(())
    })();
    if let Err(error) = written {
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(format!("The add-on couldn't be installed: its files can't be written ({error})."));
    }
    let folder = addon_folder(app_folder);
    let old = addons.join(format!(".{SLIDEV_FOLDER}-old-{}", unique()));
    let had_one = folder.exists();
    if had_one && let Err(error) = std::fs::rename(&folder, &old) {
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(format!("The add-on couldn't be installed: the earlier one can't be moved aside ({error})."));
    }
    if let Err(error) = std::fs::rename(&fresh, &folder) {
        if had_one {
            let _ = std::fs::rename(&old, &folder);
        }
        let _ = std::fs::remove_dir_all(&fresh);
        return Err(format!("The add-on couldn't be installed: its folder can't be put in place ({error})."));
    }
    sync_folder(&addons);
    if had_one {
        // Renamed before it's deleted, so that only an install cut short leaves an `-old-` folder
        // behind, which would be put back.
        let gone = addons.join(format!(".{SLIDEV_FOLDER}-removed-{}", unique()));
        let old = if std::fs::rename(&old, &gone).is_ok() { gone } else { old };
        let _ = std::fs::remove_dir_all(&old);
    }
    Ok(Installed { version: package.manifest.version, state: InstalledState::Ready(addon) })
}

/// The bytes of the package file `file`, read no further than a package may take.
fn read_package_file(file: &Path) -> Result<Vec<u8>, String> {
    let opened = std::fs::File::open(file).map_err(|_| "That add-on file isn't there or can't be read.".to_string())?;
    if !opened.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return Err("That isn't an add-on file.".into());
    }
    let mut bytes = Vec::new();
    opened.take(MAX_PACKAGE_BYTES + 1).read_to_end(&mut bytes).map_err(|_| "That add-on file can't be read.".to_string())?;
    Ok(bytes)
}

/// Takes the add-on installed under `app_folder` away, and only it. There being none is no
/// failure.
pub fn remove(app_folder: &Path) -> Result<(), String> {
    let folder = addon_folder(app_folder);
    if std::fs::symlink_metadata(&folder).is_err() {
        return Ok(());
    }
    let addons = app_folder.join(ADDONS_FOLDER);
    if std::fs::symlink_metadata(&addons).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(LINKED.to_string());
    }
    // Never waited for: it is asked for on the window's thread.
    let Ok(_lock) = Lock::try_take(&addons) else { return Err(BUSY.to_string()) };
    // Moved aside first, so that the add-on is gone at once, even if some of it can't be deleted.
    let gone = addons.join(format!(".{SLIDEV_FOLDER}-removed-{}", unique()));
    std::fs::rename(&folder, &gone).map_err(|error| format!("The add-on couldn't be removed ({error})."))?;
    // With what an earlier install left, so that none of it is put back as the add-on.
    sweep_leftovers(&addons);
    Ok(())
}

/// What remove, and an update, say while an install, an export or an import holds the add-on.
pub const BUSY: &str = "The add-on is being installed or used just now; try again in a moment.";

/// What install and remove say of an add-ons folder that is a link.
const LINKED: &str = "The add-ons folder, or the Slidev add-on's, is a link to somewhere else, so the app won't write there.";

/// Refuses an `addons` folder, or a Slidev folder in it, that is a link: what's installed is
/// written only where the app keeps it.
fn refuse_links(addons: &Path) -> Result<(), String> {
    for path in [addons.to_path_buf(), addons.join(SLIDEV_FOLDER)] {
        if std::fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(LINKED.to_string());
        }
    }
    Ok(())
}

/// [`put_back_cut_short`], with the lock already held.
fn put_back_cut_short_locked(app_folder: &Path) {
    let addons = app_folder.join(ADDONS_FOLDER);
    let folder = addon_folder(app_folder);
    if std::fs::symlink_metadata(&folder).is_ok() {
        return;
    }
    let prefix = format!(".{SLIDEV_FOLDER}-old-");
    let Ok(entries) = std::fs::read_dir(&addons) else { return };
    let old: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix) && entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect();
    if let [old] = &old[..] {
        let _ = std::fs::rename(old, &folder);
    }
}

/// The lock on `addons/.lock`, held by one install or removal at a time, in this process or
/// another. It goes when it's dropped.
struct Lock {
    /// Held only so that the lock is.
    _file: std::fs::File,
}

impl Lock {
    fn open(addons: &Path) -> std::io::Result<std::fs::File> {
        std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(addons.join(".lock"))
    }

    /// Waits for the lock.
    fn take(addons: &Path) -> Result<Lock, String> {
        let file = Lock::open(addons).map_err(|error| format!("The app's add-ons folder can't be written to ({error})."))?;
        file.lock().map_err(|error| format!("The app's add-ons folder can't be locked ({error})."))?;
        Ok(Lock { _file: file })
    }

    /// The lock, or [`BUSY`] if something else holds it now.
    fn take_now(addons: &Path) -> Result<Lock, String> {
        let file = Lock::open(addons).map_err(|error| format!("The app's add-ons folder can't be written to ({error})."))?;
        file.try_lock().map_err(|_| BUSY.to_string())?;
        Ok(Lock { _file: file })
    }

    /// The lock, if nothing else holds it now.
    fn try_take(addons: &Path) -> Result<Lock, ()> {
        let file = Lock::open(addons).map_err(|_| ())?;
        file.try_lock().map_err(|_| ())?;
        Ok(Lock { _file: file })
    }

    /// The lock shared, for reading what's installed, once no install holds it; or `None` if the
    /// lock file can't be had, as when nothing was ever installed.
    fn shared(addons: &Path) -> Option<Lock> {
        let file = Lock::open(addons).ok()?;
        file.lock_shared().ok()?;
        Some(Lock { _file: file })
    }

    /// The lock shared, if no install holds it now.
    fn try_shared(addons: &Path) -> Option<Lock> {
        let file = Lock::open(addons).ok()?;
        file.try_lock_shared().ok()?;
        Some(Lock { _file: file })
    }
}

/// Deletes what an install or a removal that stopped part way left in `addons`.
fn sweep_leftovers(addons: &Path) {
    let Ok(entries) = std::fs::read_dir(addons) else { return };
    let prefix = format!(".{SLIDEV_FOLDER}-");
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(&prefix) && entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// A name no other folder written now has.
fn unique() -> String {
    format!("{:032x}", crate::document::UntitledId::random().0)
}

fn sync_folder(folder: &Path) {
    #[cfg(unix)]
    if let Ok(folder) = std::fs::File::open(folder) {
        let _ = folder.sync_all();
    }
    #[cfg(not(unix))]
    let _ = folder;
}
