//! What a talk may hold, for an AI to read before it writes one: the layouts and their regions,
//! the settings of a talk and of a slide, what every placed thing takes, and every tag with its
//! settings. The built-ins' come from the form panel's own table, so what's said of a setting
//! here is what the form shows.

use serde::Serialize;
use slides_engine::{PLACEMENT_ATTRS, builtin_registry, layouts};
use slides_format::{DEFAULT_LAYOUT, TALK_SETTINGS};
use toolkit::{PropField, PropKind};

use super::talk::OpenTalk;
use crate::inspector::labels;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LayoutInfo {
    pub name: String,
    pub regions: Vec<String>,
    /// The region that content nothing sends elsewhere goes in.
    pub main: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SettingInfo {
    pub name: String,
    pub meaning: String,
    /// Every value it takes, where it takes only some.
    pub values: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FieldInfo {
    pub name: String,
    /// `text`, `whole number`, `number`, `yes or no`, `colour`, `file` or `choice`.
    pub kind: String,
    /// Whether the talk can't do without it. Any other left out is filled in with its default.
    pub needed: bool,
    /// What it is when left out; none for a needed or empty one.
    pub default: Option<String>,
    /// What the form panel calls it; none for a setting the form leaves out.
    pub label: Option<String>,
    pub meaning: Option<String>,
    /// For a file, the extensions it may have; for a choice, the values; otherwise none.
    pub choices: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TagInfo {
    pub tag: String,
    /// Whether every talk has it, or it is one of this talk's own components.
    pub built_in: bool,
    /// Whether what's written between its tags is used.
    pub takes_content: bool,
    pub settings: Vec<FieldInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Description {
    pub layouts: Vec<LayoutInfo>,
    pub talk_settings: Vec<SettingInfo>,
    pub slide_settings: Vec<SettingInfo>,
    pub placement: Vec<SettingInfo>,
    pub tags: Vec<TagInfo>,
}

/// The order the built-ins are listed in.
const BUILT_IN_ORDER: [&str; 8] = ["Shape", "Box", "Icon", "Picture", "Video", "Model", "Terminal", "Chart"];

fn setting(name: &str, meaning: &str, values: Option<&[&str]>) -> SettingInfo {
    SettingInfo { name: name.to_string(), meaning: meaning.to_string(), values: values.map(|all| all.iter().map(ToString::to_string).collect()) }
}

fn talk_setting(name: &str) -> SettingInfo {
    let meaning = match name {
        "title" => "The talk's name, set in the first slide's settings.",
        "color.background" => "The colour behind every slide, as `#rrggbb`, set in the first slide's settings.",
        "color.text" => "The colour of text, as `#rrggbb`, set in the first slide's settings.",
        "color.accent" => "The colour charts and other accents use, as `#rrggbb`, set in the first slide's settings.",
        "font" => "The name of the font text is set in, one the talk carries in its fonts folder, set in the first slide's settings.",
        "font.mono" => "The name of the font code is set in, one the talk carries in its fonts folder, set in the first slide's settings.",
        _ => "A setting of the whole talk, set in the first slide's settings.",
    };
    setting(name, meaning, None)
}

fn slide_settings() -> Vec<SettingInfo> {
    let names: Vec<&str> = layouts().iter().map(|layout| layout.name).collect();
    vec![
        setting("layout", &format!("Which layout the slide has; `{DEFAULT_LAYOUT}` when it names none."), Some(&names)),
        setting("transition", "How the slide arrives when presenting.", Some(&["none", "fade", "push"])),
        setting("hidden", "`true` leaves the slide out of the show and the exports; it stays in the text.", Some(&["true"])),
        setting(
            "region.NAME",
            "Places the text of the region called NAME by the words `x=`, `y=`, `w=`, `h=`, `rotate=`, `click=`, `enter=` and `align=`, as a placed thing's settings do.",
            None,
        ),
    ]
}

fn placement() -> Vec<SettingInfo> {
    PLACEMENT_ATTRS
        .iter()
        .map(|name| match *name {
            "x" => setting(name, "How far from the slide's left edge the thing starts, in slide units (the slide is 1920 by 1080).", None),
            "y" => setting(name, "How far from the slide's top edge the thing starts, in slide units.", None),
            "w" => setting(name, "How wide the thing is, in slide units.", None),
            "h" => setting(name, "How tall the thing is, in slide units.", None),
            "rotate" => setting(name, "How far the thing is turned, in degrees, clockwise.", None),
            "click" => setting(name, "The click the thing first shows on, from 1 to 999, or `each` to show a line at a time; without it, it shows from the start.", None),
            "enter" => setting(name, "How the thing arrives on its click.", Some(&["appear", "fade", "fly-up"])),
            "align" => setting(name, "Where text sits across its width.", Some(&["left", "center", "right"])),
            other => setting(other, "A setting every placed thing takes.", None),
        })
        .collect()
}

/// How `kind` reads in the description.
fn kind_name(kind: &PropKind) -> &'static str {
    match kind {
        PropKind::Text => "text",
        PropKind::Whole => "whole number",
        PropKind::Number => "number",
        PropKind::Flag => "yes or no",
        PropKind::Color => "colour",
        PropKind::File(_) => "file",
        PropKind::Choice(_) => "choice",
        _ => "text",
    }
}

fn field(tag: &str, prop: &PropField, label: Option<String>, meaning: Option<&str>, colour: bool, built_in: bool) -> FieldInfo {
    let choices = match &prop.kind {
        PropKind::File(extensions) => extensions.clone(),
        PropKind::Choice(values) => values.clone(),
        _ => Vec::new(),
    };
    let needed = built_in && labels::needed(tag, &prop.name);
    let default = match &prop.kind {
        _ if needed || prop.optional => None,
        PropKind::Whole | PropKind::Number => Some("0".to_string()),
        PropKind::Flag => Some("false".to_string()),
        PropKind::Choice(values) => values.first().cloned(),
        _ => None,
    };
    let kind = if colour { "colour" } else { kind_name(&prop.kind) };
    FieldInfo { name: prop.name.clone(), kind: kind.to_string(), needed, default, label, meaning: meaning.map(str::to_string), choices }
}

/// A tag's settings: a built-in's as the form shows them, in its order, then those it leaves
/// out; a talk's own as it gives them.
fn fields(tag: &str, props: &[PropField], built_in: bool) -> Vec<FieldInfo> {
    let shown = labels::ordered(tag, props);
    let mut out = Vec::new();
    for (shown, label) in &shown {
        let Some(prop) = props.iter().find(|prop| prop.name == shown.name) else { continue };
        let meaning = built_in.then(|| labels::meaning(tag, &prop.name)).flatten();
        out.push(field(tag, prop, Some(label.clone()), meaning, shown.kind == PropKind::Color, built_in));
    }
    for prop in props {
        if !shown.iter().any(|(shown, _)| shown.name == prop.name) {
            let meaning = built_in.then(|| labels::meaning(tag, &prop.name)).flatten();
            out.push(field(tag, prop, None, meaning, false, built_in));
        }
    }
    out
}

/// What a talk may hold: and, given one, its own components too.
pub fn describe(talk: Option<&mut OpenTalk>) -> Description {
    let layouts = layouts()
        .iter()
        .map(|layout| LayoutInfo {
            name: layout.name.to_string(),
            regions: layout.regions.iter().map(|region| region.name.to_string()).collect(),
            main: layout.main.to_string(),
        })
        .collect();
    let registry = builtin_registry();
    let mut tags: Vec<TagInfo> = BUILT_IN_ORDER
        .iter()
        .filter_map(|tag| {
            let schema = (registry.get(tag)?.schema)();
            Some(TagInfo { tag: tag.to_string(), built_in: true, takes_content: schema.children, settings: fields(tag, &schema.props, true) })
        })
        .collect();
    if let Some(talk) = talk {
        let own: Vec<String> = talk.deck.components().iter().map(|info| info.tag.clone()).collect();
        for tag in own {
            let schema = talk.deck.component_schema(&tag);
            let (takes_content, props) = schema.map_or((false, Vec::new()), |schema| (schema.children, schema.props));
            tags.push(TagInfo { tag: tag.clone(), built_in: false, takes_content, settings: fields(&tag, &props, false) });
        }
    }
    Description {
        layouts,
        talk_settings: TALK_SETTINGS.iter().map(|name| talk_setting(name)).collect(),
        slide_settings: slide_settings(),
        placement: placement(),
        tags,
    }
}

fn settings_text(heading: &str, settings: &[SettingInfo], text: &mut String) {
    text.push_str(&format!("{heading}\n"));
    for item in settings {
        text.push_str(&format!("  {} — {}", item.name, item.meaning));
        if let Some(values) = &item.values {
            text.push_str(&format!(" One of: {}.", values.join(", ")));
        }
        text.push('\n');
    }
    text.push('\n');
}

/// `described` as plain text: a heading for each part and a line for each item.
pub fn describe_text(described: &Description) -> String {
    let mut text = String::from("Layouts\n");
    for layout in &described.layouts {
        text.push_str(&format!("  {} — regions {}; content goes in {}\n", layout.name, layout.regions.join(", "), layout.main));
    }
    text.push('\n');
    settings_text("Talk settings", &described.talk_settings, &mut text);
    settings_text("Slide settings", &described.slide_settings, &mut text);
    settings_text("Placement, which every placed tag takes", &described.placement, &mut text);
    text.push_str("Tags\n  A setting left out takes its default, or is empty, unless it is marked needed.\n");
    for tag in &described.tags {
        let own = if tag.built_in { "" } else { ", this talk's own" };
        let content = if tag.takes_content { ", takes content between its tags" } else { "" };
        text.push_str(&format!("  <{}{}{}>\n", tag.tag, own, content));
        for item in &tag.settings {
            let needed = if item.needed { ", needed" } else { "" };
            text.push_str(&format!("    {} ({}{needed})", item.name, item.kind));
            if let Some(meaning) = &item.meaning {
                text.push_str(&format!(" — {meaning}"));
            }
            if let Some(default) = &item.default {
                text.push_str(&format!(" Default {default}."));
            }
            if !item.choices.is_empty() {
                let which = if item.kind == "file" { "File types" } else { "One of" };
                text.push_str(&format!(" {which}: {}.", item.choices.join(", ")));
            }
            text.push('\n');
        }
    }
    text
}
