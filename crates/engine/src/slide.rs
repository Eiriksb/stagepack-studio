//! Puts a parsed slide's content into the regions of its layout.

use slides_format::{Block, FlowPlace, Slide, Tag, Talk, Target};
use tiny_skia::PathBuilder;
use toolkit::{Color, Registry, Theme};

use crate::elements::lay_out_positioned_tag;
use crate::flow::{MAX_NOTICE_CHARS, flow_region, flow_region_each};
use slides_components::shortened;
use crate::timeline::{MAX_CLICK, RegionClick, read_region_timing, read_tag_timing, read_transition};
use crate::native::placeholder_element;
use crate::placeholder::placeholder_items;
use crate::text_geometry::{LayerText, RegionBox};
use crate::{
    Align, ComponentAsks, DisplayItem, Enter, Fonts, LayerSpec, LayoutDef, Moment, Notice, PictureCache, Rect, RegionDef, TalkComponents, TalkFiles, TextRun,
    Transition, layout_by_name, shape_text,
};

/// How wide and high a slide is, in slide units.
pub const SLIDE_WIDTH: f32 = 1920.0;
pub const SLIDE_HEIGHT: f32 = 1080.0;
const DEFAULT_LAYOUT: &str = "content";

/// The attributes that place a component, which it doesn't see as props.
pub const PLACEMENT_ATTRS: [&str; 8] = ["x", "y", "w", "h", "rotate", "click", "enter", "align"];

// The most a talk can ask for in a place on the slide, so one line of a settings block
// can't make a layer that needs gigabytes.
pub const MIN_POSITION: f32 = -3840.0;
pub const MAX_POSITION: f32 = 7680.0;
pub const MAX_SIZE: f32 = 3840.0;
pub const MAX_ROTATION: f32 = 360.0;

/// What's said of an `align` that's given where it does nothing.
pub(crate) const ALIGN_UNPLACED: &str = "Only a box placed on the slide can be aligned, so align is ignored here";

pub struct LayoutEnv<'a> {
    pub fonts: &'a mut Fonts,
    pub registry: &'a Registry,
    pub files: &'a dyn TalkFiles,
    pub pictures: &'a mut PictureCache,
    pub components: &'a mut TalkComponents,
    /// The moment components are told the slide is at.
    pub moment: Moment,
    /// What the components drawn so far looked at.
    pub asks: ComponentAsks,
}

pub struct LaidOutSlide {
    pub layers: Vec<LayerSpec>,
    pub notices: Vec<Notice>,
    /// How the slide is played on arriving at it.
    pub transition: Transition,
    /// What the slide's components looked at.
    pub asks: ComponentAsks,
    /// Where each layer's text is, beside `layers`.
    pub(crate) texts: Vec<LayerText>,
    /// The regions of the slide's layout that have nothing in them.
    pub(crate) empty_regions: Vec<EmptyRegion>,
    /// Every region of the slide's layout, in its order, as placed.
    pub(crate) regions: Vec<RegionBox>,
    /// The most steps a region shown one block at a time takes.
    pub(crate) each_steps: u32,
    /// Each region shown a piece at a time, by name, and the steps it takes.
    pub(crate) each_regions: Vec<(String, u32)>,
}

/// A place to show a thing or a region at that its source doesn't say yet, as while it's
/// dragged: its rect, in slide units before it's turned, and the degrees it's turned by.
#[derive(Clone, Debug, PartialEq)]
pub struct Placing {
    pub target: Target,
    pub rect: Rect,
    pub rotate: f32,
}

/// `value` held within `low` and `high`, or `was` if it isn't a finite number.
fn held(value: f32, was: Option<f32>, low: f32, high: f32) -> Option<f32> {
    if value.is_finite() { Some(value.clamp(low, high)) } else { was }
}

/// Where `placing` puts something its source places at `rect`, turned by `rotate`: each of
/// its numbers held within the limits a talk's are, and one that isn't a finite number left
/// as the source has it.
pub(crate) fn placed_at(rect: Rect, rotate: f32, placing: &Placing) -> (Rect, f32) {
    let to = placing.rect;
    let number = |value, was, low, high| held(value, Some(was), low, high).unwrap_or(was);
    let rect = Rect {
        x: number(to.x, rect.x, MIN_POSITION, MAX_POSITION),
        y: number(to.y, rect.y, MIN_POSITION, MAX_POSITION),
        w: number(to.w, rect.w, 0.0, MAX_SIZE),
        h: number(to.h, rect.h, 0.0, MAX_SIZE),
    };
    (rect, number(placing.rotate, rotate, -MAX_ROTATION, MAX_ROTATION))
}

/// The last of `placing` for region `name`.
fn region_placing<'a>(placing: &'a [Placing], name: &str) -> Option<&'a Placing> {
    placing.iter().rev().find(|placing| matches!(&placing.target, Target::Region(region) if region == name))
}

/// The last of `placing` for the tag whose source is `range`.
fn tag_placing<'a>(placing: &'a [Placing], range: &std::ops::Range<usize>) -> Option<&'a Placing> {
    placing.iter().rev().find(|placing| matches!(&placing.target, Target::Tag(tag) if tag == range))
}

/// A region of the slide's layout with nothing in it, where the editor offers to start text.
#[derive(Clone)]
pub(crate) struct EmptyRegion {
    pub name: &'static str,
    pub rect: Rect,
    pub rotate: f32,
    pub align: Align,
    pub size: f32,
    pub bold: bool,
}

/// Runs `lay_out` with what components look at noted apart, then adds that to the rest,
/// and says whether what it laid out changes with the moment.
fn noting_asks<'e, T>(env: &mut LayoutEnv<'e>, lay_out: impl FnOnce(&mut LayoutEnv<'e>) -> T) -> (T, bool) {
    let before = std::mem::take(&mut env.asks);
    let laid = lay_out(env);
    let asks = std::mem::replace(&mut env.asks, before);
    let varies = asks.varies();
    env.asks.add(asks);
    (laid, varies)
}

/// The talk's look, which its first slide's settings decide.
pub fn talk_theme(talk: &Talk) -> Theme {
    let mut theme = Theme::default();
    let Some(settings) = talk.slides.first().and_then(|slide| slide.settings.as_ref()) else {
        return theme;
    };
    let color = |key: &str, default: Color| settings.get(key).and_then(Color::parse).unwrap_or(default);
    theme.background = color("color.background", theme.background);
    theme.text = color("color.text", theme.text);
    theme.accent = color("color.accent", theme.accent);
    if let Some(font) = settings.get("font").filter(|font| !font.is_empty()) {
        theme.font = font.to_string();
    }
    if let Some(font) = settings.get("font.mono").filter(|font| !font.is_empty()) {
        theme.mono_font = font.to_string();
    }
    theme
}

/// Lays out slide `index`: the background, then each region with content, in layout order.
pub fn layout_slide(talk: &Talk, index: usize, env: &mut LayoutEnv) -> LaidOutSlide {
    layout_slide_placed(talk, index, env, &[])
}

/// As [`layout_slide`], with each placed tag and region of `placing` where it says, as if
/// the source said so. A placing for anything else is ignored.
pub(crate) fn layout_slide_placed(talk: &Talk, index: usize, env: &mut LayoutEnv, placing: &[Placing]) -> LaidOutSlide {
    let slide = &talk.slides[index];
    let theme = talk_theme(talk);
    env.components.new_layout();
    env.asks = ComponentAsks::default();
    let mut notices: Vec<Notice> = slide
        .inline_tags
        .iter()
        .map(|name| Notice(format!("{name} is in the middle of a line. A component goes on a line of its own")))
        .collect();
    let mut layers = vec![background(&theme)];
    let mut texts = vec![LayerText::default()];
    if let Some(error) = &slide.error {
        notices.push(Notice(error.message.clone()));
        layers.push(error_layer(slide, &error.message, &theme, env.fonts));
        texts.push(LayerText { whole: true, ..LayerText::default() });
        let asks = std::mem::take(&mut env.asks);
        return LaidOutSlide { layers, notices, transition: Transition::None, asks, texts, empty_regions: Vec::new(), regions: Vec::new(), each_steps: 0, each_regions: Vec::new() };
    }
    let mut empty_regions = Vec::new();
    let mut regions = Vec::new();
    let mut each_steps = 0;
    let mut each_regions = Vec::new();
    if aligns_inline(&talk.source, &slide.blocks) {
        notices.push(Notice(ALIGN_UNPLACED.to_string()));
    }
    let transition = read_transition(slide.settings.as_ref().and_then(|settings| settings.get("transition")), &mut notices);
    let layout = slide_layout(slide, &mut notices);
    let contents = assign_blocks(slide, layout, &mut notices);

    for (region, blocks) in layout.regions.iter().zip(&contents) {
        let (rect, rotate, moved) = placement(slide, region);
        let (rect, rotate) = region_placing(placing, region.name).map_or((rect, rotate), |placing| placed_at(rect, rotate, placing));
        regions.push(RegionBox {
            name: region.name.to_string(),
            rect,
            rotate,
            moved,
            empty: blocks.is_empty(),
            is_list: blocks.len() > 1 || blocks.iter().any(|block| matches!(block, Block::List { .. })),
        });
        let region = &RegionDef { align: region_align(slide, region, &mut notices), ..*region };
        let (Some(first), Some(last)) = (blocks.first(), blocks.last()) else {
            let RegionDef { name, size, bold, align, .. } = *region;
            empty_regions.push(EmptyRegion { name, rect, rotate, align, size, bold });
            continue;
        };
        let line = region_line(slide, region);
        let timing = read_region_timing(line, &mut notices);
        let source = first.range().start..last.range().end;
        let is_title = region.name == "title";
        let mut click = match timing.click {
            RegionClick::Step(click) => Some(click),
            RegionClick::Each => Some(1),
            RegionClick::Absent => None,
        };
        if timing.click == RegionClick::Each {
            each_steps = each_steps.max(1);
            each_regions.push((region.name.to_string(), 1));
            if rotate != 0.0 {
                notices.push(Notice(format!(
                    "The {} region is rotated, so click=each shows it all on click 1",
                    region.name
                )));
            } else {
                let (pieces, varies) =
                    noting_asks(env, |env| flow_region_each(blocks, region, rect.w, &theme, env, &mut notices));
                if !pieces.is_empty() {
                    let steps = (pieces.len() as u32).min(MAX_CLICK);
                    each_steps = each_steps.max(steps);
                    if let Some((_, taken)) = each_regions.last_mut() {
                        *taken = steps;
                    }
                    for (step, piece) in pieces.into_iter().enumerate() {
                        let rect = Rect { x: rect.x + piece.rect.x, y: rect.y + piece.rect.y, ..piece.rect };
                        layers.push(LayerSpec {
                            source: Some(piece.source),
                            rect,
                            rotate: 0.0,
                            items: piece.items,
                            click: Some((step as u32 + 1).min(MAX_CLICK)),
                            enter: timing.enter,
                            native: piece.native,
                            // A slide has one title, so only the first piece is it.
                            is_title: is_title && step == 0,
                            varies,
                        });
                        texts.push(LayerText { region: Some((region.name, rect)), ..piece.text });
                    }
                    continue;
                }
                click = Some(1);
            }
        }
        let (drawing, varies) = noting_asks(env, |env| flow_region(blocks, region, rect.w, &theme, env, &mut notices));
        let (items, native) = (drawing.items, drawing.native);
        layers.push(LayerSpec { source: Some(source), rect, rotate, items, click, enter: timing.enter, native, is_title, varies });
        texts.push(LayerText { region: Some((region.name, rect)), ..drawing.text });
    }
    for block in &slide.blocks {
        if let Block::Tag(tag) = block
            && is_positioned(tag)
        {
            let placing = tag_placing(placing, &tag.range);
            let ((mut layer, text), varies) = noting_asks(env, |env| positioned_layer(tag, placing, &theme, env, &mut notices));
            layer.varies = varies;
            layers.push(layer);
            texts.push(LayerText { whole: true, ..text });
        }
    }
    let notices = notices.into_iter().map(|notice| Notice(shortened(&notice.0, MAX_NOTICE_CHARS))).collect();
    LaidOutSlide { layers, notices, transition, asks: std::mem::take(&mut env.asks), texts, empty_regions, regions, each_steps, each_regions }
}

/// The layer of the placed tag at the top level of slide `index` whose source is `range`,
/// laid out alone at the place `placing` gives it, and where its text is. `None` if the slide
/// has no such tag.
pub(crate) fn placed_tag_layer(
    talk: &Talk,
    index: usize,
    range: &std::ops::Range<usize>,
    placing: &Placing,
    env: &mut LayoutEnv,
) -> Option<(LayerSpec, LayerText)> {
    let tag = placed_tag(talk.slides.get(index)?, range)?;
    let theme = talk_theme(talk);
    env.components.new_layout();
    let mut notices = Vec::new();
    let ((mut layer, text), varies) = noting_asks(env, |env| positioned_layer(tag, Some(placing), &theme, env, &mut notices));
    layer.varies = varies;
    Some((layer, LayerText { whole: true, ..text }))
}

/// The tag at the top level of `slide` whose source is `range`, if it's placed on the slide.
pub(crate) fn placed_tag<'a>(slide: &'a Slide, range: &std::ops::Range<usize>) -> Option<&'a Tag> {
    if slide.error.is_some() {
        return None;
    }
    slide.blocks.iter().find_map(|block| match block {
        Block::Tag(tag) if tag.range == *range && is_positioned(tag) => Some(tag),
        _ => None,
    })
}

/// The placement numbers a tag gives. One that can't be read counts as absent, and the
/// rest are held within the limits above.
struct TagPlace {
    x: Option<f32>,
    y: Option<f32>,
    w: Option<f32>,
    h: Option<f32>,
    rotate: Option<f32>,
    /// The alignment its text asks for.
    align: Option<Align>,
    click: Option<u32>,
    enter: Enter,
    /// What was wrong with the `click`, `enter` and `align` it gave.
    notices: Vec<Notice>,
}

impl TagPlace {
    fn of(tag: &Tag) -> TagPlace {
        let mut notices = Vec::new();
        let (click, enter) = read_tag_timing(tag.attr("click"), tag.attr("enter"), &mut notices);
        let number = |name: &str, low: f32, high: f32| {
            let value = tag.attr(name)?.trim().parse::<f32>().ok().filter(|value| value.is_finite())?;
            Some(value.clamp(low, high))
        };
        TagPlace {
            x: number("x", MIN_POSITION, MAX_POSITION),
            y: number("y", MIN_POSITION, MAX_POSITION),
            w: number("w", 0.0, MAX_SIZE),
            h: number("h", 0.0, MAX_SIZE),
            rotate: number("rotate", -MAX_ROTATION, MAX_ROTATION),
            align: tag.attr("align").and_then(|value| read_align(value, &format!("The {} box", tag.name), &mut notices)),
            click,
            enter,
            notices,
        }
    }
}

/// Whether a tag asks for a place on the slide, which gives it a layer of its own.
pub fn is_positioned(tag: &Tag) -> bool {
    let place = TagPlace::of(tag);
    place.x.is_some() || place.y.is_some() || place.w.is_some() || place.h.is_some()
}

fn positioned_layer(tag: &Tag, placing: Option<&Placing>, theme: &Theme, env: &mut LayoutEnv, notices: &mut Vec<Notice>) -> (LayerSpec, LayerText) {
    let mut place = TagPlace::of(tag);
    if let Some(Placing { rect, rotate, .. }) = placing {
        place.x = held(rect.x, place.x, MIN_POSITION, MAX_POSITION);
        place.y = held(rect.y, place.y, MIN_POSITION, MAX_POSITION);
        place.w = held(rect.w, place.w, 0.0, MAX_SIZE);
        place.h = held(rect.h, place.h, 0.0, MAX_SIZE);
        place.rotate = held(*rotate, place.rotate, -MAX_ROTATION, MAX_ROTATION);
    }
    for notice in place.notices {
        if !notices.contains(&notice) {
            notices.push(notice);
        }
    }
    let ((w, h), drawing) = lay_out_positioned_tag(tag, (place.w, place.h), place.align, theme, env, notices);
    let layer = LayerSpec {
        source: Some(tag.range.clone()),
        rect: Rect { x: place.x.unwrap_or(0.0), y: place.y.unwrap_or(0.0), w, h },
        rotate: place.rotate.unwrap_or(0.0),
        items: drawing.items,
        click: place.click,
        enter: place.enter,
        native: drawing.native,
        is_title: false,
        varies: false,
    };
    (layer, drawing.text)
}

/// Whether the text of any of `blocks`, at any depth, has an inline `<Text>` tag that asks for
/// an alignment, which only a placed box can have.
fn aligns_inline(source: &str, blocks: &[Block]) -> bool {
    let asks = |inline: &std::ops::Range<usize>| {
        let text = source.get(inline.clone()).unwrap_or_default();
        text.match_indices("<Text").any(|(at, _)| {
            let tag = &text[at..];
            let tag = &tag[..tag.find('>').unwrap_or(tag.len())];
            tag.split(|c: char| c.is_whitespace()).any(|part| part.starts_with("align="))
        })
    };
    blocks.iter().any(|block| match block {
        Block::Heading { inline, .. } | Block::Paragraph { inline, .. } => asks(inline),
        Block::List { items, .. } => items.iter().any(|item| aligns_inline(source, item)),
        Block::Tag(tag) => aligns_inline(source, &tag.children),
        _ => false,
    })
}

/// The alignment `value` names, or `None`, with a notice about `what`, if it names none.
fn read_align(value: &str, what: &str, notices: &mut Vec<Notice>) -> Option<Align> {
    match value.trim() {
        "left" => Some(Align::Left),
        "center" => Some(Align::Center),
        "right" => Some(Align::Right),
        _ => {
            let notice = Notice(shortened(&format!("{what} can be aligned left, center or right, not {value}"), MAX_NOTICE_CHARS));
            if !notices.contains(&notice) {
                notices.push(notice);
            }
            None
        }
    }
}

/// How a region's text is aligned on this slide: as its `region.<name>` settings line's
/// `align` says, or as the layout has it.
fn region_align(slide: &Slide, region: &RegionDef, notices: &mut Vec<Notice>) -> Align {
    let line = region_line(slide, region);
    // The first, as a tag's first attribute of a name is the one it has.
    let asked = line.split_whitespace().find_map(|pair| pair.strip_prefix("align="));
    let what = format!("The {} region", region.name);
    asked.and_then(|value| read_align(value, &what, notices)).unwrap_or(region.align)
}

/// How the text in `place` on slide `index` is set to be aligned: a region's as its
/// `region.<name>` line says or its layout has it, a positioned box's as its `align` says, or
/// at the left. It's what's asked for, whatever a list in it is drawn at. `None` if the slide
/// has no such region or box, and for a table's cell.
pub fn text_align(talk: &Talk, index: usize, place: &FlowPlace) -> Option<Align> {
    let slide = talk.slides.get(index)?;
    let mut notices = Vec::new();
    match place {
        FlowPlace::Region(name) => {
            let layout = slide_layout(slide, &mut notices);
            let region = layout.regions.iter().find(|region| region.name == name.as_str())?;
            Some(region_align(slide, region, &mut notices))
        }
        FlowPlace::Tag(range) => slide.blocks.iter().find_map(|block| match block {
            Block::Tag(tag) if tag.range == *range && is_positioned(tag) => Some(TagPlace::of(tag).align.unwrap_or(Align::Left)),
            _ => None,
        }),
        // A cell has no alignment of its own to set.
        FlowPlace::Cell { .. } => None,
    }
}

/// What the editor shows in an empty region: a dashed outline, and an offer in a muted tint
/// of the text colour to start the title or the text there.
pub(crate) fn empty_region_layer(region: &EmptyRegion, theme: &Theme, fonts: &mut Fonts) -> LayerSpec {
    const DASH: f32 = 16.0;
    const DASH_GAP: f32 = 10.0;
    const OUTLINE: f32 = 2.0;
    const OUTSIDE: f32 = 8.0;
    let mix = |text: u8, background: u8| ((u16::from(text) * 2 + u16::from(background) * 3) / 5) as u8;
    let (text, back) = (theme.text, theme.background);
    let muted = Color { r: mix(text.r, back.r), g: mix(text.g, back.g), b: mix(text.b, back.b), a: text.a };
    let Rect { w, h, .. } = region.rect;
    let mut items = Vec::new();
    let mut outline = PathBuilder::new();
    let (left, top, right, bottom) = (-OUTSIDE, -OUTSIDE, w + OUTSIDE, h + OUTSIDE);
    for (from, to) in [((left, top), (right, top)), ((right, top), (right, bottom)), ((right, bottom), (left, bottom)), ((left, bottom), (left, top))] {
        let length = (to.0 - from.0).hypot(to.1 - from.1);
        let mut along = 0.0;
        while along < length {
            let end = (along + DASH).min(length);
            let at = |d: f32| (from.0 + (to.0 - from.0) * d / length, from.1 + (to.1 - from.1) * d / length);
            let (a, b) = (at(along), at(end));
            outline.move_to(a.0, a.1);
            outline.line_to(b.0, b.1);
            along += DASH + DASH_GAP;
        }
    }
    if let Some(path) = outline.finish() {
        items.push(DisplayItem::Stroke { path, color: muted, width: OUTLINE });
    }
    let label = if region.name == "title" { "Click to add title" } else { "Click to add text" };
    let run = TextRun { text: label.to_string(), size: region.size, bold: region.bold, italic: false, underline: false, color: muted, family: theme.font.clone() };
    let text = shape_text(fonts, &[run], Some(w), region.align);
    // A title's offer sits in the middle of its region; the rest start where text would.
    let y = if region.name == "title" { ((h - text.height) / 2.0).max(0.0) } else { 0.0 };
    items.push(DisplayItem::Text { text, x: 0.0, y });
    LayerSpec {
        source: None,
        rect: region.rect,
        rotate: region.rotate,
        items,
        click: None,
        enter: Enter::Appear,
        native: Vec::new(),
        is_title: false,
        varies: false,
    }
}

/// What a slide that couldn't be parsed shows: the reason, across the whole slide.
fn error_layer(slide: &Slide, message: &str, theme: &Theme, fonts: &mut Fonts) -> LayerSpec {
    let rect = Rect { x: 0.0, y: 0.0, w: SLIDE_WIDTH, h: SLIDE_HEIGHT };
    LayerSpec {
        source: Some(slide.range.clone()),
        rect,
        rotate: 0.0,
        items: placeholder_items(message, rect, theme, fonts),
        click: None,
        enter: Enter::Appear,
        native: vec![placeholder_element(message, rect, theme)],
        is_title: false,
        varies: false,
    }
}

fn slide_layout(slide: &Slide, notices: &mut Vec<Notice>) -> &'static LayoutDef {
    let name = slide
        .settings
        .as_ref()
        .and_then(|settings| settings.get("layout"))
        .filter(|name| !name.is_empty())
        .unwrap_or(DEFAULT_LAYOUT);
    layout_by_name(name).unwrap_or_else(|| {
        notices.push(Notice(format!("There is no layout called {name}, so this slide uses {DEFAULT_LAYOUT}")));
        layout_by_name(DEFAULT_LAYOUT).expect("the default layout exists")
    })
}

/// The blocks each of the layout's regions holds. The first heading goes to the title,
/// unless a `<Region>` marker came before it: from a marker on, the author has said where
/// things go.
fn assign_blocks<'a>(slide: &'a Slide, layout: &LayoutDef, notices: &mut Vec<Notice>) -> Vec<Vec<&'a Block>> {
    let index_of = |name: &str| layout.regions.iter().position(|region| region.name == name);
    let main = index_of(layout.main).expect("a layout's main region is one of its regions");
    let mut contents = vec![Vec::new(); layout.regions.len()];
    let mut title = index_of("title");
    let mut current = main;
    for block in &slide.blocks {
        let target = match block {
            Block::Region { name, .. } => {
                title = None;
                current = index_of(name).unwrap_or_else(|| {
                    notices.push(Notice(format!("This layout has no region called {name}")));
                    main
                });
                continue;
            }
            Block::Tag(tag) if is_positioned(tag) => continue,
            Block::Heading { .. } => title.take().unwrap_or(current),
            _ => current,
        };
        contents[target].push(block);
    }
    contents
}

/// The slide's `region.<name>` settings line for a region, or nothing.
fn region_line<'a>(slide: &'a Slide, region: &RegionDef) -> &'a str {
    let key = format!("region.{}", region.name);
    slide.settings.as_ref().and_then(|settings| settings.get(&key)).unwrap_or_default()
}

/// Where a region sits on this slide: the layout's place, changed by any `region.<name>`
/// settings line. A value that isn't a finite number is ignored, and the rest are held
/// within the limits above. Whether the line places or turns it, with a word read for that.
fn placement(slide: &Slide, region: &RegionDef) -> (Rect, f32, bool) {
    let mut rect = region.rect;
    let mut rotate = 0.0;
    let mut moved = false;
    let line = region_line(slide, region);
    for pair in line.split_whitespace() {
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        let Some(value) = value.parse::<f32>().ok().filter(|value| value.is_finite()) else {
            continue;
        };
        match name {
            "x" => rect.x = value.clamp(MIN_POSITION, MAX_POSITION),
            "y" => rect.y = value.clamp(MIN_POSITION, MAX_POSITION),
            "w" => rect.w = value.clamp(0.0, MAX_SIZE),
            "h" => rect.h = value.clamp(0.0, MAX_SIZE),
            "rotate" => rotate = value.clamp(-MAX_ROTATION, MAX_ROTATION),
            // `click` says when a region appears, not where it sits.
            _ => continue,
        }
        moved = true;
    }
    (rect, rotate, moved)
}

fn background(theme: &Theme) -> LayerSpec {
    let area = tiny_skia::Rect::from_xywh(0.0, 0.0, SLIDE_WIDTH, SLIDE_HEIGHT).expect("the slide has an area");
    LayerSpec {
        source: None,
        rect: Rect { x: 0.0, y: 0.0, w: SLIDE_WIDTH, h: SLIDE_HEIGHT },
        rotate: 0.0,
        items: vec![DisplayItem::Fill { path: PathBuilder::from_rect(area), color: theme.background }],
        click: None,
        enter: Enter::Appear,
        native: Vec::new(),
        is_title: false,
        varies: false,
    }
}

#[cfg(test)]
mod region_words {
    //! The editor writes the word of a region's line that the format crate says the engine
    //! reads. These hold the two to the same reading, so neither drifts from the other.

    use proptest::prelude::*;
    use slides_format::{parse_talk, region_value};

    use super::*;

    /// Words a person might write on a region's line, with what the engine ignores.
    const WORDS: [&str; 26] = [
        "x=10", "x=-5", "x=abc", "x=1e3", "x=inf", "x=", "x==5", "y=20.5", "w=500", "h=7", "rotate=3", "rotate=NaN", "click=each", "click=2",
        "click=0", "click=1000", "click=+1", "click=zz", "enter=fade", "enter=fly-up", "enter=zoom", "align=center", "align=right",
        "align=middle", "foo=bar", "bare",
    ];

    proptest! {
        #[test]
        fn region_value_is_the_word_the_engine_reads(words in prop::collection::vec(prop::sample::select(WORDS.to_vec()), 0..6)) {
            let source = format!("---\nregion.body: {}\n---\n# T\n", words.join("\t"));
            let talk = parse_talk(&source);
            let slide = &talk.slides[0];
            let mut notices = Vec::new();
            let layout = slide_layout(slide, &mut notices);
            let region = layout.regions.iter().find(|region| region.name == "body").expect("the layout has a body");
            let value = |name: &str| region_value(slide, "body", name);
            let number = |name: &str, default: f32| value(name).map_or(default, |value| value.parse::<f32>().expect("a number"));

            let (rect, rotate, moved) = placement(slide, region);
            prop_assert_eq!(moved, ["x", "y", "w", "h", "rotate"].iter().any(|name| value(name).is_some()));
            prop_assert_eq!(rect.x, number("x", region.rect.x));
            prop_assert_eq!(rect.y, number("y", region.rect.y));
            prop_assert_eq!(rect.w, number("w", region.rect.w));
            prop_assert_eq!(rect.h, number("h", region.rect.h));
            prop_assert_eq!(rotate, number("rotate", 0.0));

            let timing = read_region_timing(region_line(slide, region), &mut notices);
            let click = match value("click") {
                None => RegionClick::Absent,
                Some("each") => RegionClick::Each,
                Some(step) => RegionClick::Step(step.parse().expect("a step")),
            };
            prop_assert_eq!(timing.click, click);
            let enter = match value("enter") {
                None | Some("appear") => Enter::Appear,
                Some("fade") => Enter::Fade,
                Some("fly-up") => Enter::FlyUp,
                Some(other) => panic!("the engine has no entrance {other}"),
            };
            prop_assert_eq!(timing.enter, enter);

            let align = match value("align") {
                None => region.align,
                Some("left") => Align::Left,
                Some("center") => Align::Center,
                Some("right") => Align::Right,
                Some(other) => panic!("the engine has no alignment {other}"),
            };
            prop_assert_eq!(region_align(slide, region, &mut notices), align);
        }
    }
}
