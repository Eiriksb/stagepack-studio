//! What the tests of add-ons share: the one test key, which is the whole process's, signed
//! packages, and made-up add-ons that give back a bundle of the test's own.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use addon_abi::package::{write_package, AddonManifest};
use addon_abi::{encode, Bundle, Notice, NoticeKind, Report, MANIFEST, REPORT};
use ed25519_dalek::{Signer as _, SigningKey};

/// The seed of the one key these tests sign with, made up for them, never the owner's.
pub const TEST_SEED: [u8; 32] = [42; 32];

/// The one key the tests sign with, put in place of the owner's for the whole process the first
/// time it's asked for. Every test asks for it before it installs anything, so they all agree.
pub fn test_key() -> &'static SigningKey {
    static KEY: OnceLock<SigningKey> = OnceLock::new();
    KEY.get_or_init(|| {
        let key = SigningKey::from_bytes(&TEST_SEED);
        stagepack_studio::addons::key::set_addon_key_for_test(key.verifying_key().to_bytes());
        key
    })
}

/// Another key, which the app never takes.
pub fn other_key() -> SigningKey {
    SigningKey::from_bytes(&[43; 32])
}

pub fn manifest(version: &str) -> AddonManifest {
    AddonManifest { name: "slidev".into(), version: version.into(), abi: 1, slidev: "53.0.0".into() }
}

/// The `.spaddon` package of `wasm` at `version`, signed with `key`.
pub fn package_signed(wasm: &[u8], version: &str, key: &SigningKey) -> Vec<u8> {
    write_package(&manifest(version), wasm, |message| key.sign(message).to_bytes())
}

/// The package of `wasm` at `version`, signed with the test key, written in `dir`.
pub fn package_file(dir: &Path, wasm: &[u8], version: &str) -> PathBuf {
    let file = dir.join(format!("slidev-{version}.spaddon"));
    std::fs::write(&file, package_signed(wasm, version, test_key())).unwrap();
    file
}

/// A data segment holding `bytes` at `at`.
fn data(at: usize, bytes: &[u8]) -> String {
    let escaped: String = bytes.iter().map(|byte| format!("\\{byte:02x}")).collect();
    format!(r#"(data (i32.const {at}) "{escaped}")"#)
}

/// Where the answer lies in a made-up add-on's memory.
const ANSWER_AT: usize = 65536;

/// A made-up add-on called `slidev` whose `run` gives back `bundle`, whatever it is given.
pub fn answering_guest(bundle: &Bundle) -> Vec<u8> {
    answering_guest_at(bundle, "0.1.0")
}

/// [`answering_guest`], saying it is at `version`.
pub fn answering_guest_at(bundle: &Bundle, version: &str) -> Vec<u8> {
    let info = format!(r#"{{"name":"slidev","version":"{version}","operations":["export","import"]}}"#);
    let info = info.as_bytes();
    let answer = encode(bundle);
    // The input goes after the answer, a page on.
    let heap = (ANSWER_AT + answer.len()).div_ceil(65536) * 65536 + 65536;
    let text = format!(
        r#"(module
  (memory (export "memory") {pages})
  (global $heap (mut i32) (i32.const {heap}))
  {info_data}
  {answer_data}
  (func (export "addon_abi") (result i32) (i32.const 1))
  (func (export "alloc") (param $len i32) (result i32)
    (local $at i32)
    (local $end i32)
    (local.set $at (global.get $heap))
    (local.set $end (i32.add (local.get $at) (local.get $len)))
    (if (i32.gt_u (local.get $end) (i32.mul (memory.size) (i32.const 65536)))
      (then
        (if (i32.eq
              (memory.grow (i32.sub (i32.add (i32.shr_u (local.get $end) (i32.const 16)) (i32.const 1)) (memory.size)))
              (i32.const -1))
          (then unreachable))))
    (global.set $heap (local.get $end))
    (local.get $at))
  (func (export "info") (result i64)
    (i64.or (i64.shl (i64.const 1024) (i64.const 32)) (i64.const {info_len})))
  (func (export "run") (param $ptr i32) (param $len i32) (result i64)
    (i64.or (i64.shl (i64.const {ANSWER_AT}) (i64.const 32)) (i64.const {answer_len}))))"#,
        pages = heap / 65536,
        info_data = data(1024, info),
        info_len = info.len(),
        answer_data = data(ANSWER_AT, &answer),
        answer_len = answer.len(),
    );
    wat::parse_str(&text).unwrap_or_else(|error| panic!("{error}"))
}

pub fn file(name: &str, data: &[u8]) -> (String, Vec<u8>) {
    (name.to_string(), data.to_vec())
}

/// The report entry of `notices`.
pub fn report(notices: Vec<Notice>) -> (String, Vec<u8>) {
    file(REPORT, &serde_json::to_vec(&Report { notices }).unwrap())
}

pub fn notice(slide: Option<usize>, kind: NoticeKind, text: &str) -> Notice {
    Notice { slide, kind, text: text.to_string() }
}

/// What a made-up export gives back: its Markdown, a picture, the manifest's fields and a
/// notice on slide 2.
pub fn export_answer() -> Bundle {
    vec![
        file("slides.md", b"---\ntheme: default\ntitle: Made up\n---\n\n# Made up\n"),
        file(MANIFEST, br#"{"title":"Made up"}"#),
        file("public/photo.png", b"not really a picture"),
        report(vec![notice(Some(2), NoticeKind::Approximation, "The gauge may not sit where it does on the slide.")]),
    ]
}

/// What a made-up import gives back: a talk of two slides, a picture and a notice on each.
pub fn import_answer() -> Bundle {
    vec![
        file("slides.md", b"# From Slidev\n\n---\n\n# The second slide\n\n![A photo](assets/photo.png)\n"),
        file("assets/photo.png", include_bytes!("../../../../fixtures/slidev-export/assets/photo.png")),
        report(vec![
            notice(None, NoticeKind::Approximation, "The talk's theme is a custom one, so it looks like the default."),
            notice(Some(2), NoticeKind::Placeholder, "A Vue component became a labelled box."),
        ]),
    ]
}

/// A Slidev talk made for Stagepack, as Stagepack writes one, in `dir`: its manifest, its
/// Markdown, a picture, Stagepack's own packages and a hidden file.
pub fn stagepack_talk(dir: &Path) -> PathBuf {
    use std::io::Write as _;

    let file = dir.join("made-up.sptx");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&file).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    for (name, data) in [
        ("stagepack.json", &br#"{"format":1,"title":"Made up","entry":"slides.md","slidev":"53.0.0"}"#[..]),
        ("slides.md", b"# From Slidev\n"),
        ("public/photo.png", include_bytes!("../../../../fixtures/slidev-export/assets/photo.png")),
        ("vendor/slidev/cli.js", b"console.log('never run')"),
        (".gitignore", b"node_modules\n"),
    ] {
        zip.start_file(name, options).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
    file
}
