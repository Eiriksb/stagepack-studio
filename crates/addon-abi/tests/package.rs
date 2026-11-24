//! The signed `.spaddon` package: what is read, and every way a package is refused.

use std::io::Write;

use addon_abi::package::{read_package, signed_message, write_package, AddonManifest, MAX_MANIFEST_BYTES, MAX_WASM_BYTES};
use ed25519_dalek::{Signer, SigningKey};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

/// A key made up for these tests, never the owner's.
fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn public(key: &SigningKey) -> [u8; 32] {
    key.verifying_key().to_bytes()
}

fn manifest() -> AddonManifest {
    AddonManifest { name: "slidev".into(), version: "0.1.0".into(), abi: 1, slidev: "53.0.0".into() }
}

const WASM: &[u8] = b"\0asm\x01\0\0\0 a made-up module";

fn signed(manifest: &AddonManifest, wasm: &[u8], key: &SigningKey) -> Vec<u8> {
    write_package(manifest, wasm, |message| key.sign(message).to_bytes())
}

/// A zip of `entries`, as they are given, deflated.
fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = SimpleFileOptions::DEFAULT.compression_method(CompressionMethod::Deflated);
    for (name, data) in entries {
        writer.start_file(*name, options).unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

/// The three entries of a package, signed by `key` over the bytes as they are given.
fn entries_signed(manifest_bytes: &[u8], wasm: &[u8], key: &SigningKey) -> Vec<u8> {
    let sig = key.sign(&signed_message(manifest_bytes, wasm)).to_bytes();
    zip_of(&[("addon.json", manifest_bytes), ("addon.wasm", wasm), ("addon.sig", &sig)])
}

#[test]
fn a_signed_package_reads() {
    let key = key(7);
    let bytes = signed(&manifest(), WASM, &key);
    let package = read_package(&bytes, &public(&key)).unwrap();
    assert_eq!(package.manifest, manifest());
    assert_eq!(package.wasm, WASM);
    assert_eq!(serde_json::from_slice::<AddonManifest>(&package.manifest_bytes).unwrap(), manifest());
    assert_eq!(package.sig, key.sign(&signed_message(&package.manifest_bytes, WASM)).to_bytes());
    // The same package is written the same way each time, so its sha256 says which it is.
    assert_eq!(bytes, signed(&manifest(), WASM, &key));
}

#[test]
fn the_signed_message_is_the_two_sha256() {
    use sha2_check::sha256;
    let message = signed_message(b"{}", b"wasm");
    assert_eq!(message[..32], sha256(b"{}"));
    assert_eq!(message[32..], sha256(b"wasm"));
}

/// The sha256 of a few bytes, as `sha256sum` gives it.
mod sha2_check {
    pub fn sha256(data: &[u8]) -> [u8; 32] {
        let hex = match data {
            b"{}" => "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a",
            b"wasm" => "336154bf67f765f8f75d16a0accee61b5ee5f6a75b2a2905703df913bd550f3e",
            _ => unreachable!(),
        };
        let mut out = [0; 32];
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap();
        }
        out
    }
}

#[test]
fn a_changed_wasm_byte_is_refused() {
    let key = key(7);
    let manifest_bytes = serde_json::to_vec(&manifest()).unwrap();
    let sig = key.sign(&signed_message(&manifest_bytes, WASM)).to_bytes();
    let mut wasm = WASM.to_vec();
    *wasm.last_mut().unwrap() ^= 1;
    let bytes = zip_of(&[("addon.json", &manifest_bytes), ("addon.wasm", &wasm), ("addon.sig", &sig)]);
    let error = read_package(&bytes, &public(&key)).unwrap_err();
    assert!(error.contains("signature"), "{error}");
}

#[test]
fn a_changed_manifest_is_refused() {
    let key = key(7);
    let manifest_bytes = serde_json::to_vec(&manifest()).unwrap();
    let sig = key.sign(&signed_message(&manifest_bytes, WASM)).to_bytes();
    let changed = serde_json::to_vec(&AddonManifest { version: "0.1.1".into(), ..manifest() }).unwrap();
    let bytes = zip_of(&[("addon.json", &changed), ("addon.wasm", WASM), ("addon.sig", &sig)]);
    let error = read_package(&bytes, &public(&key)).unwrap_err();
    assert!(error.contains("signature"), "{error}");
}

#[test]
fn the_wrong_key_is_refused() {
    let bytes = signed(&manifest(), WASM, &key(7));
    let error = read_package(&bytes, &public(&key(8))).unwrap_err();
    assert!(error.contains("signature"), "{error}");
    // Nor is a key that isn't a point on the curve taken for one.
    let error = read_package(&bytes, &[0xff; 32]).unwrap_err();
    assert!(error.contains("key"), "{error}");
}

#[test]
fn a_signature_only_a_lax_check_takes_is_refused() {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    // The identity point as the key, and R = identity, S = 0 as the signature: the plain check
    // takes that for any message, the strict one refuses the weak key.
    let mut identity = [0; 32];
    identity[0] = 1;
    let mut sig = [0; 64];
    sig[0] = 1;
    let manifest_bytes = serde_json::to_vec(&manifest()).unwrap();
    let message = signed_message(&manifest_bytes, WASM);
    let lax = VerifyingKey::from_bytes(&identity).unwrap();
    assert!(lax.verify(&message, &Signature::from_bytes(&sig)).is_ok(), "the plain check takes it, so this tests the strict one");
    let bytes = zip_of(&[("addon.json", &manifest_bytes), ("addon.wasm", WASM), ("addon.sig", &sig)]);
    let error = read_package(&bytes, &identity).unwrap_err();
    assert!(error.contains("signature"), "{error}");
}

#[test]
fn a_zip_saying_it_holds_many_entries_is_refused_before_they_are_read() {
    let key = key(7);
    let mut bytes = signed(&manifest(), WASM, &key);
    // The end-of-directory record, the last 22 bytes of a zip with no comment: its two entry counts.
    let end = bytes.len() - 22;
    assert_eq!(bytes[end..end + 4], [0x50, 0x4b, 0x05, 0x06]);
    bytes[end + 8..end + 12].copy_from_slice(&[0xff, 0xff, 0xff, 0xff]);
    let error = read_package(&bytes, &public(&key)).unwrap_err();
    assert!(error.contains("exactly 3"), "{error}");
}

#[test]
fn a_short_signature_is_refused() {
    let key = key(7);
    let manifest_bytes = serde_json::to_vec(&manifest()).unwrap();
    let bytes = zip_of(&[("addon.json", &manifest_bytes), ("addon.wasm", WASM), ("addon.sig", &[0; 63])]);
    let error = read_package(&bytes, &public(&key)).unwrap_err();
    assert!(error.contains("64"), "{error}");
}

#[test]
fn extra_entries_or_unsafe_names_are_refused() {
    let key = key(7);
    let manifest_bytes = serde_json::to_vec(&manifest()).unwrap();
    let sig = key.sign(&signed_message(&manifest_bytes, WASM)).to_bytes();
    let three: [(&str, &[u8]); 3] = [("addon.json", &manifest_bytes), ("addon.wasm", WASM), ("addon.sig", &sig)];

    let mut four = three.to_vec();
    four.push(("notes.txt", b"more"));
    let error = read_package(&zip_of(&four), &public(&key)).unwrap_err();
    assert!(error.contains('3') || error.contains("three"), "{error}");

    let error = read_package(&zip_of(&three[..2]), &public(&key)).unwrap_err();
    assert!(error.contains('3') || error.contains("three"), "{error}");

    for bad in ["../addon.wasm", "/addon.wasm", "addons\\addon.wasm", "a/addon.wasm", "addon.wasm\0", "ADDON.WASM", "other.wasm"] {
        let entries = [three[0], (bad, WASM), three[2]];
        let error = read_package(&zip_of(&entries), &public(&key)).unwrap_err();
        assert!(error.contains("name"), "{bad:?}: {error}");
    }

    // The same name twice, in place of one of the three. The zip writer won't write it, so
    // the second name is changed in the bytes, where it is spelt the same length.
    let twice = zip_of(&[three[0], three[1], ("addon.wasx", WASM)]);
    let twice = replace(&twice, b"addon.wasx", b"addon.wasm");
    let error = read_package(&twice, &public(&key)).unwrap_err();
    assert!(error.contains("twice"), "{error}");
    // And as a fourth entry, where the zip reader would see three names and keep the last copy.
    let four = zip_of(&[three[0], three[1], ("addon.wasx", b"another module"), three[2]]);
    let four = replace(&four, b"addon.wasx", b"addon.wasm");
    let error = read_package(&four, &public(&key)).unwrap_err();
    assert!(error.contains("twice"), "{error}");

    let error = read_package(b"not a zip at all", &public(&key)).unwrap_err();
    assert!(error.contains("zip"), "{error}");
}

/// `bytes` with every `from` made `to`, which is as long.
fn replace(bytes: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let mut at = 0;
    while let Some(found) = out[at..].windows(from.len()).position(|w| w == from) {
        out[at + found..at + found + to.len()].copy_from_slice(to);
        at += found + to.len();
    }
    assert_ne!(out, bytes);
    out
}

#[test]
fn a_huge_wasm_is_refused_before_hashing() {
    let key = key(7);
    // Zeros deflate to almost nothing, so the package is small and what it unpacks to isn't.
    let huge = vec![0; MAX_WASM_BYTES as usize + 1];
    let bytes = entries_signed(&serde_json::to_vec(&manifest()).unwrap(), &huge, &key);
    assert!(bytes.len() < 1024 * 1024);
    let error = read_package(&bytes, &public(&key)).unwrap_err();
    assert!(error.contains("addon.wasm") && error.contains("32 MiB"), "{error}");

    let huge = vec![b' '; MAX_MANIFEST_BYTES as usize + 1];
    let error = read_package(&entries_signed(&huge, WASM, &key), &public(&key)).unwrap_err();
    assert!(error.contains("addon.json") && error.contains("64 KiB"), "{error}");

    // A wasm as big as allowed is read.
    let biggest = vec![0; MAX_WASM_BYTES as usize];
    let bytes = entries_signed(&serde_json::to_vec(&manifest()).unwrap(), &biggest, &key);
    assert_eq!(read_package(&bytes, &public(&key)).unwrap().wasm.len(), biggest.len());
}

#[test]
fn a_wrong_abi_or_name_is_refused() {
    let key = key(7);
    let error = read_package(&signed(&AddonManifest { abi: 2, ..manifest() }, WASM, &key), &public(&key)).unwrap_err();
    assert!(error.contains("ABI") || error.contains("abi"), "{error}");

    let error = read_package(&signed(&AddonManifest { name: "other".into(), ..manifest() }, WASM, &key), &public(&key)).unwrap_err();
    assert!(error.contains("slidev"), "{error}");

    for version in ["", "../1", "1.0 beta", "1/2"] {
        let error = read_package(&signed(&AddonManifest { version: version.into(), ..manifest() }, WASM, &key), &public(&key)).unwrap_err();
        assert!(error.contains("version"), "{version:?}: {error}");
    }

    // A manifest that isn't one, however well signed.
    let error = read_package(&entries_signed(b"{\"name\":\"slidev\"}", WASM, &key), &public(&key)).unwrap_err();
    assert!(error.contains("addon.json"), "{error}");
}
