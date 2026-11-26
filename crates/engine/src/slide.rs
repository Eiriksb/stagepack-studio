//! Puts a parsed slide's content into the regions of its layout.

use slides_format::{Block, DEFAULT_LAYOUT, FlowPlace, Slide, Tag, Talk, Target};
use tiny_skia::PathBuilder;
use toolkit::{Color, Registry, Theme};

use crate::elements::{MODEL_TAG, MOST_VIEWS, VIDEO_TAG, lay_out_positioned_tag, model_view, model_views};
use crate::flow::{MAX_NOTICE_CHARS, flow_region, flow_region_each};
use slides_components::shortened;
use crate::timeline::{MAX_CLICK, RegionClick, read_region_timing, read_tag_timing, read_transition};
use crate::native::placeholder_element;
use crate::placeholder::placeholder_items;
use crate::text_geometry::{LayerText, RegionBox};
use crate::{
    Align, ComponentAsks, DisplayItem, Enter, Fonts, LayerSpec, LayoutDef, Moment, Notice, PictureCache, Rect, RegionDef, TalkComponents, TalkFiles, TextRun,
    Transition, VideoOn, ModelOn, TerminalOn, ButtonAction, ButtonOn, ComponentOn, layout_by_name, shape_text,
};
use crate::builtins::{BUTTON_IN_NOTHING, BUTTON_ON_NOTHING, BUTTON_STEP_PAST, BUTTON_TAG, button_fault_notice, read_button_action, read_button_look};
use crate::terminal_screen::{LaidTerminal, TERMINAL_TAG, lay_out_terminal, read_terminal};

/// How wide and high a slide is, in slide units.
pub const SLIDE_WIDTH: f32 = 1920.0;
pub const SLIDE_HEIGHT: f32 = 1080.0;

/// The attributes that place a component, which it doesn't see as props. `id` names it, so that
/// a button can act on it.
pub const PLACEMENT_ATTRS: [&str; 9] = ["x", "y", "w", "h", "rotate", "click", "enter", "align", "id"];

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
                            items: piece.items.into(),
                            click: Some((step as u32 + 1).min(MAX_CLICK)),
                            enter: timing.enter,
                            native: piece.native,
                            // A slide has one title, so only the first piece is it.
                            is_title: is_title && step == 0,
                            varies,
                            video: None,
                            model: None,
                            terminal: None,
                            button: None,
                            component: None,
                            id: None,
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
        layers.push(LayerSpec { source: Some(source), rect, rotate, items: items.into(), click, enter: timing.enter, native, is_title, varies, video: None, model: None, terminal: None, button: None, component: None, id: None });
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
    named_notices(&layers, steps_of(&layers, env.asks.steps), &mut notices);
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
    /// The `id` it gives, by which a button names it, if it gives one that isn't blank.
    id: Option<String>,
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
            id: tag.attr("id").map(str::trim).filter(|id| !id.is_empty()).map(str::to_string),
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
    for notice in std::mem::take(&mut place.notices) {
        if !notices.contains(&notice) {
            notices.push(notice);
        }
    }
    if tag.name == TERMINAL_TAG {
        return (terminal_layer(tag, &place, placing.is_some(), theme, env, notices), LayerText::default());
    }
    let ((w, h), drawing) = lay_out_positioned_tag(tag, (place.w, place.h), place.align, theme, env, notices);
    let rect = Rect { x: place.x.unwrap_or(0.0), y: place.y.unwrap_or(0.0), w, h };
    let rotate = place.rotate.unwrap_or(0.0);
    let video = (tag.name == VIDEO_TAG).then(|| VideoOn {
        src: tag.attr("src").unwrap_or_default().trim().to_string(),
        looped: read_loop(tag.attr("loop"), notices),
        rect,
        rotate,
    });
    let button = (tag.name == BUTTON_TAG).then(|| {
        let action = read_button_action(tag);
        if let Err(fault) = action {
            add_notice(notices, button_fault_notice(fault).to_string());
        }
        let (look, _) = read_button_look(tag.attr("look"));
        ButtonOn { label: tag.attr("label").unwrap_or_default().to_string(), look, action, id: place.id.clone(), rect, rotate }
    });
    let model = (tag.name == MODEL_TAG).then(|| {
        let (views, left_out) = model_views(tag.attr("views"));
        if left_out {
            add_notice(notices, format!("Some of this 3D model's views can't be read, or there are more than {MOST_VIEWS}, so they're left out"));
        }
        ModelOn {
            src: tag.attr("src").unwrap_or_default().trim().to_string(),
            turn: read_turn(tag.attr("turn"), notices),
            view: model_view(tag.attr("view")).ok().flatten(),
            views,
            rect,
            rotate,
        }
    });
    let component = drawing.interactive.map(|state| ComponentOn { hits: drawing.hits, rect, rotate, state });
    let layer = LayerSpec {
        source: Some(tag.range.clone()),
        rect,
        rotate,
        items: drawing.items.into(),
        click: place.click,
        enter: place.enter,
        native: drawing.native,
        is_title: false,
        varies: false,
        video,
        model,
        terminal: None,
        button,
        component,
        id: place.id.clone(),
    };
    (layer, drawing.text)
}

/// The layer of a terminal recording placed at `place`: its last screen, which it plays over
/// when asked. It's drawn by the engine alone, so it has nothing of its own for export, which
/// takes it as a picture of its screen.
fn terminal_layer(tag: &Tag, place: &TagPlace, placing: bool, theme: &Theme, env: &mut LayoutEnv, notices: &mut Vec<Notice>) -> LayerSpec {
    let LaidTerminal { size: (w, h), items, stops, problem, pending } = lay_out_terminal(tag, (place.w, place.h), placing, theme, env);
    if let Some(problem) = problem {
        add_notice(notices, problem);
    }
    let rect = Rect { x: place.x.unwrap_or(0.0), y: place.y.unwrap_or(0.0), w, h };
    let rotate = place.rotate.unwrap_or(0.0);
    let written = read_terminal(tag);
    LayerSpec {
        source: Some(tag.range.clone()),
        rect,
        rotate,
        items,
        click: place.click,
        enter: place.enter,
        native: Vec::new(),
        is_title: false,
        varies: false,
        video: None,
        model: None,
        terminal: Some(TerminalOn { src: written.src, run: written.run, cols: written.cols, rows: written.rows, rect, rotate, stops, pending }),
        button: None,
        component: None,
        id: place.id.clone(),
    }
}

/// The steps a slide of `layers` takes: as far as any of them appears, a model's views and a
/// terminal recording's stops go on from where it appears, and `stepped` are those its components
/// step through.
pub(crate) fn steps_of(layers: &[LayerSpec], stepped: u32) -> u32 {
    let views = layers.iter().filter_map(|spec| Some(spec.click.unwrap_or(0) + spec.model.as_ref()?.views.len() as u32));
    let stops = layers.iter().filter_map(|spec| Some(spec.click.unwrap_or(0).saturating_add(spec.terminal.as_ref()?.stops.min(MAX_CLICK as usize) as u32)));
    let clicks = layers.iter().filter_map(|spec| spec.click).chain(views).chain(stops).max().unwrap_or(0);
    clicks.max(stepped).min(MAX_CLICK)
}

/// What's said of an `id` that more than one thing on a slide has: a button naming it acts on neither.
pub fn duplicate_id_notice(id: &str) -> String {
    format!("Two things on this slide have the id \"{id}\".")
}

/// What's said of what the slide's `layers` name by `id`: each `id` given twice, and what's wrong
/// with what each button names, on a slide of `steps` steps.
fn named_notices(layers: &[LayerSpec], steps: u32, notices: &mut Vec<Notice>) {
    let ids: Vec<&str> = layers.iter().filter_map(|spec| spec.id.as_deref()).collect();
    for (at, id) in ids.iter().enumerate() {
        if ids[..at].contains(id) {
            add_notice(notices, duplicate_id_notice(id));
        }
    }
    for on in layers.iter().filter_map(|spec| spec.button.as_ref()) {
        if let Some(problem) = button_target_problem(on, layers, steps) {
            add_notice(notices, problem);
        }
    }
}

/// What's wrong with what the button `on` names, among the slide's `layers`, on a slide of
/// `steps` steps: an `id` more than one thing has, a live terminal its `in` doesn't name, a thing
/// its `on` doesn't, or a step past the last. Why it does nothing at all is said apart.
pub(crate) fn button_target_problem(on: &ButtonOn, layers: &[LayerSpec], steps: u32) -> Option<String> {
    let named = |id: &str| layers.iter().filter(|spec| spec.id.as_deref() == Some(id)).count();
    match on.action.as_ref().ok()? {
        ButtonAction::Run { terminal: id, .. } | ButtonAction::Do { target: id, .. } if named(id) > 1 => Some(duplicate_id_notice(id)),
        ButtonAction::Run { terminal, .. } => {
            let live = |spec: &LayerSpec| spec.id.as_deref() == Some(terminal.as_str()) && spec.terminal.as_ref().is_some_and(|on| on.run.is_some());
            (!layers.iter().any(live)).then(|| BUTTON_IN_NOTHING.to_string())
        }
        ButtonAction::Do { target, .. } => {
            let interactive = |spec: &LayerSpec| spec.id.as_deref() == Some(target.as_str()) && spec.component.is_some();
            (!layers.iter().any(interactive)).then(|| BUTTON_ON_NOTHING.to_string())
        }
        ButtonAction::Step(step) => (*step > steps).then(|| BUTTON_STEP_PAST.to_string()),
    }
}

/// Adds `text` to `notices`, shortened, unless it's there already.
fn add_notice(notices: &mut Vec<Notice>, text: String) {
    let notice = Notice(shortened(&text, MAX_NOTICE_CHARS));
    if !notices.contains(&notice) {
        notices.push(notice);
    }
}

/// Whether a model's `turn` says to turn slowly on its own: `auto`. Left out, it stays still,
/// and anything else says it stays still and is a notice.
fn read_turn(value: Option<&str>, notices: &mut Vec<Notice>) -> bool {
    let Some(value) = value else { return false };
    match value.trim().to_ascii_lowercase().as_str() {
        "auto" => true,
        "" | "none" | "no" => false,
        _ => {
            add_notice(notices, format!("A 3D model's turn can be auto, or left out to stay still, not {value}, so it stays still"));
            false
        }
    }
}

/// Whether a video's `loop` says to go round again: written bare, or as `true`, `yes` or `1`.
/// `false`, `no` and `0` say not to, and anything else says not to and is a notice.
fn read_loop(value: Option<&str>, notices: &mut Vec<Notice>) -> bool {
    let Some(value) = value else { return false };
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "true" | "yes" | "1" => true,
        "false" | "no" | "0" => false,
        _ => {
            let notice = Notice(shortened(&format!("A video's loop can be yes or no, not {value}, so it plays once"), MAX_NOTICE_CHARS));
            if !notices.contains(&notice) {
                notices.push(notice);
            }
            false
        }
    }
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

/// How much what the editor draws over a slide, as the offer in an empty region, the caret or
/// a selection's outline, stands out from the slide's background at least: the contrast WCAG
/// asks of what has to be seen but isn't text to read.
pub const MARKS_STAND_OUT: f32 = 3.0;

/// The contrast of `a` with `b`, as WCAG works it out from their relative luminance: from 1,
/// for two colours the same, to 21, for black and white, whichever way round. Each is taken
/// as it is drawn on its own, whatever its alpha.
pub fn contrast(a: Color, b: Color) -> f32 {
    let luminance = |color: Color| {
        let channel = |value: u8| {
            let value = f32::from(value) / 255.0;
            if value <= 0.03928 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
    };
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// `color`, if it stands out from `background` as [`MARKS_STAND_OUT`] says, or else `color`
/// taken toward whichever of a near black and a near white stands out more, only as far as it
/// takes to stand out: all the way, that's enough on any background.
pub fn standing_out(color: Color, background: Color) -> Color {
    const DARK: Color = Color::rgb(0x0a, 0x0a, 0x0a);
    const LIGHT: Color = Color::rgb(0xfa, 0xfa, 0xfa);
    /// How many steps there are from the colour to the extreme.
    const STEPS: u16 = 255;
    if contrast(color, background) >= MARKS_STAND_OUT {
        return color;
    }
    let extreme = if contrast(DARK, background) >= contrast(LIGHT, background) { DARK } else { LIGHT };
    let toward = |steps: u16| {
        let channel = |from: u8, to: u8| ((u16::from(from) * (STEPS - steps) + u16::from(to) * steps + STEPS / 2) / STEPS) as u8;
        Color { r: channel(color.r, extreme.r), g: channel(color.g, extreme.g), b: channel(color.b, extreme.b), a: color.a }
    };
    (1..=STEPS).map(toward).find(|stepped| contrast(*stepped, background) >= MARKS_STAND_OUT).unwrap_or(extreme)
}

/// The colour of the editor's offer in an empty region: the theme's text colour faded toward
/// its background, as far as it still stands out from it, two fifths of the text at the
/// least; and where even the text doesn't stand out, what [`standing_out`] gives.
pub fn placeholder_color(theme: &Theme) -> Color {
    let (text, back) = (theme.text, theme.background);
    let mix = |fifths: u16| {
        let channel = |text: u8, background: u8| ((u16::from(text) * fifths + u16::from(background) * (5 - fifths)) / 5) as u8;
        Color { r: channel(text.r, back.r), g: channel(text.g, back.g), b: channel(text.b, back.b), a: text.a }
    };
    (2..=5).map(mix).find(|muted| contrast(*muted, back) >= MARKS_STAND_OUT).unwrap_or_else(|| standing_out(text, back))
}

/// What the editor shows in an empty region: a dashed outline, and an offer in a muted tint
/// of the text colour to start the title or the text there.
pub(crate) fn empty_region_layer(region: &EmptyRegion, theme: &Theme, fonts: &mut Fonts) -> LayerSpec {
    const DASH: f32 = 16.0;
    const DASH_GAP: f32 = 10.0;
    const OUTLINE: f32 = 2.0;
    const OUTSIDE: f32 = 8.0;
    let muted = placeholder_color(theme);
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
        items: items.into(),
        click: None,
        enter: Enter::Appear,
        native: Vec::new(),
        is_title: false,
        varies: false,
        video: None,
        model: None,
        terminal: None,
        button: None,
        component: None,
        id: None,
    }
}

/// What a slide that couldn't be parsed shows: the reason, across the whole slide.
fn error_layer(slide: &Slide, message: &str, theme: &Theme, fonts: &mut Fonts) -> LayerSpec {
    let rect = Rect { x: 0.0, y: 0.0, w: SLIDE_WIDTH, h: SLIDE_HEIGHT };
    LayerSpec {
        source: Some(slide.range.clone()),
        rect,
        rotate: 0.0,
        items: placeholder_items(message, rect, theme, fonts).into(),
        click: None,
        enter: Enter::Appear,
        native: vec![placeholder_element(message, rect, theme)],
        is_title: false,
        varies: false,
        video: None,
        model: None,
        terminal: None,
        button: None,
        component: None,
        id: None,
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
        items: vec![DisplayItem::Fill { path: PathBuilder::from_rect(area), color: theme.background }].into(),
        click: None,
        enter: Enter::Appear,
        native: Vec::new(),
        is_title: false,
        varies: false,
        video: None,
        model: None,
        terminal: None,
        button: None,
        component: None,
        id: None,
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
