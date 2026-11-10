//! Animations, transitions and notes must be byte-for-byte what the JavaScript exporter wrote for
//! the matching input. `tests/reference/README.md` says which call made each reference file.

use slides_engine::{Enter, Transition};
use slides_pptx::notes::{notes_master_xml, notes_slide_xml};
use slides_pptx::text::TextCtx;
use slides_pptx::timing::{Effect, animation_timing_xml, transition_xml};

fn ctx() -> TextCtx {
    TextCtx { lang: "en-US".into(), link: Box::new(|_| "rId7".into()) }
}

fn effect(spid: u32, enter: Enter) -> Effect {
    Effect { spid, is_text: false, has_fill: false, click: 1, enter }
}

fn effect_on(spid: u32, click: u32) -> Effect {
    Effect { click, ..effect(spid, Enter::Fade) }
}

fn text(spid: u32, click: u32, has_fill: bool) -> Effect {
    Effect { is_text: true, has_fill, ..effect_on(spid, click) }
}

#[test]
fn one_fade_matches_the_reference() {
    let xml = animation_timing_xml(&[effect(4, Enter::Fade)]);
    assert_eq!(xml, include_str!("reference/timing-fade-one.xml"));
}

#[test]
fn a_text_and_a_picture_on_two_clicks_match_the_reference() {
    let xml = animation_timing_xml(&[text(4, 1, false), effect_on(5, 2)]);
    assert_eq!(xml, include_str!("reference/timing-two-clicks.xml"));
}

#[test]
fn text_groups_and_shared_clicks_match_the_reference() {
    let xml = animation_timing_xml(&[text(4, 1, true), effect_on(5, 1), text(4, 2, false), text(6, 1, false)]);
    assert_eq!(xml, include_str!("reference/timing-text-groups.xml"));
}

#[test]
fn transitions_match_the_reference() {
    assert_eq!(transition_xml(Transition::Fade), include_str!("reference/transition-fade.xml"));
    assert_eq!(transition_xml(Transition::Push), include_str!("reference/transition-push.xml"));
    assert_eq!(transition_xml(Transition::None), "");
}

#[test]
fn notes_match_the_reference() {
    assert_eq!(notes_master_xml(&mut ctx()), include_str!("reference/notes-master.xml"));
    let text = "First line\n\n  second & <third>  \n\n\n\n";
    assert_eq!(notes_slide_xml(text, &mut ctx()), include_str!("reference/notes-slide.xml"));
    assert_eq!(notes_slide_xml("\n  \n", &mut ctx()), include_str!("reference/notes-slide-empty.xml"));
}

#[test]
fn nothing_to_time_is_empty() {
    assert_eq!(animation_timing_xml(&[]), "");
}

#[test]
fn appear_only_sets_visibility() {
    let xml = animation_timing_xml(&[effect(1, Enter::Appear)]);
    assert!(xml.contains("presetID=\"1\"") && xml.contains("style.visibility"));
    assert!(!xml.contains("animEffect") && !xml.contains("p:anim "));
}

#[test]
fn fly_up_comes_from_below_the_slide() {
    let xml = animation_timing_xml(&[effect(1, Enter::FlyUp)]);
    assert!(xml.contains("presetID=\"2\"") && xml.contains("presetSubtype=\"4\""));
    assert!(xml.contains("1+#ppt_h/2") && xml.contains("ppt_y"));
    assert_eq!(xml.matches("<p:anim ").count(), 2);
    assert_eq!(xml.matches("dur=\"500\"").count(), 2);
    assert!(xml.find("style.visibility").unwrap() < xml.find("ppt_x").unwrap());
}

#[test]
fn clicks_are_in_order_and_share_a_step() {
    let xml = animation_timing_xml(&[effect_on(5, 2), effect_on(6, 1), effect_on(7, 1)]);
    assert_eq!(xml.matches("nodeType=\"clickEffect\"").count(), 2);
    assert_eq!(xml.matches("nodeType=\"withEffect\"").count(), 1);
    assert!(xml.find("spid=\"6\"").unwrap() < xml.find("spid=\"5\"").unwrap());
}

#[test]
fn every_time_node_has_a_unique_id() {
    let mixed = [text(4, 1, true), effect(5, Enter::FlyUp), effect(6, Enter::Appear), text(4, 3, false), effect_on(7, 3)];
    let xml = animation_timing_xml(&mixed);
    let ids: Vec<u32> = xml
        .split("<p:cTn id=\"")
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap().parse().unwrap())
        .collect();
    assert!(ids.len() > 10);
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "a time node id repeats: {ids:?}");
}
