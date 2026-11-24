//! The `.spaddon` package: a zip of exactly three entries, `addon.json` (an [`AddonManifest`]),
//! `addon.wasm` (the add-on) and `addon.sig`, the 64-byte ed25519 signature of
//! [`signed_message`]. Nothing in it is trusted until the signature is checked against the
//! key the app was built with, and nothing is unpacked past the limits below.

use std::io::{Cursor, Read, Write};

use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::{safe_name, ABI_VERSION};

/// The add-on's manifest, in the package.
pub const MANIFEST_ENTRY: &str = "addon.json";
/// The add-on itself, in the package.
pub const WASM_ENTRY: &str = "addon.wasm";
/// The signature, in the package.
pub const SIG_ENTRY: &str = "addon.sig";
/// The entries of a package, each exactly once, in the order they are written.
pub const ENTRIES: [&str; 3] = [MANIFEST_ENTRY, WASM_ENTRY, SIG_ENTRY];

/// The most `addon.wasm` may unpack to.
pub const MAX_WASM_BYTES: u64 = 32 * 1024 * 1024;
/// The most `addon.json` may unpack to.
pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
/// The most a package file may take: its entries stored as they are, and room for the zip's own records.
pub const MAX_PACKAGE_BYTES: u64 = 33 * 1024 * 1024;
/// The length of a signature.
pub const SIG_BYTES: usize = 64;

/// The one add-on the app takes.
pub const ADDON_NAME: &str = "slidev";

/// What `addon.json` says of the add-on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddonManifest {
    /// Always [`ADDON_NAME`].
    pub name: String,
    /// The add-on's own version, which names the package file: `slidev-<version>.spaddon`.
    pub version: String,
    /// The add-on interface it is made for, [`ABI_VERSION`].
    pub abi: u32,
    /// The version of Slidev it writes for.
    pub slidev: String,
}

/// What every package's address starts with, and every index's: the repository's release files.
pub const PACKAGE_PREFIX: &str = "https://github.com/Eiriksb/stagepack-studio-addons/releases/download/";

/// The tag of the one release that holds the indexes, a file for each line of apps: an app asks
/// only for its own line's, which is always at the same address, so an app that has shipped
/// keeps finding the last add-on made for it, whatever is published for later apps.
pub const INDEX_TAG: &str = "index";

/// The index file of the apps that take add-on interface `abi` and write for Slidev `slidev`:
/// `index-abi<abi>-slidev<version>.json`, by the exact Slidev version, since that is what an app
/// accepts: an app for 53.0.0 and one for 53.1.0 are different lines, and neither is cut off when
/// the other's add-on is published.
pub fn index_file(abi: u32, slidev: &str) -> String {
    format!("index-abi{abi}-slidev{slidev}.json")
}

/// Where the index of that line is: [`index_file`] in the release tagged [`INDEX_TAG`].
pub fn index_url(abi: u32, slidev: &str) -> String {
    format!("{PACKAGE_PREFIX}{INDEX_TAG}/{}", index_file(abi, slidev))
}

/// The three numbers of `version`, `X.Y.Z`, with no leading zeros, signs or anything else: the
/// versions an index offers, and the only ones the app compares. The app and the packaging tool
/// both ask this, so they can't disagree on what a version is.
pub fn version_numbers(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.').map(|part| {
        let digits = !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()) && (part == "0" || !part.starts_with('0'));
        if digits { part.parse::<u64>().ok() } else { None }
    });
    let numbers = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(numbers)
}

/// The tag of the release that publishes version `version`: `slidev-v<version>`.
pub fn release_tag(version: &str) -> String {
    format!("{ADDON_NAME}-v{version}")
}

/// What an index file says of the package offered to its line of apps. It proves nothing: the app takes it
/// only to know what to download, and checks the download against it, and then against the
/// package's own signed `addon.json`. The packaging tool writes it and the app reads it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Index {
    /// Always [`ADDON_NAME`].
    pub name: String,
    /// The version offered, three numbers.
    pub version: String,
    /// The version of Slidev it writes for.
    pub slidev: String,
    /// The add-on interface it is made for, [`ABI_VERSION`].
    pub abi: u32,
    /// The package's length in bytes.
    pub size: u64,
    /// The sha256 of the package, as 64 lowercase hex digits.
    pub sha256: String,
    /// Where the package is.
    pub url: String,
}

/// A package whose signature was checked, and whose manifest is one the app takes. Only
/// [`read_package`] makes one (it can't be built outside this crate), so holding one means
/// the check was made.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Package {
    pub manifest: AddonManifest,
    pub wasm: Vec<u8>,
    /// `addon.json` as it was signed.
    pub manifest_bytes: Vec<u8>,
    pub sig: [u8; SIG_BYTES],
}

/// The most entries a zip may say it holds before its directory is read at all. A package
/// holds 3; this lets a few more through only so that the sentence can say which is wrong.
pub const MAX_LISTED_ENTRIES: u64 = 8;

/// What is signed: `sha256(addon.json) || sha256(addon.wasm)`.
pub fn signed_message(manifest_bytes: &[u8], wasm: &[u8]) -> [u8; 64] {
    let mut message = [0; 64];
    message[..32].copy_from_slice(&Sha256::digest(manifest_bytes));
    message[32..].copy_from_slice(&Sha256::digest(wasm));
    message
}

/// The file name of the package of `manifest`: `slidev-<version>.spaddon`.
pub fn package_file_name(manifest: &AddonManifest) -> String {
    format!("{}-{}.spaddon", manifest.name, manifest.version)
}

/// The package of `manifest` and `wasm`, signed by `sign`, which is given [`signed_message`]
/// and gives back the signature. The same inputs make the same bytes.
///
/// # Panics
///
/// If `wasm` is 4 GiB or more, which is far past what a package may hold.
pub fn write_package(manifest: &AddonManifest, wasm: &[u8], sign: impl Fn(&[u8]) -> [u8; SIG_BYTES]) -> Vec<u8> {
    let manifest_bytes = serde_json::to_vec_pretty(manifest).expect("a manifest is always JSON");
    let sig = sign(&signed_message(&manifest_bytes, wasm));
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    // A fixed time stamp, so that the package says nothing of when it was made.
    let options = SimpleFileOptions::DEFAULT.compression_method(CompressionMethod::Deflated);
    for (name, data) in ENTRIES.iter().zip([&manifest_bytes[..], wasm, &sig[..]]) {
        writer.start_file(*name, options).expect("a zip in memory takes an entry");
        writer.write_all(data).expect("a zip in memory takes the data");
    }
    writer.finish().expect("a zip in memory is finished").into_inner()
}

/// The package in `bytes`, if it is signed with `key` and its manifest is one the app takes;
/// otherwise one sentence on what is wrong with it. Every entry's name and size are checked
/// before any of it is unpacked, and nothing is unpacked past its limit.
pub fn read_package(bytes: &[u8], key: &[u8; 32]) -> Result<Package, String> {
    if bytes.len() as u64 > MAX_PACKAGE_BYTES {
        return Err(format!("The add-on file is {} bytes, over the {} MiB an add-on package may take.", bytes.len(), MAX_PACKAGE_BYTES >> 20));
    }
    let key = VerifyingKey::from_bytes(key).map_err(|_| "The add-on key built into the app isn't an ed25519 public key.".to_string())?;
    let not_a_zip = || "The add-on file isn't a zip archive that can be read.".to_string();
    // The count the zip gives for itself, so that a directory of many entries is never read in.
    let listed = listed_entries(bytes).ok_or_else(not_a_zip)??;
    if listed > MAX_LISTED_ENTRIES {
        return Err(wrong_count(listed));
    }
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|_| not_a_zip())?;
    // The zip reader keeps one entry of a name it finds twice, so this is asked of it before
    // the entries are counted: other zip tools would show the copy it left out.
    if archive.has_duplicate_names() {
        return Err("The add-on file holds an entry twice.".into());
    }
    if archive.len() != ENTRIES.len() {
        return Err(wrong_count(archive.len() as u64));
    }

    // Where each of the three is in the zip, found from the zip's own directory, unpacking nothing.
    let mut at = [None; 3];
    for index in 0..ENTRIES.len() {
        let file = archive.by_index_raw(index).map_err(|_| format!("Entry {} of the add-on file can't be read.", index + 1))?;
        let name = std::str::from_utf8(file.name_raw()).map_err(|_| format!("The name of entry {} of the add-on file isn't UTF-8.", index + 1))?;
        if !safe_name(name) {
            // Not named: nothing of an unchecked package is said back.
            return Err(format!("The name of entry {} of the add-on file isn't a safe one.", index + 1));
        }
        let slot = ENTRIES
            .iter()
            .position(|entry| *entry == name)
            .ok_or_else(|| format!("The name of entry {} of the add-on file isn't one of the three a package holds.", index + 1))?;
        // With no name twice and three entries, each of the three is found once.
        at[slot] = Some(index);
        if file.encrypted() || !file.is_file() {
            return Err(format!("The add-on file's {name} isn't a plain file."));
        }
        let limit = limit(slot);
        if file.size() > limit {
            return Err(format!("The add-on's {name} unpacks to {} bytes, over the {} a package's {name} may take.", file.size(), limit_text(slot)));
        }
    }

    let mut unpack = |slot: usize| -> Result<Vec<u8>, String> {
        let name = ENTRIES[slot];
        let index = at[slot].expect("each of the three entries is there once");
        let file = archive.by_index(index).map_err(|_| format!("The add-on's {name} can't be unpacked."))?;
        let mut data = Vec::new();
        // Read one byte past the limit, so that a size the zip misstates is caught too.
        file.take(limit(slot) + 1).read_to_end(&mut data).map_err(|_| format!("The add-on's {name} can't be unpacked."))?;
        if data.len() as u64 > limit(slot) {
            return Err(format!("The add-on's {name} unpacks to more than the {} a package's {name} may take.", limit_text(slot)));
        }
        Ok(data)
    };
    let manifest_bytes = unpack(0)?;
    let wasm = unpack(1)?;
    let sig: [u8; SIG_BYTES] = unpack(2)?
        .try_into()
        .map_err(|sig: Vec<u8>| format!("The add-on's addon.sig is {} bytes, where a signature is {SIG_BYTES}.", sig.len()))?;
    let manifest = verify_parts(&manifest_bytes, &wasm, &sig, key.as_bytes())?;
    Ok(Package { manifest, wasm, manifest_bytes, sig })
}

/// The manifest of the add-on made of `manifest_bytes` (`addon.json`), `wasm` (`addon.wasm`)
/// and `sig` (`addon.sig`), if `sig` is their signature by `key` and the manifest is one the
/// app takes; otherwise one sentence on what is wrong. It is the check [`read_package`]
/// makes of a package, made again of an add-on once it's installed.
pub fn verify_parts(manifest_bytes: &[u8], wasm: &[u8], sig: &[u8], key: &[u8; 32]) -> Result<AddonManifest, String> {
    let key = VerifyingKey::from_bytes(key).map_err(|_| "The add-on key built into the app isn't an ed25519 public key.".to_string())?;
    for (slot, len) in [manifest_bytes.len(), wasm.len()].into_iter().enumerate() {
        if len as u64 > limit(slot) {
            let name = ENTRIES[slot];
            return Err(format!("The add-on's {name} is {len} bytes, over the {} a package's {name} may take.", limit_text(slot)));
        }
    }
    let sig: [u8; SIG_BYTES] =
        sig.try_into().map_err(|_| format!("The add-on's addon.sig is {} bytes, where a signature is {SIG_BYTES}.", sig.len()))?;
    key.verify_strict(&signed_message(manifest_bytes, wasm), &Signature::from_bytes(&sig))
        .map_err(|_| "The add-on's signature doesn't match: it wasn't signed with Stagepack's key, or it was changed after it was signed.".to_string())?;

    let manifest: AddonManifest =
        serde_json::from_slice(manifest_bytes).map_err(|e| format!("The add-on's addon.json isn't an add-on manifest: {e}."))?;
    if manifest.name != ADDON_NAME {
        return Err(format!("The add-on is named {:?}, where the only add-on Stagepack takes is \"{ADDON_NAME}\".", manifest.name));
    }
    if manifest.abi != ABI_VERSION {
        return Err(format!("The add-on is made for ABI {}, where Stagepack runs add-ons made for ABI {ABI_VERSION}.", manifest.abi));
    }
    for (field, value) in [("version", &manifest.version), ("slidev", &manifest.slidev)] {
        if !version_like(value) {
            return Err(format!("The add-on's {field} {value:?} isn't a version number."));
        }
    }
    Ok(manifest)
}

fn wrong_count(count: u64) -> String {
    format!("The add-on file holds {count} entries, where a package holds exactly 3: addon.json, addon.wasm and addon.sig.")
}

/// One end-of-directory record found in the last bytes of a zip: what it says of the zip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EndRecord {
    /// The larger of its two entry counts. A zip64 count reads as 65 535.
    pub entries: u64,
    /// The size it gives the central directory. A zip64 size reads as 4 GiB less one byte.
    pub directory_size: u64,
    /// Where the zip64 end record is, if a zip64 locator comes right before this record.
    pub zip64_at: Option<u64>,
}

/// The length of a zip's end-of-directory record, without its comment.
const END_RECORD: usize = 22;
/// The length of a zip64 end-of-directory locator.
const ZIP64_LOCATOR: usize = 20;
/// The most of a zip's end that can hold its end record: the record, its longest comment and the
/// zip64 locator before it. Reading this much of a file is enough for [`end_records`].
pub const END_SEARCH_BYTES: usize = END_RECORD + u16::MAX as usize + ZIP64_LOCATOR;

/// Every end-of-directory record in `tail`, the last bytes of a zip, that a zip reader might take
/// as the zip's: each signature whose comment ends at or before the end of the file, as the zip
/// reader allows, latest first. Whatever is refused of these is refused of the one it reads.
pub fn end_records(tail: &[u8]) -> Vec<EndRecord> {
    let Some(last) = tail.len().checked_sub(END_RECORD) else { return Vec::new() };
    let earliest = tail.len().saturating_sub(END_RECORD + usize::from(u16::MAX));
    (earliest..=last)
        .rev()
        .filter_map(|at| {
            let record = &tail[at..at + END_RECORD];
            let comment = usize::from(u16::from_le_bytes([record[20], record[21]]));
            if record[..4] != [0x50, 0x4b, 0x05, 0x06] || at + END_RECORD + comment > tail.len() {
                return None;
            }
            let on_disk = u16::from_le_bytes([record[8], record[9]]);
            let total = u16::from_le_bytes([record[10], record[11]]);
            let directory_size = u32::from_le_bytes([record[12], record[13], record[14], record[15]]);
            let zip64_at = at.checked_sub(ZIP64_LOCATOR).map(|from| &tail[from..at]).filter(|locator| locator[..4] == [0x50, 0x4b, 0x06, 0x07]).map(|locator| {
                u64::from_le_bytes(locator[8..16].try_into().expect("eight bytes"))
            });
            Some(EndRecord { entries: u64::from(on_disk.max(total)), directory_size: u64::from(directory_size), zip64_at })
        })
        .collect()
}

/// The most a package's central directory may say it takes: [`MAX_LISTED_ENTRIES`] entries, each
/// with its fixed part, a name of up to 64 bytes and 64 KiB of extra fields and comment.
pub const MAX_DIRECTORY_BYTES: u64 = MAX_LISTED_ENTRIES * (46 + 64 + 64 * 1024);

/// What the zip's end records say of the package, refused before the zip is read if it could make
/// the reader read much: the larger entry count of any, if none is zip64 or says the directory
/// is larger than [`MAX_DIRECTORY_BYTES`]; or `None` if there is no end record.
fn listed_entries(bytes: &[u8]) -> Option<Result<u64, String>> {
    let records = end_records(&bytes[bytes.len().saturating_sub(END_SEARCH_BYTES)..]);
    if records.is_empty() {
        return None;
    }
    if records.iter().any(|record| record.zip64_at.is_some()) {
        return Some(Err("The add-on file is a zip64 archive, which a package never is.".into()));
    }
    if records.iter().any(|record| record.directory_size > MAX_DIRECTORY_BYTES) {
        return Some(Err("The add-on file's central directory says it is larger than a package's can be.".into()));
    }
    records.iter().map(|record| record.entries).max().map(Ok)
}

fn limit(slot: usize) -> u64 {
    [MAX_MANIFEST_BYTES, MAX_WASM_BYTES, SIG_BYTES as u64][slot]
}

fn limit_text(slot: usize) -> String {
    match slot {
        0 => format!("{} KiB", MAX_MANIFEST_BYTES >> 10),
        1 => format!("{} MiB", MAX_WASM_BYTES >> 20),
        _ => format!("{SIG_BYTES} bytes"),
    }
}

/// Whether `value` reads as a version number, `1.2.3` or `1.2.3-beta.1+build`: it names the
/// package file and the folder the add-on is installed in, so it holds nothing else.
fn version_like(value: &str) -> bool {
    value.len() <= 64
        && value.starts_with(|c: char| c.is_ascii_digit())
        && value.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
        && !value.contains("..")
}
