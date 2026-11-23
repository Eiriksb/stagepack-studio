//! The commands that use a talk without a window, run as the built program.

use std::path::Path;
use std::process::{Command, Output};

/// Runs the program with `args`, its folders for data, cache and settings inside `home`, and no
/// display to open a window on.
fn run(args: &[&str], home: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_stagepack-studio"))
        .args(args)
        .env("HOME", home)
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .output()
        .expect("the program runs")
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A talk of three slides: a title, a two-column slide with one click, a hidden one with notes.
fn scratch_talk(dir: &Path) -> std::path::PathBuf {
    let talk = dir.join("talk");
    std::fs::create_dir_all(&talk).unwrap();
    let slides = "---\nlayout: title\n---\n\n# Hello there\n\n---\nlayout: two-columns\n---\n\n# Two sides\n\nleft words\n\n<Region name=\"right\" />\n\n<Box x=\"100\" y=\"100\" w=\"300\" h=\"100\" click=\"1\" fill=\"#ff0000\" />\n\n---\nhidden: true\n---\n\n# Spare\n\n<Notes>\nsay this\n</Notes>\n";
    std::fs::write(talk.join("slides.md"), slides).unwrap();
    talk
}

#[test]
fn outline_lists_each_slide() {
    let dir = tempfile::tempdir().unwrap();
    let talk = scratch_talk(dir.path());
    let talk = talk.to_str().unwrap();
    let output = run(&["outline", talk], dir.path());
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    let printed = text(&output.stdout);
    let lines: Vec<&str> = printed.lines().collect();
    assert_eq!(lines.len(), 3, "{printed}");
    assert!(lines[0].starts_with("1. ") && lines[0].contains("Hello there"), "{printed}");
    assert!(lines[1].starts_with("2. ") && lines[1].contains("1 click)"), "{printed}");
    assert!(lines[2].starts_with("3. ") && lines[2].contains("notes") && lines[2].ends_with(", hidden)"), "{printed}");

    let output = run(&["outline", talk, "--json"], dir.path());
    assert_eq!(output.status.code(), Some(0));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let slides = json.as_array().unwrap();
    let steps: Vec<u64> = slides.iter().map(|slide| slide["steps"].as_u64().unwrap()).collect();
    let hidden: Vec<bool> = slides.iter().map(|slide| slide["hidden"].as_bool().unwrap()).collect();
    assert_eq!(steps, [0, 1, 0]);
    assert_eq!(hidden, [false, false, true]);
    assert_eq!(slides[0]["layout"], "title");
    assert_eq!(slides[2]["notes"], true);
}

#[test]
fn long_text_is_cut_and_clicks_are_counted() {
    let dir = tempfile::tempdir().unwrap();
    let talk = dir.path().join("talk");
    std::fs::create_dir_all(&talk).unwrap();
    let long = "word ".repeat(40);
    std::fs::write(talk.join("slides.md"), format!("# {long}\n\n<Box x=\"100\" y=\"100\" w=\"300\" h=\"100\" click=\"1\" fill=\"#ff0000\" />\n\n<Box x=\"100\" y=\"300\" w=\"300\" h=\"100\" click=\"2\" fill=\"#00ff00\" />\n")).unwrap();
    let output = run(&["outline", talk.to_str().unwrap(), "--json"], dir.path());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let words = json[0]["first_words"].as_str().unwrap();
    assert_eq!(words.chars().count(), 81, "{words}");
    assert!(words.ends_with('…'));
    assert_eq!(json[0]["steps"], 2);
    let output = run(&["outline", talk.to_str().unwrap()], dir.path());
    assert!(text(&output.stdout).contains("2 clicks)"));
}

#[test]
fn a_path_that_isnt_a_talk_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let empty = root.join("empty");
    std::fs::create_dir(&empty).unwrap();
    let bad_text = root.join("bad text");
    std::fs::create_dir(&bad_text).unwrap();
    std::fs::write(bad_text.join("slides.md"), [0xff, 0xfe]).unwrap();
    let random = root.join("x.sptx");
    std::fs::write(&random, (0..2000u32).map(|n| (n.wrapping_mul(2654435761) >> 7) as u8).collect::<Vec<u8>>()).unwrap();
    let paths = [root.join("missing"), empty, bad_text, random, root.join("a path with a space")];
    for path in paths {
        let output = run(&["outline", path.to_str().unwrap()], root);
        let (out, err) = (text(&output.stdout), text(&output.stderr));
        assert_eq!(output.status.code(), Some(2), "{path:?}: {err}");
        assert_eq!(out, "", "{path:?}");
        assert_eq!(err.lines().count(), 1, "{path:?}: {err}");
        assert!(!err.contains("panicked"), "{path:?}: {err}");
    }
}

#[test]
fn a_command_opens_no_window() {
    let dir = tempfile::tempdir().unwrap();
    let talk = scratch_talk(dir.path());
    let output = run(&["outline", talk.to_str().unwrap()], dir.path());
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert!(!dir.path().join("data").join("stagepack-studio").exists());
    assert!(!dir.path().join(".local/share/stagepack-studio").exists());
}

#[test]
fn help_names_every_command() {
    let dir = tempfile::tempdir().unwrap();
    for word in ["help", "--help", "-h"] {
        let output = run(&[word], dir.path());
        assert_eq!(output.status.code(), Some(0));
        let printed = text(&output.stdout);
        for name in ["describe", "check", "outline", "picture", "guide", "mcp"] {
            assert!(printed.contains(name), "{word}: {printed}");
        }
    }
    let output = run(&["outline", "somewhere", "--frobnicate"], dir.path());
    assert_eq!(output.status.code(), Some(2));
    assert!(text(&output.stderr).contains("--frobnicate"));
    assert_eq!(text(&output.stdout), "");
}
