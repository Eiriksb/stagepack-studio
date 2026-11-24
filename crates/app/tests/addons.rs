//! The Slidev add-on in the app: installing it from a signed file, finding it damaged, taking it
//! away, the pictures drawn for an export, an export too large to give whole, and an import that
//! never writes to the talk it reads. Every app folder is a scratch folder of the test's own.

#[path = "support/addons.rs"]
mod support;

use std::io::Read as _;
use std::path::Path;
use std::sync::Arc;

use addon_abi::NoticeKind;
use sha2::{Digest, Sha256};
use slides_engine::{Deck, FolderFiles, Fonts, TalkFiles};
use stagepack_studio::addons::{self, InstalledState};
use stagepack_studio::export::{ExportInput, export_slidev};
use support::*;

fn installed_names(app: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(app.join("addons/slidev"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn ready_version(app: &Path) -> Option<String> {
    match addons::load(app) {
        Some(addons::Installed { version, state: InstalledState::Ready(_) }) => Some(version),
        _ => None,
    }
}

#[test]
fn install_from_file_puts_three_files_in_addons_slidev() {
    test_key();
    let app = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let file = package_file(scratch.path(), &answering_guest(&export_answer()), "0.1.0");

    let installed = addons::install_from_file(app.path(), &file).unwrap();
    assert_eq!(installed.version, "0.1.0");
    assert!(matches!(installed.state, InstalledState::Ready(_)));
    assert_eq!(installed_names(app.path()), ["addon.json", "addon.sig", "addon.wasm"]);
    // Nothing else is left beside it, the folder it was written in first among them.
    let beside: Vec<_> = std::fs::read_dir(app.path().join("addons")).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(beside.iter().filter(|name| *name != ".lock").collect::<Vec<_>>(), ["slidev"]);
    assert_eq!(ready_version(app.path()).as_deref(), Some("0.1.0"));

    // A later version takes its place.
    let file = package_file(scratch.path(), &answering_guest_at(&export_answer(), "0.2.0"), "0.2.0");
    assert_eq!(addons::install_from_file(app.path(), &file).unwrap().version, "0.2.0");
    assert_eq!(ready_version(app.path()).as_deref(), Some("0.2.0"));
    assert_eq!(installed_names(app.path()), ["addon.json", "addon.sig", "addon.wasm"]);
}

#[test]
fn a_bad_signature_is_refused_and_an_earlier_install_stays() {
    test_key();
    let app = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let wasm = answering_guest(&export_answer());
    addons::install_from_file(app.path(), &package_file(scratch.path(), &wasm, "0.1.0")).unwrap();

    let forged = scratch.path().join("slidev-0.2.0.spaddon");
    std::fs::write(&forged, package_signed(&wasm, "0.2.0", &other_key())).unwrap();
    let refused = addons::install_from_file(app.path(), &forged).unwrap_err();
    assert!(refused.contains("signature"), "{refused}");
    assert!(!refused.contains('\n'), "one line: {refused}");
    assert_eq!(ready_version(app.path()).as_deref(), Some("0.1.0"));

    // Nor is a file that isn't a package, or one that isn't there.
    let not_a_package = scratch.path().join("notes.spaddon");
    std::fs::write(&not_a_package, b"not a zip").unwrap();
    assert!(addons::install_from_file(app.path(), &not_a_package).is_err());
    assert!(addons::install_from_file(app.path(), &scratch.path().join("missing.spaddon")).is_err());
    // Nor a package for another Slidev.
    let other_slidev = scratch.path().join("slidev-0.3.0.spaddon");
    let mut manifest = manifest("0.3.0");
    manifest.slidev = "52.0.0".into();
    let key = test_key();
    std::fs::write(&other_slidev, addon_abi::package::write_package(&manifest, &wasm, |m| ed25519_dalek::Signer::sign(key, m).to_bytes())).unwrap();
    let refused = addons::install_from_file(app.path(), &other_slidev).unwrap_err();
    assert!(refused.contains("53.0.0"), "{refused}");
    assert_eq!(ready_version(app.path()).as_deref(), Some("0.1.0"));
}

#[test]
fn a_damaged_install_loads_as_damaged() {
    test_key();
    let app = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    addons::install_from_file(app.path(), &package_file(scratch.path(), &answering_guest(&export_answer()), "0.1.0")).unwrap();
    let wasm = app.path().join("addons/slidev/addon.wasm");
    let mut bytes = std::fs::read(&wasm).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    std::fs::write(&wasm, bytes).unwrap();

    match addons::load(app.path()) {
        Some(addons::Installed { version, state: InstalledState::Damaged(sentence) }) => {
            assert_eq!(version, "0.1.0");
            assert!(sentence.contains("signature"), "{sentence}");
        }
        other => panic!("expected damaged, got {other:?}"),
    }
    assert!(matches!(addons::status(app.path()), addons::Status::Damaged(_)));

    // One of the three gone is damage too.
    std::fs::remove_file(app.path().join("addons/slidev/addon.sig")).unwrap();
    assert!(matches!(addons::load(app.path()), Some(addons::Installed { state: InstalledState::Damaged(_), .. })));
}

#[test]
fn remove_takes_only_the_addon_folder() {
    test_key();
    let app = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    std::fs::write(app.path().join("build-permissions.json"), "{}").unwrap();
    std::fs::create_dir_all(app.path().join("addons/other")).unwrap();
    std::fs::write(app.path().join("addons/other/kept.txt"), "kept").unwrap();
    addons::install_from_file(app.path(), &package_file(scratch.path(), &answering_guest(&export_answer()), "0.1.0")).unwrap();

    addons::remove(app.path()).unwrap();
    assert!(!app.path().join("addons/slidev").exists());
    assert!(app.path().join("build-permissions.json").is_file());
    assert_eq!(std::fs::read_to_string(app.path().join("addons/other/kept.txt")).unwrap(), "kept");
    assert!(addons::load(app.path()).is_none());
    assert_eq!(addons::status(app.path()), addons::Status::NotInstalled);
    // Taking away what isn't there is no failure.
    addons::remove(app.path()).unwrap();
}

/// The tags of the fixture's slides the app draws for an export, by slide and `n`, as the
/// export's contract numbers them: every tag but Box, Shape, Text and Picture, depth first.
fn baked_tags(source: &str) -> Vec<(usize, usize, String)> {
    fn walk(blocks: &[slides_format::Block], found: &mut Vec<String>) {
        for block in blocks {
            match block {
                slides_format::Block::Tag(tag) => {
                    if !["Box", "Shape", "Text", "Picture"].contains(&tag.name.as_str()) {
                        found.push(tag.name.clone());
                    }
                    walk(&tag.children, found);
                }
                slides_format::Block::List { items, .. } => items.iter().for_each(|item| walk(item, found)),
                _ => {}
            }
        }
    }
    let talk = slides_format::parse_talk(source);
    let mut all = Vec::new();
    for (index, slide) in talk.slides.iter().enumerate() {
        let mut found = Vec::new();
        walk(&slide.blocks, &mut found);
        all.extend(found.into_iter().enumerate().map(|(n, tag)| (index + 1, n, tag)));
    }
    all
}

fn fixture() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/slidev-export")
}

#[test]
fn bake_writes_an_index_and_a_png_per_baked_tag() {
    let source = std::fs::read_to_string(fixture().join("slides.md")).unwrap();
    let mut deck = Deck::open(Box::new(FolderFiles::new(fixture())), Fonts::with_system_fallback(), source.clone());
    let baked = addons::bake::bake(&mut deck);

    let (_, index) = baked.iter().find(|(name, _)| name == "baked/index.json").expect("an index");
    let index: Vec<serde_json::Value> = serde_json::from_slice(index).unwrap();
    let expected = baked_tags(&source);
    let drawn: Vec<(usize, usize, String)> = index
        .iter()
        .map(|entry| (entry["slide"].as_u64().unwrap() as usize, entry["n"].as_u64().unwrap() as usize, entry["tag"].as_str().unwrap().to_string()))
        .collect();
    // The fixture's icon and its component, each by the number the export finds it by.
    assert_eq!(drawn, expected);
    assert!(expected.iter().any(|(_, _, tag)| tag == "Icon") && expected.iter().any(|(_, _, tag)| tag == "TideGauge"));
    for entry in &index {
        let (slide, n) = (entry["slide"].as_u64().unwrap(), entry["n"].as_u64().unwrap());
        let (w, h) = (entry["w"].as_f64().unwrap(), entry["h"].as_f64().unwrap());
        let name = format!("baked/{slide}/{n}.png");
        let (_, png) = baked.iter().find(|(file, _)| *file == name).unwrap_or_else(|| panic!("no {name}"));
        let picture = image::load_from_memory(png).unwrap();
        // Twice the size it is on the slide.
        assert!((f64::from(picture.width()) - 2.0 * w).abs() <= 2.0, "{name}: {} for {w}", picture.width());
        assert!((f64::from(picture.height()) - 2.0 * h).abs() <= 2.0, "{name}: {} for {h}", picture.height());
        assert!(picture.to_rgba8().pixels().any(|pixel| pixel[3] > 0), "{name} is blank");
        assert_eq!(entry["steps"], serde_json::json!([]));
        assert_eq!(entry["animated"], false);
    }
    // The icon is where its tag puts it, in slide units.
    let entry = index.iter().find(|entry| entry["tag"] == "Icon").unwrap();
    assert_eq!((entry["x"].as_f64().unwrap(), entry["y"].as_f64().unwrap()), (100.0, 840.0));
    // Every file is the index or one of its pictures.
    assert_eq!(baked.len(), index.len() + 1);
}

/// A talk in a folder: a slide with a video of `video_bytes`, made sparse, so nothing is
/// written of it, and a picture.
fn talk_with_a_video(video_bytes: u64) -> tempfile::TempDir {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(
        folder.path().join("slides.md"),
        "# A long video\n\n<Video src=\"assets/long.mp4\" x=\"100\" y=\"200\" w=\"960\" h=\"540\" />\n\n![A photo](assets/photo.png)\n",
    )
    .unwrap();
    std::fs::create_dir_all(folder.path().join("assets")).unwrap();
    std::fs::copy(fixture().join("assets/photo.png"), folder.path().join("assets/photo.png")).unwrap();
    let video = std::fs::File::create(folder.path().join("assets/long.mp4")).unwrap();
    video.set_len(video_bytes).unwrap();
    folder
}

fn input_for(folder: &Path) -> ExportInput {
    let files: Arc<dyn TalkFiles + Send + Sync> = Arc::new(FolderFiles::new(folder.to_path_buf()));
    ExportInput { source: std::fs::read_to_string(folder.join("slides.md")).unwrap(), files, talk_fonts: Vec::new() }
}

fn zip_names(file: &Path) -> Vec<String> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(file).unwrap()).unwrap();
    (0..zip.len()).map(|at| zip.by_index(at).unwrap().name().unwrap().to_string()).collect()
}

fn zip_entry(file: &Path, name: &str) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(file).unwrap()).unwrap();
    let mut bytes = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut bytes).unwrap();
    bytes
}

fn now() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339("2026-11-24T09:30:00Z").unwrap().to_utc()
}

/// A report's notices as lines, each by its slide.
fn texts(report: &addon_abi::Report) -> Vec<String> {
    report
        .notices
        .iter()
        .map(|notice| match notice.slide {
            Some(slide) => format!("Slide {slide}: {}", notice.text),
            None => notice.text.clone(),
        })
        .collect()
}

const ID: &str = "0b5c7a4e-3f1d-4c2a-9e8b-6d5f4a3b2c1d";

#[test]
fn an_export_over_the_limit_leaves_video_out_with_a_notice() {
    let addon = slides_components::addon::Addon::load(&answering_guest(&export_answer())).unwrap();
    let folder = talk_with_a_video(600 * 1024 * 1024);
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("A long video (Slidev).sptx");

    let warnings = texts(&export_slidev(input_for(folder.path()), &addon, &out, now(), ID).unwrap());
    assert!(
        warnings.iter().any(|warning| warning == "assets/long.mp4 is too large to include; copy it into public/ yourself."),
        "{warnings:?}"
    );
    // The add-on's own notice, by its slide.
    assert!(warnings.iter().any(|warning| warning == "Slide 2: The gauge may not sit where it does on the slide."), "{warnings:?}");
    let names = zip_names(&out);
    assert_eq!(names, ["slides.md", "stagepack.json", "public/photo.png"]);

    let manifest: serde_json::Value = serde_json::from_slice(&zip_entry(&out, "stagepack.json")).unwrap();
    assert_eq!(
        manifest,
        serde_json::json!({
            "format": 1,
            "id": ID,
            "title": "Made up",
            "created": "2026-11-24T09:30:00Z",
            "updated": "2026-11-24T09:30:00Z",
            "slidev": "53.0.0",
            "entry": "slides.md",
            "meta": {},
        })
    );

    // A video that fits goes in whole: only one too large for the bundle is left out.
    let small = talk_with_a_video(1024);
    let out = out_dir.path().join("small.sptx");
    let warnings = texts(&export_slidev(input_for(small.path()), &addon, &out, now(), ID).unwrap());
    assert!(!warnings.iter().any(|warning| warning.contains("too large")), "{warnings:?}");
}

#[test]
fn an_export_still_too_large_without_its_videos_is_refused() {
    let addon = slides_components::addon::Addon::load(&answering_guest(&export_answer())).unwrap();
    let folder = tempfile::tempdir().unwrap();
    // Three pictures of 200 MiB each, made sparse: none is a video, so none can be left out.
    let mut source = String::from("# Large pictures\n\n");
    std::fs::create_dir_all(folder.path().join("assets")).unwrap();
    for n in 0..3 {
        let file = std::fs::File::create(folder.path().join(format!("assets/big-{n}.png"))).unwrap();
        file.set_len(200 * 1024 * 1024).unwrap();
        source.push_str(&format!("![Big](assets/big-{n}.png)\n\n"));
    }
    std::fs::write(folder.path().join("slides.md"), source).unwrap();
    let out = folder.path().join("out.sptx");
    let refused = export_slidev(input_for(folder.path()), &addon, &out, now(), ID).unwrap_err();
    assert!(refused.contains("320 MiB"), "{refused}");
    // It has no videos, so it says nothing of leaving them out.
    assert!(!refused.contains("videos"), "{refused}");
    assert!(!out.exists());
}

fn sha256_of(file: &Path) -> Vec<u8> {
    Sha256::digest(std::fs::read(file).unwrap()).to_vec()
}

#[test]
fn import_leaves_the_original_untouched() {
    let addon = slides_components::addon::Addon::load(&answering_guest(&import_answer())).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let talk = stagepack_talk(dir.path());
    let (hash, modified) = (sha256_of(&talk), std::fs::metadata(&talk).unwrap().modified().unwrap());

    let (slides, files, report) = stagepack_studio::addons::import_slidev(&talk, &addon).unwrap();
    assert!(slides.starts_with("# From Slidev"));
    assert_eq!(files.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(), ["assets/photo.png"]);
    let vendor = report.notices.iter().find(|notice| notice.text == "Stagepack's bundled packages (vendor/) aren't used.").expect("a vendor notice");
    assert_eq!((vendor.slide, vendor.kind), (None, NoticeKind::LeftOut));
    let hidden = report.notices.iter().find(|notice| notice.text.contains(".gitignore")).expect("the hidden file is named");
    assert_eq!(hidden.kind, NoticeKind::LeftOut);
    // The add-on's own notices follow.
    assert!(report.notices.iter().any(|notice| notice.slide == Some(2) && notice.kind == NoticeKind::Placeholder));

    assert_eq!(sha256_of(&talk), hash);
    assert_eq!(std::fs::metadata(&talk).unwrap().modified().unwrap(), modified);
    // And nothing was written beside it.
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn names_left_out_of_an_import_are_listed_once_twenty_at_most() {
    use std::io::Write as _;

    let addon = slides_components::addon::Addon::load(&answering_guest(&import_answer())).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let talk = dir.path().join("many.sptx");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&talk).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    let mut names: Vec<String> = vec![
        "stagepack.json".into(),
        "slides.md".into(),
        "public/Photo.png".into(),
        "public/photo.png".into(),
        // The same name composed and decomposed, as macOS would take it: one file there.
        "public/caf\u{e9}.png".into(),
        "public/cafe\u{301}.png".into(),
        // Stagepack's own, in another case.
        "Vendor/cli.js".into(),
    ];
    names.extend((0..25).map(|n| format!("public/.hidden-{n:02}")));
    for name in &names {
        zip.start_file(name.as_str(), options).unwrap();
        zip.write_all(b"x").unwrap();
    }
    zip.finish().unwrap();

    let (_, _, report) = stagepack_studio::addons::import_slidev(&talk, &addon).unwrap();
    let left: Vec<_> = report.notices.iter().filter(|notice| notice.text.contains("hidden-")).collect();
    assert_eq!(left.len(), 1, "{left:?}");
    assert!(left[0].text.contains("public/photo.png"), "a name differing only in case is left out: {}", left[0].text);
    // photo.png, the decomposed café.png, then 18 of the 25 hidden: 7 more.
    assert!(left[0].text.contains("cafe\u{301}.png"), "the decomposed name is left out: {}", left[0].text);
    assert!(!left[0].text.contains("caf\u{e9}.png"), "the first of the two stays: {}", left[0].text);
    assert!(left[0].text.contains("and 7 more"), "{}", left[0].text);
    assert!(left[0].text.starts_with("27 files"), "{}", left[0].text);
    assert!(report.notices.iter().any(|notice| notice.text == stagepack_studio::addons::VENDOR_NOTICE));
}

mod probes {
    use toolkit::prelude::*;

    /// The click the slide is on, of three.
    #[component]
    pub fn Tally(ctx: &Ctx) -> Element {
        text(ctx.steps(3).to_string())
    }

    /// The time since the slide appeared.
    #[component]
    pub fn Clock(ctx: &Ctx) -> Element {
        text(format!("{:.2}", ctx.time()))
    }
}

#[test]
fn a_part_that_changes_on_clicks_has_a_picture_for_each_and_one_that_moves_has_one() {
    let mut registry = slides_engine::builtin_registry();
    registry.add(probes::Tally_component());
    registry.add(probes::Clock_component());
    let source = "# Changing\n\n<Tally x=\"100\" y=\"300\" w=\"300\" h=\"200\" />\n\n<Clock x=\"600\" y=\"300\" w=\"300\" h=\"200\" />\n";
    let mut deck = Deck::with_registry(Box::new(FolderFiles::new(fixture())), Fonts::bundled_only(), source.to_string(), registry);
    let baked = addons::bake::bake(&mut deck);
    let (_, index) = baked.iter().find(|(name, _)| name == "baked/index.json").unwrap();
    let index: Vec<serde_json::Value> = serde_json::from_slice(index).unwrap();
    let tally = index.iter().find(|entry| entry["tag"] == "Tally").expect("the tally is drawn");
    assert_eq!((tally["n"].clone(), tally["steps"].clone(), tally["animated"].clone()), (serde_json::json!(0), serde_json::json!([0, 1, 2, 3]), serde_json::json!(false)));
    let clock = index.iter().find(|entry| entry["tag"] == "Clock").expect("the clock is drawn");
    assert_eq!((clock["n"].clone(), clock["steps"].clone(), clock["animated"].clone()), (serde_json::json!(1), serde_json::json!([]), serde_json::json!(true)));
    let mut names: Vec<&str> = baked.iter().map(|(name, _)| name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["baked/1/0@0.png", "baked/1/0@1.png", "baked/1/0@2.png", "baked/1/0@3.png", "baked/1/1.png", "baked/index.json"]);
    // Each step's picture is of that step.
    let step = |at: &str| baked.iter().find(|(name, _)| name == at).unwrap().1.clone();
    assert_ne!(step("baked/1/0@0.png"), step("baked/1/0@3.png"));
}

#[test]
fn a_video_that_would_take_the_export_past_the_add_ons_memory_is_left_out() {
    let addon = slides_components::addon::Addon::load(&answering_guest(&export_answer())).unwrap();
    // 200 MiB of video and 200 MiB of picture: under the 512 MiB a bundle may be, over the 320 MiB
    // the add-on can hold three times over.
    let folder = talk_with_a_video(200 * 1024 * 1024);
    let picture = std::fs::File::create(folder.path().join("assets/photo.png")).unwrap();
    picture.set_len(200 * 1024 * 1024).unwrap();
    assert_eq!(stagepack_studio::export::SLIDEV_EXPORT_BUDGET, 320 * 1024 * 1024);
    let out = folder.path().join("out.sptx");
    let warnings = texts(&export_slidev(input_for(folder.path()), &addon, &out, now(), ID).unwrap());
    assert!(warnings.iter().any(|warning| warning == "assets/long.mp4 is too large to include; copy it into public/ yourself."), "{warnings:?}");
}

#[test]
fn an_install_cut_short_between_its_two_moves_puts_the_earlier_one_back() {
    test_key();
    let app = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    addons::install_from_file(app.path(), &package_file(scratch.path(), &answering_guest(&export_answer()), "0.1.0")).unwrap();
    // As if the app stopped once the earlier add-on was moved aside.
    std::fs::rename(app.path().join("addons/slidev"), app.path().join("addons/.slidev-old-0123")).unwrap();
    assert_eq!(addons::status(app.path()), addons::Status::Installed("0.1.0".into()));
    assert!(app.path().join("addons/slidev/addon.wasm").is_file());
}

#[cfg(unix)]
#[test]
fn an_add_ons_folder_that_is_a_link_is_not_written_through() {
    test_key();
    let app = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), app.path().join("addons")).unwrap();
    let refused = addons::install_from_file(app.path(), &package_file(scratch.path(), &answering_guest(&export_answer()), "0.1.0")).unwrap_err();
    assert!(refused.contains("link"), "{refused}");
    assert_eq!(std::fs::read_dir(elsewhere.path()).unwrap().count(), 0);
}

#[test]
fn a_part_inside_a_turned_box_is_placed_in_slide_units_and_turned_with_it() {
    let source = "# Inside\n\n<Box x=\"400\" y=\"300\" w=\"600\" h=\"400\" rotate=\"15\" fill=\"#fde68a\">\n\n<Icon name=\"arrow-right\" size=\"96\" />\n\n</Box>\n";
    let mut deck = Deck::open(Box::new(FolderFiles::new(fixture())), Fonts::with_system_fallback(), source.to_string());
    let baked = addons::bake::bake(&mut deck);
    let (_, index) = baked.iter().find(|(name, _)| name == "baked/index.json").unwrap();
    let index: Vec<serde_json::Value> = serde_json::from_slice(index).unwrap();
    let icon = index.iter().find(|entry| entry["tag"] == "Icon").expect("the icon is drawn");
    assert_eq!(icon["rotate"], 0.0, "the box turns it, not its own turn");
    let (x, y) = (icon["x"].as_f64().unwrap(), icon["y"].as_f64().unwrap());
    // Inside the box, as it is before it's turned: at its top left, past the box's padding.
    assert!((400.0..1000.0).contains(&x) && (300.0..700.0).contains(&y), "{icon}");
    assert!((x - 424.0).abs() < 1.0 && (y - 324.0).abs() < 1.0, "{icon}");
    // Turning a part about the box's centre keeps its size: the icon's own.
    let (w, h) = (icon["w"].as_f64().unwrap(), icon["h"].as_f64().unwrap());
    assert!((w - 96.0).abs() < 1.0 && (h - 96.0).abs() < 1.0, "{icon}");
    // And its picture is of the icon, not of the box's fill alone.
    let picture = image::load_from_memory(&baked.iter().find(|(name, _)| name == "baked/1/0.png").unwrap().1).unwrap().to_rgba8();
    let fill = image::Rgba([0xfd, 0xe6, 0x8a, 0xff]);
    assert!(picture.pixels().any(|pixel| *pixel != fill && pixel[3] > 0), "the icon's picture is only the box's fill");
}

#[test]
fn the_add_ons_path_rule_is_the_engines() {
    for path in ["photo.png", "./photo.png", "a/../photo.png", "assets//x/./../a.png", "/photo.png", "../photo.png", "a/../../b", "", ".", "media/"] {
        assert_eq!(addon_abi::normalise(path), slides_engine::normalise(path), "{path:?}");
    }
}

/// A Stagepack talk whose zip holds `vendor` files under `vendor/` and `own` files of its own,
/// besides its manifest and Markdown.
fn talk_of_many(dir: &Path, vendor: usize, own: usize) -> std::path::PathBuf {
    use std::io::Write as _;

    let talk = dir.join("many.sptx");
    let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(std::fs::File::create(&talk).unwrap()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.add_directory("vendor/", options).unwrap();
    for (name, data) in [("stagepack.json", &br#"{"format":1,"entry":"slides.md"}"#[..]), ("slides.md", b"# Many\n")] {
        zip.start_file(name, options).unwrap();
        zip.write_all(data).unwrap();
    }
    for n in 0..vendor {
        zip.start_file(format!("vendor/node_modules/theme/{n}.js"), options).unwrap();
    }
    for n in 0..own {
        zip.start_file(format!("public/{n}.txt"), options).unwrap();
    }
    zip.finish().unwrap();
    talk
}

#[test]
fn a_talk_bundling_many_packages_imports_without_them() {
    let addon = slides_components::addon::Addon::load(&answering_guest(&import_answer())).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (slides, _, report) = stagepack_studio::addons::import_slidev(&talk_of_many(dir.path(), 12_000, 0), &addon).unwrap();
    assert!(slides.starts_with("# From Slidev"));
    assert!(report.notices.iter().any(|notice| notice.text == stagepack_studio::addons::VENDOR_NOTICE));

    // Its own files are what count: 9 998 with the slides fit, and one more doesn't.
    assert!(stagepack_studio::addons::import_slidev(&talk_of_many(dir.path(), 0, 9_997), &addon).is_ok());
    let refused = stagepack_studio::addons::import_slidev(&talk_of_many(dir.path(), 0, 9_998), &addon).unwrap_err();
    assert!(refused.contains("more than 9998 files"), "{refused}");
}

/// `bytes`, a zip, with `insert` put just before its end record, which is its last 22 bytes.
fn before_the_end_record(bytes: &[u8], insert: &[u8]) -> Vec<u8> {
    let end = bytes.len() - 22;
    let mut out = bytes[..end].to_vec();
    out.extend(insert);
    out.extend(&bytes[end..]);
    out
}

#[test]
fn a_talk_whose_zip_lists_a_huge_directory_is_refused_before_it_is_read() {
    let addon = slides_components::addon::Addon::load(&answering_guest(&import_answer())).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let talk = stagepack_talk(dir.path());
    let bytes = std::fs::read(&talk).unwrap();

    // The end record's directory size, at 12..16, said to be 4 GiB less two bytes.
    let mut large = bytes.clone();
    let end = large.len() - 22;
    large[end + 12..end + 16].copy_from_slice(&0xffff_fffeu32.to_le_bytes());
    std::fs::write(&talk, &large).unwrap();
    let refused = stagepack_studio::addons::import_slidev(&talk, &addon).unwrap_err();
    assert!(refused.contains("list of entries"), "{refused}");

    // A zip64 end record, which a locator before the end record points to, listing a million entries.
    let at = (bytes.len() - 22) as u64;
    let mut zip64 = vec![0x50, 0x4b, 0x06, 0x06];
    zip64.extend(44u64.to_le_bytes());
    zip64.extend([45, 0, 45, 0]);
    zip64.extend([0u8; 8]);
    zip64.extend(1_000_000u64.to_le_bytes());
    zip64.extend(1_000_000u64.to_le_bytes());
    zip64.extend(100u64.to_le_bytes());
    zip64.extend(0u64.to_le_bytes());
    let mut locator = vec![0x50, 0x4b, 0x06, 0x07];
    locator.extend(0u32.to_le_bytes());
    locator.extend(at.to_le_bytes());
    locator.extend(1u32.to_le_bytes());
    std::fs::write(&talk, before_the_end_record(&bytes, &[zip64, locator].concat())).unwrap();
    let refused = stagepack_studio::addons::import_slidev(&talk, &addon).unwrap_err();
    assert!(refused.contains("200000 entries"), "{refused}");
}

#[test]
fn names_the_add_on_gives_back_that_would_be_one_file_are_kept_once() {
    // An import's assets, composed and decomposed.
    let mut answer = import_answer();
    answer.push(file("assets/caf\u{e9}.png", b"one"));
    answer.push(file("assets/cafe\u{301}.png", b"two"));
    let addon = slides_components::addon::Addon::load(&answering_guest(&answer)).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (_, files, report) = stagepack_studio::addons::import_slidev(&stagepack_talk(dir.path()), &addon).unwrap();
    let names: Vec<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["assets/photo.png", "assets/caf\u{e9}.png"]);
    assert!(report.notices.iter().any(|notice| notice.text.contains("cafe\u{301}.png") && notice.text.starts_with("1 file the add-on gave back")), "{:?}", report.notices);

    // And an export's files under public/, composed and decomposed. Letter case alone the add-on's
    // answer can't differ in at all.
    let mut answer = export_answer();
    answer.push(file("public/caf\u{e9}.png", b"one"));
    answer.push(file("public/cafe\u{301}.png", b"two"));
    let addon = slides_components::addon::Addon::load(&answering_guest(&answer)).unwrap();
    let folder = talk_with_a_video(1024);
    let out = folder.path().join("out.sptx");
    let warnings = texts(&export_slidev(input_for(folder.path()), &addon, &out, now(), ID).unwrap());
    assert_eq!(zip_names(&out), ["slides.md", "stagepack.json", "public/photo.png", "public/caf\u{e9}.png"]);
    assert!(warnings.iter().any(|warning| warning.contains("public/cafe\u{301}.png")), "{warnings:?}");
}

#[test]
fn exporting_again_over_an_export_keeps_its_id_and_when_it_was_made() {
    let addon = slides_components::addon::Addon::load(&answering_guest(&export_answer())).unwrap();
    let folder = talk_with_a_video(1024);
    let out = folder.path().join("again.sptx");
    export_slidev(input_for(folder.path()), &addon, &out, now(), ID).unwrap();
    let later = now() + chrono::Duration::hours(2);
    export_slidev(input_for(folder.path()), &addon, &out, later, "11111111-2222-4333-8444-555555555555").unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(&zip_entry(&out, "stagepack.json")).unwrap();
    assert_eq!(manifest["id"], ID);
    assert_eq!(manifest["created"], "2026-11-24T09:30:00Z");
    assert_eq!(manifest["updated"], "2026-11-24T11:30:00Z");
}

#[test]
fn a_module_saying_another_version_than_its_manifest_is_refused() {
    test_key();
    let app = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    // Signed as 0.2.0, where the module says it is 0.1.0.
    let file = package_file(scratch.path(), &answering_guest(&export_answer()), "0.2.0");
    let refused = addons::install_from_file(app.path(), &file).unwrap_err();
    assert!(refused.contains("version 0.1.0") && refused.contains("0.2.0"), "{refused}");
    assert!(!app.path().join("addons/slidev").exists());
}

#[test]
fn remove_isnt_undone_by_what_an_earlier_install_left() {
    test_key();
    let app = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    addons::install_from_file(app.path(), &package_file(scratch.path(), &answering_guest(&export_answer()), "0.1.0")).unwrap();
    // As if an earlier install's old folder couldn't be deleted.
    let left = app.path().join("addons/.slidev-old-0123");
    std::fs::create_dir(&left).unwrap();
    for name in ["addon.json", "addon.wasm", "addon.sig"] {
        std::fs::copy(app.path().join("addons/slidev").join(name), left.join(name)).unwrap();
    }
    addons::remove(app.path()).unwrap();
    assert_eq!(addons::status(app.path()), addons::Status::NotInstalled);
    assert!(addons::load(app.path()).is_none());
    assert!(!left.exists());
}

#[test]
fn remove_doesnt_wait_for_an_install_going_on() {
    test_key();
    let app = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    addons::install_from_file(app.path(), &package_file(scratch.path(), &answering_guest(&export_answer()), "0.1.0")).unwrap();
    // The lock an install holds, held here.
    let lock = std::fs::OpenOptions::new().write(true).open(app.path().join("addons/.lock")).unwrap();
    lock.lock().unwrap();
    assert_eq!(addons::remove(app.path()).unwrap_err(), addons::BUSY);
    assert!(app.path().join("addons/slidev/addon.wasm").is_file());
    lock.unlock().unwrap();
    addons::remove(app.path()).unwrap();
}

#[cfg(unix)]
#[test]
fn an_add_ons_folder_that_is_a_link_is_not_read_through() {
    test_key();
    let app = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    // A real install, elsewhere, which the app's folder only links to.
    addons::install_from_file(elsewhere.path(), &package_file(scratch.path(), &answering_guest(&export_answer()), "0.1.0")).unwrap();
    std::os::unix::fs::symlink(elsewhere.path().join("addons"), app.path().join("addons")).unwrap();
    assert!(matches!(addons::status(app.path()), addons::Status::Damaged(sentence) if sentence.contains("link")));
    assert!(matches!(addons::load(app.path()), Some(addons::Installed { state: InstalledState::Damaged(sentence), .. }) if sentence.contains("link")));
}

#[test]
fn parts_that_change_on_clicks_past_the_budget_are_one_picture_with_a_notice() {
    let mut registry = slides_engine::builtin_registry();
    registry.add(probes::Tally_component());
    let source = "# Changing\n\n<Tally x=\"100\" y=\"300\" w=\"300\" h=\"200\" />\n";
    let mut deck = Deck::with_registry(Box::new(FolderFiles::new(fixture())), Fonts::bundled_only(), source.to_string(), registry);
    let (baked, notices) = addons::bake::bake_within(&mut deck, u64::MAX, 3);
    let mut names: Vec<&str> = baked.iter().map(|(name, _)| name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["baked/1/0.png", "baked/index.json"]);
    let (_, index) = baked.iter().find(|(name, _)| name == "baked/index.json").unwrap();
    let index: Vec<serde_json::Value> = serde_json::from_slice(index).unwrap();
    assert_eq!(index[0]["steps"], serde_json::json!([]));
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!((notices[0].slide, notices[0].kind), (Some(1), NoticeKind::Frozen));
    assert!(notices[0].text.starts_with("The Tally changes on clicks"), "{notices:?}");
}

#[test]
fn an_export_the_add_on_fails_says_so_in_one_sentence() {
    let addon = slides_components::addon::Addon::load(&answering_guest(&vec![file(addon_abi::ERROR, b"The talk has no slides.md to export.")])).unwrap();
    let folder = talk_with_a_video(1024);
    let out = folder.path().join("failed.sptx");
    let refused = export_slidev(input_for(folder.path()), &addon, &out, now(), ID).unwrap_err();
    let start = format!("Could not export {}: the add-on couldn't finish: ", out.display());
    assert!(refused.starts_with(&start), "{refused}");
    assert!(refused.ends_with('.') && !refused.ends_with(".."), "{refused}");
    assert_eq!(refused.matches(". ").count(), 0, "one sentence: {refused}");
}

#[test]
fn an_addon_in_use_makes_an_update_or_removal_say_busy_and_an_install_wait() {
    test_key();
    let app = tempfile::tempdir().unwrap();
    let package = |version: &str| package_signed(&answering_guest_at(&export_answer(), version), version, test_key());
    addons::install_from_bytes(app.path(), &package("0.1.0")).unwrap();

    // As an export or an import holds it, loaded through the same lock.
    let (in_use, loaded) = addons::load_in_use(app.path());
    assert!(matches!(loaded, Some(addons::Installed { state: InstalledState::Ready(_), .. })));
    assert!(addons::is_busy(app.path()));
    assert_eq!(addons::update_from_bytes(app.path(), &package("0.2.0")).err().as_deref(), Some(addons::BUSY));
    assert_eq!(addons::remove(app.path()).err().as_deref(), Some(addons::BUSY));
    let waiting = {
        let (app, package) = (app.path().to_path_buf(), package("0.3.0"));
        std::thread::spawn(move || addons::install_from_bytes(&app, &package).map(|installed| installed.version))
    };
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(!waiting.is_finished(), "an install from a file waits for it");
    assert_eq!(ready_version(app.path()).as_deref(), Some("0.1.0"));
    drop(in_use);
    assert_eq!(waiting.join().unwrap().as_deref(), Ok("0.3.0"));
    assert!(!addons::is_busy(app.path()));
}

#[test]
fn an_update_only_ever_goes_forward() {
    test_key();
    let app = tempfile::tempdir().unwrap();
    let package = |version: &str| package_signed(&answering_guest_at(&export_answer(), version), version, test_key());
    addons::install_from_bytes(app.path(), &package("0.2.0")).unwrap();
    for older in ["0.2.0", "0.1.0"] {
        let refused = addons::update_from_bytes(app.path(), &package(older)).unwrap_err();
        assert!(refused.contains("0.2.0 is installed already") && !refused.contains('\n'), "{refused}");
        assert_eq!(ready_version(app.path()).as_deref(), Some("0.2.0"));
    }
    assert_eq!(addons::update_from_bytes(app.path(), &package("0.3.0")).unwrap().version, "0.3.0");
    // A file is the owner's own choice, and may go back.
    assert_eq!(addons::install_from_bytes(app.path(), &package("0.1.0")).unwrap().version, "0.1.0");
    // And with nothing installed, an update installs.
    let fresh = tempfile::tempdir().unwrap();
    assert_eq!(addons::update_from_bytes(fresh.path(), &package("0.1.0")).unwrap().version, "0.1.0");
}

#[test]
fn an_install_cut_short_is_put_back_before_an_export_holds_it() {
    test_key();
    let app = tempfile::tempdir().unwrap();
    addons::install_from_bytes(app.path(), &package_signed(&answering_guest_at(&export_answer(), "0.1.0"), "0.1.0", test_key())).unwrap();
    // As an install that stopped after moving the earlier add-on aside.
    let addons_folder = app.path().join("addons");
    std::fs::rename(addons_folder.join("slidev"), addons_folder.join(".slidev-old-0123")).unwrap();
    let (_in_use, loaded) = addons::load_in_use(app.path());
    assert!(matches!(loaded, Some(addons::Installed { state: InstalledState::Ready(_), .. })));
    assert!(addons_folder.join("slidev/addon.wasm").is_file());
}

#[test]
fn a_version_installed_that_isnt_three_numbers_can_be_updated() {
    test_key();
    let app = tempfile::tempdir().unwrap();
    let package = |version: &str| package_signed(&answering_guest_at(&export_answer(), version), version, test_key());
    // As from a file: a version the app can't compare.
    addons::install_from_bytes(app.path(), &package("1.0.0-rc1")).unwrap();
    assert_eq!(addons::update_from_bytes(app.path(), &package("0.2.0")).unwrap().version, "0.2.0");
    // An update to one that isn't three numbers is never taken as later.
    let refused = addons::update_from_bytes(app.path(), &package("0.3.0-rc1")).unwrap_err();
    assert!(refused.contains("0.2.0 is installed already"), "{refused}");
}

#[test]
fn is_busy_never_makes_a_lock_file() {
    let app = tempfile::tempdir().unwrap();
    assert!(!addons::is_busy(app.path()));
    std::fs::create_dir_all(app.path().join("addons")).unwrap();
    assert!(!addons::is_busy(app.path()));
    assert!(!app.path().join("addons/.lock").exists());
}

#[cfg(unix)]
#[test]
fn is_busy_makes_nothing_behind_a_linked_add_ons_folder() {
    let app = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), app.path().join("addons")).unwrap();
    assert!(!addons::is_busy(app.path()));
    assert_eq!(std::fs::read_dir(elsewhere.path()).unwrap().count(), 0);
}
