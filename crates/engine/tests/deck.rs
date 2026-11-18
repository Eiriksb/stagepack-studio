mod common;
use slides_engine::*;
use std::sync::{Arc, Weak};
use proptest::prelude::*;

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
fn text_aligned_anew_is_drawn_anew() {
    // Seen in the real window: centring a region kept its words drawn at the left, as the
    // layers were kept by where the text was and its size, which aligning doesn't change.
    let left = "# One\n\n---\n\n# Title\n\nBody\n";
    let centred = "# One\n\n---\nregion.body: align=center\n---\n\n# Title\n\nBody\n";
    let mut d = deck(left);
    let before = d.layers(1, 0.5);
    d.update_text(slides_format::parse_talk(centred));
    let after = d.layers(1, 0.5);
    let fresh = deck(centred).layers(1, 0.5);
    assert_eq!(after.len(), fresh.len());
    assert_ne!(before[2].pixmap.data(), fresh[2].pixmap.data(), "centring moves the body");
    for (after, fresh) in after.iter().zip(&fresh) {
        assert_eq!((after.x, after.y), (fresh.x, fresh.y));
        assert!(after.pixmap.data() == fresh.pixmap.data(), "drawn as the centred talk is drawn");
    }
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

/// The talk's look and title with `settings` in its first slide's settings block.
fn talk_wide(settings: &[(&str, &str)]) -> (toolkit::Theme, Option<String>) {
    let lines: String = settings.iter().map(|(key, value)| format!("{key}: {value}\n")).collect();
    let deck = deck(&format!("---\n{lines}---\n\n# One\n\n---\n\n# Two\n"));
    (talk_theme(deck.talk()), deck.title())
}

#[test]
fn the_talk_wide_settings_are_the_ones_the_engine_reads_as_talk_wide() {
    let none = talk_wide(&[]);
    // Each key the engine reads from the first slide for the whole talk is one the format
    // crate keeps with whichever slide is first, and the other way round.
    let read = [
        ("title", "A talk"),
        ("color.background", "#123456"),
        ("color.text", "#654321"),
        ("color.accent", "#abcdef"),
        ("font", "Some Font"),
        ("font.mono", "Some Mono"),
    ];
    for (key, value) in read {
        assert!(slides_format::TALK_SETTINGS.contains(&key), "{key} isn't kept as the talk's");
        assert_ne!(talk_wide(&[(key, value)]), none, "{key} isn't read");
    }
    assert_eq!(read.len(), slides_format::TALK_SETTINGS.len(), "a talk-wide key the engine doesn't read");
    // Any other key changes neither the look nor the title.
    let others = [
        "layout", "hidden", "transition", "region.body", "region.title", "click", "background", "color", "color.link", "color.heading",
        "color.title", "color.code", "font.heading", "font.title", "font.body", "font.size", "size", "theme", "accent", "text", "author",
        "date", "logo", "subtitle", "aspect", "footer",
    ];
    for key in others {
        assert!(!slides_format::TALK_SETTINGS.contains(&key), "{key}");
        for value in ["#ff0000", "Inter", "true", "A talk"] {
            assert_eq!(talk_wide(&[(key, value)]), none, "{key}: {value} changed the talk's look or title");
        }
    }
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
            target.draw_pixmap(layer.x, layer.y, tiny_skia::Pixmap::as_ref(&layer.pixmap), &tiny_skia::PixmapPaint::default(), tiny_skia::Transform::identity(), None);
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

const PICTURE_SIDE: u32 = 256;
/// What one of the pictures below takes in memory.
const PICTURE_BYTES: usize = 256 * 256 * 4;

fn png_of_side(side: u32, tint: u8) -> Vec<u8> {
    let mut pixmap = tiny_skia::Pixmap::new(side, side).unwrap();
    pixmap.fill(tiny_skia::Color::from_rgba8(tint, 255 - tint, 40, 255));
    pixmap.encode_png().unwrap()
}

/// A talk with one slide for each of `count` pictures.
fn talk_of_pictures(count: usize) -> String {
    (0..count).map(|index| format!("![](p{index}.png)\n")).collect::<Vec<_>>().join("\n---\n\n")
}

/// A deck showing `source`, in a folder holding `count` pictures named as in `talk_of_pictures`.
fn deck_of_pictures_showing(source: &str, count: usize) -> (tempfile::TempDir, Deck) {
    let dir = tempfile::tempdir().unwrap();
    for index in 0..count {
        std::fs::write(dir.path().join(format!("p{index}.png")), png_of_side(PICTURE_SIDE, index as u8)).unwrap();
    }
    let files = FolderFiles::new(dir.path().to_path_buf());
    let deck = Deck::open(Box::new(files), Fonts::bundled_only(), source.to_string());
    (dir, deck)
}

fn deck_of_pictures(count: usize) -> (tempfile::TempDir, Deck) {
    deck_of_pictures_showing(&talk_of_pictures(count), count)
}

/// Slide `index`'s picture, seen in a way that doesn't keep it in memory.
fn picture_on(deck: &mut Deck, index: usize) -> Weak<tiny_skia::Pixmap> {
    let drawn = deck.specs(index).iter().flat_map(|spec| &spec.items).find_map(|item| match item {
        DisplayItem::Picture { pixmap, .. } => Some(Arc::downgrade(pixmap)),
        _ => None,
    });
    drawn.expect("the slide has a picture")
}

fn bytes_still_held(pictures: &[Weak<tiny_skia::Pixmap>]) -> usize {
    pictures.iter().filter_map(Weak::upgrade).map(|picture| picture.data().len()).sum()
}

#[test]
fn walking_a_talk_of_large_pictures_stays_within_the_budget() {
    let (_dir, mut deck) = deck_of_pictures(16);
    let budget = 4 * PICTURE_BYTES;
    deck.set_picture_budget(budget);
    let mut seen = Vec::new();
    for index in 0..16 {
        seen.push(picture_on(&mut deck, index));
        assert!(deck.picture_bytes() <= budget, "slide {index}: {}", deck.picture_bytes());
        assert!(bytes_still_held(&seen) <= budget, "slide {index}: {}", bytes_still_held(&seen));
    }
}

#[test]
fn the_slide_asked_for_keeps_its_pictures_however_much_they_take() {
    let (_dir, mut deck) = deck_of_pictures_showing("![](p0.png)\n\n![](p1.png)\n\n---\n\n![](p2.png)\n", 3);
    deck.set_picture_budget(1);
    let first = picture_on(&mut deck, 0);
    assert!(first.upgrade().is_some());
    let second = picture_on(&mut deck, 1);
    assert!(second.upgrade().is_some());
    assert!(first.upgrade().is_none(), "nothing else is worth keeping over budget");
}

#[test]
fn the_slides_furthest_from_the_one_asked_for_are_let_go_first() {
    let (_dir, mut deck) = deck_of_pictures(5);
    deck.set_picture_budget(3 * PICTURE_BYTES);
    let p0 = picture_on(&mut deck, 0);
    let p1 = picture_on(&mut deck, 1);
    let p2 = picture_on(&mut deck, 2);
    let p4 = picture_on(&mut deck, 4);
    assert!(p0.upgrade().is_none(), "slide 0 is the furthest from slide 4");
    let p3 = picture_on(&mut deck, 3);
    assert!(p1.upgrade().is_none(), "slide 1 is the furthest from slide 3");
    assert!(p2.upgrade().is_some() && p3.upgrade().is_some() && p4.upgrade().is_some());
}

#[test]
fn a_slide_walked_away_from_lays_out_again() {
    let (_dir, mut deck) = deck_of_pictures_showing("<Nonesuch />\n\n![](p0.png)\n\n---\n\n![](p1.png)\n", 2);
    deck.set_picture_budget(PICTURE_BYTES);
    let notices = deck.notices(0);
    assert!(!notices.is_empty());
    let first = deck.layers(0, 0.5);
    let timeline = deck.timeline(0);

    let gone = picture_on(&mut deck, 0);
    picture_on(&mut deck, 1);
    assert!(gone.upgrade().is_none(), "walking on let it go");

    assert_eq!(deck.notices(0), notices, "its notices are back");
    assert_eq!(deck.timeline(0).layers.len(), timeline.layers.len());
    let back = picture_on(&mut deck, 0).upgrade().expect("it's there again");
    assert_eq!((back.width(), back.height()), (PICTURE_SIDE, PICTURE_SIDE));
    let again = deck.layers(0, 0.5);
    assert_eq!(again.len(), first.len());
    assert!(first.iter().zip(&again).all(|(a, b)| Arc::ptr_eq(a, b)), "what was drawn is still good");
}

#[test]
fn a_picture_on_many_slides_counts_once() {
    let (_dir, mut deck) = deck_of_pictures_showing("![](p0.png)\n\n---\n\n![](p0.png)\n\n---\n\n![](p0.png)\n", 1);
    deck.set_picture_budget(PICTURE_BYTES);
    let pictures = [picture_on(&mut deck, 0), picture_on(&mut deck, 1), picture_on(&mut deck, 2)];
    assert_eq!(deck.picture_bytes(), PICTURE_BYTES);
    assert!(pictures.iter().all(|picture| picture.upgrade().is_some()));
}

#[test]
fn pictures_no_slide_holds_are_let_go_oldest_first() {
    let source = talk_of_pictures(3);
    let (_dir, mut deck) = deck_of_pictures(3);
    deck.set_picture_budget(2 * PICTURE_BYTES);
    let older = picture_on(&mut deck, 0);
    let newer = picture_on(&mut deck, 1);
    // Their slides change, so that only the cache is left holding the two pictures.
    deck.update(source.replace("![](p0.png)", "# A\n\n![](p0.png)").replace("![](p1.png)", "# B\n\n![](p1.png)"));
    assert!(older.upgrade().is_some() && newer.upgrade().is_some());

    let third = picture_on(&mut deck, 2);

    assert!(older.upgrade().is_none(), "used longest ago");
    assert!(newer.upgrade().is_some() && third.upgrade().is_some());
}

/// Puts a picture of another size over `p{index}.png`, which no stamp could mistake for the old one.
fn replace_picture(dir: &tempfile::TempDir, index: usize) {
    std::fs::write(dir.path().join(format!("p{index}.png")), png_of_side(PICTURE_SIDE / 2, 200)).unwrap();
}

#[test]
fn a_picture_let_go_and_then_replaced_is_still_noticed() {
    let (dir, mut deck) = deck_of_pictures(2);
    deck.set_picture_budget(PICTURE_BYTES);
    let gone = picture_on(&mut deck, 0);
    picture_on(&mut deck, 1);
    assert!(gone.upgrade().is_none());

    replace_picture(&dir, 0);

    assert_eq!(deck.update(talk_of_pictures(2)), vec![0, 1], "every slide is reported");
    let back = picture_on(&mut deck, 0).upgrade().expect("the new picture");
    assert_eq!(back.width(), PICTURE_SIDE / 2);
}

#[test]
fn a_picture_replaced_and_read_again_before_the_save_is_still_noticed() {
    let (dir, mut deck) = deck_of_pictures(2);
    deck.set_picture_budget(PICTURE_BYTES);
    picture_on(&mut deck, 0);
    picture_on(&mut deck, 1);

    replace_picture(&dir, 0);
    picture_on(&mut deck, 0);

    assert_eq!(deck.update(talk_of_pictures(2)), vec![0, 1], "drawn layers of the old picture have to go");
}

const ONE_HIDDEN: &str = "# One\n\n---\nhidden: true\n---\n\n# Two\n\n---\n\n# Three\n";

#[test]
fn a_hidden_slide_is_left_out_of_a_show() {
    let d = deck(ONE_HIDDEN);
    assert!(d.is_hidden(1));
    assert!(!d.is_hidden(0) && !d.is_hidden(2));
    assert_eq!(d.shown_slides(), [0, 2]);
    assert_eq!(d.slide_count(), 3, "the slide is still in the talk");
}

#[test]
fn a_talk_with_every_slide_hidden_still_shows_them() {
    let d = deck("---\nhidden: true\n---\n\n# One\n\n---\nhidden: true\n---\n\n# Two\n");
    assert!(d.is_hidden(0) && d.is_hidden(1));
    assert_eq!(d.shown_slides(), [0, 1]);
}

/// A slide with one red box covering the middle of it.
const RED_BOX: &str = "<Box x=\"600\" y=\"300\" w=\"720\" h=\"480\" fill=\"#ff0000\" />\n";

fn pixel(pixmap: &tiny_skia::Pixmap, x: u32, y: u32) -> [u8; 4] {
    let p = pixmap.pixel(x, y).unwrap();
    [p.red(), p.green(), p.blue(), p.alpha()]
}

#[test]
fn a_thumbnail_is_the_whole_slide_at_the_width_asked_for() {
    let mut d = deck(RED_BOX);
    let thumbnail = d.thumbnail(0, 320).unwrap();
    assert_eq!((thumbnail.width(), thumbnail.height()), (320, 180));
    assert_eq!(pixel(&thumbnail, 160, 90), [255, 0, 0, 255], "inside the box");
    assert_ne!(pixel(&thumbnail, 5, 5), [255, 0, 0, 255], "outside it");
    assert_eq!(pixel(&thumbnail, 5, 5)[3], 255, "the theme's background is drawn, so it's opaque");
}

#[test]
fn a_thumbnail_is_kept_until_its_slide_changes() {
    let mut d = deck(TALK);
    let (one, two) = (d.thumbnail(0, 160).unwrap(), d.thumbnail(1, 160).unwrap());
    assert!(Arc::ptr_eq(&one, &d.thumbnail(0, 160).unwrap()));
    assert_eq!(d.update(TALK.replace("Two", "Changed")), vec![1]);
    assert!(Arc::ptr_eq(&one, &d.thumbnail(0, 160).unwrap()), "slide 0 didn't change");
    let again = d.thumbnail(1, 160).unwrap();
    assert!(!Arc::ptr_eq(&two, &again), "slide 1 did");
    assert!(Arc::ptr_eq(&again, &d.thumbnail(1, 160).unwrap()));
}

#[test]
fn a_thumbnail_at_another_width_is_drawn_again() {
    let mut d = deck(TALK);
    let small = d.thumbnail(0, 160).unwrap();
    let large = d.thumbnail(0, 320).unwrap();
    assert_eq!((small.width(), large.width()), (160, 320));
    assert_eq!(d.thumbnail(0, 320).unwrap().width(), 320);
}

#[test]
fn thumbnails_stay_within_the_memory_budget() {
    let mut d = deck(&long_talk(20));
    let one = d.thumbnail(0, 160).unwrap().data().len();
    let budget = 3 * one;
    d.set_memory_budget(budget);
    for index in 0..20 {
        d.thumbnail(index, 160);
        let drawn = d.drawn_bytes();
        assert!(drawn >= one && drawn <= budget, "slide {index}: {drawn} bytes drawn with a budget of {budget}");
    }
}

#[test]
fn a_thumbnail_counts_against_the_drawn_layers_budget() {
    let mut d = deck(TALK);
    assert_eq!(d.drawn_bytes(), 0);
    let thumbnail = d.thumbnail(0, 160).unwrap();
    assert!(d.drawn_bytes() >= thumbnail.data().len());
}

#[test]
fn a_thumbnail_of_a_slide_that_builds_shows_its_last_step() {
    let source = "<Box x=\"600\" y=\"300\" w=\"720\" h=\"480\" fill=\"#ff0000\" click=\"2\" />\n";
    let mut d = deck(source);
    assert_eq!(pixel(&d.thumbnail(0, 320).unwrap(), 160, 90), [255, 0, 0, 255]);
}

#[test]
fn a_thumbnail_of_a_slide_that_is_not_there_is_none() {
    let mut d = deck(TALK);
    assert!(d.thumbnail(3, 160).is_none());
    assert!(d.thumbnail(usize::MAX, 160).is_none());
    assert!(d.thumbnail(2, 160).is_some(), "the last slide is there");
}

#[test]
fn a_long_pass_of_thumbnails_gives_way_before_the_slide_on_screen() {
    let mut d = deck(&long_talk(30));
    let on_screen = d.layers(0, SCALE);
    let thumbnail = d.thumbnail(1, 160).unwrap().data().len();
    let budget = bytes(&on_screen[0]) + bytes(&on_screen[1]) + 3 * thumbnail;
    d.set_memory_budget(budget);
    for index in 0..30 {
        d.thumbnail(index, 160);
        assert!(d.drawn_bytes() <= budget, "slide {index}: {} bytes with a budget of {budget}", d.drawn_bytes());
    }
    assert!(Arc::ptr_eq(&on_screen[1], &d.layers(0, SCALE)[1]), "the slide on screen kept its layers");
}

#[test]
fn the_slides_a_show_goes_through_are_worked_out_once_per_update() {
    let mut d = deck(ONE_HIDDEN);
    let (first, again) = (d.shown_slides(), d.shown_slides());
    assert_eq!(first.as_ptr(), again.as_ptr(), "the same list, not a new one");
    assert_eq!(d.shown_slides(), [0, 2]);
    d.update(ONE_HIDDEN.replace("hidden: true", "hidden: false"));
    assert_eq!(d.shown_slides(), [0, 1, 2]);
}

#[test]
fn asking_for_a_thumbnail_already_drawn_draws_nothing() {
    let mut d = deck(TALK);
    assert!(d.drawn_thumbnail(0, 160).is_none(), "none drawn yet");
    assert_eq!(d.drawn_bytes(), 0, "and asking drew none");
    let drawn = d.thumbnail(0, 160).unwrap();
    assert!(Arc::ptr_eq(&drawn, &d.drawn_thumbnail(0, 160).unwrap()));
    assert!(d.drawn_thumbnail(0, 320).is_none(), "not at that width");
    assert!(d.drawn_thumbnail(7, 160).is_none(), "no such slide");
    d.update(TALK.replace("One", "Changed"));
    assert!(d.drawn_thumbnail(0, 160).is_none(), "the slide changed");
}

#[test]
fn a_slide_keeps_its_version_until_it_changes() {
    let mut d = deck(TALK);
    let before: Vec<_> = (0..3).map(|index| d.slide_version(index).unwrap()).collect();
    assert!(d.slide_version(3).is_none());
    assert_eq!(before.iter().collect::<std::collections::HashSet<_>>().len(), 3, "each its own");
    d.update(TALK.replace("Two", "Changed"));
    assert_eq!(d.slide_version(0), Some(before[0]));
    assert_ne!(d.slide_version(1), Some(before[1]));
    assert_eq!(d.slide_version(2), Some(before[2]));
    // Taking a slide out moves the ones after it to where its text differs.
    d.update("# One\n\n---\n\n# Three\n".to_string());
    assert_eq!(d.slide_version(0), Some(before[0]));
    assert_ne!(d.slide_version(1), Some(before[2]));
}

#[test]
fn a_slide_added_in_the_middle_keeps_the_others_drawn() {
    let mut d = deck(TALK);
    let before = d.layers(2, SCALE);
    let version = d.slide_version(2);
    let changed = d.update_text(slides_format::parse_talk(&TALK.replace("---\n\n# Two", "---\n\n# New\n\n---\n\n# Two")));
    assert_eq!(changed, vec![1, 2, 3], "the slides whose place now holds other text");
    assert_eq!(d.slide_version(3), version);
    assert!(Arc::ptr_eq(&before[1], &d.layers(3, SCALE)[1]));
}

/// Files that count how often they're asked anything.
#[derive(Clone, Default)]
struct CountingFiles(Arc<std::sync::atomic::AtomicUsize>);

impl CountingFiles {
    fn asked(&self) -> usize { self.0.load(std::sync::atomic::Ordering::SeqCst) }
    fn ask(&self) { self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst); }
}

impl TalkFiles for CountingFiles {
    fn read(&self, _: &str) -> Option<Vec<u8>> { self.ask(); Some(one_colour_png(tiny_skia::Color::BLACK)) }
    fn stamp(&self, _: &str) -> Option<(u64, std::time::SystemTime)> { self.ask(); None }
    fn len(&self, _: &str) -> Option<u64> { self.ask(); None }
    fn names_in(&self, _: &str) -> Vec<String> { self.ask(); Vec::new() }
}

#[test]
fn an_edit_made_in_the_app_reads_no_files() {
    let files = CountingFiles::default();
    let mut d = Deck::open(Box::new(files.clone()), Fonts::bundled_only(), PICTURE_TALK.to_string());
    d.layers(0, SCALE);
    let asked = files.asked();
    assert!(asked > 0, "opening and drawing the talk read its picture");
    d.update_text(slides_format::parse_talk(&PICTURE_TALK.replace("Two", "Changed")));
    assert_eq!(files.asked(), asked);
}

struct SomeFiles(Vec<(&'static str, Vec<u8>)>);
impl TalkFiles for SomeFiles {
    fn read(&self, path: &str) -> Option<Vec<u8>> { self.0.iter().find(|(name, _)| *name == path).map(|(_, bytes)| bytes.clone()) }
}

fn blank_png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::new(width, height).write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).unwrap();
    bytes
}

#[test]
fn a_new_pictures_size_fits_and_keeps_its_shape() {
    let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"200\" height=\"100\"></svg>".to_vec();
    let files = vec![("wide.png", blank_png(4000, 1000)), ("small.png", blank_png(100, 50)), ("tall.png", blank_png(300, 1080)), ("bad.png", b"not a picture".to_vec()), ("drawn.svg", svg)];
    let mut d = Deck::open(Box::new(SomeFiles(files)), Fonts::bundled_only(), "# One\n".to_string());
    let picture = |d: &mut Deck, src: &str| d.default_size("Picture", &[("src", src)]);
    assert_eq!(picture(&mut d, "wide.png"), (960.0, 240.0));
    assert_eq!(picture(&mut d, "small.png"), (100.0, 50.0), "a small picture isn't made bigger");
    assert_eq!(picture(&mut d, "tall.png"), (150.0, 540.0));
    assert_eq!(picture(&mut d, "drawn.svg"), (200.0, 100.0));
    assert_eq!(picture(&mut d, "bad.png"), (640.0, 360.0), "unreadable");
    assert_eq!(picture(&mut d, "missing.png"), (640.0, 360.0));
    assert_eq!(d.default_size("Picture", &[]), (640.0, 360.0));
    assert_eq!(d.default_size("Icon", &[("name", "star"), ("size", "96")]), (96.0, 96.0));
    assert_eq!(d.default_size("Icon", &[("name", "star")]), (64.0, 64.0), "an icon's own size");
    assert_eq!(d.default_size("Icon", &[("size", "nonsense")]), (64.0, 64.0));
    assert_eq!(d.default_size("Icon", &[("size", "1e30")]), (3840.0, 3840.0), "held to the most a box can be");
    assert_eq!(d.default_size("Box", &[]), (640.0, 360.0));
    assert_eq!(d.default_size("Chart", &[("kind", "bar")]), (640.0, 360.0));
}

#[test]
fn a_picture_outside_the_talks_folder_is_not_measured() {
    let dir = tempfile::tempdir().unwrap();
    let talk = dir.path().join("talk");
    std::fs::create_dir_all(talk.join("assets")).unwrap();
    std::fs::write(dir.path().join("outside.png"), blank_png(100, 50)).unwrap();
    std::fs::write(talk.join("assets/inside.png"), blank_png(100, 50)).unwrap();
    let mut d = Deck::open(Box::new(FolderFiles::new(talk.clone())), Fonts::bundled_only(), "# One\n".to_string());
    assert_eq!(d.default_size("Picture", &[("src", "assets/inside.png")]), (100.0, 50.0), "one in the folder is");
    assert_eq!(d.default_size("Picture", &[("src", "../outside.png")]), (640.0, 360.0));
    let absolute = dir.path().join("outside.png");
    assert_eq!(d.default_size("Picture", &[("src", absolute.to_str().unwrap())]), (640.0, 360.0));
}

const PLACED_TALK: &str = "# Title\n\nBody\n\n<Box x=\"100\" y=\"600\" w=\"300\" h=\"200\" fill=\"#ff0000\" />\n\n<Box x=\"1200\" y=\"600\" w=\"300\" h=\"200\" fill=\"#0000ff\" />\n\n---\n\n# Two\n";

/// The source of the top-level block of slide 0 that starts with `text`.
fn top_block(d: &Deck, text: &str) -> std::ops::Range<usize> {
    let at = d.source().find(text).unwrap_or_else(|| panic!("no {text}"));
    d.talk().slides[0].blocks.iter().map(|block| block.range().clone()).find(|range| range.start == at).expect("a block starts there")
}

fn layer_of(d: &mut Deck, source: &std::ops::Range<usize>) -> usize {
    d.specs(0).iter().position(|spec| spec.source.as_ref() == Some(source)).expect("a layer for it")
}

fn place(target: slides_format::Target, x: f32, y: f32, w: f32, h: f32) -> Placing { Placing { target, rect: Rect { x, y, w, h }, rotate: 0.0 } }

fn same_pixels(a: &[Arc<Layer>], b: &[Arc<Layer>]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| (a.x, a.y) == (b.x, b.y) && a.pixmap.data() == b.pixmap.data())
}

#[test]
fn a_placing_shows_without_changing_the_source() {
    let mut d = deck(PLACED_TALK);
    let red = top_block(&d, "<Box x=\"100\"");
    let before = d.editor_layers(0, SCALE);
    let thumbnail = d.thumbnail(0, 320).unwrap();
    let (source, version) = (d.source().to_string(), d.slide_version(0));
    let old = d.text_layout(0).element(&red).unwrap().rect;
    assert_eq!(old, Rect { x: 100.0, y: 600.0, w: 300.0, h: 200.0 });

    let moved = Rect { x: 500.0, y: 300.0, w: 300.0, h: 200.0 };
    d.set_placing(0, vec![Placing { target: slides_format::Target::Tag(red.clone()), rect: moved, rotate: 0.0 }]);
    assert_eq!(d.placing().map(|(slide, placing)| (slide, placing.len())), Some((0, 1)));
    let layer = layer_of(&mut d, &red);
    assert_eq!(d.specs(0)[layer].rect, moved);
    assert_eq!(d.text_layout(0).element(&red).unwrap().rect, moved);
    assert_eq!(d.text_layout(0).elements().iter().find(|element| element.source == red).unwrap().rect, moved);
    let shown = d.editor_layers(0, SCALE);
    assert_ne!((shown[layer].x, shown[layer].y), (before[layer].x, before[layer].y));
    assert_eq!((d.source(), d.slide_version(0)), (source.as_str(), version));
    assert!(Arc::ptr_eq(&thumbnail, &d.thumbnail(0, 320).unwrap()), "the thumbnail is the source's");
    let steps = d.timeline(0).steps;
    assert!(same_pixels(&d.layers_at(0, Moment { step: steps, time: 0.0 }, SCALE), &before[..shown.len()]), "a moment is the source's");

    d.clear_placing();
    assert_eq!(d.placing(), None);
    assert_eq!(d.specs(0)[layer].rect, old);
    assert_eq!(d.text_layout(0).element(&red).unwrap().rect, old);
    assert!(same_pixels(&d.editor_layers(0, SCALE), &before));
}

#[test]
fn a_placing_of_a_region_moves_its_text() {
    let mut d = deck(PLACED_TALK);
    let flows = slides_format::text_flows(d.talk(), 0);
    let body = flows.iter().position(|flow| flow.place == slides_format::FlowPlace::Region("body".into())).unwrap();
    let start = slides_format::Pos { para: 0, offset: 0 };
    let before = d.text_layout(0).caret(body, start).unwrap();
    let region = d.text_layout(0).region("body").unwrap().rect;
    let target = || slides_format::Target::Region("body".into());

    d.set_placing(0, vec![place(target(), region.x + 50.0, region.y + 30.0, region.w, region.h)]);
    let after = d.text_layout(0).caret(body, start).unwrap();
    assert_eq!((after.x, after.top), (before.x + 50.0, before.top + 30.0));
    assert_eq!(d.text_layout(0).region("body").unwrap().rect.x, region.x + 50.0);
    let body_source = top_block(&d, "Body");
    let layer = d.specs(0).iter().position(|spec| spec.source.as_ref().is_some_and(|s| s.start == body_source.start)).unwrap();
    assert_eq!(d.specs(0)[layer].rect.y, region.y + 30.0);
    d.clear_placing();
    let before = d.editor_layers(0, SCALE);
    d.set_placing(0, vec![place(target(), region.x + 50.0, region.y + 30.0, region.w, region.h)]);
    assert!(Arc::ptr_eq(&before[layer].pixmap, &d.editor_layers(0, SCALE)[layer].pixmap), "only moved");

    // An empty region's offer moves with it.
    let mut empty = deck("# Title\n");
    let before = empty.editor_layers(0, SCALE);
    let (rect, _) = empty.text_layout(0).placeholder("body").unwrap();
    empty.set_placing(0, vec![place(target(), rect.x + 100.0, rect.y, rect.w, rect.h)]);
    assert_eq!(empty.text_layout(0).placeholder("body").unwrap().0.x, rect.x + 100.0);
    let after = empty.editor_layers(0, SCALE);
    let (was, now) = (before.last().unwrap(), after.last().unwrap());
    assert!(Arc::ptr_eq(&was.pixmap, &now.pixmap));
    assert_eq!(now.x, was.x + 50);

    // Made narrower, its text is laid out again in the new width.
    d.set_placing(0, vec![place(target(), 300.0, 300.0, 600.0, 400.0)]);
    assert_eq!(d.text_layout(0).region("body").unwrap().rect, Rect { x: 300.0, y: 300.0, w: 600.0, h: 400.0 });
    assert_eq!(d.text_layout(0).caret(body, start).unwrap().x, 300.0);
    let fresh = deck(&PLACED_TALK.replacen("# Title", "---\nregion.body: x=300 y=300 w=600 h=400\n---\n\n# Title", 1)).editor_layers(0, SCALE);
    assert!(same_pixels(&d.editor_layers(0, SCALE), &fresh), "as if the source said so");
}

#[test]
fn an_update_clears_the_placing() {
    let mut d = deck(PLACED_TALK);
    let red = top_block(&d, "<Box x=\"100\"");
    let placing = || vec![place(slides_format::Target::Tag(red.clone()), 0.0, 0.0, 300.0, 200.0)];
    d.set_placing(0, placing());
    d.update(PLACED_TALK.replace("Two", "Three"));
    assert_eq!(d.placing(), None);
    d.set_placing(0, placing());
    d.update_parsed(slides_format::parse_talk(PLACED_TALK));
    assert_eq!(d.placing(), None);
    d.set_placing(0, placing());
    d.update_text(slides_format::parse_talk(PLACED_TALK));
    assert_eq!(d.placing(), None);
    d.set_placing(0, placing());
    d.set_files(Box::new(NoFiles));
    assert_eq!(d.placing(), None);
    assert_eq!(d.text_layout(0).element(&red).unwrap().rect.x, 100.0);
}

#[test]
fn a_moved_layer_is_not_drawn_again() {
    let mut d = deck(PLACED_TALK);
    let red = top_block(&d, "<Box x=\"100\"");
    let layer = layer_of(&mut d, &red);
    let before = d.editor_layers(0, SCALE);
    for step in 1..4 {
        let shift = 40.0 * step as f32;
        d.set_placing(0, vec![place(slides_format::Target::Tag(red.clone()), 100.0 + shift, 600.0 + shift, 300.0, 200.0)]);
        let moved = d.editor_layers(0, SCALE);
        assert!(Arc::ptr_eq(&before[layer].pixmap, &moved[layer].pixmap), "the same picture");
        assert_eq!((moved[layer].x, moved[layer].y), (before[layer].x + 20 * step, before[layer].y + 20 * step));
        for other in (0..before.len()).filter(|other| *other != layer) {
            assert!(Arc::ptr_eq(&before[other], &moved[other]), "layer {other} is as it was");
        }
    }
    d.set_placing(0, vec![place(slides_format::Target::Tag(red.clone()), 100.0, 600.0, 400.0, 200.0)]);
    let resized = d.editor_layers(0, SCALE);
    assert!(!Arc::ptr_eq(&before[layer].pixmap, &resized[layer].pixmap), "a new size is drawn again");
    assert!(resized[layer].pixmap.width() > before[layer].pixmap.width());
    d.set_placing(0, vec![Placing { target: slides_format::Target::Tag(red.clone()), rect: Rect { x: 100.0, y: 600.0, w: 300.0, h: 200.0 }, rotate: 30.0 }]);
    assert!(!Arc::ptr_eq(&before[layer].pixmap, &d.editor_layers(0, SCALE)[layer].pixmap), "and so is a new turn");
    d.clear_placing();
    let back = d.editor_layers(0, SCALE);
    assert!(before.iter().zip(&back).all(|(a, b)| Arc::ptr_eq(a, b)), "the source's layers are kept");
}

#[test]
fn a_placing_for_a_missing_target_is_ignored() {
    let source = "# Title\n\n<Box fill=\"#dbeafe\">\n\nIn the text\n\n</Box>\n\n<Box x=\"100\" y=\"600\" w=\"300\" h=\"200\" fill=\"#ff0000\" />\n";
    let mut d = deck(source);
    let in_text = top_block(&d, "<Box fill");
    let before = d.editor_layers(0, SCALE);
    let layout = d.text_layout(0);
    let targets = [
        slides_format::Target::Tag(0..3),
        slides_format::Target::Tag(in_text),
        slides_format::Target::Tag(0..source.len() + 10),
        slides_format::Target::Region("left".into()),
        slides_format::Target::Region(String::new()),
    ];
    for target in targets {
        d.set_placing(0, vec![place(target.clone(), 700.0, 100.0, 50.0, 50.0)]);
        let after = d.editor_layers(0, SCALE);
        assert!(before.iter().zip(&after).all(|(a, b)| Arc::ptr_eq(a, b)) && before.len() == after.len(), "{target:?}");
        assert_eq!(*d.text_layout(0), *layout, "{target:?}");
    }
    d.set_placing(7, vec![place(slides_format::Target::Region("body".into()), 0.0, 0.0, 1.0, 1.0)]);
    assert!(same_pixels(&d.editor_layers(0, SCALE), &before));
}

fn any_number() -> impl Strategy<Value = f32> {
    prop_oneof![
        Just(f32::NAN),
        Just(f32::INFINITY),
        Just(f32::NEG_INFINITY),
        Just(0.0),
        Just(-0.0),
        Just(f32::MAX),
        Just(f32::MIN),
        -10_000.0f32..10_000.0,
        any::<f32>(),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn placing_never_panics(target in 0usize..6, x in any_number(), y in any_number(), w in any_number(), h in any_number(), rotate in any_number()) {
        let source = "---\nregion.body: click=each\n---\n\n# Title\n\n- one\n- two\n\n<Box x=\"100\" y=\"600\" w=\"300\" h=\"200\" fill=\"#ff0000\">\n\nIn it\n\n</Box>\n\n<Icon x=\"900\" y=\"600\" name=\"star\" />\n";
        let mut d = deck(source);
        let targets = [
            slides_format::Target::Tag(top_block(&d, "<Box")),
            slides_format::Target::Tag(top_block(&d, "<Icon")),
            slides_format::Target::Region("body".into()),
            slides_format::Target::Region("title".into()),
            slides_format::Target::Region("left".into()),
            slides_format::Target::Tag(std::ops::Range { start: 5, end: 1 }),
        ];
        d.editor_layers(0, 0.1);
        d.set_placing(0, vec![Placing { target: targets[target].clone(), rect: Rect { x, y, w, h }, rotate }]);
        d.specs(0);
        d.editor_layers(0, 0.1);
        let layout = d.text_layout(0);
        prop_assert_eq!(layout.regions().len(), 2);
        for region in layout.regions() {
            prop_assert!([region.rect.x, region.rect.y, region.rect.w, region.rect.h, region.rotate].iter().all(|n| n.is_finite()));
        }
        for element in layout.elements() {
            prop_assert!([element.rect.x, element.rect.y, element.rect.w, element.rect.h, element.rotate].iter().all(|n| n.is_finite()));
        }
        d.clear_placing();
        d.editor_layers(0, 0.1);
    }
}

#[test]
fn a_one_at_a_time_area_only_moved_is_laid_out_as_written() {
    let talk = |line: &str| format!("---\nregion.body: {line}\n---\n\n# Title\n\n- one\n- two\n- three\n");
    let mut d = deck(&talk("click=each"));
    let region = d.text_layout(0).region("body").unwrap().rect;
    d.set_placing(0, vec![place(slides_format::Target::Region("body".into()), region.x + 60.0, region.y + 40.0, region.w, region.h)]);
    let placed = d.text_layout(0);
    let mut fresh = deck(&talk(&format!("click=each x={} y={}", region.x + 60.0, region.y + 40.0)));
    let written = fresh.text_layout(0);
    assert_eq!(placed.regions().iter().map(|region| region.rect).collect::<Vec<_>>(), written.regions().iter().map(|region| region.rect).collect::<Vec<_>>());
    assert_eq!(placed.paras(), written.paras());
    assert_eq!(placed.elements(), written.elements());
    for y in (0..1080).step_by(8) {
        for x in (0..1920).step_by(16) {
            let point = (x as f32, y as f32);
            assert_eq!(placed.hit(point.0, point.1), written.hit(point.0, point.1), "{point:?}");
        }
    }
}

#[test]
fn many_small_moves_end_where_one_large_move_ends() {
    let target = |d: &Deck| slides_format::Target::Tag(top_block(d, "<Box x=\"100\""));
    // Nothing drawn at this scale before the placing, so every picture shown is one drawn with it.
    let mut small = deck(PLACED_TALK);
    let red = top_block(&small, "<Box x=\"100\"");
    let layer = layer_of(&mut small, &red);
    for step in 0..=20 {
        small.set_placing(0, vec![place(target(&small), 100.0 + 0.45 * step as f32, 600.0 + 0.45 * step as f32, 300.0, 200.0)]);
        small.layers(0, SCALE);
    }
    let mut large = deck(PLACED_TALK);
    large.set_placing(0, vec![place(target(&large), 100.0, 600.0, 300.0, 200.0)]);
    large.layers(0, SCALE);
    large.set_placing(0, vec![place(target(&large), 109.0, 609.0, 300.0, 200.0)]);
    let (small, large) = (small.layers(0, SCALE), large.layers(0, SCALE));
    assert_eq!((small[layer].x, small[layer].y), (large[layer].x, large[layer].y));

    // A thing made bigger and then only moved under one drag is shown from where it was drawn.
    let mut d = deck(PLACED_TALK);
    d.set_placing(0, vec![place(target(&d), 100.0, 600.0, 320.0, 200.0)]);
    let drawn = d.layers(0, SCALE)[layer].clone();
    for step in 1..=20 {
        d.set_placing(0, vec![place(target(&d), 100.0 + 0.45 * step as f32, 600.0, 320.0, 200.0)]);
        d.layers(0, SCALE);
    }
    let moved = d.layers(0, SCALE)[layer].clone();
    assert!(Arc::ptr_eq(&drawn.pixmap, &moved.pixmap));
    assert_eq!(moved.x, drawn.x + (9.0 * SCALE).round() as i32);
}

#[test]
fn a_placing_on_one_slide_leaves_the_others_alone() {
    let mut d = deck(PLACED_TALK);
    let mut fresh = deck(PLACED_TALK);
    let red = top_block(&d, "<Box x=\"100\"");
    d.set_placing(0, vec![place(slides_format::Target::Tag(red), 500.0, 300.0, 300.0, 200.0)]);
    let rects = |d: &mut Deck| d.specs(1).iter().map(|spec| spec.rect).collect::<Vec<_>>();
    assert_eq!(rects(&mut d), rects(&mut fresh));
    assert!(same_pixels(&d.editor_layers(1, SCALE), &fresh.editor_layers(1, SCALE)));
    assert_eq!(*d.text_layout(1), *fresh.text_layout(1));
    assert_eq!(d.placing().map(|(slide, _)| slide), Some(0), "asking for another slide keeps it");
}

#[test]
fn a_thumbnail_first_drawn_under_a_placing_shows_the_source() {
    let mut d = deck(PLACED_TALK);
    let red = top_block(&d, "<Box x=\"100\"");
    d.set_placing(0, vec![place(slides_format::Target::Tag(red), 500.0, 300.0, 300.0, 200.0)]);
    d.layers(0, SCALE);
    let thumbnail = d.thumbnail(0, 320).unwrap();
    assert_eq!(thumbnail.data(), deck(PLACED_TALK).thumbnail(0, 320).unwrap().data());
}
