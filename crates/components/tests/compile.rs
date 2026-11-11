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

/// A bundle whose `rustc` is `script`, run by `/bin/sh`.
#[cfg(unix)]
fn fake_compiler(script: &str) -> (tempfile::TempDir, Bundle) {
    use std::os::unix::fs::PermissionsExt;
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join("bin")).unwrap();
    let rustc = folder.path().join("bin/rustc");
    std::fs::write(&rustc, format!("#!/bin/sh\n{script}\n")).unwrap();
    std::fs::set_permissions(&rustc, std::fs::Permissions::from_mode(0o755)).unwrap();
    let bundle = Bundle { root: folder.path().to_path_buf() };
    (folder, bundle)
}

#[cfg(unix)]
#[test]
fn a_compile_that_fills_the_disk_is_stopped() {
    use std::time::{Duration, Instant};
    // Writes 1 GiB, slowly enough to be caught along the way, and then waits.
    let (_folder, bundle) = fake_compiler("/usr/bin/dd if=/dev/zero of=big bs=1M count=1024 2>/dev/null; sleep 60");
    let source = "#[component]\nfn Score() -> Element { text(\"a\") }\n";
    let started = Instant::now();
    let errors = compile(&bundle, FILE, source).unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(20), "took {:?}", started.elapsed());
    assert_eq!(errors[0].message, "Building components/score.rs used too much disk space", "{errors:?}");
}

#[cfg(unix)]
#[test]
fn an_output_too_large_to_run_is_not_read() {
    let (_folder, bundle) = fake_compiler("/usr/bin/dd if=/dev/zero of=component.wasm bs=1M count=5 2>/dev/null");
    let source = "#[component]\nfn Score() -> Element { text(\"a\") }\n";
    let errors = compile(&bundle, FILE, source).unwrap_err();
    assert!(errors[0].message.contains("over the limit of 4 MiB"), "{errors:?}");
}

#[cfg(unix)]
#[test]
fn stopping_a_compile_stops_what_it_started() {
    use std::time::Duration;
    // A child that outlives its parent unless the whole group is stopped.
    let marker = tempfile::tempdir().unwrap();
    let path = marker.path().join("alive");
    let script = format!("(sleep 2; echo yes > {}) & sleep 60", path.display());
    let (_folder, bundle) = fake_compiler(&script);
    let source = "#[component]\nfn Score() -> Element { text(\"a\") }\n";
    let errors = slides_components::compile_with_limit(&bundle, FILE, source, Duration::from_millis(300)).unwrap_err();
    assert_eq!(errors[0].message, "Building components/score.rs took too long, more than 0.3 seconds", "{errors:?}");
    std::thread::sleep(Duration::from_secs(3));
    assert!(!path.exists(), "the compiler's own child was left running");
}

#[test]
#[ignore = "needs the compiler bundle"]
fn a_source_that_asks_for_gigabytes_is_stopped() {
    use std::time::{Duration, Instant};
    // From the review: this took 10.5 GB and 24 s, and wrote a 1 GB build.
    let source = "use toolkit::prelude::*;\n\nstatic BIG: [u8; 1 << 30] = [7; 1 << 30];\n\n#[component]\nfn Score(team: String) -> Element {\n    text(format!(\"{team} {}\", BIG[team.len()]))\n}\n";
    let started = Instant::now();
    let errors = compile(&bundle(), FILE, source).unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(30), "took {:?}", started.elapsed());
    let message = &errors[0].message;
    assert!(
        message == "Building components/score.rs used too much memory" || message == "Building components/score.rs used too much disk space",
        "{errors:?}"
    );
}
