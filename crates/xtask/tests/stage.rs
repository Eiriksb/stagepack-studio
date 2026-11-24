//! Stages the installed folder from release builds and runs it with nothing else around.
//! Run explicitly: `scripts/cargo-safe.sh test -p xtask --test stage -- --ignored`.

use slides_terminal_helper::wire::{self, FromHelper, StartSpec, ToHelper};
use std::path::Path;
use std::process::{Command, Stdio};
use xtask::stage::{BUDGET_MB, copy_tree, folder_bytes, may_replace, over_budget, release_flags, repo_root};

#[test]
fn a_folder_over_the_budget_says_so() {
    assert_eq!(over_budget(601 * 1024 * 1024), "The app folder is 601 MB, over the 600 MB budget.");
    assert_eq!(BUDGET_MB, 600);
}

#[test]
fn a_copy_keeps_links_and_sizes() {
    let dir = tempfile::tempdir().unwrap();
    let from = dir.path().join("from");
    std::fs::create_dir_all(from.join("sub")).unwrap();
    std::fs::write(from.join("sub/a"), vec![0u8; 1000]).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("sub/a", from.join("link")).unwrap();
    copy_tree(&from, &dir.path().join("to")).unwrap();
    assert_eq!(std::fs::read(dir.path().join("to/sub/a")).unwrap().len(), 1000);
    #[cfg(unix)]
    assert_eq!(std::fs::read_link(dir.path().join("to/link")).unwrap(), Path::new("sub/a"));
    assert!(folder_bytes(&dir.path().join("to")).unwrap() >= 1000);
}

#[test]
fn the_windows_programs_carry_their_c_runtime() {
    let repo = repo_root();
    let windows: Vec<String> = release_flags(&repo, true).split('\x1f').map(str::to_string).collect();
    let linux: Vec<String> = release_flags(&repo, false).split('\x1f').map(str::to_string).collect();
    assert!(windows.iter().any(|f| f == "-Ctarget-feature=+crt-static"), "{windows:?}");
    assert!(!linux.iter().any(|f| f.contains("crt-static")), "{linux:?}");
    // Both keep the builder's paths out.
    for flags in [&windows, &linux] {
        assert!(flags.iter().any(|f| f.starts_with("--remap-path-prefix=") && f.ends_with("=/studio")), "{flags:?}");
    }
    assert_eq!(windows.len(), linux.len() + 1);
}

#[test]
fn only_a_staged_app_folder_is_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let exe = if cfg!(windows) { "stagepack-studio.exe" } else { "stagepack-studio" };
    assert!(may_replace(&dir.path().join("missing")));
    std::fs::create_dir_all(dir.path().join("empty")).unwrap();
    assert!(may_replace(&dir.path().join("empty")));
    // A bin folder that holds a link or a copy of the program is not an app folder.
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join(exe), "the program").unwrap();
    std::fs::write(bin.join("other-tool"), "mine").unwrap();
    assert!(!may_replace(&bin));
    std::fs::write(bin.join("NOTICES.txt"), "notices").unwrap();
    assert!(!may_replace(&bin), "without bundle/ it still isn't one");
    std::fs::create_dir_all(bin.join("bundle")).unwrap();
    assert!(may_replace(&bin));
    // And stage itself refuses one, leaving it as it was.
    std::fs::remove_dir_all(bin.join("bundle")).unwrap();
    let message = xtask::stage::stage(&bin, None).unwrap_err();
    assert!(message.contains("isn't an app folder"), "{message}");
    assert_eq!(std::fs::read_to_string(bin.join("other-tool")).unwrap(), "mine");
}

/// What the staged program is run with: no Rust on the path, no `STUDIO_*`, a scratch home.
fn bare(program: &Path, home: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .env_clear()
        .env("HOME", home)
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8");
    command
}

#[test]
#[ignore = "builds a release and the bundle; run by test-all"]
fn the_staged_app_needs_nothing_else() {
    let repo = repo_root();
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("app");
    let bundle = repo.join("target/bundle");
    assert!(bundle.join("bin").is_dir(), "make target/bundle first (scripts/make-bundle.sh target/bundle)");
    let bytes = xtask::stage::stage(&out, Some(&bundle)).unwrap();
    let mb = bytes.div_ceil(1024 * 1024);
    eprintln!("the staged app folder is {mb} MB");
    assert!(mb <= BUDGET_MB, "{}", over_budget(bytes));

    // The layout.
    for part in [
        "stagepack-studio",
        "stagepack-terminal",
        "bundle/bin/rustc",
        "guide/components.md",
        "guide/writing-talks-for-ai.md",
        "LICENSE",
        "NOTICES.txt",
        "notices/rust-COPYRIGHT.html",
        "icons/icon-256.png",
        "icons/stagepack-studio.ico",
    ] {
        assert!(out.join(part).exists(), "{part} is missing from the staged folder");
    }
    assert!(std::fs::read_to_string(out.join("NOTICES.txt")).unwrap().contains("OpenH264"));

    // Nothing of the builder's home or the repository's place is written into a shipped program.
    let builder = [std::env::var("HOME").unwrap(), repo.display().to_string()];
    for program in ["stagepack-studio", "stagepack-terminal"] {
        let bytes = std::fs::read(out.join(program)).unwrap();
        for path in &builder {
            let found = bytes.windows(path.len()).any(|window| window == path.as_bytes());
            assert!(!found, "{program} has {path} written in it");
        }
    }

    let home = scratch.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let app = out.join("stagepack-studio");

    // A talk with components, built because the owner's permission file says so.
    let talk = scratch.path().join("components-talk");
    copy_tree(&repo.join("fixtures/components"), &talk).unwrap();
    let _ = std::fs::remove_dir_all(talk.join("build"));
    let permissions = home.join("data/stagepack-studio/build-permissions.json");
    std::fs::create_dir_all(permissions.parent().unwrap()).unwrap();
    let canonical = talk.canonicalize().unwrap();
    std::fs::write(&permissions, serde_json::json!({ "always_build": [canonical] }).to_string()).unwrap();
    let output = bare(&app, &home).args(["check", talk.to_str().unwrap()]).output().unwrap();
    let said = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert_eq!(output.status.code(), Some(0), "{said}");
    assert!(talk.join("build").is_dir(), "the components weren't built: {said}");

    // A packed talk.
    let packed = scratch.path().join("show.sptx");
    slides_engine::pack_folder(&repo.join("fixtures/show"), &packed, "test").unwrap();
    let output = bare(&app, &home).args(["check", packed.to_str().unwrap()]).output().unwrap();
    let said = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert_eq!(output.status.code(), Some(0), "{said}");

    // The helper, found beside the app: it starts a program and says it ended.
    let mut helper = bare(&out.join("stagepack-terminal"), &home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let spec = StartSpec {
        program: "sh".into(),
        args: vec!["-c".into(), "exit 0".into()],
        cwd: home.clone(),
        cols: 80,
        rows: 24,
        env: vec![("TERM".into(), "xterm-256color".into())],
    };
    ToHelper::Start(spec).write(helper.stdin.as_mut().unwrap()).unwrap();
    let mut stdout = helper.stdout.take().unwrap();
    let mut exited = false;
    while let Ok(Some((kind, body))) = wire::read_frame(&mut stdout) {
        if let Ok(FromHelper::Exited(exit)) = FromHelper::decode(kind, &body) {
            assert_eq!(exit.code, Some(0));
            exited = true;
            break;
        }
    }
    drop(helper.stdin.take());
    let _ = helper.kill();
    let _ = helper.wait();
    assert!(exited, "the helper never said the program exited");
}
