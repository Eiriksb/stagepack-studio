//! Puts a parsed slide's content into the regions of its layout.

use slides_format::{Block, Slide, Tag, Talk};
use tiny_skia::PathBuilder;
use toolkit::{Color, Registry, Theme};

use crate::elements::lay_out_positioned_tag;
use crate::flow::{flow_region, flow_region_each};
use crate::timeline::{MAX_CLICK, RegionClick, read_region_timing, read_tag_timing, read_transition};
use crate::native::placeholder_element;
use crate::placeholder::placeholder_items;
use crate::{
    ComponentAsks, DisplayItem, Enter, Fonts, LayerSpec, LayoutDef, Moment, Notice, PictureCache, Rect, RegionDef, TalkComponents, TalkFiles, Transition, layout_by_name,
};

pub(crate) const SLIDE_WIDTH: f32 = 1920.0;
pub(crate) const SLIDE_HEIGHT: f32 = 1080.0;
const DEFAULT_LAYOUT: &str = "content";

/// The attributes that place a component, which it doesn't see as props.
pub const PLACEMENT_ATTRS: [&str; 7] = ["x", "y", "w", "h", "rotate", "click", "enter"];

// The most a talk can ask for in a place on the slide, so one line of a settings block
// can't make a layer that needs gigabytes.
pub(crate) const MIN_POSITION: f32 = -3840.0;
pub(crate) const MAX_POSITION: f32 = 7680.0;
pub(crate) const MAX_SIZE: f32 = 3840.0;
pub(crate) const MAX_ROTATION: f32 = 360.0;

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
    if let Some(error) = &slide.error {
        notices.push(Notice(error.message.clone()));
        layers.push(error_layer(slide, &error.message, &theme, env.fonts));
        return LaidOutSlide { layers, notices, transition: Transition::None, asks: std::mem::take(&mut env.asks) };
    }
    let transition = read_transition(slide.settings.as_ref().and_then(|settings| settings.get("transition")), &mut notices);
    let layout = slide_layout(slide, &mut notices);
    let contents = assign_blocks(slide, layout, &mut notices);

    for (region, blocks) in layout.regions.iter().zip(&contents) {
        let (Some(first), Some(last)) = (blocks.first(), blocks.last()) else {
            continue;
        };
        let (rect, rotate) = placement(slide, region);
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
            if rotate != 0.0 {
                notices.push(Notice(format!(
                    "The {} region is rotated, so click=each shows it all on click 1",
                    region.name
                )));
            } else {
                let (pieces, varies) =
                    noting_asks(env, |env| flow_region_each(blocks, region, rect.w, &theme, env, &mut notices));
                if !pieces.is_empty() {
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
                    }
                    continue;
                }
                click = Some(1);
            }
        }
        let ((items, native), varies) = noting_asks(env, |env| flow_region(blocks, region, rect.w, &theme, env, &mut notices));
        layers.push(LayerSpec { source: Some(source), rect, rotate, items, click, enter: timing.enter, native, is_title, varies });
    }
    for block in &slide.blocks {
        if let Block::Tag(tag) = block
            && is_positioned(tag)
        {
            let (mut layer, varies) = noting_asks(env, |env| positioned_layer(tag, &theme, env, &mut notices));
            layer.varies = varies;
            layers.push(layer);
        }
    }
    LaidOutSlide { layers, notices, transition, asks: std::mem::take(&mut env.asks) }
}

/// The placement numbers a tag gives. One that can't be read counts as absent, and the
/// rest are held within the limits above.
struct TagPlace {
    x: Option<f32>,
    y: Option<f32>,
    w: Option<f32>,
    h: Option<f32>,
    rotate: Option<f32>,
    click: Option<u32>,
    enter: Enter,
    /// What was wrong with the `click` and `enter` it gave.
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
            click,
            enter,
            notices,
        }
    }
}

/// Whether a tag asks for a place on the slide, which gives it a layer of its own.
pub(crate) fn is_positioned(tag: &Tag) -> bool {
    let place = TagPlace::of(tag);
    place.x.is_some() || place.y.is_some() || place.w.is_some() || place.h.is_some()
}

fn positioned_layer(tag: &Tag, theme: &Theme, env: &mut LayoutEnv, notices: &mut Vec<Notice>) -> LayerSpec {
    let place = TagPlace::of(tag);
    for notice in place.notices {
        if !notices.contains(&notice) {
            notices.push(notice);
        }
    }
    let ((w, h), items, native) = lay_out_positioned_tag(tag, (place.w, place.h), theme, env, notices);
    LayerSpec {
        source: Some(tag.range.clone()),
        rect: Rect { x: place.x.unwrap_or(0.0), y: place.y.unwrap_or(0.0), w, h },
        rotate: place.rotate.unwrap_or(0.0),
        items,
        click: place.click,
        enter: place.enter,
        native,
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
/// within the limits above.
fn placement(slide: &Slide, region: &RegionDef) -> (Rect, f32) {
    let mut rect = region.rect;
    let mut rotate = 0.0;
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
            _ => {}
        }
    }
    (rect, rotate)
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
