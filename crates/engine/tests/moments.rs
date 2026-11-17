//! A slide at a moment: a component that asks which click the slide is on, or what time it
//! is, can make a slide differ from one step to the next and over time.

use std::sync::{Arc, Weak};

use slides_engine::*;
use slides_format::parse_talk;
use toolkit::Registry;

struct NoFiles;

impl TalkFiles for NoFiles {
    fn read(&self, _: &str) -> Option<Vec<u8>> {
        None
    }
}

mod probes {
    use std::cell::Cell;
    use toolkit::prelude::*;

    thread_local! {
        /// How many times `Rows` has been drawn on this thread, which is one test's.
        pub static ROWS_RENDERED: Cell<u32> = const { Cell::new(0) };
    }

    #[component]
    pub fn Count(ctx: &Ctx) -> Element {
        text(ctx.steps(3).to_string())
    }

    #[component]
    pub fn Clock(ctx: &Ctx) -> Element {
        text(ctx.time().to_string())
    }

    /// The same square at every step, red and then blue.
    #[component]
    pub fn Tint(ctx: &Ctx) -> Element {
        let color = if ctx.steps(1) == 0 { "red" } else { "blue" };
        shape(ShapeKind::Rect, 100.0, 100.0).fill(Color::parse(color).unwrap())
    }

    /// Doesn't look at the moment at all.
    #[component]
    pub fn Rows() -> Element {
        ROWS_RENDERED.set(ROWS_RENDERED.get() + 1);
        text("rows")
    }

    /// Nothing before its click, and then a picture.
    #[component]
    pub fn Reveal(ctx: &Ctx, src: String) -> Element {
        if ctx.steps(1) == 0 { text("soon") } else { picture(src, 0.0, 0.0) }
    }

    /// Nothing at all before its click, and then some text.
    #[component]
    pub fn Late(ctx: &Ctx) -> Element {
        if ctx.steps(1) == 0 { Element::Empty } else { text("late") }
    }
}

fn registry() -> Registry {
    let mut registry = builtin_registry();
    registry.add(probes::Count_component());
    registry.add(probes::Clock_component());
    registry.add(probes::Tint_component());
    registry.add(probes::Rows_component());
    registry.add(probes::Reveal_component());
    registry.add(probes::Late_component());
    registry
}

fn deck(source: &str) -> Deck {
    Deck::with_registry(Box::new(NoFiles), Fonts::bundled_only(), source.to_string(), registry())
}

fn at_step(step: u32) -> Moment {
    Moment { step, time: 0.0 }
}

/// Slide 0 of `source` laid out at `moment`.
fn lay_out_at(source: &str, moment: Moment) -> LaidOutSlide {
    let mut fonts = Fonts::bundled_only();
    let talk = parse_talk(source);
    let registry = registry();
    let mut env = LayoutEnv {
        fonts: &mut fonts,
        registry: &registry,
        files: &NoFiles,
        pictures: &mut PictureCache::default(),
        components: &mut TalkComponents::none(),
        moment,
        asks: ComponentAsks::default(),
    };
    layout_slide(&talk, 0, &mut env)
}

fn texts_of(specs: &[LayerSpec]) -> Vec<String> {
    let mut lines = Vec::new();
    for spec in specs {
        for item in &spec.items {
            if let DisplayItem::Text { text, .. } = item {
                lines.extend(text.buffer.lines.iter().map(|line| line.text().trim_end().to_string()));
            }
        }
    }
    lines
}

#[test]
fn a_component_that_asks_for_steps_adds_them_to_the_slide() {
    assert_eq!(deck("<Count />\n").timeline(0).steps, 3);
}

#[test]
fn the_most_clicks_on_the_slide_win() {
    let mut d = deck("<Count />\n\n<Box x=\"0\" y=\"0\" w=\"100\" h=\"100\" click=\"5\" />\n");
    assert_eq!(d.timeline(0).steps, 5);
}

#[test]
fn each_step_shows_what_the_component_gives_at_that_step() {
    let mut d = deck("<Count />\n");
    let first = d.layers_at(0, at_step(0), 0.5);
    let third = d.layers_at(0, at_step(2), 0.5);
    assert_ne!(first[1].pixmap.data(), third[1].pixmap.data());
    assert_eq!(texts_of(&lay_out_at("<Count />\n", at_step(2)).layers), ["2"]);
    assert_eq!(texts_of(&lay_out_at("<Count />\n", at_step(0)).layers), ["0"]);
}

#[test]
fn a_colour_change_between_steps_is_not_mistaken_for_the_same_layer() {
    let mut d = deck("<Tint />\n");
    let red = d.layers_at(0, at_step(0), 0.5);
    let blue = d.layers_at(0, at_step(1), 0.5);
    assert!(!Arc::ptr_eq(&red[1], &blue[1]));
    assert_ne!(red[1].pixmap.data(), blue[1].pixmap.data());
}

#[test]
fn what_does_not_vary_keeps_its_image_between_steps() {
    let mut d = deck("# Heading\n\n<Count />\n");
    let first = d.layers_at(0, at_step(0), 0.5);
    let second = d.layers_at(0, at_step(1), 0.5);
    assert!(Arc::ptr_eq(&first[1], &second[1]), "the heading");
    assert!(!Arc::ptr_eq(&first[2], &second[2]), "the count");
}

#[test]
fn the_same_moment_is_not_drawn_twice() {
    let mut d = deck("# Heading\n\n<Count />\n");
    let once = d.layers_at(0, at_step(2), 0.5);
    let twice = d.layers_at(0, at_step(2), 0.5);
    assert!(once.iter().zip(&twice).all(|(a, b)| Arc::ptr_eq(a, b)));
}

#[test]
fn a_slide_with_nothing_varying_is_laid_out_once() {
    let mut d = deck("<Rows />\n\n<Box x=\"0\" y=\"0\" w=\"100\" h=\"100\" click=\"2\" />\n");
    let before = probes::ROWS_RENDERED.get();
    let layers: Vec<_> = (0..3).map(|step| d.layers_at(0, Moment { step, time: step as f32 }, 0.5)).collect();
    d.specs(0);
    assert_eq!(probes::ROWS_RENDERED.get() - before, 1);
    assert!(layers.windows(2).all(|pair| pair[0].iter().zip(&pair[1]).all(|(a, b)| Arc::ptr_eq(a, b))));
    assert!(!d.timeline(0).animated);
}

#[test]
fn the_whole_slide_is_its_last_step() {
    let mut d = deck("<Count />\n");
    // Another moment first, so that the whole slide isn't simply the last one laid out.
    let _ = d.layers_at(0, at_step(1), 0.5);
    assert_eq!(texts_of(d.specs(0)), ["3"]);
    let _ = d.layers_at(0, at_step(2), 0.5);
    let (specs, _, _) = d.specs_fonts_and_files(0);
    assert_eq!(texts_of(specs), ["3"]);
    let whole = d.layers(0, 0.5);
    let last = d.layers_at(0, at_step(3), 0.5);
    assert!(whole.iter().zip(&last).all(|(a, b)| Arc::ptr_eq(a, b)));
}

#[test]
fn the_time_reaches_the_component() {
    assert_eq!(texts_of(&lay_out_at("<Clock />\n", Moment { step: 0, time: 1.5 }).layers), ["1.5"]);
    let mut d = deck("<Clock />\n");
    let start = d.layers_at(0, Moment::START, 0.5);
    let later = d.layers_at(0, Moment { step: 0, time: 1.5 }, 0.5);
    assert_ne!(start[1].pixmap.data(), later[1].pixmap.data());
}

#[test]
fn a_slide_that_moves_says_so() {
    let mut d = deck("<Clock />\n");
    let timeline = d.timeline(0);
    assert!(timeline.animated);
    assert_eq!(timeline.steps, 0, "a time isn't a click");
    assert_eq!(d.varying_tags(0), ["Clock"]);
    assert!(!deck("<Count />\n").timeline(0).animated);
}

#[test]
fn a_slide_that_moves_is_still_at_time_zero_for_an_export() {
    let mut d = deck("<Clock />\n");
    // Another moment first, so that time 0 isn't simply the last one laid out.
    let _ = d.layers_at(0, Moment { step: 0, time: 1.5 }, 0.5);
    assert_eq!(texts_of(d.specs(0)), ["0"]);
    assert!(d.notices_whole(0).is_empty());
}

#[test]
fn the_components_that_vary_are_named_once_each() {
    let mut d = deck("<Count />\n\n<Rows />\n\n<Count />\n\n<Tint />\n");
    assert_eq!(d.varying_tags(0), ["Count", "Tint"]);
    assert!(deck("<Rows />\n").varying_tags(0).is_empty());
}

#[test]
fn a_layer_says_whether_it_varies() {
    let slide = lay_out_at("# Heading\n\n<Count />\n", Moment::START);
    assert_eq!(slide.layers.iter().map(|layer| layer.varies).collect::<Vec<_>>(), [false, false, true]);
    assert_eq!(slide.asks, ComponentAsks { steps: 3, stepped: true, animated: false, tags: vec!["Count".into()] });
}

const PICTURE_BYTES: usize = 256 * 256 * 4;

fn png(tint: u8) -> Vec<u8> {
    let mut pixmap = tiny_skia::Pixmap::new(256, 256).unwrap();
    pixmap.fill(tiny_skia::Color::from_rgba8(tint, 255 - tint, 40, 255));
    pixmap.encode_png().unwrap()
}

/// Slide `index`'s picture in its whole state, seen in a way that doesn't keep it in memory.
fn picture_on(deck: &mut Deck, index: usize) -> Weak<tiny_skia::Pixmap> {
    let drawn = deck.specs(index).iter().flat_map(|spec| &spec.items).find_map(|item| match item {
        DisplayItem::Picture { pixmap, .. } => Some(Arc::downgrade(pixmap)),
        _ => None,
    });
    drawn.expect("the slide has a picture")
}

#[test]
fn pictures_held_at_a_moment_are_let_go_within_the_budget() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("p0.png"), png(0)).unwrap();
    std::fs::write(dir.path().join("p1.png"), png(1)).unwrap();
    let files = FolderFiles::new(dir.path().to_path_buf());
    let source = "<Reveal src=\"p0.png\" />\n\n---\n\n![](p1.png)\n";
    let mut d = Deck::with_registry(Box::new(files), Fonts::bundled_only(), source.to_string(), registry());
    d.set_picture_budget(PICTURE_BYTES);

    let revealed = picture_on(&mut d, 0);
    assert!(revealed.upgrade().is_some(), "the slide's last step shows it");
    picture_on(&mut d, 1);
    assert!(revealed.upgrade().is_none(), "walking on let it go");
    assert!(d.picture_bytes() <= PICTURE_BYTES, "{}", d.picture_bytes());

    let back = picture_on(&mut d, 0).upgrade().expect("it's there again");
    assert_eq!((back.width(), back.height()), (256, 256));
}

#[test]
fn timings_at_a_moment_match_its_layers() {
    let source = "---\nregion.body: click=each\n---\n\n# T\n\nIntro\n\n<Late />\n\nOutro\n";
    let mut d = deck(source);
    let start = d.timings_at(0, at_step(0));
    assert_eq!(start.len(), d.layers_at(0, at_step(0), 0.5).len());
    let later = d.timings_at(0, at_step(1));
    assert_eq!(later.len(), d.layers_at(0, at_step(1), 0.5).len());
    assert_ne!(start.len(), later.len(), "the component's piece comes and goes");
    let clicks = |timings: &[LayerTiming]| timings.iter().map(|timing| timing.click).collect::<Vec<_>>();
    assert_eq!(clicks(&later).last(), Some(&Some(3)), "a piece more, and a click more for the last");
    assert_eq!(clicks(&start), clicks(&d.timeline(0).layers), "the start is the timeline's");
}

#[test]
fn fixed_steps_counts_a_component_that_steps() {
    assert_eq!(deck("<Count />\n").fixed_steps(0), 3);
    assert_eq!(deck("---\nregion.body: click=each\n---\n\n# T\n\n- one\n- two\n\n<Count x=\"0\" y=\"0\" w=\"99\" h=\"99\" />\n").fixed_steps(0), 3);
    assert_eq!(deck("<Rows />\n").fixed_steps(0), 0);
}
