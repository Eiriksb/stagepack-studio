use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use slides_engine::FolderFiles;
use stagepack_studio::export::{
    ExportInput, ExportKind, file_stem_for, open_in_stagepack, run_export, stagepack_executable,
};
use zip::ZipArchive;

const TALK: &str = "---\ntitle: Review\n---\n\n# One\n\n---\n\n# Two\n";

fn input(source: &str) -> ExportInput {
    ExportInput {
        source: source.to_string(),
        files: Arc::new(FolderFiles::new(PathBuf::from("no-such-talk"))),
        talk_fonts: Vec::new(),
    }
}

fn target(name: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join(name);
    (dir, out)
}

#[test]
fn run_export_writes_a_pptx() {
    let (_dir, out) = target("talk.pptx");
    run_export(input(TALK), ExportKind::PowerPoint, &out).unwrap();
    assert!(std::fs::read(&out).unwrap().starts_with(b"PK"));
    let mut archive = ZipArchive::new(File::open(&out).unwrap()).unwrap();
    assert!(archive.by_name("ppt/slides/slide1.xml").is_ok());
}

#[test]
fn run_export_writes_a_pdf() {
    let (_dir, out) = target("talk.pdf");
    run_export(input(TALK), ExportKind::Pdf, &out).unwrap();
    assert!(std::fs::read(&out).unwrap().starts_with(b"%PDF"));
}

#[test]
fn run_export_replaces_an_existing_file() {
    let (_dir, out) = target("talk.pdf");
    std::fs::write(&out, "the old file").unwrap();
    run_export(input(TALK), ExportKind::Pdf, &out).unwrap();
    assert!(std::fs::read(&out).unwrap().starts_with(b"%PDF"));
}

#[test]
fn a_pdf_export_passes_on_what_the_slides_said() {
    let (_dir, out) = target("talk.pdf");
    let source = "# One\n\n---\nlayout: nope\n---\n\n# Two\n";
    let notices = run_export(input(source), ExportKind::Pdf, &out).unwrap();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(
        notices[0].starts_with("Slide 2:") && notices[0].contains("nope"),
        "{notices:?}"
    );
}

#[test]
fn a_powerpoint_export_passes_on_what_the_slides_said() {
    let (_dir, out) = target("talk.pptx");
    let source = "# One\n\n---\nlayout: nope\n---\n\n# Two\n";
    let notices = run_export(input(source), ExportKind::PowerPoint, &out).unwrap();
    assert!(
        notices
            .iter()
            .any(|notice| notice.starts_with("Slide 2:") && notice.contains("nope")),
        "{notices:?}"
    );
}

#[test]
fn a_broken_talk_font_still_exports() {
    let (_dir, out) = target("talk.pdf");
    let mut broken = input(TALK);
    broken.talk_fonts.push(std::sync::Arc::new(b"not a font".to_vec()));
    run_export(broken, ExportKind::Pdf, &out).unwrap();
    assert!(std::fs::read(&out).unwrap().starts_with(b"%PDF"));
}

#[test]
fn a_target_in_a_missing_folder_says_so() {
    let (_dir, out) = target("missing/talk.pdf");
    let problem = run_export(input(TALK), ExportKind::Pdf, &out).unwrap_err();
    assert!(problem.contains("talk.pdf"), "{problem}");
}

#[cfg(unix)]
#[test]
fn a_failed_export_leaves_the_old_file_alone() {
    use std::os::unix::fs::PermissionsExt as _;

    let (dir, out) = target("talk.pdf");
    std::fs::write(&out, "the old file").unwrap();
    let set_mode =
        |mode| std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(mode)).unwrap();
    set_mode(0o555);
    let result = run_export(input(TALK), ExportKind::Pdf, &out);
    set_mode(0o755);

    let problem = result.unwrap_err();
    assert!(problem.contains("talk.pdf"), "{problem}");
    assert_eq!(std::fs::read(&out).unwrap(), b"the old file");
}

#[cfg(not(windows))]
#[test]
fn stagepack_is_found_where_it_installs() {
    let env = |name: &str| match name {
        "XDG_DATA_HOME" => Some("/data".to_string()),
        "HOME" => Some("/home/me".to_string()),
        _ => None,
    };
    let in_data_home = Path::new("/data/stagepack/stagepack");
    let in_home = Path::new("/home/me/.local/share/stagepack/stagepack");

    assert_eq!(
        stagepack_executable(env, |_| true).as_deref(),
        Some(in_data_home),
        "the data home comes first"
    );
    assert_eq!(
        stagepack_executable(env, |path| path == in_home).as_deref(),
        Some(in_home),
        "then the home folder"
    );
    let only_home = |name: &str| (name == "HOME").then(|| "/home/me".to_string());
    assert_eq!(
        stagepack_executable(only_home, |_| true).as_deref(),
        Some(in_home)
    );
}

#[cfg(windows)]
#[test]
fn stagepack_is_found_where_it_installs() {
    let env =
        |name: &str| (name == "LOCALAPPDATA").then(|| r"C:\Users\me\AppData\Local".to_string());
    assert_eq!(
        stagepack_executable(env, |_| true),
        Some(PathBuf::from(
            r"C:\Users\me\AppData\Local\Stagepack\stagepack.exe"
        ))
    );
}

#[cfg(not(windows))]
#[test]
fn a_relative_data_home_is_not_a_place_to_run_things_from() {
    let env = |name: &str| match name {
        "XDG_DATA_HOME" => Some("somewhere/relative".to_string()),
        "HOME" => Some("/home/me".to_string()),
        _ => None,
    };
    assert_eq!(
        stagepack_executable(env, |_| true).as_deref(),
        Some(Path::new("/home/me/.local/share/stagepack/stagepack"))
    );
}

#[test]
fn no_stagepack_means_no_offer() {
    let env = |name: &str| {
        matches!(name, "XDG_DATA_HOME" | "HOME" | "LOCALAPPDATA").then(|| {
            if cfg!(windows) {
                r"C:\Users\me".to_string()
            } else {
                "/home/me".to_string()
            }
        })
    };
    assert_eq!(stagepack_executable(env, |_| false), None);
    assert_eq!(stagepack_executable(|_| None, |_| true), None);
}

#[cfg(unix)]
#[test]
fn stagepack_is_asked_to_open_the_file() {
    use std::os::unix::fs::PermissionsExt as _;
    use std::time::{Duration, Instant};

    let dir = tempfile::tempdir().unwrap();
    let said = dir.path().join("said");
    let stagepack = dir.path().join("stagepack");
    std::fs::write(
        &stagepack,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}.tmp'\nmv '{0}.tmp' '{0}'\n",
            said.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&stagepack, std::fs::Permissions::from_mode(0o755)).unwrap();

    open_in_stagepack(&stagepack, Path::new("/talks/a talk.sptx")).unwrap();

    let start = Instant::now();
    while !said.exists() {
        assert!(start.elapsed() < Duration::from_secs(10), "it never ran");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        std::fs::read_to_string(&said).unwrap(),
        "open\n/talks/a talk.sptx\n"
    );
}

#[test]
fn a_stagepack_that_will_not_start_says_so() {
    let dir = tempfile::tempdir().unwrap();
    assert!(open_in_stagepack(&dir.path().join("gone"), Path::new("a.sptx")).is_err());
}

#[test]
fn a_talks_name_is_a_safe_file_name() {
    assert_eq!(file_stem_for("Quarterly review"), "Quarterly review");
    assert_eq!(file_stem_for("../../etc/passwd"), "-..-etc-passwd");
    assert_eq!(file_stem_for(r"C:\temp\a:b*c?"), "C--temp-a-b-c-");
    assert_eq!(file_stem_for("  .hidden. "), "hidden");
    assert_eq!(file_stem_for("line\nbreak"), "line-break");
}

#[test]
fn a_talk_with_nothing_usable_in_its_name_is_still_called_something() {
    assert_eq!(file_stem_for(""), "talk");
    assert_eq!(file_stem_for(" ... "), "talk");
}

#[test]
fn a_very_long_name_fits_in_a_file_name() {
    let stem = file_stem_for(&"𝄞".repeat(1000));
    assert!(stem.len() <= 240, "{}", stem.len());
    assert!(!stem.is_empty());
}

// Videos, which PowerPoint and PDF get as their stills.

/// A video of three blue pictures, 64 by 36.
fn blue_clip() -> Vec<u8> {
    slides_video::make_test_video(64, 36, 3, 10, |_| [255, 0, 0, 255].repeat(64 * 36))
}

/// A talk folder of `source`, holding `video` as `assets/clip.mp4`, and its export input.
fn with_video(source: &str, video: &[u8]) -> (tempfile::TempDir, ExportInput) {
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir(folder.path().join("assets")).unwrap();
    std::fs::write(folder.path().join("assets/clip.mp4"), video).unwrap();
    let input = ExportInput {
        source: source.to_string(),
        files: Arc::new(FolderFiles::new(folder.path().to_path_buf())),
        talk_fonts: Vec::new(),
    };
    (folder, input)
}

const ONE_VIDEO: &str = "# Video\n\n<Video src=\"assets/clip.mp4\" x=\"100\" y=\"100\" w=\"640\" h=\"360\" />\n";

/// The pictures stored in the PowerPoint file at `out`, decoded.
fn pptx_pictures(out: &Path) -> Vec<image::RgbaImage> {
    let mut archive = ZipArchive::new(File::open(out).unwrap()).unwrap();
    let names: Vec<String> = archive.file_names().filter_map(Result::ok).map(|name| name.into_owned()).filter(|name| name.starts_with("ppt/media/")).collect();
    names
        .iter()
        .map(|name| {
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut archive.by_name(name).unwrap(), &mut bytes).unwrap();
            image::load_from_memory(&bytes).unwrap().to_rgba8()
        })
        .collect()
}

fn slide_xml(out: &Path, index: usize) -> String {
    let mut archive = ZipArchive::new(File::open(out).unwrap()).unwrap();
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut archive.by_name(&format!("ppt/slides/slide{index}.xml")).unwrap(), &mut xml).unwrap();
    xml
}

fn holds(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

#[test]
fn a_video_is_a_picture_in_the_pdf_and_in_powerpoint() {
    let (_folder, input) = with_video(ONE_VIDEO, &blue_clip());
    let (_dir, out) = target("talk.pptx");
    run_export(input, ExportKind::PowerPoint, &out).unwrap();
    assert!(slide_xml(&out, 1).contains("<p:pic>"), "{}", slide_xml(&out, 1));
    let pictures = pptx_pictures(&out);
    assert_eq!(pictures.len(), 1);
    // Its first picture, blue, not the plain panel.
    let middle = pictures[0].get_pixel(pictures[0].width() / 2, pictures[0].height() / 2).0;
    assert!(middle[2] > 200 && middle[0] < 40 && middle[1] < 40, "{middle:?}");

    let (_folder, input) = with_video(ONE_VIDEO, &blue_clip());
    let (_dir, pdf) = target("talk.pdf");
    run_export(input, ExportKind::Pdf, &pdf).unwrap();
    assert!(holds(&std::fs::read(&pdf).unwrap(), b"/Subtype/Image"), "the still is a picture");

    // One that can't be played is its panel, and no picture.
    let (_folder, input) = with_video(ONE_VIDEO, b"these are words, not pictures");
    let (_dir, out) = target("talk.pptx");
    let notices = run_export(input, ExportKind::PowerPoint, &out).unwrap();
    assert!(pptx_pictures(&out).is_empty());
    assert!(!slide_xml(&out, 1).contains("<p:pic>"));
    assert!(notices.iter().any(|notice| notice == "Slide 1: This file isn't a video."), "{notices:?}");
    let (_folder, input) = with_video(ONE_VIDEO, b"these are words, not pictures");
    let (_dir, pdf) = target("talk.pdf");
    run_export(input, ExportKind::Pdf, &pdf).unwrap();
    assert!(!holds(&std::fs::read(&pdf).unwrap(), b"/Subtype/Image"));
}

#[test]
fn the_export_says_how_many() {
    let three = format!("{ONE_VIDEO}\n<Video src=\"assets/clip.mp4\" x=\"900\" y=\"100\" w=\"640\" h=\"360\" />\n\n---\n\n{}", ONE_VIDEO.replace("# Video", "# Again"));
    for kind in [ExportKind::PowerPoint, ExportKind::Pdf] {
        let said = |source: &str| {
            let (_folder, input) = with_video(source, &blue_clip());
            let (_dir, out) = target(&format!("talk.{}", kind.extension()));
            run_export(input, kind, &out).unwrap()
        };
        let notices = said(&three);
        let counted: Vec<&String> = notices.iter().filter(|notice| notice.contains("exported as")).collect();
        assert_eq!(counted, ["3 videos were exported as pictures."], "{kind:?}: {notices:?}");
        let notices = said(ONE_VIDEO);
        assert!(notices.iter().any(|notice| notice == "1 video was exported as a picture."), "{kind:?}: {notices:?}");
        let notices = said(TALK);
        assert!(notices.iter().all(|notice| !notice.contains("exported as")), "{kind:?}: {notices:?}");
    }
    // A slide left out of the PDF leaves its videos out of the count.
    let hidden = format!("{ONE_VIDEO}\n---\nhidden: true\n---\n\n{ONE_VIDEO}");
    let (_folder, input) = with_video(&hidden, &blue_clip());
    let (_dir, out) = target("talk.pdf");
    let notices = run_export(input, ExportKind::Pdf, &out).unwrap();
    assert!(notices.iter().any(|notice| notice == "1 video was exported as a picture."), "{notices:?}");
}
