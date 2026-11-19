//! A terminal recording on a slide of a talk the app has open.

use std::io::Write as _;
use std::sync::Arc;

use slides_engine::Fonts;
use slides_format::Block;
use stagepack_studio::document::Document;
use stagepack_studio::saving::SaveTarget;
use stagepack_studio::video_player::{Making, make_one};

const TALK: &str = "# One\n\n<Terminal src=\"assets/run.cast\" x=\"100\" y=\"100\" w=\"640\" h=\"360\" />\n";

fn cast() -> Vec<u8> {
    slides_terminal::make_test_cast(40, 6, &[(0.1, "o", "$ echo packed\r\npacked\r\n")])
}

/// Makes every still the talk wants, as the window's background tasks do, and says how many.
fn make_stills_now(document: &mut Document) -> usize {
    let mut made = 0;
    loop {
        let jobs = document.still_jobs();
        if jobs.is_empty() {
            return made;
        }
        for (job, open) in jobs {
            assert!(job.is_terminal());
            let Making::Made(still) = std::thread::spawn(move || make_one(job, open)).join().unwrap() else { panic!("a still waits") };
            assert!(!still.failed(), "the recording couldn't be read");
            document.take_still(still);
            made += 1;
        }
    }
}

/// The place in the talk's text of its recording.
fn terminal_source(document: &Document) -> std::ops::Range<usize> {
    document.talk().slides[0]
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Tag(tag) if tag.name == "Terminal" => Some(tag.range.clone()),
            _ => None,
        })
        .expect("the talk has a recording")
}

/// Opens the pack at `pack` as the window does, its recording read aside, and checks it's read.
fn read_from(pack: &std::path::Path) {
    let mut document = Document::open_packed(pack, Fonts::bundled_only()).unwrap();
    document.make_stills_aside(Arc::new(|| {}));
    document.editor_layers(1280. / 1920.);
    assert!(document.terminal_pending(0), "read aside");
    assert_eq!(make_stills_now(&mut document), 1);
    assert!(!document.terminal_pending(0));
    let source = terminal_source(&document);
    assert_eq!(document.terminal_problem_on(0, &source), None, "it's read");
    assert!(document.notices().is_empty(), "{:?}", document.notices());
}

#[test]
fn a_recording_in_a_pack_is_read() {
    let work = tempfile::tempdir().unwrap();
    let folder = work.path().join("talk");
    std::fs::create_dir_all(folder.join("assets")).unwrap();
    std::fs::write(folder.join("slides.md"), TALK).unwrap();
    std::fs::write(folder.join("assets/run.cast"), cast()).unwrap();
    // Saved as a pack by the app.
    let pack = work.path().join("talk.sptx");
    let saved = Document::open(&folder, Fonts::bundled_only()).unwrap().save_as_job(SaveTarget::Packed(pack.clone())).run();
    assert!(saved.is_ok());
    read_from(&pack);
    // And by another program that compresses everything.
    let other = work.path().join("compressed.sptx");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&other).unwrap());
    let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("talk.json", deflated).unwrap();
    zip.write_all(br#"{"format":1,"app":"another","entry":"slides.md"}"#).unwrap();
    zip.start_file("slides.md", deflated).unwrap();
    zip.write_all(TALK.as_bytes()).unwrap();
    zip.start_file("assets/run.cast", deflated).unwrap();
    zip.write_all(&cast()).unwrap();
    zip.finish().unwrap();
    read_from(&other);
}
