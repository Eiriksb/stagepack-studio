use slides_components::{compile, Bundle};

const FILE: &str = "components/score.rs";

fn bundle() -> Bundle {
    Bundle::find().expect("STUDIO_BUNDLE should name a bundle made by scripts/make-bundle.sh")
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

#[test]
#[ignore = "needs the compiler bundle"]
fn a_component_compiles_to_webassembly() {
    let source = "use toolkit::prelude::*;\n\n#[component]\nfn Score(team: String) -> Element {\n    text(team)\n}\n";
    let wasm = compile(&bundle(), FILE, source).unwrap();
    assert_eq!(&wasm[..4], b"\0asm");
}

#[test]
#[ignore = "needs the compiler bundle"]
fn an_error_names_the_file_and_line() {
    let source = "use toolkit::prelude::*;\n#[component]\nfn Score(team: String) -> Element {\n    let count: u32 = team;\n    text(\"a\")\n}\n";
    let errors = compile(&bundle(), FILE, source).unwrap_err();
    assert_eq!(errors[0].file, FILE, "{errors:?}");
    assert_eq!(errors[0].line, 4, "{errors:?}");
    assert!(errors[0].message.contains("mismatched types"), "{errors:?}");
}

// A source that can panic, and that asks for its own name, is the one that would carry a path out.
#[test]
#[ignore = "needs the compiler bundle"]
fn nothing_of_this_computer_is_in_the_build() {
    let source = "use toolkit::prelude::*;\n#[component]\nfn Score(team: String) -> Element {\n    let number: u32 = team.parse().unwrap();\n    if number > 3 { panic!(\"too many\"); }\n    text(file!())\n}\n";
    let wasm = compile(&bundle(), FILE, source).unwrap();
    let temp = std::env::temp_dir();
    let temp = temp.to_str().unwrap().trim_end_matches(['/', '\\']);
    assert!(!contains(&wasm, temp.as_bytes()), "the build holds {temp}");
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap();
    assert!(!contains(&wasm, home.as_bytes()), "the build holds {home}");
}

#[cfg(unix)]
#[test]
fn a_compile_that_never_ends_is_stopped() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join("bin")).unwrap();
    let rustc = folder.path().join("bin/rustc");
    std::fs::write(&rustc, "#!/bin/sh\nsleep 60\n").unwrap();
    std::fs::set_permissions(&rustc, std::fs::Permissions::from_mode(0o755)).unwrap();
    let bundle = Bundle { root: folder.path().to_path_buf() };

    let source = "#[component]\nfn Score() -> Element { text(\"a\") }\n";
    let started = Instant::now();
    let errors = slides_components::compile_with_limit(&bundle, FILE, source, Duration::from_millis(200)).unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(5), "took {:?}", started.elapsed());
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].file, FILE);
    assert!(errors[0].message.contains("seconds"), "{errors:?}");
}
