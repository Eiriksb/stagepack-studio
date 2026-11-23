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

/// [`run`] with one more environment variable.
fn run_with(args: &[&str], home: &Path, name: &str, value: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_stagepack-studio"))
        .args(args)
        .env("HOME", home)
        .env("XDG_DATA_HOME", home.join("data"))
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env(name, value)
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

const SCORE_SOURCE: &str = include_str!("../../components/tests/fixtures/v1/score.rs");
const SCORE_BUILD: &[u8] = include_bytes!("../../components/tests/fixtures/v1/score.wasm");

fn names(json: &serde_json::Value, key: &str) -> Vec<String> {
    json[key].as_array().unwrap().iter().map(|item| item["name"].as_str().unwrap().to_string()).collect()
}

#[test]
fn describe_without_a_talk_lists_every_built_in() {
    let dir = tempfile::tempdir().unwrap();
    let output = run(&["describe", "--json"], dir.path());
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let tags = json["tags"].as_array().unwrap();
    for tag in ["Shape", "Box", "Icon", "Picture", "Video", "Model", "Terminal", "Chart"] {
        let found = tags.iter().find(|entry| entry["tag"] == tag).unwrap_or_else(|| panic!("no {tag}"));
        assert_eq!(found["built_in"], true);
    }
    let video = tags.iter().find(|entry| entry["tag"] == "Video").unwrap();
    let settings = video["settings"].as_array().unwrap();
    let src = settings.iter().find(|item| item["name"] == "src").unwrap();
    assert_eq!(src["needed"], true);
    assert!(src["meaning"].as_str().unwrap().len() > 10);
    assert!(settings.iter().any(|item| item["name"] == "loop" && item["label"] == "Loop"));
    for tag in tags {
        for item in tag["settings"].as_array().unwrap() {
            assert!(item["meaning"].as_str().is_some_and(|said| !said.is_empty()), "{tag}: {item}");
        }
    }

    let layouts = json["layouts"].as_array().unwrap();
    let named: Vec<&str> = layouts.iter().map(|layout| layout["name"].as_str().unwrap()).collect();
    assert_eq!(named, ["title", "content", "two-columns", "section", "blank"]);
    assert_eq!(layouts[2]["regions"], serde_json::json!(["title", "left", "right"]));
    assert_eq!(layouts[2]["main"], "left");

    assert_eq!(names(&json, "placement"), ["x", "y", "w", "h", "rotate", "click", "enter", "align"]);
    let enter = json["placement"].as_array().unwrap().iter().find(|item| item["name"] == "enter").unwrap();
    assert_eq!(enter["values"], serde_json::json!(["appear", "fade", "fly-up"]));
    assert!(names(&json, "slide_settings").contains(&"transition".to_string()));
    assert!(names(&json, "talk_settings").contains(&"color.background".to_string()));
}

#[test]
fn describe_as_text_has_a_heading_per_part() {
    let dir = tempfile::tempdir().unwrap();
    let output = run(&["describe"], dir.path());
    assert_eq!(output.status.code(), Some(0));
    let printed = text(&output.stdout);
    for heading in ["Layouts\n", "Talk settings\n", "Slide settings\n", "Placement", "Tags\n"] {
        assert!(printed.contains(heading), "{heading}\n{printed}");
    }
    assert!(printed.contains("src (file, needed) — The video to play"), "{printed}");
}

#[test]
fn describe_with_a_talk_adds_its_components() {
    let dir = tempfile::tempdir().unwrap();
    let talk = dir.path().join("talk");
    std::fs::create_dir_all(talk.join("components")).unwrap();
    std::fs::write(talk.join("slides.md"), "# One\n").unwrap();
    std::fs::write(talk.join("components/score.rs"), SCORE_SOURCE).unwrap();
    let compile: stagepack_studio::builds::Compile = std::sync::Arc::new(|_: &str, _: &str| Ok(SCORE_BUILD.to_vec()));
    let built = stagepack_studio::builds::build_components(&talk, &compile);
    assert_eq!(built.built, ["Score"], "{:?}", built.problems);

    let output = run(&["describe", talk.to_str().unwrap(), "--json"], dir.path());
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let score = json["tags"].as_array().unwrap().iter().find(|entry| entry["tag"] == "Score").expect("Score is listed");
    assert_eq!(score["built_in"], false);
    let settings: Vec<&str> = score["settings"].as_array().unwrap().iter().map(|item| item["name"].as_str().unwrap()).collect();
    assert_eq!(settings, ["team", "points", "colour"]);
    assert!(score["settings"][0]["meaning"].is_null());

    let none = run(&["describe", "--json"], dir.path());
    let json: serde_json::Value = serde_json::from_slice(&none.stdout).unwrap();
    assert!(json["tags"].as_array().unwrap().iter().all(|entry| entry["tag"] != "Score"));
}

#[test]
fn only_a_file_is_needed_and_the_rest_have_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let output = run(&["describe", "--json"], dir.path());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let setting = |tag: &str, name: &str| {
        let tag = json["tags"].as_array().unwrap().iter().find(|entry| entry["tag"] == tag).unwrap();
        tag["settings"].as_array().unwrap().iter().find(|item| item["name"] == name).unwrap().clone()
    };
    for (tag, name) in [("Box", "radius"), ("Shape", "outline-width")] {
        let item = setting(tag, name);
        assert_eq!(item["needed"], false, "{tag} {name}");
        assert_eq!(item["default"], "0", "{tag} {name}");
    }
    assert_eq!(setting("Video", "src")["needed"], true);
    assert!(setting("Video", "src")["default"].is_null());
    assert_eq!(setting("Chart", "kind")["default"], "bar");
    assert_eq!(setting("Chart", "labels")["needed"], false);

    // Left out, they lay out without a notice.
    let talk = dir.path().join("talk");
    std::fs::create_dir_all(&talk).unwrap();
    std::fs::write(talk.join("slides.md"), "# One\n\n<Box />\n\n<Shape />\n").unwrap();
    let mut open = stagepack_studio::ai_tool::open_talk(&talk).unwrap();
    assert!(open.deck.notices_whole(0).is_empty(), "{:?}", open.deck.notices_whole(0));
}

/// A talk of three slides where slide 2 names a picture that isn't there and slide 3 a video
/// that is a text file.
fn talk_with_two_problems(dir: &Path) -> std::path::PathBuf {
    let talk = dir.join("broken");
    std::fs::create_dir_all(talk.join("assets")).unwrap();
    std::fs::write(talk.join("assets/clip.mp4"), "this is not a video\n").unwrap();
    std::fs::write(
        talk.join("slides.md"),
        "# Fine\n\n---\n\n# Gone\n\n<Picture src=\"assets/missing.png\" x=\"100\" y=\"100\" w=\"300\" h=\"200\" />\n\n---\n\n# Clip\n\n<Video src=\"assets/clip.mp4\" x=\"100\" y=\"100\" w=\"300\" h=\"200\" />\n",
    )
    .unwrap();
    talk
}

#[test]
fn check_says_each_problem_by_slide() {
    let dir = tempfile::tempdir().unwrap();
    let talk = talk_with_two_problems(dir.path());
    let path = talk.to_str().unwrap();

    // What the engine says of those slides, taken from it.
    let mut open = stagepack_studio::ai_tool::open_talk(&talk).unwrap();
    let picture = open.deck.notices_whole(1);
    let video = stagepack_studio::ai_tool::slide_problems(&mut open, 2);
    assert_eq!(picture.len(), 1, "{picture:?}");
    assert!(video.iter().any(|said| said == "This file isn't a video."), "{video:?}");

    let output = run(&["check", path], dir.path());
    assert_eq!(output.status.code(), Some(1), "{}", text(&output.stderr));
    let printed = text(&output.stdout);
    assert!(printed.contains(&format!("Slide 2: {}", picture[0])), "{printed}");
    assert!(printed.contains("Slide 3: This file isn't a video."), "{printed}");
    assert!(!printed.contains("Slide 1:"), "{printed}");
    assert_eq!(printed.lines().last(), Some("2 problems on 2 slides."), "{printed}");

    let output = run(&["check", path, "--json"], dir.path());
    assert_eq!(output.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let slides = json["slides"].as_array().unwrap();
    let numbers: Vec<u64> = slides.iter().map(|slide| slide["slide"].as_u64().unwrap()).collect();
    assert_eq!(numbers, [2, 3]);
    assert_eq!(slides[0]["hidden"], false);
    assert_eq!(slides[0]["problems"][0], picture[0].as_str());
    assert_eq!(json["talk"], serde_json::json!([]));

    let clean = scratch_talk(dir.path());
    let output = run(&["check", clean.to_str().unwrap()], dir.path());
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stdout));
    assert_eq!(text(&output.stdout), "No problems found.\n");
}

#[cfg(unix)]
#[test]
fn a_link_out_of_the_talk_is_a_complaint() {
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("outside.png");
    std::fs::write(&outside, "OUTSIDE-SECRET-BYTES").unwrap();
    let talk = dir.path().join("linked");
    std::fs::create_dir_all(talk.join("assets")).unwrap();
    std::os::unix::fs::symlink(&outside, talk.join("assets/x.png")).unwrap();
    std::fs::write(talk.join("slides.md"), "# One\n\n<Picture src=\"assets/x.png\" x=\"100\" y=\"100\" w=\"300\" h=\"200\" />\n").unwrap();
    let output = run(&["check", talk.to_str().unwrap()], dir.path());
    assert_eq!(output.status.code(), Some(1), "{}", text(&output.stdout));
    let printed = text(&output.stdout);
    assert!(printed.contains("Slide 1: "), "{printed}");
    assert!(!printed.contains("OUTSIDE-SECRET"), "{printed}");
    assert!(!text(&output.stderr).contains("OUTSIDE-SECRET"));
}

fn components_fixture(into: &Path) -> std::path::PathBuf {
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                copy(&entry.path(), &to.join(entry.file_name()));
            } else {
                std::fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
            }
        }
    }
    let talk = into.join("components-talk");
    copy(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/components"), &talk);
    let _ = std::fs::remove_dir_all(talk.join("build"));
    talk
}

#[test]
fn builds_only_where_always_built() {
    let dir = tempfile::tempdir().unwrap();
    let talk = components_fixture(dir.path());
    let path = talk.to_str().unwrap();

    // No permission: the unbuilt components are reported, and nothing is built.
    let output = run(&["check", path], dir.path());
    assert_eq!(output.status.code(), Some(1), "{}", text(&output.stdout));
    let printed = text(&output.stdout);
    assert!(printed.contains("needs building"), "{printed}");
    assert!(!talk.join("build").exists());

    // The owner has said this folder is always built, in the app's own file.
    let kept = dir.path().join("data/stagepack-studio");
    std::fs::create_dir_all(&kept).unwrap();
    let canonical = talk.canonicalize().unwrap();
    std::fs::write(kept.join("build-permissions.json"), serde_json::json!({ "always_build": [canonical] }).to_string()).unwrap();

    // A packed copy of the talk never builds, permission or not.
    let packed = dir.path().join("components.sptx");
    slides_engine::pack_folder(&talk, &packed, "test").unwrap();
    let output = run(&["check", packed.to_str().unwrap()], dir.path());
    assert_eq!(output.status.code(), Some(1), "{}", text(&output.stdout));
    assert!(!talk.join("build").exists());

    // The folder is built only where this copy of the app has its compiler, as the app finds it.
    let bundled = std::env::var_os("STUDIO_BUNDLE").is_some() && slides_components::Bundle::find().is_some();
    if !bundled {
        eprintln!("skipping the permitted build: no compiler bundle (set STUDIO_BUNDLE)");
        let output = run(&["check", path], dir.path());
        assert_eq!(output.status.code(), Some(1), "{}", text(&output.stdout));
        assert!(!talk.join("build").exists());
        return;
    }
    let output = run(&["check", path], dir.path());
    assert_eq!(output.status.code(), Some(0), "{}{}", text(&output.stdout), text(&output.stderr));
    assert!(talk.join("build").is_dir());
}

#[test]
fn a_big_talk_is_checked_quickly() {
    let dir = tempfile::tempdir().unwrap();
    let talk = dir.path().join("big");
    std::fs::create_dir_all(&talk).unwrap();
    let mut slides = String::new();
    for n in 1..=300 {
        if n > 1 {
            slides.push_str("\n---\n\n");
        }
        slides.push_str(&format!(
            "# Slide {n}\n\nSome words on slide {n}, enough to wrap onto a second line of the slide.\n\n<Box x=\"100\" y=\"500\" w=\"300\" h=\"100\" fill=\"#336699\" />\n\n<Chart kind=\"bar\" labels=\"A,B,C\" values=\"3,5,2\" x=\"600\" y=\"300\" w=\"500\" h=\"300\" />\n"
        ));
    }
    std::fs::write(talk.join("slides.md"), slides).unwrap();
    let started = std::time::Instant::now();
    let output = run(&["check", talk.to_str().unwrap()], dir.path());
    let took = started.elapsed();
    assert!(took < std::time::Duration::from_secs(10), "took {took:?}");
    assert!(output.status.code().is_some_and(|code| code <= 1), "{}", text(&output.stderr));
    eprintln!("checked 300 slides in {took:?}: {}", text(&output.stdout).lines().last().unwrap_or(""));
}

#[test]
fn a_slide_that_cant_be_read_says_so_once() {
    let dir = tempfile::tempdir().unwrap();
    let talk = dir.path().join("deep");
    std::fs::create_dir_all(&talk).unwrap();
    let deep = ">".repeat(100);
    std::fs::write(talk.join("slides.md"), format!("# Fine\n\n---\n\n{deep} too deep\n")).unwrap();
    let output = run(&["check", talk.to_str().unwrap()], dir.path());
    assert_eq!(output.status.code(), Some(1), "{}", text(&output.stdout));
    let printed = text(&output.stdout);
    assert_eq!(printed.matches("nested too deeply").count(), 1, "{printed}");
    assert!(printed.contains("Slide 2: "), "{printed}");
    assert_eq!(printed.lines().last(), Some("1 problem on 1 slide."), "{printed}");
}

#[test]
fn no_compiler_is_said_as_the_window_says_it() {
    let dir = tempfile::tempdir().unwrap();
    let talk = components_fixture(dir.path());
    let nowhere = dir.path().join("no-bundle");
    let output = run_with(&["check", talk.to_str().unwrap()], dir.path(), "STUDIO_BUNDLE", &nowhere);
    assert_eq!(output.status.code(), Some(1));
    let printed = text(&output.stdout);
    assert!(printed.contains("This copy of the app has no compiler, so components can't be built.\n"), "{printed}");
    assert!(!printed.contains("need building to show"), "{printed}");
    assert!(printed.lines().last().unwrap().ends_with(" and in the talk."), "{printed}");
    assert!(!talk.join("build").exists());
}

#[test]
fn the_summary_names_where_the_problems_are() {
    use stagepack_studio::ai_tool::{Checked, SlideProblems, check_text};
    let slide = |slide: usize, many: usize| SlideProblems { slide, hidden: false, problems: vec!["x".to_string(); many] };
    let said = |slides: Vec<SlideProblems>, talk: usize| {
        let checked = Checked { slides, talk: vec!["y".to_string(); talk] };
        check_text(&checked).lines().last().unwrap().to_string()
    };
    assert_eq!(said(vec![], 0), "No problems found.");
    assert_eq!(said(vec![], 1), "1 problem in the talk.");
    assert_eq!(said(vec![], 3), "3 problems in the talk.");
    assert_eq!(said(vec![slide(2, 1)], 0), "1 problem on 1 slide.");
    assert_eq!(said(vec![slide(2, 2), slide(5, 1)], 0), "3 problems on 2 slides.");
    assert_eq!(said(vec![slide(2, 2), slide(5, 1)], 1), "4 problems on 2 slides and in the talk.");
    assert_eq!(said(vec![slide(2, 1)], 1), "2 problems on 1 slide and in the talk.");
}
