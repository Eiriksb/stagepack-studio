use slides_engine::*;
use slides_format::parse_talk;

struct NoFiles;
impl TalkFiles for NoFiles { fn read(&self, _: &str) -> Option<Vec<u8>> { None } }

fn lay_out(source: &str) -> LaidOutSlide {
    let mut fonts = Fonts::bundled_only();
    let talk = parse_talk(source);
    let registry = builtin_registry();
    layout_slide(&talk, 0, &mut LayoutEnv { fonts: &mut fonts, registry: &registry, files: &NoFiles, pictures: &mut PictureCache::default(), components: &mut TalkComponents::none(), moment: Moment::START, asks: ComponentAsks::default() })
}

fn deck(source: &str) -> Deck { Deck::open(Box::new(NoFiles), Fonts::bundled_only(), source.to_string()) }

#[test]
fn a_tag_appears_on_its_click() {
    let slide = lay_out("<Box x=\"0\" y=\"0\" w=\"100\" h=\"100\" click=\"2\" enter=\"fade\" />\n");
    let layer = slide.layers.last().unwrap();
    assert_eq!((layer.click, layer.enter), (Some(2), Enter::Fade));
}

#[test]
fn layers_with_no_click_are_there_from_the_start() {
    let slide = lay_out("# T\n\nBody\n");
    assert!(slide.layers.iter().all(|l| l.click.is_none() && l.enter == Enter::Appear));
}

#[test]
fn a_region_line_sets_click_and_enter() {
    let slide = lay_out("---\nregion.body: click=1 enter=fly-up\n---\n\n# T\n\nBody\n");
    let body = slide.layers.last().unwrap();
    assert_eq!((body.click, body.enter), (Some(1), Enter::FlyUp));
}

#[test]
fn click_each_gives_every_list_item_its_own_layer() {
    let slide = lay_out("---\nregion.body: click=each\n---\n\n# T\n\n- one\n- two\n- three\n");
    let items = &slide.layers[2..];
    assert_eq!(items.iter().map(|l| l.click).collect::<Vec<_>>(), [Some(1), Some(2), Some(3)]);
    assert!(items[0].rect.y < items[1].rect.y && items[1].rect.y < items[2].rect.y);
    assert_eq!(items[0].rect.x, 100.0);
}

#[test]
fn click_each_counts_blocks_and_items_in_order() {
    let slide = lay_out("---\nregion.body: click=each\n---\n\n# T\n\nIntro\n\n- one\n- two\n");
    assert_eq!(slide.layers[2..].iter().map(|l| l.click).collect::<Vec<_>>(), [Some(1), Some(2), Some(3)]);
}

#[test]
fn a_rotated_region_reveals_as_one_with_a_notice() {
    let slide = lay_out("---\nregion.body: rotate=5 click=each\n---\n\n# T\n\n- one\n- two\n");
    assert_eq!(slide.layers.last().unwrap().click, Some(1));
    assert_eq!(slide.layers.len(), 3);
    assert!(!slide.notices.is_empty());
}

#[test]
fn a_bad_click_is_ignored_with_a_notice() {
    for bad in ["soon", "0", "1000", "-1", "1.5"] {
        let slide = lay_out(&format!("<Box x=\"0\" y=\"0\" w=\"9\" h=\"9\" click=\"{bad}\" />\n"));
        assert_eq!(slide.layers.last().unwrap().click, None, "{bad}");
        assert!(!slide.notices.is_empty(), "{bad}");
    }
}

#[test]
fn an_unknown_entrance_appears_with_a_notice() {
    let slide = lay_out("<Box x=\"0\" y=\"0\" w=\"9\" h=\"9\" click=\"1\" enter=\"spin\" />\n");
    assert_eq!(slide.layers.last().unwrap().enter, Enter::Appear);
    assert!(!slide.notices.is_empty());
}

#[test]
fn the_timeline_lists_steps_and_layers() {
    let mut d = deck("<Box x=\"0\" y=\"0\" w=\"9\" h=\"9\" click=\"2\" />\n\n<Box x=\"9\" y=\"0\" w=\"9\" h=\"9\" click=\"1\" />\n");
    let timeline = d.timeline(0);
    assert_eq!(timeline.steps, 2);
    assert_eq!(timeline.layers.len(), d.layers(0, 1.0).len());
    assert_eq!(timeline.layers[0].click, None, "the background is always there");
}

#[test]
fn transition_is_read_from_the_slide() {
    assert_eq!(deck("---\ntransition: fade\n---\n\n# A\n").timeline(0).transition, Transition::Fade);
    assert_eq!(deck("---\ntransition: push\n---\n\n# A\n").timeline(0).transition, Transition::Push);
    assert_eq!(deck("# A\n").timeline(0).transition, Transition::None);
}

#[test]
fn an_unknown_transition_is_none_with_a_notice() {
    let mut d = deck("---\ntransition: spin\n---\n\n# A\n");
    assert_eq!(d.timeline(0).transition, Transition::None);
    assert!(!d.notices(0).is_empty());
}

#[test]
fn notes_come_from_the_slide() {
    let d = deck("# A\n\n<Notes>\nSay hello.\n</Notes>\n\n---\n\n# B\n");
    assert_eq!(d.notes(0).as_deref(), Some("Say hello."));
    assert_eq!(d.notes(1), None);
}

#[test]
fn click_each_items_with_the_same_text_are_drawn_apart() {
    let mut d = deck("---\nregion.body: click=each\n---\n\n# T\n\n- same\n- same\n");
    let layers = d.layers(0, 0.5);
    assert!(!std::sync::Arc::ptr_eq(&layers[2], &layers[3]));
    assert_ne!((layers[2].x, layers[2].y), (layers[3].x, layers[3].y));
}

#[test]
fn fixed_steps_counts_a_one_at_a_time_list() {
    assert_eq!(deck("---\nregion.body: click=each\n---\n\n# T\n\n- one\n- two\n- three\n").fixed_steps(0), 3);
    assert_eq!(deck("<Box x=\"0\" y=\"0\" w=\"9\" h=\"9\" click=\"2\" />\n\n<Box x=\"9\" y=\"0\" w=\"9\" h=\"9\" click=\"1\" />\n").fixed_steps(0), 0);
    assert_eq!(deck("---\nregion.body: click=2\n---\n\n# T\n\n- one\n- two\n").fixed_steps(0), 0, "a numbered region");
    assert_eq!(deck("# T\n").fixed_steps(5), 0, "a slide the talk doesn't have");
}
