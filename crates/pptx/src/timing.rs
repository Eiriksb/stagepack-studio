//! Slide timing: native click effects and slide transitions, in the form PowerPoint for the web
//! plays. A port of `timing.mjs` from `slidev-pptx-export`, whose forms were measured there:
//! each animation on a text shape needs a `grpId` of its own and a `p:bldP`, or the web drops
//! the second one. Videos are not ported, there are none here.

use std::collections::BTreeMap;

use slides_engine::{Enter, Transition};

const FADE_MS: u32 = 400;
const FLY_MS: u32 = 500;

/// One layer coming in on a click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effect {
    /// The id of the shape on the slide.
    pub spid: u32,
    /// The shape is a text box or a shape that holds text.
    pub is_text: bool,
    /// The shape also has a fill or an outline, which comes in with its text.
    pub has_fill: bool,
    /// The click it comes in on, counted from 1. Effects of one click play together, in order.
    pub click: u32,
    pub enter: Enter,
}

fn target(spid: u32) -> String {
    format!("<p:tgtEl><p:spTgt spid=\"{spid}\"/></p:tgtEl>")
}

struct Writer {
    n: u32,
    /// How many effects each text shape has had so far, which is its next `grpId`.
    groups: BTreeMap<u32, u32>,
    builds: Vec<String>,
}

impl Writer {
    fn next(&mut self) -> u32 {
        let id = self.n;
        self.n += 1;
        id
    }

    /// The `grpId` attribute of a text shape's next effect, and its `p:bldP`.
    fn group_of(&mut self, e: &Effect) -> String {
        if !e.is_text {
            return String::new();
        }
        let group = self.groups.entry(e.spid).or_insert(0);
        let g = *group;
        *group += 1;
        let animate_bg = if e.has_fill { " animBg=\"1\"" } else { "" };
        self.builds.push(format!("<p:bldP spid=\"{}\" grpId=\"{g}\"{animate_bg}/>", e.spid));
        format!(" grpId=\"{g}\"")
    }

    fn visibility(&mut self, spid: u32) -> String {
        let id = self.next();
        format!(
            "<p:set><p:cBhvr><p:cTn id=\"{id}\" dur=\"1\" fill=\"hold\"><p:stCondLst><p:cond delay=\"0\"/></p:stCondLst></p:cTn>{}\
             <p:attrNameLst><p:attrName>style.visibility</p:attrName></p:attrNameLst></p:cBhvr>\
             <p:to><p:strVal val=\"visible\"/></p:to></p:set>",
            target(spid)
        )
    }

    fn fade(&mut self, spid: u32) -> String {
        let id = self.next();
        format!(
            "<p:animEffect transition=\"in\" filter=\"fade\"><p:cBhvr><p:cTn id=\"{id}\" dur=\"{FADE_MS}\"/>{}</p:cBhvr></p:animEffect>",
            target(spid)
        )
    }

    fn slide(&mut self, spid: u32, attr: &str, from: &str, to: &str) -> String {
        let id = self.next();
        format!(
            "<p:anim calcmode=\"lin\" valueType=\"num\"><p:cBhvr additive=\"base\"><p:cTn id=\"{id}\" dur=\"{FLY_MS}\" fill=\"hold\"/>{}\
             <p:attrNameLst><p:attrName>{attr}</p:attrName></p:attrNameLst></p:cBhvr>\
             <p:tavLst><p:tav tm=\"0\"><p:val><p:strVal val=\"{from}\"/></p:val></p:tav>\
             <p:tav tm=\"100000\"><p:val><p:strVal val=\"{to}\"/></p:val></p:tav></p:tavLst></p:anim>",
            target(spid)
        )
    }

    fn effect(&mut self, e: &Effect, node_type: &str) -> String {
        let id = self.next();
        let (preset, subtype) = match e.enter {
            Enter::Appear => (1, 0),
            Enter::Fade => (10, 0),
            Enter::FlyUp => (2, 4),
        };
        let mut body = self.visibility(e.spid);
        match e.enter {
            Enter::Appear => {}
            Enter::Fade => body.push_str(&self.fade(e.spid)),
            Enter::FlyUp => {
                body.push_str(&self.slide(e.spid, "ppt_x", "#ppt_x", "#ppt_x"));
                body.push_str(&self.slide(e.spid, "ppt_y", "1+#ppt_h/2", "#ppt_y"));
            }
        }
        let group = self.group_of(e);
        format!(
            "<p:par><p:cTn id=\"{id}\" presetID=\"{preset}\" presetClass=\"entr\" presetSubtype=\"{subtype}\" fill=\"hold\"{group} nodeType=\"{node_type}\">\
             <p:stCondLst><p:cond delay=\"0\"/></p:stCondLst><p:childTnLst>{body}</p:childTnLst></p:cTn></p:par>"
        )
    }

    /// One click: an outer step waiting for it, holding the effects that play together.
    fn step(&mut self, effects: &[&Effect]) -> String {
        let outer = self.next();
        let held = self.next();
        let inner: String = effects
            .iter()
            .enumerate()
            .map(|(i, e)| self.effect(e, if i == 0 { "clickEffect" } else { "withEffect" }))
            .collect();
        format!(
            "<p:par><p:cTn id=\"{outer}\" fill=\"hold\"><p:stCondLst><p:cond delay=\"indefinite\"/></p:stCondLst><p:childTnLst>\
             <p:par><p:cTn id=\"{held}\" fill=\"hold\"><p:stCondLst><p:cond delay=\"0\"/></p:stCondLst><p:childTnLst>{inner}</p:childTnLst></p:cTn></p:par>\
             </p:childTnLst></p:cTn></p:par>"
        )
    }
}

/// The slide's timing. Empty when there is nothing to time.
pub fn animation_timing_xml(effects: &[Effect]) -> String {
    if effects.is_empty() {
        return String::new();
    }
    let mut w = Writer { n: 1, groups: BTreeMap::new(), builds: Vec::new() };
    let root = w.next();
    let main = w.next();
    let mut clicks: BTreeMap<u32, Vec<&Effect>> = BTreeMap::new();
    for e in effects {
        clicks.entry(e.click).or_default().push(e);
    }
    let steps: String = clicks.values().map(|list| w.step(list)).collect();
    let seq = format!(
        "<p:seq concurrent=\"1\" nextAc=\"seek\"><p:cTn id=\"{main}\" dur=\"indefinite\" nodeType=\"mainSeq\"><p:childTnLst>{steps}</p:childTnLst></p:cTn>\
         <p:prevCondLst><p:cond evt=\"onPrev\" delay=\"0\"><p:tgtEl><p:sldTgt/></p:tgtEl></p:cond></p:prevCondLst>\
         <p:nextCondLst><p:cond evt=\"onNext\" delay=\"0\"><p:tgtEl><p:sldTgt/></p:tgtEl></p:cond></p:nextCondLst></p:seq>"
    );
    let build_list = if w.builds.is_empty() { String::new() } else { format!("<p:bldLst>{}</p:bldLst>", w.builds.concat()) };
    format!(
        "<p:timing><p:tnLst><p:par><p:cTn id=\"{root}\" dur=\"indefinite\" restart=\"never\" nodeType=\"tmRoot\"><p:childTnLst>{seq}</p:childTnLst></p:cTn></p:par></p:tnLst>{build_list}</p:timing>"
    )
}

/// A slide's transition. Empty for none.
pub fn transition_xml(transition: Transition) -> String {
    match transition {
        Transition::None => String::new(),
        Transition::Fade => "<p:transition><p:fade/></p:transition>".into(),
        Transition::Push => "<p:transition><p:push dir=\"l\"/></p:transition>".into(),
    }
}
