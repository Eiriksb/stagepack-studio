//! `package addon-keygen` and `package addon`. Every key here is a scratch key made in a
//! scratch folder, never the owner's.
//!
//! Building the add-on takes a while, so that test is run by name:
//!
//! `scripts/cargo-safe.sh test -p xtask --test addon -- --ignored --nocapture`

use std::path::Path;
use std::process::Command;

use addon_abi::package::read_package;
use ed25519_dalek::{SigningKey, VerifyingKey};

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
