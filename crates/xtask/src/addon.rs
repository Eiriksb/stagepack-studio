//! `package addon-keygen` and `package addon`: the owner's signing key, and the signed
//! `.spaddon` package of the Slidev add-on.
//!
//! The key file is the 32 raw bytes of an ed25519 secret key. It is only ever read here, to
//! sign, and nothing here prints, logs or copies it: only its public half is shown.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use addon_abi::package::{index_file, package_file_name, read_package, release_tag, version_numbers, write_package, AddonManifest, Index, ADDON_NAME, INDEX_TAG, PACKAGE_PREFIX};
use addon_abi::ABI_VERSION;
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};

/// The version of Slidev the add-on writes for: the `@slidev/cli` Stagepack bundles.
pub const SLIDEV_TARGET: &str = "53.0.0";

/// The line of Rust that builds `public` into the app, as `crates/app/src/addons/key.rs` holds it.
pub fn public_key_line(public: &[u8; 32]) -> String {
    let bytes: Vec<String> = public.iter().map(u8::to_string).collect();
    format!("pub const ADDON_PUBLIC_KEY: [u8; 32] = [{}];", bytes.join(", "))
}

/// The public key built into the app, read from `crates/app/src/addons/key.rs`.
pub fn built_in_key() -> Result<[u8; 32], String> {
    let file = crate::stage::repo_root().join("crates/app/src/addons/key.rs");
    let text = std::fs::read_to_string(&file).map_err(|e| format!("can't read {}: {e}", file.display()))?;
    let start = text.find("ADDON_PUBLIC_KEY: [u8; 32] = [").ok_or("key.rs has no ADDON_PUBLIC_KEY")? + "ADDON_PUBLIC_KEY: [u8; 32] = [".len();
    let end = start + text[start..].find(']').ok_or("ADDON_PUBLIC_KEY isn't closed")?;
    let bytes: Result<Vec<u8>, _> = text[start..end].split(',').map(str::trim).filter(|b| !b.is_empty()).map(str::parse).collect();
    bytes.map_err(|_| "ADDON_PUBLIC_KEY isn't a list of bytes".to_string())?.try_into().map_err(|_| "ADDON_PUBLIC_KEY isn't 32 bytes".to_string())
}

/// Makes a new signing key in `path` and gives back its public half. The file is made with
/// mode 0600, and its folder, if it isn't there yet, with mode 0700; a folder that is there
/// already and that others can open is refused. A file that is there already is never written
/// over, and one that couldn't be written in full is taken away again.
pub fn keygen(path: &Path) -> Result<[u8; 32], String> {
    let folder = path.parent().filter(|f| !f.as_os_str().is_empty()).unwrap_or(Path::new("."));
    if !folder.exists() {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(folder).map_err(|e| format!("can't create {}: {e}", folder.display()))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(folder).map_err(|e| format!("can't read {}: {e}", folder.display()))?.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(format!("the folder {} can be opened by others (mode {mode:o}); make it mode 0700 first.", folder.display()));
        }
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => format!("{} is there already, and a key file is never written over.", path.display()),
        _ => format!("can't create {}: {e}", path.display()),
    })?;
    let key = SigningKey::generate(&mut rand_core::OsRng);
    if let Err(e) = file.write_all(key.as_bytes()).and_then(|()| file.sync_all()) {
        drop(file);
        // The file was made by this run, so taking it away leaves nothing of anyone's.
        let _ = std::fs::remove_file(path);
        return Err(format!("can't write {}: {e}", path.display()));
    }
    Ok(key.verifying_key().to_bytes())
}

/// The signing key in `path`, which must be 32 bytes that only its owner can read.
pub fn read_signing_key(path: &Path) -> Result<SigningKey, String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("can't open the key file {}: {e}", path.display()))?;
    let meta = file.metadata().map_err(|e| format!("can't read the key file {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = meta.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(format!("the key file {} can be read by others (mode {mode:o}); make it mode 0600 first.", path.display()));
        }
    }
    if meta.len() != 32 {
        return Err(format!("the key file {} is {} bytes, where a key file is 32.", path.display(), meta.len()));
    }
    // Wiped when it goes, as the signing key itself is.
    let mut secret = zeroize::Zeroizing::new([0; 32]);
    file.read_exact(&mut secret[..]).map_err(|e| format!("can't read the key file {}: {e}", path.display()))?;
    Ok(SigningKey::from_bytes(&secret))
}

/// The add-on's version, from `crates/slidev-addon/Cargo.toml`.
fn addon_version(repo: &Path) -> Result<String, String> {
    let file = repo.join("crates/slidev-addon/Cargo.toml");
    let text = std::fs::read_to_string(&file).map_err(|e| format!("can't read {}: {e}", file.display()))?;
    text.lines()
        .find_map(|line| line.strip_prefix("version = \"").and_then(|rest| rest.strip_suffix('"')))
        .map(str::to_string)
        .ok_or_else(|| format!("{} has no version", file.display()))
}

/// Builds the add-on for WebAssembly in release, with the builder's folders written out of it,
/// in `target/addon-build`; signs it with the key in `key_file`; and writes
/// `slidev-<version>.spaddon` in `out`. Gives back the file and its sha256, in hex. The package
/// is read back with the key's public half before it is given back.
pub fn package_addon(key_file: &Path, out: &Path) -> Result<(PathBuf, String), String> {
    let key = read_signing_key(key_file)?;
    let repo = crate::stage::repo_root();
    let target = repo.join("target/addon-build");
    let status = Command::new(crate::stage::cargo(&repo))
        .current_dir(&repo)
        .env("CARGO_ENCODED_RUSTFLAGS", crate::stage::release_flags(&repo, false))
        .args(["build", "--release", "-p", "slidev-addon", "--lib", "--target", "wasm32-unknown-unknown", "--target-dir"])
        .arg(&target)
        .status()
        .map_err(|e| format!("couldn't run cargo: {e}"))?;
    if !status.success() {
        return Err(format!("building the add-on failed ({status})"));
    }
    let wasm_file = target.join("wasm32-unknown-unknown/release/slidev_addon.wasm");
    let wasm = std::fs::read(&wasm_file).map_err(|e| format!("can't read {}: {e}", wasm_file.display()))?;

    let manifest = AddonManifest { name: ADDON_NAME.into(), version: addon_version(&repo)?, abi: ABI_VERSION, slidev: SLIDEV_TARGET.into() };
    let bytes = write_package(&manifest, &wasm, |message| key.sign(message).to_bytes());
    read_package(&bytes, &key.verifying_key().to_bytes()).map_err(|e| format!("the package doesn't read back: {e}"))?;

    std::fs::create_dir_all(out).map_err(|e| format!("can't create {}: {e}", out.display()))?;
    let file = out.join(package_file_name(&manifest));
    std::fs::write(&file, &bytes).map_err(|e| format!("can't write {}: {e}", file.display()))?;
    let sha256: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    Ok((file, sha256))
}

/// Whether the key in `key_file` is the one built into the app, so that the app takes its packages.
pub fn is_built_in(key_file: &Path) -> Result<bool, String> {
    Ok(read_signing_key(key_file)?.verifying_key().to_bytes() == built_in_key()?)
}

/// Writes the index of `package`'s line of apps, `index-abi<abi>-slidev<version>.json`, in the
/// folder of `package`: the package is read with `public` first, so an index is only ever made
/// for a package the app would take, and its `size`, `sha256` and `url` are the package's own.
/// The file name must be the one `package addon` gives, and the version three numbers as the app
/// reads them. Gives back the file and what it says.
pub fn write_release_index(package: &Path, public: &[u8; 32]) -> Result<(PathBuf, Index), String> {
    let bytes = std::fs::read(package).map_err(|e| format!("can't read {}: {e}", package.display()))?;
    let manifest = read_package(&bytes, public).map_err(|e| format!("{} isn't a package the app takes: {e}", package.display()))?.manifest;
    if version_numbers(&manifest.version).is_none() {
        return Err(format!("the version {} isn't three numbers.", manifest.version));
    }
    let name = package_file_name(&manifest);
    if package.file_name().and_then(|n| n.to_str()) != Some(name.as_str()) {
        return Err(format!("the package must be called {name}, and is {}.", package.display()));
    }
    let index = Index {
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        slidev: manifest.slidev.clone(),
        abi: manifest.abi,
        size: bytes.len() as u64,
        sha256: Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect(),
        url: format!("{PACKAGE_PREFIX}{}/{name}", release_tag(&manifest.version)),
    };
    let mut text = serde_json::to_string_pretty(&index).map_err(|e| format!("can't write the index: {e}"))?;
    text.push('\n');
    let file = package.with_file_name(index_file(manifest.abi, &manifest.slidev));
    std::fs::write(&file, text).map_err(|e| format!("can't write {}: {e}", file.display()))?;
    Ok((file, index))
}

/// The repository the add-on is published in.
const RELEASES_REPO: &str = "Eiriksb/stagepack-studio-addons";

/// The owner's steps after `index`, for `package addon-release` to print: the package in a
/// release of its own, then its index in place of the line's earlier one, in the release tagged
/// [`INDEX_TAG`], which is made once, before the first.
pub fn publishing_steps(package: &Path, index_file: &Path, index: &Index) -> String {
    format!(
        "To publish (the full steps are in docs/guide/publishing-the-addon.md):\n\
         Once only, before the first release (skip it if the release {INDEX_TAG} is there already):\n  \
         gh release create {INDEX_TAG} --repo {RELEASES_REPO} --title \"Add-on index\" --notes \"The index files the app reads, one for each line of apps. Never delete them.\"\n\
         The package, in a release of its own:\n  \
         gh release create {tag} {package} --repo {RELEASES_REPO} --title \"Slidev add-on {version}\" --notes \"Slidev add-on {version}\"\n\
         Then its index, in place of the earlier one, once the package is up:\n  \
         gh release upload {INDEX_TAG} {index} --clobber --repo {RELEASES_REPO}\n\
         Never upload the signing key.",
        tag = release_tag(&index.version),
        package = package.display(),
        index = index_file.display(),
        version = index.version,
    )
}

/// The package `package addon` writes by default: `target/package/slidev-<version>.spaddon`.
pub fn default_package() -> Result<PathBuf, String> {
    let repo = crate::stage::repo_root();
    Ok(repo.join("target/package").join(format!("{ADDON_NAME}-{}.spaddon", addon_version(&repo)?)))
}
