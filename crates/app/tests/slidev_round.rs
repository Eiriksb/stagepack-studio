//! The real Slidev add-on, end to end: built for WebAssembly and signed as `package addon` does
//! it, installed, given the export fixture, and given back what it wrote.

#[path = "support/addons.rs"]
mod support;

use std::io::Read as _;
use std::path::Path;
use std::sync::Arc;

use slides_engine::{FolderFiles, TalkFiles};
use stagepack_studio::addons::{self, InstalledState};
use stagepack_studio::export::{ExportInput, export_slidev};
use support::*;

/// The keys, in order, that Stagepack's manifest has.
const MANIFEST_KEYS: [&str; 8] = ["created", "entry", "format", "id", "meta", "slidev", "title", "updated"];

#[test]
#[ignore = "builds the add-on for WebAssembly"]
fn slidev_round() {
    let scratch = tempfile::tempdir().unwrap();
    // The one test key, as a key file `package addon` reads: only its owner may read it.
    test_key();
    let key_file = scratch.path().join("keys/addon-signing.key");
    std::fs::create_dir_all(key_file.parent().unwrap()).unwrap();
    std::fs::write(&key_file, TEST_SEED).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(key_file.parent().unwrap(), std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&key_file, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let (package, _) = xtask::addon::package_addon(&key_file, &scratch.path().join("package")).unwrap();

    let app = scratch.path().join("app");
    let installed = addons::install_from_file(&app, &package).unwrap();
    let InstalledState::Ready(addon) = installed.state else { panic!("the add-on is damaged") };

    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/slidev-export");
    let files: Arc<dyn TalkFiles + Send + Sync> = Arc::new(FolderFiles::new(fixture.clone()));
    let input = ExportInput { source: std::fs::read_to_string(fixture.join("slides.md")).unwrap(), files, talk_fonts: Vec::new() };
    let out = scratch.path().join("Lantern Reef (Slidev).sptx");
    let now = chrono::Utc::now();
    let warnings = export_slidev(input, &addon, &out, now, &uuid::Uuid::new_v4().to_string()).unwrap();
    println!("export warnings: {warnings:#?}");

    let mut zip = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
    let names: Vec<String> = (0..zip.len()).map(|at| zip.by_index(at).unwrap().name().unwrap().to_string()).collect();
    assert_eq!(names[0], "slides.md");
    for name in &names {
        assert!(name == "slides.md" || name == "stagepack.json" || name.starts_with("public/"), "{name}");
        assert!(!slidev_addon::is_code_name(name), "{name} is on the code list");
    }
    let mut manifest = Vec::new();
    zip.by_name("stagepack.json").unwrap().read_to_end(&mut manifest).unwrap();
    let manifest: serde_json::Map<String, serde_json::Value> = serde_json::from_slice(&manifest).unwrap();
    let mut keys: Vec<&str> = manifest.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, MANIFEST_KEYS);
    assert_eq!(manifest["slidev"], "53.0.0");
    assert_eq!(manifest["format"], 1);
    assert_eq!(manifest["entry"], "slides.md");
    assert_eq!(manifest["title"], "Notes from the Lantern Reef Survey");
    assert_eq!(manifest["meta"], serde_json::json!({}));
    let id = uuid::Uuid::parse_str(manifest["id"].as_str().unwrap()).unwrap();
    assert_eq!(id.get_version_num(), 4);
    for stamp in ["created", "updated"] {
        let at = chrono::DateTime::parse_from_rfc3339(manifest[stamp].as_str().unwrap()).unwrap();
        assert_eq!(at.offset().local_minus_utc(), 0);
        assert!(manifest[stamp].as_str().unwrap().ends_with('Z'));
    }
    // The pictures the app drew are in the export.
    let baked: Vec<&String> = names.iter().filter(|name| name.starts_with("public/stagepack-baked/")).collect();
    assert!(!baked.is_empty(), "{names:?}");

    // And read back, it's a talk again, every slide readable, each drawn part said to be a picture.
    let (slides, files, report) = addons::import_slidev(&out, &addon).unwrap();
    let talk = slides_format::parse_talk(&slides);
    for (index, slide) in talk.slides.iter().enumerate() {
        assert!(slide.error.is_none(), "slide {} can't be read: {:?}\n{slides}", index + 1, slide.error);
    }
    assert!(files.iter().all(|(name, _)| name.starts_with("assets/")), "{files:?}");
    println!("import notices: {:#?}", report.notices);
    let pictured = files.iter().filter(|(name, _)| name.contains("stagepack-baked")).count();
    assert_eq!(pictured, baked.len(), "{files:?}");
    for (name, _) in files.iter().filter(|(name, _)| name.contains("stagepack-baked")) {
        let file = name.rsplit('/').next().unwrap();
        assert!(
            report.notices.iter().any(|notice| notice.kind == addon_abi::NoticeKind::Frozen && notice.text.contains(file)),
            "nothing says {name} is a picture now"
        );
    }
}

#[test]
#[ignore = "builds the add-on for WebAssembly"]
fn slidev_round_just_under_the_budget() {
    let scratch = tempfile::tempdir().unwrap();
    test_key();
    let key_file = scratch.path().join("keys/addon-signing.key");
    std::fs::create_dir_all(key_file.parent().unwrap()).unwrap();
    std::fs::write(&key_file, TEST_SEED).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(key_file.parent().unwrap(), std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&key_file, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let (package, _) = xtask::addon::package_addon(&key_file, &scratch.path().join("package")).unwrap();
    let installed = addons::install_from_file(&scratch.path().join("app"), &package).unwrap();
    let InstalledState::Ready(addon) = installed.state else { panic!("the add-on is damaged") };

    // Two pictures that aren't videos, so nothing can be left out, coming to 8 MiB under the
    // budget with everything else the bundle holds.
    let talk = scratch.path().join("talk");
    std::fs::create_dir_all(talk.join("assets")).unwrap();
    let each = (stagepack_studio::export::SLIDEV_EXPORT_BUDGET - 8 * 1024 * 1024) / 2;
    let mut source = String::from("# Large pictures\n\n");
    for n in 0..2 {
        let file = std::fs::File::create(talk.join(format!("assets/big-{n}.png"))).unwrap();
        file.set_len(each).unwrap();
        source.push_str(&format!("![Big](assets/big-{n}.png)\n\n"));
    }
    std::fs::write(talk.join("slides.md"), &source).unwrap();
    let files: Arc<dyn TalkFiles + Send + Sync> = Arc::new(FolderFiles::new(talk.clone()));
    let out = scratch.path().join("big (Slidev).sptx");
    let warnings = export_slidev(ExportInput { source, files, talk_fonts: Vec::new() }, &addon, &out, chrono::Utc::now(), &uuid::Uuid::new_v4().to_string())
        .unwrap_or_else(|problem| panic!("an export under the budget fails: {problem}"));
    println!("export warnings: {warnings:#?}");
    let zip = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
    let names: Vec<String> = zip.file_names().map(|name| name.unwrap().into_owned()).collect();
    assert!(names.iter().any(|name| name == "public/big-0.png") && names.iter().any(|name| name == "public/big-1.png"), "{names:?}");
}
