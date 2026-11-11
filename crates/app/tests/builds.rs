use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use slides_components::{BuildIndex, Bundle, Diagnostic, INDEX_FILE, component_tag, source_hash};
use slides_engine::{ComponentState, FolderFiles, Fonts, TalkComponents};
use stagepack_studio::builds::{BuildOutcome, Compile, behind, build_components};
use stagepack_studio::cargo_file::{write_cargo_file, write_cargo_file_with_cargo_home};
use stagepack_studio::document::{Document, TalkPlace};
use stagepack_studio::permissions::BuildPermissions;
use tempfile::TempDir;

/// The source of a component called `tag`.
fn source(tag: &str) -> String {
    format!(
        "use toolkit::prelude::*;\n\n#[component]\nfn {tag}() -> Element {{\n    text(\"{tag}\")\n}}\n"
    )
}

/// A source the fake compiler refuses, at line 12.
fn broken(tag: &str) -> String {
    format!("{}// broken\n", source(tag))
}

/// A module that calls itself `tag`, and renders a line of text.
fn module(tag: &str) -> Vec<u8> {
    let schema = format!(r#"{{"name":"{tag}","props":[],"children":false}}"#);
    let answer = r#"{"element":{"Text":{"text":"built","size":null,"bold":false,"color":null}},"animated":false}"#;
    let packed = |at: u64, text: &str| ((at << 32) | text.len() as u64) as i64;
    let data = |at: u32, text: &str| {
        let escaped: String = text.bytes().map(|byte| format!("\\{byte:02x}")).collect();
        format!(r#"(data (i32.const {at}) "{escaped}")"#)
    };
    let text = format!(
        r#"(module
  (memory (export "memory") 8)
  (global $heap (mut i32) (i32.const 262144))
  {}
  {}
  (func (export "abi_version") (result i32) (i32.const 1))
  (func (export "alloc") (param $len i32) (result i32)
    (local $at i32)
    (local.set $at (global.get $heap))
    (global.set $heap (i32.add (global.get $heap) (local.get $len)))
    (local.get $at))
  (func (export "schema") (result i64) (i64.const {}))
  (func (export "render") (param $ptr i32) (param $len i32) (result i64) (i64.const {})))"#,
        data(1024, &schema),
        data(8192, answer),
        packed(1024, &schema),
        packed(8192, answer),
    );
    wat::parse_str(&text).unwrap_or_else(|error| panic!("{error}\n{text}"))
}

/// A compiler that counts its calls, refuses a source marked broken, and otherwise gives a
/// module named for the source's component.
fn fake_compiler() -> (Compile, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let compile: Compile = Arc::new(move |file: &str, source: &str| {
        counted.fetch_add(1, Ordering::SeqCst);
        if source.contains("// broken") {
            return Err(vec![Diagnostic {
                file: file.to_string(),
                line: 12,
                message: "expected `;`".to_string(),
            }]);
        }
        Ok(module(&component_tag(source).expect("a component")))
    });
    (compile, calls)
}

/// A talk folder holding `sources`, as file names in `components` and their text.
fn talk(sources: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), "# One\n").unwrap();
    std::fs::create_dir(dir.path().join("components")).unwrap();
    for (name, text) in sources {
        std::fs::write(dir.path().join("components").join(name), text).unwrap();
    }
    dir
}

fn index(folder: &Path) -> BuildIndex {
    serde_json::from_slice(&std::fs::read(folder.join(INDEX_FILE)).unwrap()).unwrap()
}

fn states(folder: &Path) -> Vec<(String, ComponentState)> {
    let scan = TalkComponents::scan(Rc::new(FolderFiles::new(folder.to_path_buf())));
    scan.infos()
        .iter()
        .map(|info| (info.tag.clone(), info.state))
        .collect()
}

fn files_in(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn building_writes_the_build_and_its_index() {
    let score = source("Score");
    let dir = talk(&[("score.rs", &score)]);
    let (compile, calls) = fake_compiler();

    let BuildOutcome { built, problems } = build_components(dir.path(), &compile);

    assert_eq!(built, ["Score"]);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        std::fs::read(dir.path().join("build/score.wasm")).unwrap(),
        module("Score")
    );
    let entries = index(dir.path()).components;
    assert_eq!(entries.len(), 1);
    assert_eq!(
        (entries[0].tag.as_str(), entries[0].file.as_str()),
        ("Score", "score")
    );
    assert_eq!(entries[0].source_hash, source_hash(score.as_bytes()));
    assert_eq!(
        states(dir.path()),
        [("Score".to_string(), ComponentState::UpToDate)]
    );
}

#[test]
fn an_up_to_date_component_is_not_built_again() {
    let dir = talk(&[("score.rs", &source("Score"))]);
    let (compile, calls) = fake_compiler();
    build_components(dir.path(), &compile);

    let again = build_components(dir.path(), &compile);

    assert!(again.built.is_empty());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn other_entries_of_the_index_are_kept() {
    let dir = talk(&[("score.rs", &source("Score"))]);
    std::fs::create_dir(dir.path().join("build")).unwrap();
    std::fs::write(dir.path().join("build/clock.wasm"), module("Clock")).unwrap();
    let kept = r#"{"format":1,"components":[{"tag":"Clock","file":"clock","source_hash":"built elsewhere"}]}"#;
    std::fs::write(dir.path().join(INDEX_FILE), kept).unwrap();
    let (compile, _) = fake_compiler();

    build_components(dir.path(), &compile);

    let tags: Vec<String> = index(dir.path())
        .components
        .into_iter()
        .map(|entry| entry.tag)
        .collect();
    assert_eq!(tags, ["Clock", "Score"]);
}

#[test]
fn a_failed_build_keeps_the_last_good_one() {
    let dir = talk(&[("score.rs", &source("Score"))]);
    let (compile, _) = fake_compiler();
    build_components(dir.path(), &compile);
    let wasm = std::fs::read(dir.path().join("build/score.wasm")).unwrap();
    let index_bytes = std::fs::read(dir.path().join(INDEX_FILE)).unwrap();
    let build_files = files_in(&dir.path().join("build"));

    std::fs::write(dir.path().join("components/score.rs"), broken("Score")).unwrap();
    let outcome = build_components(dir.path(), &compile);

    assert!(outcome.built.is_empty());
    assert_eq!(
        outcome.problems,
        [Diagnostic {
            file: "components/score.rs".to_string(),
            line: 12,
            message: "expected `;`".to_string()
        }]
    );
    assert_eq!(
        std::fs::read(dir.path().join("build/score.wasm")).unwrap(),
        wasm
    );
    assert_eq!(
        std::fs::read(dir.path().join(INDEX_FILE)).unwrap(),
        index_bytes
    );
    assert_eq!(files_in(&dir.path().join("build")), build_files);
    assert_eq!(
        states(dir.path()),
        [("Score".to_string(), ComponentState::OutOfDate)]
    );
}

#[test]
fn one_failure_does_not_stop_the_others() {
    let dir = talk(&[("a.rs", &broken("First")), ("b.rs", &source("Second"))]);
    let (compile, calls) = fake_compiler();

    let outcome = build_components(dir.path(), &compile);

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(outcome.built, ["Second"]);
    assert_eq!(outcome.problems.len(), 1);
    assert_eq!(outcome.problems[0].file, "components/a.rs");
    assert!(dir.path().join("build/b.wasm").is_file());
    assert!(!dir.path().join("build/a.wasm").exists());
}

#[test]
fn a_file_with_an_unsafe_name_is_not_built() {
    let dir = talk(&[
        ("bad name.rs", &source("Bad")),
        ("dotted.name.rs", &source("Dotted")),
    ]);
    let (compile, calls) = fake_compiler();

    let outcome = build_components(dir.path(), &compile);

    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(outcome.built.is_empty());
    assert!(!dir.path().join("build").exists());
}

#[test]
fn a_component_named_like_a_built_in_is_not_built() {
    let dir = talk(&[("chart.rs", &source("Chart"))]);
    let (compile, calls) = fake_compiler();

    build_components(dir.path(), &compile);

    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(!dir.path().join("build").exists());
}

#[test]
fn a_source_over_the_limit_is_not_built() {
    let mut large = source("Large");
    large.push_str(&"// padding\n".repeat(slides_engine::MAX_SOURCE_BYTES as usize / 10));
    let dir = talk(&[("large.rs", &large)]);
    let (compile, calls) = fake_compiler();

    build_components(dir.path(), &compile);

    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn a_source_in_a_folder_below_components_is_not_built() {
    let dir = talk(&[]);
    std::fs::create_dir(dir.path().join("components/deeper")).unwrap();
    std::fs::write(
        dir.path().join("components/deeper/inner.rs"),
        source("Inner"),
    )
    .unwrap();
    let (compile, calls) = fake_compiler();

    build_components(dir.path(), &compile);

    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[cfg(unix)]
#[test]
fn a_linked_source_is_not_built() {
    let elsewhere = tempfile::tempdir().unwrap();
    std::fs::write(elsewhere.path().join("outside.rs"), source("Outside")).unwrap();
    let dir = talk(&[]);
    std::os::unix::fs::symlink(
        elsewhere.path().join("outside.rs"),
        dir.path().join("components/outside.rs"),
    )
    .unwrap();
    let (compile, calls) = fake_compiler();

    build_components(dir.path(), &compile);

    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[cfg(unix)]
#[test]
fn a_linked_build_folder_is_not_written_through() {
    let elsewhere = tempfile::tempdir().unwrap();
    let dir = talk(&[("score.rs", &source("Score"))]);
    std::os::unix::fs::symlink(elsewhere.path(), dir.path().join("build")).unwrap();
    let (compile, _) = fake_compiler();

    let outcome = build_components(dir.path(), &compile);

    assert!(outcome.built.is_empty());
    assert!(!outcome.problems.is_empty());
    assert!(files_in(elsewhere.path()).is_empty());
}

#[cfg(unix)]
#[test]
fn a_linked_build_file_is_not_written_through() {
    let elsewhere = tempfile::tempdir().unwrap();
    let target = elsewhere.path().join("victim");
    std::fs::write(&target, "keep").unwrap();
    for linked in ["score.wasm", "index.json"] {
        let dir = talk(&[("score.rs", &source("Score"))]);
        std::fs::create_dir(dir.path().join("build")).unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join("build").join(linked)).unwrap();
        let (compile, _) = fake_compiler();

        let outcome = build_components(dir.path(), &compile);

        assert!(outcome.built.is_empty(), "{linked}");
        assert!(!outcome.problems.is_empty(), "{linked}");
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "keep",
            "{linked}"
        );
        assert!(
            std::fs::symlink_metadata(dir.path().join("build").join(linked))
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}

#[test]
fn permission_is_kept_outside_the_talk() {
    let dir = talk(&[("score.rs", &source("Score"))]);
    let before = files_in(dir.path());
    let app_data = tempfile::tempdir().unwrap();
    let file = app_data.path().join("studio").join("permissions.json");
    let mut permissions = BuildPermissions::load(file.clone());
    assert!(!permissions.always(dir.path()));

    permissions.set_always(dir.path()).unwrap();

    assert!(permissions.always(dir.path()));
    assert!(
        BuildPermissions::load(file.clone()).always(dir.path()),
        "it was written down"
    );
    assert!(file.is_file());
    assert_eq!(files_in(dir.path()), before);
}

#[test]
fn a_folder_is_known_however_it_is_written() {
    let parent = tempfile::tempdir().unwrap();
    let folder = parent.path().join("talk");
    std::fs::create_dir(&folder).unwrap();
    let app_data = tempfile::tempdir().unwrap();
    let mut permissions = BuildPermissions::load(app_data.path().join("permissions.json"));

    permissions.set_always(&folder).unwrap();

    assert!(permissions.always(&folder.join("..").join("talk")));
    assert!(!permissions.always(parent.path()));
}

#[test]
fn a_broken_permissions_file_counts_as_none() {
    let dir = talk(&[]);
    let app_data = tempfile::tempdir().unwrap();
    let file = app_data.path().join("permissions.json");
    std::fs::write(&file, "{ not quite").unwrap();

    let mut permissions = BuildPermissions::load(file.clone());

    assert!(!permissions.always(dir.path()));
    permissions.set_always(dir.path()).unwrap();
    assert!(BuildPermissions::load(file).always(dir.path()));
}

#[test]
fn the_app_keeps_its_permissions_in_its_data_folder() {
    if let Some(file) = BuildPermissions::default_file() {
        assert!(
            file.starts_with(dirs::data_dir().unwrap()),
            "{}",
            file.display()
        );
    }
}

#[test]
fn a_packed_copy_says_which_builds_are_behind() {
    let dir = talk(&[
        ("score.rs", &source("Score")),
        ("clock.rs", &source("Clock")),
    ]);
    let (compile, _) = fake_compiler();
    build_components(dir.path(), &compile);
    std::fs::write(dir.path().join("components/score.rs"), broken("Score")).unwrap();
    build_components(dir.path(), &compile);

    assert_eq!(behind(dir.path()), ["Score"]);
    let elsewhere = tempfile::tempdir().unwrap();
    let out = elsewhere.path().join("talk.sptx");
    let notices = TalkPlace::Folder(dir.path().to_path_buf())
        .save_packed(&out)
        .unwrap();
    assert_eq!(
        notices,
        ["Score couldn't be built just now, so the packed talk carries its last good build"]
    );
}

/// A bundle holding only the toolkit's source, which is all a Cargo file needs.
fn bundle_with_toolkit_source() -> (TempDir, Bundle) {
    let dir = tempfile::tempdir().unwrap();
    let toolkit = dir.path().join("toolkit-src").join("toolkit");
    std::fs::create_dir_all(&toolkit).unwrap();
    std::fs::write(
        toolkit.join("Cargo.toml"),
        "[package]\nname = \"toolkit\"\n",
    )
    .unwrap();
    let bundle = Bundle {
        root: dir.path().to_path_buf(),
    };
    (dir, bundle)
}

/// How a path is written in a TOML string.
fn toml_path(path: &Path) -> String {
    path.to_str().unwrap().replace('\\', "\\\\")
}

#[test]
fn the_cargo_file_lists_each_component() {
    let dir = talk(&[
        ("score.rs", &source("Score")),
        ("clock.rs", &source("Clock")),
        ("bad name.rs", &source("Bad")),
        ("chart.rs", &source("Chart")),
    ]);
    let (_bundle_dir, bundle) = bundle_with_toolkit_source();

    let written = write_cargo_file(dir.path(), &bundle).unwrap();

    assert_eq!(written, dir.path().join("Cargo.toml"));
    let text = std::fs::read_to_string(&written).unwrap();
    let toolkit = bundle.root.join("toolkit-src").join("toolkit");
    assert!(text.contains("\n[workspace]\n"), "{text}");
    assert!(
        text.contains(&format!(
            "toolkit = {{ path = \"{}\" }}",
            toml_path(&toolkit)
        )),
        "{text}"
    );
    let examples: Vec<&str> = text.split("[[example]]").skip(1).map(str::trim).collect();
    assert_eq!(
        examples,
        [
            "name = \"clock\"\npath = \"components/clock.rs\"\ncrate-type = [\"cdylib\"]",
            "name = \"score\"\npath = \"components/score.rs\"\ncrate-type = [\"cdylib\"]",
        ],
        "{text}"
    );
}

#[test]
fn an_existing_cargo_file_is_left_alone() {
    let dir = talk(&[("score.rs", &source("Score"))]);
    std::fs::write(dir.path().join("Cargo.toml"), "# mine\n").unwrap();
    let (_bundle_dir, bundle) = bundle_with_toolkit_source();

    let refused = write_cargo_file(dir.path(), &bundle).unwrap_err();

    assert!(refused.contains("Cargo.toml"), "{refused}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("Cargo.toml")).unwrap(),
        "# mine\n"
    );
}

/// Refuses with the talk in `dir` holding `name`, and writes nothing.
fn assert_refused_for(dir: &TempDir, name: &str) {
    let (_bundle_dir, bundle) = bundle_with_toolkit_source();
    let refused = write_cargo_file(dir.path(), &bundle).unwrap_err();
    assert!(refused.contains(name), "{name}: {refused}");
    assert!(refused.contains("code editor"), "{name}: {refused}");
    assert!(!dir.path().join("Cargo.toml").exists(), "{name}");
}

#[test]
fn a_talk_with_a_build_script_is_not_set_up() {
    let dir = talk(&[("score.rs", &source("Score"))]);
    std::fs::write(dir.path().join("build.rs"), "fn main() {}\n").unwrap();
    assert_refused_for(&dir, "build.rs");
}

#[test]
fn a_talk_with_cargo_settings_is_not_set_up() {
    let dir = talk(&[("score.rs", &source("Score"))]);
    std::fs::create_dir(dir.path().join(".cargo")).unwrap();
    assert_refused_for(&dir, ".cargo");
}

#[test]
fn a_talk_that_picks_a_toolchain_is_not_set_up() {
    for name in ["rust-toolchain", "rust-toolchain.toml"] {
        let dir = talk(&[("score.rs", &source("Score"))]);
        std::fs::write(dir.path().join(name), "[toolchain]\n").unwrap();
        assert_refused_for(&dir, name);
    }
}

#[test]
fn a_talk_with_a_src_folder_is_not_set_up() {
    let dir = talk(&[("score.rs", &source("Score"))]);
    std::fs::create_dir(dir.path().join("src")).unwrap();
    assert_refused_for(&dir, "src");
}

#[cfg(unix)]
#[test]
fn a_linked_build_script_counts_too() {
    let dir = talk(&[("score.rs", &source("Score"))]);
    std::os::unix::fs::symlink(dir.path().join("nowhere.rs"), dir.path().join("build.rs")).unwrap();
    assert_refused_for(&dir, "build.rs");
}

#[test]
fn a_talk_with_editor_or_cargo_leftovers_is_not_set_up() {
    let folders = [".vscode", ".zed", ".helix", ".idea", ".fleet", "target"];
    let files = ["rust-analyzer.toml", ".rust-analyzer.toml", "Cargo.lock"];
    for name in folders {
        let dir = talk(&[("score.rs", &source("Score"))]);
        std::fs::create_dir(dir.path().join(name)).unwrap();
        assert_refused_for(&dir, name);
    }
    for name in files {
        let dir = talk(&[("score.rs", &source("Score"))]);
        std::fs::write(dir.path().join(name), "").unwrap();
        assert_refused_for(&dir, name);
    }
}

/// A talk in `talk` inside a folder that holds `file`, given as a path below that folder.
fn talk_below(file: &str) -> (TempDir, std::path::PathBuf) {
    let outer = tempfile::tempdir().unwrap();
    let found = outer.path().join(file);
    std::fs::create_dir_all(found.parent().unwrap()).unwrap();
    std::fs::write(&found, "[build]\n").unwrap();
    let folder = outer.path().join("talks").join("talk");
    std::fs::create_dir_all(folder.join("components")).unwrap();
    std::fs::write(folder.join("slides.md"), "# One\n").unwrap();
    std::fs::write(folder.join("components/score.rs"), source("Score")).unwrap();
    (outer, folder)
}

#[test]
fn settings_in_a_folder_above_the_talk_refuse() {
    for file in [
        ".cargo/config.toml",
        ".cargo/config",
        "rust-toolchain",
        "rust-toolchain.toml",
    ] {
        let (_outer, folder) = talk_below(file);
        let (_bundle_dir, bundle) = bundle_with_toolkit_source();

        let refused = write_cargo_file_with_cargo_home(&folder, &bundle, None).unwrap_err();

        let name = Path::new(file).file_name().unwrap().to_str().unwrap();
        assert!(refused.contains(name), "{file}: {refused}");
        assert!(refused.contains("code editor"), "{file}: {refused}");
        assert!(!folder.join("Cargo.toml").exists(), "{file}");
    }
}

#[test]
fn the_cargo_home_above_the_talk_does_not_refuse() {
    let (outer, folder) = talk_below(".cargo/config.toml");
    let (_bundle_dir, bundle) = bundle_with_toolkit_source();
    let cargo_home = outer.path().join(".cargo");

    let written = write_cargo_file_with_cargo_home(&folder, &bundle, Some(&cargo_home));

    assert_eq!(written, Ok(folder.join("Cargo.toml")));
}

#[test]
fn the_cargo_file_finds_nothing_cargo_would_run_or_add() {
    let dir = talk(&[("score.rs", &source("Score"))]);
    let (_bundle_dir, bundle) = bundle_with_toolkit_source();

    let text = std::fs::read_to_string(write_cargo_file(dir.path(), &bundle).unwrap()).unwrap();

    let package = text.split("[workspace]").next().unwrap();
    for line in [
        "build = false",
        "autolib = false",
        "autobins = false",
        "autoexamples = false",
        "autotests = false",
        "autobenches = false",
    ] {
        assert!(
            package.lines().any(|written| written == line),
            "{line}\n{text}"
        );
    }
}

#[test]
fn a_bundle_without_the_toolkit_source_writes_nothing() {
    let dir = talk(&[("score.rs", &source("Score"))]);
    let bundle = Bundle {
        root: tempfile::tempdir().unwrap().path().to_path_buf(),
    };

    assert!(write_cargo_file(dir.path(), &bundle).is_err());
    assert!(!dir.path().join("Cargo.toml").exists());
}

#[test]
fn nothing_writes_a_cargo_file_unasked() {
    let dir = talk(&[("score.rs", &source("Score"))]);
    let (compile, _) = fake_compiler();

    let document = Document::open(dir.path(), Fonts::with_system_fallback()).unwrap();
    build_components(dir.path(), &compile);
    drop(document);

    assert!(dir.path().join("build/score.wasm").is_file());
    assert!(!dir.path().join("Cargo.toml").exists());
}

/// The example talk that the guide for coders quotes.
fn example_talk() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/components")
}

/// A copy of the folder `from` in `to`, files and folders only.
fn copy_folder(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_folder(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
#[ignore = "needs the compiler bundle"]
fn the_example_talk_needs_building_then_builds_and_shows() {
    assert!(
        !example_talk().join("build").exists(),
        "the example talk is kept unbuilt"
    );
    let dir = tempfile::tempdir().unwrap();
    copy_folder(&example_talk(), dir.path());
    let unbuilt = states(dir.path());
    assert_eq!(unbuilt.len(), 5, "{unbuilt:?}");
    assert!(
        unbuilt
            .iter()
            .all(|(_, state)| *state == ComponentState::NeedsBuilding),
        "{unbuilt:?}"
    );

    let compile =
        stagepack_studio::builds::bundled_compiler().expect("STUDIO_BUNDLE names a bundle");
    let outcome = build_components(dir.path(), &compile);

    assert!(outcome.problems.is_empty(), "{:?}", outcome.problems);
    assert_eq!(outcome.built.len(), 5);
    let mut document = Document::open(dir.path(), Fonts::with_system_fallback()).unwrap();
    assert!(document.component_problems().is_empty());
    for slide in 0..document.slide_count() {
        assert!(!document.layers(0.25).is_empty(), "slide {}", slide + 1);
        assert_eq!(
            document.notices(),
            Vec::<String>::new(),
            "slide {}",
            slide + 1
        );
        document.next();
    }
}

#[cfg(unix)]
#[test]
#[ignore = "needs the compiler bundle"]
fn the_cargo_file_works_with_cargo() {
    let bundle = Bundle::find().expect("STUDIO_BUNDLE names a bundle");
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("talk");
    copy_folder(&example_talk(), &folder);
    write_cargo_file(&folder, &bundle).unwrap();
    // A build script that arrives later, as from a `git pull`, must not be run.
    let marker = dir.path().join("build-script-ran");
    let script = format!("fn main() {{ std::fs::write({marker:?}, \"ran\").unwrap(); }}\n");
    std::fs::write(folder.join("build.rs"), script).unwrap();

    let cargo_safe = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/cargo-safe.sh");
    let checked = std::process::Command::new(cargo_safe)
        .current_dir(&folder)
        .args(["check", "--examples", "--offline", "--quiet"])
        .arg("--target-dir")
        .arg(dir.path().join("target"))
        // What the cargo running this test tells its tests, which isn't for this one.
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET")
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .output()
        .unwrap();

    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    assert!(!marker.exists(), "cargo ran the talk's build script");
}
