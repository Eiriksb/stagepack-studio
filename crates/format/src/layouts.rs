//! The layouts a slide can name, and the regions each one has. The engine gives them
//! their places and looks; this is the one list of their names.

/// A layout's name, its regions in order, and the region for what nothing sends elsewhere.
#[derive(Debug, PartialEq, Eq)]
pub struct LayoutNames {
    pub name: &'static str,
    pub regions: &'static [&'static str],
    pub main: &'static str,
}

/// Every layout, in the order a picker offers them: title, content, two-columns, section, blank.
pub const LAYOUT_NAMES: [LayoutNames; 5] = [
    LayoutNames { name: "title", regions: &["title", "body"], main: "body" },
    LayoutNames { name: "content", regions: &["title", "body"], main: "body" },
    LayoutNames { name: "two-columns", regions: &["title", "left", "right"], main: "left" },
    LayoutNames { name: "section", regions: &["title", "body"], main: "body" },
    LayoutNames { name: "blank", regions: &["body"], main: "body" },
];

/// The layout a slide has without a `layout:` setting.
pub const DEFAULT_LAYOUT: &str = "content";

/// The layout called `name`, if there is one.
pub fn layout_names(name: &str) -> Option<&'static LayoutNames> {
    LAYOUT_NAMES.iter().find(|layout| layout.name == name)
}
