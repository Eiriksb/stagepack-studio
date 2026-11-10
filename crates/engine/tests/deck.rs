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

fn one_colour_png(color: tiny_skia::Color) -> Vec<u8> {
    let mut pixmap = tiny_skia::Pixmap::new(8, 8).unwrap();
    pixmap.fill(color);
    pixmap.encode_png().unwrap()
}

const PICTURE_TALK: &str = "# One\n\n![](photo.png)\n\n---\n\n# Two\n";

/// The layer that holds the picture on `PICTURE_TALK`'s first slide.
const PICTURE_LAYER: usize = 2;

fn talk_with_a_picture() -> (tempfile::TempDir, Deck) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("photo.png"), one_colour_png(tiny_skia::Color::BLACK)).unwrap();
    let files = FolderFiles::new(dir.path().to_path_buf());
    (dir, Deck::open(Box::new(files), Fonts::bundled_only(), PICTURE_TALK.to_string()))
}

/// Writes a different picture over the old one. The two are the same length, so the
/// modified time is moved on: two quick writes can land on the same tick of the clock.
fn replace_the_picture(dir: &tempfile::TempDir) {
    let path = dir.path().join("photo.png");
    let written = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::fs::write(&path, one_colour_png(tiny_skia::Color::WHITE)).unwrap();
    let file = std::fs::File::options().write(true).open(&path).unwrap();
    file.set_modified(written + std::time::Duration::from_secs(2)).unwrap();
}

fn pixels(layer: &Layer) -> &[u8] { layer.pixmap.data() }

#[test]
fn a_replaced_picture_shows_after_the_next_save() {
    let (dir, mut d) = talk_with_a_picture();
    let before = d.layers(0, 1.0);
    replace_the_picture(&dir);
    d.update(PICTURE_TALK.replace("One", "Changed"));
    assert!(pixels(&d.layers(0, 1.0)[PICTURE_LAYER]) != pixels(&before[PICTURE_LAYER]), "still the old picture");
}

#[test]
fn a_replaced_picture_shows_even_if_the_text_is_unchanged() {
    let (dir, mut d) = talk_with_a_picture();
    let before = d.layers(0, 1.0);
    replace_the_picture(&dir);
    assert_eq!(d.update(PICTURE_TALK.to_string()), vec![0, 1], "every slide is reported");
    assert!(pixels(&d.layers(0, 1.0)[PICTURE_LAYER]) != pixels(&before[PICTURE_LAYER]), "still the old picture");
}

#[test]
fn an_unchanged_picture_keeps_its_layers() {
    let (_dir, mut d) = talk_with_a_picture();
    let before = d.layers(0, 1.0);
    assert_eq!(d.update(PICTURE_TALK.to_string()), Vec::<usize>::new());
    assert!(Arc::ptr_eq(&before[PICTURE_LAYER], &d.layers(0, 1.0)[PICTURE_LAYER]));
}

#[test]
fn a_deleted_picture_stops_showing() {
    let (dir, mut d) = talk_with_a_picture();
    assert!(d.notices(0).is_empty());
    std::fs::remove_file(dir.path().join("photo.png")).unwrap();
    assert_eq!(d.update(PICTURE_TALK.to_string()), vec![0, 1]);
    assert_eq!(d.notices(0).len(), 1, "{:?}", d.notices(0));
}

/// Files with no stamp to go by, as a talk kept somewhere other than a folder might be.
#[derive(Clone)]
struct UnstampedFiles(Arc<std::sync::Mutex<Vec<u8>>>);

impl TalkFiles for UnstampedFiles {
    fn read(&self, _: &str) -> Option<Vec<u8>> { Some(self.0.lock().unwrap().clone()) }
}

#[test]
fn with_no_stamp_a_picture_is_compared_by_its_bytes() {
    let files = UnstampedFiles(Arc::new(std::sync::Mutex::new(one_colour_png(tiny_skia::Color::BLACK))));
    let mut d = Deck::open(Box::new(files.clone()), Fonts::bundled_only(), PICTURE_TALK.to_string());
    let before = d.layers(0, 1.0);

    assert_eq!(d.update(PICTURE_TALK.to_string()), Vec::<usize>::new(), "the same bytes are no change");
    assert!(Arc::ptr_eq(&before[PICTURE_LAYER], &d.layers(0, 1.0)[PICTURE_LAYER]));

    *files.0.lock().unwrap() = one_colour_png(tiny_skia::Color::WHITE);
    assert_eq!(d.update(PICTURE_TALK.to_string()), vec![0, 1]);
    assert!(pixels(&d.layers(0, 1.0)[PICTURE_LAYER]) != pixels(&before[PICTURE_LAYER]), "still the old picture");
}

const SCALE: f32 = 0.5;

fn long_talk(slides: usize) -> String {
    (0..slides).map(|n| format!("# Slide {n}\n")).collect::<Vec<_>>().join("\n---\n\n")
}

fn bytes(layer: &Layer) -> usize { layer.pixmap.data().len() }

#[test]
fn slides_share_one_background() {
    let mut d = deck(TALK);
    let first = d.layers(0, SCALE);
    let second = d.layers(1, SCALE);
    assert!(Arc::ptr_eq(&first[0], &second[0]));
    assert_eq!(d.drawn_bytes(), bytes(&first[0]) + bytes(&first[1]) + bytes(&second[1]), "the background counts once");
}

#[test]
fn drawn_layers_stay_within_the_memory_budget() {
    let mut d = deck(&long_talk(20));
    let first = d.layers(0, SCALE);
    let budget = bytes(&first[0]) + 3 * bytes(&first[1]);
    d.set_memory_budget(budget);
    for index in 0..20 {
        d.layers(index, SCALE);
        let drawn = d.drawn_bytes();
        assert!(drawn > 0 && drawn <= budget, "slide {index}: {drawn} bytes drawn with a budget of {budget}");
    }
}

#[test]
fn the_least_recently_used_slide_is_evicted_first() {
    let mut d = deck(&long_talk(3));
    let zero = d.layers(0, SCALE);
    d.set_memory_budget(bytes(&zero[0]) + 2 * bytes(&zero[1]));
    let one = d.layers(1, SCALE);
    d.layers(0, SCALE);
    d.layers(2, SCALE);
    assert!(Arc::ptr_eq(&zero[1], &d.layers(0, SCALE)[1]), "slide 0 was asked for more recently");
    assert!(!Arc::ptr_eq(&one[1], &d.layers(1, SCALE)[1]), "slide 1 was the one to go");
}

#[test]
fn the_slide_being_requested_can_exceed_the_budget_alone() {
    let mut d = deck(TALK);
    d.set_memory_budget(1);
    let layers = d.layers(0, SCALE);
    assert!(d.drawn_bytes() > 1);
    assert!(Arc::ptr_eq(&layers[1], &d.layers(0, SCALE)[1]));
}

#[test]
fn going_back_to_an_evicted_slide_draws_it_again() {
    let mut d = deck(TALK);
    d.set_memory_budget(1);
    let before = d.layers(0, SCALE);
    d.layers(1, SCALE);
    let again = d.layers(0, SCALE);
    assert!(!Arc::ptr_eq(&before[1], &again[1]), "it was evicted");
    assert_eq!(before.len(), again.len());
    for (old, new) in before.iter().zip(&again) {
        assert!(pixels(old) == pixels(new), "drawn differently the second time");
    }
}
