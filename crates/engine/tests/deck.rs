mod common;
use slides_engine::*;
use std::sync::Arc;

struct NoFiles;
impl TalkFiles for NoFiles { fn read(&self, _: &str) -> Option<Vec<u8>> { None } }

const TALK: &str = "# One\n\n---\n\n# Two\n\n---\n\n# Three\n";

fn deck(source: &str) -> Deck { Deck::open(Box::new(NoFiles), Fonts::bundled_only(), source.to_string()) }

#[test]
fn editing_one_slide_reports_only_that_slide() {
    let mut d = deck(TALK);
    assert_eq!(d.update(TALK.replace("Two", "Changed")), vec![1]);
}

#[test]
fn unchanged_slides_keep_their_layers() {
    let mut d = deck(TALK);
    let before = d.layers(0, 0.5);
    d.update(TALK.replace("Two", "Changed"));
    assert!(Arc::ptr_eq(&before[1], &d.layers(0, 0.5)[1]));
}

#[test]
fn only_changed_blocks_get_new_layers() {
    let src = "# Title\n\nBody\n";
    let mut d = deck(src);
    let before = d.layers(0, 0.5);
    d.update("# Title\n\nBody changed\n".to_string());
    let after = d.layers(0, 0.5);
    assert!(Arc::ptr_eq(&before[1], &after[1]), "the title didn't change");
    assert!(!Arc::ptr_eq(&before[2], &after[2]), "the body did");
}

#[test]
fn a_new_scale_redraws() {
    let mut d = deck(TALK);
    let small = d.layers(0, 0.5);
    let large = d.layers(0, 1.0);
    assert!(large[0].pixmap.width() > small[0].pixmap.width());
}

#[test]
fn adding_a_slide_reports_the_slides_whose_text_changed() {
    let mut d = deck(TALK);
    assert_eq!(d.update(format!("{TALK}\n---\n\n# Four\n")), vec![2, 3]);
    assert_eq!(d.slide_count(), 4);
}

#[test]
fn changing_the_talks_settings_redraws_everything() {
    let mut d = deck("---\ncolor.text: red\n---\n\n# One\n\n---\n\n# Two\n");
    assert_eq!(d.update("---\ncolor.text: blue\n---\n\n# One\n\n---\n\n# Two\n".to_string()), vec![0, 1]);
}

#[test]
fn title_comes_from_the_talks_settings() {
    assert_eq!(deck("---\ntitle: Review\n---\n\n# One\n").title().as_deref(), Some("Review"));
}

#[test]
fn golden_showcase() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/showcase");
    let source = std::fs::read_to_string(root.join("slides.md")).unwrap();
    let mut d = Deck::open(Box::new(FolderFiles::new(root)), Fonts::bundled_only(), source);
    for index in 0..d.slide_count() {
        assert!(d.notices(index).is_empty(), "slide {index}: {:?}", d.notices(index));
        let layers: Vec<_> = d.layers(index, 1.0);
        let mut target = tiny_skia::Pixmap::new(1920, 1080).unwrap();
        for layer in &layers {
            target.draw_pixmap(layer.x, layer.y, layer.pixmap.as_ref(), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
        }
        common::assert_golden(&format!("showcase-{index}"), &target);
    }
}
