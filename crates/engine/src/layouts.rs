//! The layouts a slide can name, and the regions each one has.

use crate::{Align, Rect};

/// A named place on the slide, and how text in it looks.
#[derive(Clone, Copy)]
pub struct RegionDef {
    pub name: &'static str,
    pub rect: Rect,
    pub size: f32,
    pub bold: bool,
    pub align: Align,
}

pub struct LayoutDef {
    pub name: &'static str,
    pub regions: &'static [RegionDef],
    /// The region for content that nothing sends elsewhere.
    pub main: &'static str,
}

const fn title(x: f32, y: f32, w: f32, h: f32) -> RegionDef {
    RegionDef { name: "title", rect: Rect { x, y, w, h }, size: 64.0, bold: true, align: Align::Left }
}

const fn body(name: &'static str, x: f32, y: f32, w: f32, h: f32) -> RegionDef {
    RegionDef { name, rect: Rect { x, y, w, h }, size: 40.0, bold: false, align: Align::Left }
}

const fn big_title(x: f32, y: f32, w: f32, h: f32) -> RegionDef {
    RegionDef { name: "title", rect: Rect { x, y, w, h }, size: 96.0, bold: true, align: Align::Center }
}

const fn subtitle(x: f32, y: f32, w: f32, h: f32) -> RegionDef {
    RegionDef { name: "body", rect: Rect { x, y, w, h }, size: 44.0, bold: false, align: Align::Center }
}

/// In the order `layouts` gives them.
static LAYOUTS: [LayoutDef; 5] = [
    LayoutDef {
        name: "title",
        regions: &[big_title(160.0, 340.0, 1600.0, 220.0), subtitle(160.0, 580.0, 1600.0, 180.0)],
        main: "body",
    },
    LayoutDef {
        name: "content",
        regions: &[title(100.0, 60.0, 1720.0, 140.0), body("body", 100.0, 220.0, 1720.0, 780.0)],
        main: "body",
    },
    LayoutDef {
        name: "two-columns",
        regions: &[
            title(100.0, 60.0, 1720.0, 140.0),
            body("left", 100.0, 220.0, 840.0, 780.0),
            body("right", 980.0, 220.0, 840.0, 780.0),
        ],
        main: "left",
    },
    LayoutDef {
        name: "section",
        regions: &[big_title(160.0, 420.0, 1600.0, 240.0), subtitle(160.0, 680.0, 1600.0, 200.0)],
        main: "body",
    },
    LayoutDef {
        name: "blank",
        regions: &[body("body", 0.0, 0.0, 1920.0, 1080.0)],
        main: "body",
    },
];

/// Every layout, in the order a picker offers them: title, content, two-columns, section, blank.
pub fn layouts() -> &'static [LayoutDef] {
    &LAYOUTS
}

pub fn layout_by_name(name: &str) -> Option<&'static LayoutDef> {
    LAYOUTS.iter().find(|layout| layout.name == name)
}
