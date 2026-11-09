//! Puts a parsed slide's content into the regions of its layout.

use slides_format::{Block, Slide, Talk};
use tiny_skia::PathBuilder;
use toolkit::{Color, Registry, Theme};

use crate::flow::flow_region;
use crate::{DisplayItem, Fonts, LayerSpec, LayoutDef, Notice, Rect, RegionDef, TalkFiles, layout_by_name};

const SLIDE_WIDTH: f32 = 1920.0;
const SLIDE_HEIGHT: f32 = 1080.0;
const DEFAULT_LAYOUT: &str = "content";

pub struct LayoutEnv<'a> {
    pub fonts: &'a mut Fonts,
    pub registry: &'a Registry,
    pub files: &'a dyn TalkFiles,
}

pub struct LaidOutSlide {
    pub layers: Vec<LayerSpec>,
    pub notices: Vec<Notice>,
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
    let mut notices = Vec::new();
    let layout = slide_layout(slide, &mut notices);
    let contents = assign_blocks(slide, layout, &mut notices);

    let mut layers = vec![background(&theme)];
    for (region, blocks) in layout.regions.iter().zip(&contents) {
        let (Some(first), Some(last)) = (blocks.first(), blocks.last()) else {
            continue;
        };
        let (rect, rotate) = placement(slide, region);
        let items = flow_region(blocks, region, rect.w, &theme, env.fonts, &mut notices);
        layers.push(LayerSpec { source: Some(first.range().start..last.range().end), rect, rotate, items });
    }
    LaidOutSlide { layers, notices }
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
            Block::Tag(_) | Block::Picture { .. } => continue,
            Block::Heading { .. } => title.take().unwrap_or(current),
            _ => current,
        };
        contents[target].push(block);
    }
    contents
}

/// Where a region sits on this slide: the layout's place, changed by any `region.<name>`
/// settings line. A value that isn't a finite number is ignored.
fn placement(slide: &Slide, region: &RegionDef) -> (Rect, f32) {
    let mut rect = region.rect;
    let mut rotate = 0.0;
    let key = format!("region.{}", region.name);
    let line = slide.settings.as_ref().and_then(|settings| settings.get(&key)).unwrap_or_default();
    for pair in line.split_whitespace() {
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        let Some(value) = value.parse::<f32>().ok().filter(|value| value.is_finite()) else {
            continue;
        };
        match name {
            "x" => rect.x = value,
            "y" => rect.y = value,
            "w" => rect.w = value,
            "h" => rect.h = value,
            "rotate" => rotate = value,
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
    }
}
