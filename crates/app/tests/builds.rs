use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use slides_components::{BuildIndex, Bundle, Diagnostic, INDEX_FILE, component_tag, source_hash};
use slides_engine::{ComponentState, FolderFiles, Fonts, TalkComponents};
use stagepack_studio::builds::{BuildOutcome, Compile, behind, build_components};
use stagepack_studio::cargo_file::write_cargo_file;
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
fn a_source_that_cannot_be_read_says_where_when_built() {
    let dir = talk(&[("score.rs", &source("Score"))]);
    let (compile, calls) = fake_compiler();
    build_components(dir.path(), &compile);
    let unclosed = "use toolkit::prelude::*;\n\n#[component]\nfn Score() -> Element {\n    text(\"Score)\n}\n";
    std::fs::write(dir.path().join("components/score.rs"), unclosed).unwrap();
    assert_eq!(states(dir.path()), [("Score".to_string(), ComponentState::OutOfDate)]);

    let outcome = build_components(dir.path(), &compile);
    assert!(outcome.built.is_empty());
    assert_eq!(outcome.problems.len(), 1, "{:?}", outcome.problems);
    assert_eq!(outcome.problems[0].file, "components/score.rs");
    assert!(outcome.problems[0].line > 0, "{:?}", outcome.problems);
    assert!(outcome.problems[0].message.contains("unclosed quote"), "{:?}", outcome.problems);
    assert_eq!(calls.load(Ordering::SeqCst), 1, "it isn't given to the compiler");
    assert_eq!(states(dir.path()), [("Score".to_string(), ComponentState::OutOfDate)]);
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

/// Every file and folder in `folder` and below, relative to it.
fn everything_in(folder: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(folder).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.file_type().unwrap().is_dir() {
            found.extend(everything_in(&entry.path()).into_iter().map(|below| format!("{name}/{below}")));
        }
        found.push(name);
    }
    found.sort();
    found
}

/// The project written for the talk in `talk`, with projects kept in `projects`.
fn set_up(talk: &Path, projects: &Path) -> (std::path::PathBuf, toml::Table) {
    let (_bundle_dir, bundle) = bundle_with_toolkit_source();
    let project = write_cargo_file(talk, &bundle, projects).unwrap();
    let text = std::fs::read_to_string(project.join("Cargo.toml")).unwrap();
    (project, text.parse::<toml::Table>().unwrap_or_else(|error| panic!("{error}\n{text}")))
}

/// The name and path of each `[[example]]`.
fn examples(cargo: &toml::Table) -> Vec<(String, String)> {
    cargo
        .get("example")
        .and_then(|examples| examples.as_array())
        .map(|examples| {
            examples
                .iter()
                .map(|example| (example["name"].as_str().unwrap().to_string(), example["path"].as_str().unwrap().to_string()))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn the_cargo_file_lists_each_component_by_its_path() {
    let dir = talk(&[
        ("score.rs", &source("Score")),
        ("clock.rs", &source("Clock")),
        ("bad name.rs", &source("Bad")),
        ("chart.rs", &source("Chart")),
    ]);
    let projects = tempfile::tempdir().unwrap();
    let (project, cargo) = set_up(dir.path(), projects.path());

    assert!(project.starts_with(projects.path().canonicalize().unwrap()), "{project:?}");
    assert!(cargo.contains_key("workspace"));
    let talk = dir.path().canonicalize().unwrap();
    assert_eq!(
        examples(&cargo),
        [
            ("clock".to_string(), talk.join("components/clock.rs").to_str().unwrap().to_string()),
            ("score".to_string(), talk.join("components/score.rs").to_str().unwrap().to_string()),
        ]
    );
    let package = cargo["package"].as_table().unwrap();
    for key in ["build", "autolib", "autobins", "autoexamples", "autotests", "autobenches"] {
        assert_eq!(package[key].as_bool(), Some(false), "{key}");
    }
}

#[test]
fn setting_up_writes_nothing_in_the_talk() {
    let dir = talk(&[("score.rs", &source("Score"))]);
    let before = everything_in(dir.path());
    let projects = tempfile::tempdir().unwrap();
    set_up(dir.path(), projects.path());
    assert_eq!(everything_in(dir.path()), before);
}

#[test]
fn the_cargo_file_is_written_again_with_a_new_component() {
    let dir = talk(&[("score.rs", &source("Score"))]);
    let projects = tempfile::tempdir().unwrap();
    let (first, cargo) = set_up(dir.path(), projects.path());
    assert_eq!(examples(&cargo).len(), 1);
    std::fs::write(dir.path().join("components/clock.rs"), source("Clock")).unwrap();
    let (again, cargo) = set_up(dir.path(), projects.path());
    assert_eq!(again, first);
    let names: Vec<String> = examples(&cargo).into_iter().map(|(name, _)| name).collect();
    assert_eq!(names, ["clock", "score"]);
}

#[test]
fn two_talks_of_the_same_name_have_projects_of_their_own() {
    let (one, two) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let projects = tempfile::tempdir().unwrap();
    let mut made = Vec::new();
    for parent in [&one, &two] {
        let folder = parent.path().join("talk");
        std::fs::create_dir_all(folder.join("components")).unwrap();
        std::fs::write(folder.join("components/score.rs"), source("Score")).unwrap();
        made.push(set_up(&folder, projects.path()).0);
    }
    assert_ne!(made[0], made[1]);
    for project in &made {
        assert!(project.file_name().unwrap().to_str().unwrap().starts_with("talk-"), "{project:?}");
    }
}

#[test]
fn a_talk_path_with_a_quote_or_a_backslash_gives_good_toml() {
    let outer = tempfile::tempdir().unwrap();
    let name = if cfg!(windows) { "the \"best\" talk" } else { "the \"best\" \\ talk" };
    let folder = outer.path().join(name);
    std::fs::create_dir_all(folder.join("components")).unwrap();
    std::fs::write(folder.join("components/score.rs"), source("Score")).unwrap();
    let projects = tempfile::tempdir().unwrap();
    let (_, cargo) = set_up(&folder, projects.path());
    let path = folder.canonicalize().unwrap().join("components/score.rs");
    assert_eq!(examples(&cargo), [("score".to_string(), path.to_str().unwrap().to_string())]);
}

#[test]
fn a_projects_folder_inside_the_talk_is_refused() {
    let dir = talk(&[("score.rs", &source("Score"))]);
    let (_bundle_dir, bundle) = bundle_with_toolkit_source();
    let refused = write_cargo_file(dir.path(), &bundle, &dir.path().join("projects")).unwrap_err();
    assert!(refused.contains("inside the talk"), "{refused}");
    assert!(!dir.path().join("projects").join("Cargo.toml").exists());
}

#[test]
fn a_bundle_without_the_toolkit_source_writes_nothing() {
    let dir = talk(&[("score.rs", &source("Score"))]);
    let bundle = Bundle {
        root: tempfile::tempdir().unwrap().path().to_path_buf(),
    };
    let projects = tempfile::tempdir().unwrap();
    assert!(write_cargo_file(dir.path(), &bundle, projects.path()).is_err());
    assert!(everything_in(projects.path()).iter().all(|name| !name.ends_with("Cargo.toml")));
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
    // What a talk could carry to make Cargo or rustup run its own code: none takes effect,
    // as the project isn't in the talk.
    let marker = dir.path().join("talk-code-ran");
    let script = format!("fn main() {{ std::fs::write({marker:?}, \"build.rs\").unwrap(); }}\n");
    std::fs::write(folder.join("build.rs"), script).unwrap();
    let wrapper = folder.join("wrapper.sh");
    std::fs::write(&wrapper, format!("#!/bin/sh\necho wrapper > {}\nexec \"$@\"\n", marker.display())).unwrap();
    #[cfg(unix)]
    std::fs::set_permissions(&wrapper, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    std::fs::create_dir(folder.join(".cargo")).unwrap();
    std::fs::write(folder.join(".cargo/config.toml"), format!("[build]\nrustc-wrapper = {:?}\n", wrapper.to_str().unwrap())).unwrap();
    std::fs::write(folder.join("rust-toolchain.toml"), "[toolchain]\nchannel = \"nightly-2001-01-01\"\n").unwrap();
    let before = everything_in(&folder);

    let project = write_cargo_file(&folder, &bundle, &dir.path().join("projects")).unwrap();

    let cargo_safe = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/cargo-safe.sh");
    let checked = std::process::Command::new(cargo_safe)
        .current_dir(&project)
        .args(["check", "--examples", "--offline", "--quiet"])
        .arg("--target-dir")
        .arg(dir.path().join("target"))
        // What the cargo running this test tells its tests, which isn't for this one.
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_TARGET")
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("RUSTUP_TOOLCHAIN")
        .env_remove("CARGO_BUILD_RUSTC_WRAPPER")
        .env_remove("RUSTC_WRAPPER")
        .output()
        .unwrap();

    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    assert!(!marker.exists(), "the talk's {} ran", std::fs::read_to_string(&marker).unwrap_or_default());
    assert_eq!(everything_in(&folder), before, "cargo wrote nothing in the talk");
}
