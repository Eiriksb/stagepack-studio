//! `package addon-keygen` and `package addon`. Every key here is a scratch key made in a
//! scratch folder, never the owner's.
//!
//! Building the add-on takes a while, so that test is run by name:
//!
//! `scripts/cargo-safe.sh test -p xtask --test addon -- --ignored --nocapture`

use std::path::Path;
use std::process::Command;

use addon_abi::package::{read_package, write_package, AddonManifest, Index, PACKAGE_PREFIX};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};

fn public_of(key_file: &Path) -> [u8; 32] {
    let secret: [u8; 32] = std::fs::read(key_file).unwrap().try_into().unwrap();
    SigningKey::from_bytes(&secret).verifying_key().to_bytes()
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn addon_keygen_writes_a_private_file_and_refuses_to_overwrite() {
    let scratch = tempfile::tempdir().unwrap();
    let folder = scratch.path().join("signing");
    let key_file = folder.join("addon-signing.key");

    let out = Command::new(env!("CARGO_BIN_EXE_package")).arg("addon-keygen").arg(&key_file).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let secret = std::fs::read(&key_file).unwrap();
    assert_eq!(secret.len(), 32);
    #[cfg(unix)]
    {
        assert_eq!(mode(&key_file), 0o600);
        assert_eq!(mode(&folder), 0o700);
    }

    // Only the public half is printed, as a line of Rust.
    let public = public_of(&key_file);
    let printed = String::from_utf8(out.stdout).unwrap();
    assert!(printed.contains(&xtask::addon::public_key_line(&public)), "{printed}");
    let secret_line = format!("{secret:?}");
    let secret_line = &secret_line[1..secret_line.len() - 1];
    assert!(!printed.contains(secret_line) && !String::from_utf8_lossy(&out.stderr).contains(secret_line));

    // A second run leaves the first key as it was.
    let again = Command::new(env!("CARGO_BIN_EXE_package")).arg("addon-keygen").arg(&key_file).output().unwrap();
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("never written over"), "{}", String::from_utf8_lossy(&again.stderr));
    assert_eq!(std::fs::read(&key_file).unwrap(), secret);
    assert!(xtask::addon::keygen(&key_file).is_err());
    assert_eq!(std::fs::read(&key_file).unwrap(), secret);
}

#[cfg(unix)]
#[test]
fn a_key_file_others_can_read_is_refused() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = tempfile::tempdir().unwrap();
    let key_file = scratch.path().join("k/addon-signing.key");
    xtask::addon::keygen(&key_file).unwrap();
    std::fs::set_permissions(&key_file, std::fs::Permissions::from_mode(0o644)).unwrap();
    let error = xtask::addon::read_signing_key(&key_file).unwrap_err();
    assert!(error.contains("0600"), "{error}");

    // Nor is a key made in a folder others can read.
    let open = scratch.path().join("open");
    std::fs::create_dir(&open).unwrap();
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).unwrap();
    let error = xtask::addon::keygen(&open.join("addon-signing.key")).unwrap_err();
    assert!(error.contains("0700"), "{error}");
    assert!(!open.join("addon-signing.key").exists());

    let short = scratch.path().join("short.key");
    std::fs::write(&short, [1; 31]).unwrap();
    std::fs::set_permissions(&short, std::fs::Permissions::from_mode(0o600)).unwrap();
    let error = xtask::addon::read_signing_key(&short).unwrap_err();
    assert!(error.contains("32"), "{error}");
}

#[test]
fn the_built_in_key_is_a_public_key() {
    let key = xtask::addon::built_in_key().unwrap();
    assert!(!VerifyingKey::from_bytes(&key).unwrap().is_weak());
    assert!(key.iter().any(|b| *b != key[0]), "the built-in key looks like a placeholder");
}

#[test]
#[ignore = "builds the add-on for WebAssembly"]
fn package_addon_signs_what_read_package_accepts() {
    let scratch = tempfile::tempdir().unwrap();
    let key_file = scratch.path().join("keys/addon-signing.key");
    xtask::addon::keygen(&key_file).unwrap();
    let out = scratch.path().join("out");

    let (file, sha256) = xtask::addon::package_addon(&key_file, &out).unwrap();
    assert_eq!(file, out.join("slidev-0.1.0.spaddon"));
    let bytes = std::fs::read(&file).unwrap();
    println!("{} is {} bytes, sha256 {sha256}", file.display(), bytes.len());
    assert_eq!(sha256.len(), 64);

    let package = read_package(&bytes, &public_of(&key_file)).unwrap();
    assert_eq!((package.manifest.name.as_str(), package.manifest.version.as_str(), package.manifest.abi), ("slidev", "0.1.0", 1));
    assert_eq!(package.manifest.slidev, "53.0.0");
    assert!(package.wasm.starts_with(b"\0asm"));
    // The builder's folders are written out of the add-on.
    let repo = xtask::stage::repo_root();
    for folder in [repo.display().to_string(), std::env::var("HOME").unwrap_or_default()].into_iter().filter(|f| f.len() > 1) {
        assert!(!package.wasm.windows(folder.len()).any(|w| w == folder.as_bytes()), "the add-on names {folder}");
    }
    // And nothing but the owner's key opens it in the app.
    assert!(read_package(&bytes, &xtask::addon::built_in_key().unwrap()).is_err());
}

/// A scratch package signed by `key`, as `package addon` would write it, without the build.
fn scratch_package(folder: &Path, key: &SigningKey, version: &str) -> std::path::PathBuf {
    let manifest = AddonManifest { name: "slidev".into(), version: version.into(), abi: 1, slidev: "53.0.0".into() };
    let bytes = write_package(&manifest, b"\0asm\x01\0\0\0", |m| key.sign(m).to_bytes());
    std::fs::create_dir_all(folder).unwrap();
    let file = folder.join(format!("slidev-{version}.spaddon"));
    std::fs::write(&file, bytes).unwrap();
    file
}

#[test]
fn the_release_index_matches_its_package() {
    let scratch = tempfile::tempdir().unwrap();
    let key = SigningKey::from_bytes(&[7; 32]);
    let public = key.verifying_key().to_bytes();
    let package = scratch_package(&scratch.path().join("package"), &key, "1.2.3");
    let bytes = std::fs::read(&package).unwrap();

    let (file, written) = xtask::addon::write_release_index(&package, &public).unwrap();
    // Named for its line of apps: add-on interface 1, Slidev 53.
    assert_eq!(file, scratch.path().join("package").join("index-abi1-slidev53.0.0.json"));

    // The app's own type reads it, and it says what the package beside it is.
    let index: Index = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(index, written);
    assert_eq!((index.name.as_str(), index.version.as_str(), index.slidev.as_str(), index.abi), ("slidev", "1.2.3", "53.0.0", 1));
    assert_eq!(index.size, bytes.len() as u64);
    let hex: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(index.sha256, hex);
    // The address is the one the published steps' tag makes, under the prefix the app insists on.
    assert_eq!(index.url, format!("{PACKAGE_PREFIX}slidev-v1.2.3/slidev-1.2.3.spaddon"));
    let steps = xtask::addon::publishing_steps(&package, &file, &index);
    let lines: Vec<&str> = steps.lines().map(str::trim).collect();
    // The package in a release of its own, with notes so that gh asks nothing, and not marked
    // latest, which nothing reads.
    let create = lines.iter().find(|line| line.starts_with("gh release create slidev-v1.2.3 ")).unwrap_or_else(|| panic!("{steps}"));
    assert!(create.contains(&package.display().to_string()) && !create.contains("index-abi"), "{create}");
    assert!(create.contains("--repo Eiriksb/stagepack-studio-addons") && create.contains("--title \"Slidev add-on 1.2.3\""), "{create}");
    assert!(create.contains("--notes ") && !create.contains("--latest"), "{create}");
    // Then the index, put in place of the line's earlier one in the release tagged index.
    let upload = format!("gh release upload index {} --clobber --repo Eiriksb/stagepack-studio-addons", file.display());
    let at_upload = lines.iter().position(|line| *line == upload).unwrap_or_else(|| panic!("{steps}"));
    assert!(lines.iter().position(|line| line == create).unwrap() < at_upload, "the package goes up before its index: {steps}");
    // And, once only, the release that holds the indexes.
    let once = lines.iter().find(|line| line.starts_with("gh release create index ")).unwrap_or_else(|| panic!("{steps}"));
    assert!(once.contains("--repo Eiriksb/stagepack-studio-addons") && once.contains("--title \"Add-on index\"") && once.contains("--notes "), "{once}");
    assert!(lines.iter().position(|line| line == once).unwrap() < at_upload, "{steps}");
    assert!(!steps.contains("--latest"), "{steps}");

    // A package the key didn't sign gets no index, and nor does one under the wrong name.
    let other = SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes();
    let other_dir = scratch.path().join("other");
    std::fs::create_dir_all(&other_dir).unwrap();
    let copy = other_dir.join("slidev-1.2.3.spaddon");
    std::fs::copy(&package, &copy).unwrap();
    assert!(xtask::addon::write_release_index(&copy, &other).is_err());
    assert_eq!(std::fs::read_dir(&other_dir).unwrap().count(), 1, "no index was written");
    let renamed = other_dir.join("slidev-9.9.9.spaddon");
    std::fs::copy(&package, &renamed).unwrap();
    assert!(xtask::addon::write_release_index(&renamed, &public).unwrap_err().contains("must be called"));
    // A version that isn't three numbers gets none.
    // Nor does one with a leading zero, which the app wouldn't read as a version.
    for version in ["1.2", "01.2.3"] {
        let odd = scratch_package(&scratch.path().join("odd").join(version), &key, version);
        assert!(xtask::addon::write_release_index(&odd, &public).unwrap_err().contains("three numbers"), "{version}");
    }
}
