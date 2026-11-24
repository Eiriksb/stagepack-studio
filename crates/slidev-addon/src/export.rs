//! A talk as a Slidev talk: its `slides.md`, the files it shows under `public/`, and what the
//! app needs for the manifest. Everything is written as Slidev's own Markdown and HTML, with
//! nothing that runs: what Slidev can't draw as we do is a picture the app drew for it.

use std::collections::BTreeMap;

use addon_abi::{safe_name, Bundle, Notice, NoticeKind, Report, MANIFEST};
use serde::Deserialize;
use slides_format::{is_hidden, parse_talk, Block, Slide, Span, Style, Tag, Talk};

use crate::escape;
use crate::fonts;
use crate::shapes;

/// What an export is asked to do, from the operation's `.args.json`.
#[derive(Clone, Debug, Deserialize)]
pub struct ExportArgs {
    /// The talk's title, for the headmatter and the manifest.
    pub title: String,
}

/// One part of a slide the app drew as a picture, from `baked/index.json`. `n` counts the
/// slide's baked parts in the order they're written in, from 0, as [`is_baked`] tells them.
/// Its place is on the slide, in slide units, before anything it's in is turned.
#[derive(Clone, Debug, Deserialize)]
pub struct Baked {
    pub slide: usize,
    pub n: usize,
    pub tag: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    #[serde(default)]
    pub rotate: f32,
    /// The clicks at which it looks different, with a picture for each, `<n>@<step>.png`.
    #[serde(default)]
    pub steps: Vec<u32>,
    /// Whether it changes with time and not only with clicks, so a picture can't show it all.
    #[serde(default)]
    pub animated: bool,
}

/// Where the list of baked parts is.
pub const BAKED_INDEX: &str = "baked/index.json";
/// The talk's Markdown, given and given back.
pub const SLIDES: &str = "slides.md";

/// The tags drawn as Slidev's own HTML. The app draws every other tag as a picture, and a
/// video's still as one, which is what makes them baked.
const DRAWN_HERE: [&str; 4] = ["Box", "Shape", "Text", "Picture"];

/// Whether the app draws the tag `name` as a picture for an export: an icon, a chart, a 3D
/// model, a terminal, a video's still, and every component of the talk's own.
pub fn is_baked(name: &str) -> bool {
    !DRAWN_HERE.contains(&name)
}

// As the engine holds them, so that a talk is placed where it's drawn.
const MIN_POSITION: f32 = -3840.0;
const MAX_POSITION: f32 = 7680.0;
const MAX_SIZE: f32 = 3840.0;
const MAX_ROTATION: f32 = 360.0;
const MAX_CLICK: u32 = 999;
/// The space a box with a fill or an outline keeps around what's in it.
const DEFAULT_BOX_PADDING: f32 = 24.0;
/// The size of a shape that gives none.
const DEFAULT_SHAPE: (f32, f32) = (400.0, 200.0);
/// The most notices a report holds.
const MAX_NOTICES: usize = 10_000;
/// The most characters of a notice.
const MAX_NOTICE_CHARS: usize = 500;

/// File names Stagepack takes for code in a Slidev talk, which an export never holds.
const CODE_EXTENSIONS: [&str; 11] = ["vue", "js", "mjs", "cjs", "ts", "mts", "cts", "jsx", "tsx", "node", "wasm"];
const CODE_PREFIXES: [&str; 5] = ["vite.config", "uno.config", "unocss.config", "postcss.config", ".postcssrc"];

/// Whether Stagepack would take a file at `name`, a path in the export, for code.
pub fn is_code_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let base = lower.rsplit('/').next().unwrap_or(&lower);
    let extension = base.rsplit_once('.').map(|(_, extension)| extension);
    extension.is_some_and(|extension| CODE_EXTENSIONS.contains(&extension))
        || lower.split('/').any(|part| part == "setup")
        || base == "index.html"
        || base == "package.json"
        || CODE_PREFIXES.iter().any(|prefix| base.starts_with(prefix))
}

/// A region of one of the engine's layouts, and where it is.
struct RegionRect {
    name: &'static str,
    rect: Rect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

const fn region(name: &'static str, x: f32, y: f32, w: f32, h: f32) -> RegionRect {
    RegionRect { name, rect: Rect { x, y, w, h } }
}

/// One of the engine's layouts: its name, Slidev's, its regions in the engine's order and the
/// region for what nothing sends elsewhere. The places are the engine's `layouts.rs`.
struct Layout {
    name: &'static str,
    slidev: &'static str,
    regions: &'static [RegionRect],
    main: &'static str,
}

static LAYOUTS: [Layout; 5] = [
    Layout {
        name: "title",
        slidev: "cover",
        regions: &[region("title", 160.0, 340.0, 1600.0, 220.0), region("body", 160.0, 580.0, 1600.0, 180.0)],
        main: "body",
    },
    Layout {
        name: "content",
        slidev: "default",
        regions: &[region("title", 100.0, 60.0, 1720.0, 140.0), region("body", 100.0, 220.0, 1720.0, 780.0)],
        main: "body",
    },
    Layout {
        name: "two-columns",
        slidev: "two-cols",
        regions: &[region("title", 100.0, 60.0, 1720.0, 140.0), region("left", 100.0, 220.0, 840.0, 780.0), region("right", 980.0, 220.0, 840.0, 780.0)],
        main: "left",
    },
    Layout {
        name: "section",
        slidev: "section",
        regions: &[region("title", 160.0, 420.0, 1600.0, 240.0), region("body", 160.0, 680.0, 1600.0, 200.0)],
        main: "body",
    },
    Layout { name: "blank", slidev: "none", regions: &[region("body", 0.0, 0.0, 1920.0, 1080.0)], main: "body" },
];

const DEFAULT_LAYOUT: &str = "content";

/// How a part comes in when its click arrives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Enter {
    Appear,
    Fade,
    FlyUp,
}

fn read_enter(value: &str) -> Option<Enter> {
    match value.trim() {
        "appear" => Some(Enter::Appear),
        "fade" => Some(Enter::Fade),
        "fly-up" => Some(Enter::FlyUp),
        _ => None,
    }
}

/// A click as a talk writes it: a whole number from 1 to 999.
fn read_click(value: &str) -> Option<u32> {
    let value = value.trim();
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse::<u32>().ok().filter(|click| (1..=MAX_CLICK).contains(click))
}

/// What a region's line says of when it appears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RegionClick {
    Absent,
    Step(u32),
    Each,
}

/// The talk's look, from its first slide's settings, with the engine's defaults.
struct Theme {
    background: String,
    text: String,
    accent: String,
    font: String,
    mono: String,
}

/// A colour as the engine reads it, written `#rrggbb`. One it can't read is absent.
pub fn colour(text: &str) -> Option<String> {
    let text = text.trim();
    if let Some(hex) = text.strip_prefix('#') {
        if !hex.is_ascii() || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let hex = hex.to_ascii_lowercase();
        return match hex.len() {
            3 => Some(hex.chars().flat_map(|c| [c, c]).fold(String::from("#"), |mut out, c| {
                out.push(c);
                out
            })),
            6 => Some(format!("#{hex}")),
            _ => None,
        };
    }
    let rgb = match text {
        "black" => "#000000",
        "white" => "#ffffff",
        "red" => "#ff0000",
        "green" => "#008000",
        "blue" => "#0000ff",
        "yellow" => "#ffff00",
        "orange" => "#ffa500",
        "purple" => "#800080",
        "gray" => "#808080",
        _ => return None,
    };
    Some(rgb.to_string())
}

fn number(value: Option<&str>) -> Option<f32> {
    value?.trim().parse::<f32>().ok().filter(|value| value.is_finite())
}

/// A length in CSS pixels, which are slide units, as short as it can be written.
pub fn px(value: f32) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    // Never `-0`.
    let rounded = if rounded == 0.0 { 0.0 } else { rounded };
    format!("{rounded}")
}

/// Where a placed tag asks to be, its numbers held within the engine's limits.
#[derive(Clone, Copy, Debug, Default)]
struct Place {
    x: Option<f32>,
    y: Option<f32>,
    w: Option<f32>,
    h: Option<f32>,
    rotate: Option<f32>,
}

impl Place {
    fn of(tag: &Tag) -> Place {
        let held = |name: &str, low: f32, high: f32| number(tag.attr(name)).map(|value| value.clamp(low, high));
        Place {
            x: held("x", MIN_POSITION, MAX_POSITION),
            y: held("y", MIN_POSITION, MAX_POSITION),
            w: held("w", 0.0, MAX_SIZE),
            h: held("h", 0.0, MAX_SIZE),
            rotate: held("rotate", -MAX_ROTATION, MAX_ROTATION),
        }
    }

    fn is_positioned(&self) -> bool {
        self.x.is_some() || self.y.is_some() || self.w.is_some() || self.h.is_some()
    }
}

/// Whether a tag at a slide's top level is placed on the slide, as the engine has it.
fn is_positioned(tag: &Tag) -> bool {
    Place::of(tag).is_positioned()
}

/// The CSS that places something at `x`, `y`, `w` by `h`, turned by `rotate`.
fn placed_css(x: f32, y: f32, w: Option<f32>, h: Option<f32>, rotate: f32) -> Vec<String> {
    let mut css = vec!["position:absolute".to_string(), format!("left:{}px", px(x)), format!("top:{}px", px(y))];
    css.extend(w.map(|w| format!("width:{}px", px(w))));
    css.extend(h.map(|h| format!("height:{}px", px(h))));
    if rotate != 0.0 {
        css.push(format!("transform:rotate({}deg)", px(rotate)));
    }
    css
}

/// An HTML element's opening tag, with its CSS and its other attributes.
fn open(element: &str, css: &[String], attrs: &str) -> String {
    let style = if css.is_empty() { String::new() } else { format!(" style=\"{}\"", css.join(";")) };
    format!("<{element}{style}{attrs}>")
}

/// `content`, Markdown, inside `<element …>`, set apart by blank lines so it's read as Markdown.
fn wrap(element: &str, css: &[String], attrs: &str, content: &str) -> String {
    let open = open(element, css, attrs);
    if content.trim().is_empty() { format!("{open}\n</{element}>\n") } else { format!("{open}\n\n{}\n\n</{element}>\n", content.trim_end()) }
}

/// The attributes that make something come in at `click`, as `enter` says, and the CSS they
/// need beside them.
fn timing(click: u32, enter: Enter) -> (String, Option<String>) {
    match enter {
        Enter::FlyUp => (format!(" v-motion :initial=\"{{y:40,opacity:0}}\" :click-{click}=\"{{y:0,opacity:1}}\""), None),
        // Slidev fades what comes in on a click, and only the CSS can make it appear at once.
        Enter::Appear => (format!(" v-click=\"{click}\""), Some("transition:none".to_string())),
        Enter::Fade => (format!(" v-click=\"{click}\""), None),
    }
}

/// A URL path for `name`, a file under `public/`.
fn public_url(name: &str) -> String {
    let mut url = String::from("/");
    for byte in name.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' | b'@' => url.push(char::from(byte)),
            _ => url.push_str(&format!("%{byte:02X}")),
        }
    }
    url
}

fn shortened(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_string();
    }
    let mut short: String = text.chars().take(most - 1).collect();
    short.push('…');
    short
}

/// What the export keeps track of across the talk.
struct Exporter<'a> {
    files: BTreeMap<&'a str, &'a [u8]>,
    baked: BTreeMap<(usize, usize), &'a Baked>,
    theme: Theme,
    notices: Vec<Notice>,
    public: Vec<(String, Vec<u8>)>,
    /// The files under `public/`, in lower case, and the file each came from.
    public_names: BTreeMap<String, String>,
    /// How many arrows have been drawn, for each head's id.
    arrows: usize,
    /// Whether the first slide written has a transition. In the headmatter it is every
    /// slide's, so each slide with none of its own says it has none.
    deck_transition: bool,
    slide: SlideState,
}

/// What the export keeps track of on the slide it's writing.
#[derive(Default)]
struct SlideState {
    /// Counting from 1.
    number: usize,
    /// The `n` of each baked tag, by where it starts.
    baked_n: BTreeMap<usize, usize>,
    /// The most clicks anything on the slide asks for.
    clicks: u32,
    /// Whether a baked part shows a picture for each of its clicks.
    stepped: bool,
    /// Where the box that absolutely placed parts are placed in starts on the slide.
    origin: (f32, f32),
    /// Whether that box, or one it is in, is turned.
    turned: bool,
}

/// Writes `talk`, as `args` says, with `files` its own and what the app drew for it.
pub fn export(args: &ExportArgs, files: &Bundle) -> Result<(Bundle, Report), String> {
    let (_, source) = files.iter().find(|(name, _)| name == SLIDES).ok_or("The talk has no slides.md to export.")?;
    let source = std::str::from_utf8(source).map_err(|_| "The talk's slides.md isn't UTF-8 text.".to_string())?;
    let index: Vec<Baked> = match files.iter().find(|(name, _)| name == BAKED_INDEX) {
        Some((_, data)) => serde_json::from_slice(data).map_err(|error| format!("The list of parts drawn for the export, {BAKED_INDEX}, can't be read: {error}."))?,
        None => Vec::new(),
    };
    let talk = parse_talk(source);
    let mut exporter = Exporter {
        files: files.iter().map(|(name, data)| (name.as_str(), data.as_slice())).collect(),
        baked: index.iter().map(|baked| ((baked.slide, baked.n), baked)).collect(),
        theme: talk_theme(&talk),
        notices: Vec::new(),
        public: Vec::new(),
        public_names: BTreeMap::new(),
        arrows: 0,
        deck_transition: false,
        slide: SlideState::default(),
    };
    let markdown = exporter.talk(&talk, &args.title);
    let manifest = serde_json::to_vec(&serde_json::json!({ "title": args.title })).map_err(|error| error.to_string())?;
    let mut bundle: Bundle = vec![(SLIDES.to_string(), markdown.into_bytes()), (MANIFEST.to_string(), manifest)];
    bundle.append(&mut exporter.public);
    let mut notices = exporter.notices;
    if notices.len() > MAX_NOTICES {
        let more = notices.len() - (MAX_NOTICES - 1);
        notices.truncate(MAX_NOTICES - 1);
        notices.push(Notice { slide: None, kind: NoticeKind::Approximation, text: format!("And {more} more things that couldn't be carried over as they were.") });
    }
    Ok((bundle, Report { notices }))
}

fn talk_theme(talk: &Talk) -> Theme {
    let settings = talk.slides.first().and_then(|slide| slide.settings.as_ref());
    let get = |key: &str| settings.and_then(|settings| settings.get(key));
    let colour_or = |key: &str, default: &str| get(key).and_then(colour).unwrap_or_else(|| default.to_string());
    let font_or = |key: &str, default: &str| get(key).filter(|font| !font.is_empty()).unwrap_or(default).to_string();
    Theme {
        background: colour_or("color.background", "#ffffff"),
        text: colour_or("color.text", "#1e293b"),
        accent: colour_or("color.accent", "#2563eb"),
        font: font_or("font", "Inter"),
        mono: font_or("font.mono", "JetBrains Mono"),
    }
}

fn layout_of(slide: &Slide) -> Result<&'static Layout, String> {
    let name = slide.settings.as_ref().and_then(|settings| settings.get("layout")).filter(|name| !name.is_empty()).unwrap_or(DEFAULT_LAYOUT);
    LAYOUTS.iter().find(|layout| layout.name == name).ok_or_else(|| name.to_string())
}

fn default_layout() -> &'static Layout {
    LAYOUTS.iter().find(|layout| layout.name == DEFAULT_LAYOUT).expect("the default layout is one of them")
}

/// The slide's `region.<name>` line, or nothing.
fn region_line<'a>(slide: &'a Slide, name: &str) -> &'a str {
    slide.settings.as_ref().and_then(|settings| settings.get(&format!("region.{name}"))).unwrap_or_default()
}

/// Where a region sits, as the engine's `placement` reads its line, and whether it's moved.
fn placement(line: &str, region: &RegionRect) -> (Rect, f32, bool) {
    let (mut rect, mut rotate, mut moved) = (region.rect, 0.0, false);
    for pair in line.split_whitespace() {
        let Some((name, value)) = pair.split_once('=') else { continue };
        let Some(value) = value.parse::<f32>().ok().filter(|value| value.is_finite()) else { continue };
        match name {
            "x" => rect.x = value.clamp(MIN_POSITION, MAX_POSITION),
            "y" => rect.y = value.clamp(MIN_POSITION, MAX_POSITION),
            "w" => rect.w = value.clamp(0.0, MAX_SIZE),
            "h" => rect.h = value.clamp(0.0, MAX_SIZE),
            "rotate" => rotate = value.clamp(-MAX_ROTATION, MAX_ROTATION),
            _ => continue,
        }
        moved = true;
    }
    (rect, rotate, moved)
}

/// When a region comes in, as the engine's `read_region_timing` reads its line.
fn region_timing(line: &str) -> (RegionClick, Enter) {
    let (mut click, mut enter) = (RegionClick::Absent, Enter::Appear);
    for pair in line.split_whitespace() {
        match pair.split_once('=') {
            Some(("click", "each")) => click = RegionClick::Each,
            Some(("click", value)) => {
                if let Some(step) = read_click(value) {
                    click = RegionClick::Step(step);
                }
            }
            Some(("enter", value)) => {
                if let Some(read) = read_enter(value) {
                    enter = read;
                }
            }
            _ => {}
        }
    }
    (click, enter)
}

/// The blocks each region of `layout` holds, as the engine's `assign_blocks` gives them out.
fn assign_blocks<'a>(slide: &'a Slide, layout: &Layout) -> Vec<Vec<&'a Block>> {
    let index_of = |name: &str| layout.regions.iter().position(|region| region.name == name);
    let main = index_of(layout.main).expect("a layout's main region is one of its regions");
    let mut contents = vec![Vec::new(); layout.regions.len()];
    let mut title = index_of("title");
    let mut current = main;
    for block in &slide.blocks {
        let target = match block {
            Block::Region { name, .. } => {
                title = None;
                current = index_of(name).unwrap_or(main);
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

/// Each baked tag of `blocks`, at any depth, in the order they're written, with its `n`.
fn number_baked(blocks: &[Block], numbers: &mut BTreeMap<usize, usize>) {
    for block in blocks {
        match block {
            Block::Tag(tag) => {
                if is_baked(&tag.name) {
                    let n = numbers.len();
                    numbers.insert(tag.range.start, n);
                }
                number_baked(&tag.children, numbers);
            }
            Block::List { items, .. } => items.iter().for_each(|item| number_baked(item, numbers)),
            _ => {}
        }
    }
}

/// The YAML for a string: JSON's double-quoted string is one YAML reads the same.
fn yaml_string(text: &str) -> String {
    serde_json::Value::String(text.to_string()).to_string()
}

impl<'a> Exporter<'a> {
    fn notice(&mut self, kind: NoticeKind, text: String) {
        let text = shortened(&text.split_whitespace().collect::<Vec<_>>().join(" "), MAX_NOTICE_CHARS);
        let notice = Notice { slide: Some(self.slide.number).filter(|number| *number > 0), kind, text };
        if !self.notices.contains(&notice) {
            self.notices.push(notice);
        }
    }

    fn talk(&mut self, talk: &Talk, title: &str) -> String {
        let headmatter = format!("theme: default\ntitle: {}\ncanvasWidth: 1920\naspectRatio: 16/9\n", yaml_string(title));
        let mut slides = Vec::new();
        for (index, slide) in talk.slides.iter().enumerate() {
            self.slide = SlideState { number: index + 1, ..SlideState::default() };
            if let Some(error) = &slide.error {
                self.notice(NoticeKind::LeftOut, format!("This slide can't be read ({}), so it was left out.", error.message));
                continue;
            }
            let first = slides.is_empty();
            let body = self.slide_body(slide, first);
            let mut keys = if first { headmatter.clone() } else { String::new() };
            keys.push_str(&self.slide_keys(slide, first));
            slides.push(format!("---\n{keys}---\n\n{body}"));
        }
        if slides.is_empty() {
            slides.push(format!("---\n{headmatter}layout: none\n---\n"));
        }
        slides.join("\n")
    }

    /// The slide's own frontmatter keys, each on a line.
    fn slide_keys(&mut self, slide: &Slide, first: bool) -> String {
        let layout = match layout_of(slide) {
            Ok(layout) => layout,
            Err(name) => {
                self.notice(NoticeKind::Approximation, format!("There is no layout called {}, so this slide uses content.", shortened(&name, 80)));
                default_layout()
            }
        };
        let mut keys = format!("layout: {}\n", layout.slidev);
        if let Some(transition) = slide.settings.as_ref().and_then(|settings| settings.get("transition")) {
            let slidev = match transition.trim() {
                "" | "none" => "none",
                "fade" => "fade",
                "push" => "slide-left",
                other => {
                    let text = format!("transition: {} isn't none, fade or push, so this slide has none.", shortened(other, 80));
                    self.notice(NoticeKind::Approximation, text);
                    "none"
                }
            };
            keys.push_str(&format!("transition: {slidev}\n"));
            if first {
                self.deck_transition = slidev != "none";
            }
        } else if self.deck_transition {
            keys.push_str("transition: none\n");
        }
        if is_hidden(slide) {
            keys.push_str("hide: true\n");
        }
        // Slidev counts the clicks its directives ask for, and a picture for each click asks
        // for none, so the slide says how many it takes.
        if self.slide.stepped && self.slide.clicks > 0 {
            keys.push_str(&format!("clicks: {}\n", self.slide.clicks));
        }
        keys
    }

    fn slide_body(&mut self, slide: &Slide, first: bool) -> String {
        let mut out = String::new();
        if first {
            out.push_str(&self.style_block());
            out.push('\n');
        }
        number_baked(&slide.blocks, &mut self.slide.baked_n);
        let layout = layout_of(slide).unwrap_or_else(|_| default_layout());
        let contents = assign_blocks(slide, layout);
        for (region, blocks) in layout.regions.iter().zip(&contents) {
            if blocks.is_empty() {
                continue;
            }
            if layout.name == "two-columns" && region.name == "right" {
                out.push_str("::right::\n\n");
            }
            out.push_str(self.region(slide, region, blocks).trim_end());
            out.push_str("\n\n");
        }
        for block in &slide.blocks {
            if let Block::Tag(tag) = block
                && is_positioned(tag)
            {
                let written = self.tag(tag, true);
                if !written.trim().is_empty() {
                    out.push_str(written.trim_end());
                    out.push_str("\n\n");
                }
            }
        }
        if let Some(notes) = &slide.notes {
            out.push_str(&format!("<!--\n{}\n-->\n", escape::notes(&notes.text)));
        }
        while out.ends_with("\n\n") {
            out.pop();
        }
        out
    }

    /// The talk's colours and fonts, and its own fonts' faces, for the first slide. A slide's
    /// style is its own in Slidev, so each rule says it is the whole talk's.
    fn style_block(&mut self) -> String {
        let theme = &self.theme;
        let mut style = format!(
            "<style>\n:global(:root) {{\n  --slidev-theme-primary: {accent};\n}}\n:global(.slidev-layout) {{\n  background: {background};\n  color: {text};\n  font-family: {font}, sans-serif;\n  font-size: 40px;\n}}\n:global(.slidev-layout h1) {{\n  font-size: 64px;\n}}\n:global(.slidev-layout.cover h1), :global(.slidev-layout.section h1) {{\n  font-size: 96px;\n}}\n:global(.slidev-layout code), :global(.slidev-layout pre) {{\n  font-family: {mono}, monospace;\n}}\n",
            accent = theme.accent,
            background = theme.background,
            text = theme.text,
            font = escape::font_family(&theme.font),
            mono = escape::font_family(&theme.mono),
        );
        let fonts: Vec<(&str, &[u8])> = self
            .files
            .iter()
            .filter(|(name, _)| name.strip_prefix("assets/fonts/").is_some_and(|file| !file.contains('/')))
            .filter(|(name, _)| name.rsplit_once('.').is_some_and(|(_, extension)| matches!(extension.to_ascii_lowercase().as_str(), "ttf" | "otf" | "woff" | "woff2")))
            .map(|(name, data)| (*name, *data))
            .collect();
        for (name, data) in fonts {
            let file = &name["assets/".len()..];
            let Some(url) = self.publish(file, name, data) else { continue };
            let stem = file["fonts/".len()..].rsplit_once('.').map_or(file, |(stem, _)| stem);
            let face = fonts::face(data, stem);
            style.push_str(&format!(
                "@font-face {{\n  font-family: {};\n  src: url(\"{url}\");\n  font-weight: {};\n  font-style: {};\n}}\n",
                escape::font_family(&face.family),
                face.weight,
                if face.italic { "italic" } else { "normal" }
            ));
        }
        style.push_str("</style>\n");
        style
    }

    /// Puts `data`, the talk's file `from`, at `public/<name>`, and gives its URL, or `None`
    /// with a notice if it can't be there.
    fn publish(&mut self, name: &str, from: &str, data: &[u8]) -> Option<String> {
        if !safe_name(name) || is_code_name(name) {
            self.notice(NoticeKind::LeftOut, format!("The file {} can't be in a Slidev talk, so it was left out.", shortened(from, 120)));
            return None;
        }
        let path = format!("public/{name}");
        match self.public_names.get(&path.to_lowercase()) {
            Some(was) if was == from => return Some(public_url(name)),
            Some(_) => {
                self.notice(NoticeKind::LeftOut, format!("The file {} has the name of another in the export, so it was left out.", shortened(from, 120)));
                return None;
            }
            None => {}
        }
        self.public_names.insert(path.to_lowercase(), from.to_string());
        self.public.push((path, data.to_vec()));
        Some(public_url(name))
    }

    /// The URL of the talk's file `src`, put under `public/`, or `None` with a notice.
    fn asset(&mut self, src: &str, what: &str) -> Option<String> {
        let src = src.trim();
        let Some(&data) = self.files.get(src) else {
            self.notice(NoticeKind::LeftOut, format!("The {what} {} isn't in the talk, so it was left out.", shortened(src, 120)));
            return None;
        };
        let name = src.strip_prefix("assets/").unwrap_or(src);
        self.publish(name, src, data)
    }

    /// `work`, with absolutely placed parts placed inside a box at `origin` on the slide.
    fn inside<T>(&mut self, origin: (f32, f32), turned: bool, work: impl FnOnce(&mut Self) -> T) -> T {
        let was = (self.slide.origin, self.slide.turned);
        self.slide.origin = origin;
        self.slide.turned |= turned;
        let result = work(self);
        (self.slide.origin, self.slide.turned) = was;
        result
    }

    fn clicked(&mut self, click: u32) {
        self.slide.clicks = self.slide.clicks.max(click);
    }

    fn region(&mut self, slide: &Slide, region: &RegionRect, blocks: &[&Block]) -> String {
        let line = region_line(slide, region.name);
        let (rect, rotate, moved) = placement(line, region);
        let (click, enter) = region_timing(line);
        let content = if moved { self.inside((rect.x, rect.y), rotate != 0.0, |this| this.blocks(blocks)) } else { self.blocks(blocks) };
        let content = match click {
            RegionClick::Each => {
                let steps = match blocks {
                    [Block::List { items, .. }] => items.len(),
                    _ => blocks.len(),
                };
                self.clicked((steps as u32).min(MAX_CLICK));
                if enter == Enter::FlyUp {
                    self.notice(NoticeKind::Approximation, format!("The {} region comes in a piece at a time, and in Slidev each piece fades in rather than flying up.", region.name));
                }
                wrap("v-clicks", &[], "", &content)
            }
            _ => content,
        };
        let (attrs, timing_css) = match click {
            RegionClick::Step(step) => {
                self.clicked(step);
                timing(step, enter)
            }
            _ => (String::new(), None),
        };
        let mut css = if moved { placed_css(rect.x, rect.y, Some(rect.w), Some(rect.h), rotate) } else { Vec::new() };
        css.extend(timing_css);
        if css.is_empty() && attrs.is_empty() { content } else { wrap("div", &css, &attrs, &content) }
    }

    /// Blocks, each followed by a blank line.
    fn blocks(&mut self, blocks: &[&Block]) -> String {
        let mut out = String::new();
        for block in blocks {
            let written = self.block(block);
            if written.trim().is_empty() {
                continue;
            }
            out.push_str(written.trim_end());
            out.push_str("\n\n");
        }
        out
    }

    fn block(&mut self, block: &Block) -> String {
        let written = match block {
            Block::Heading { level, spans, .. } => format!("{} {}", "#".repeat(usize::from((*level).clamp(1, 6))), self.inline(spans, false)),
            Block::Paragraph { spans, .. } if spans.is_empty() => "&nbsp;".to_string(),
            Block::Paragraph { spans, .. } => self.inline(spans, false),
            Block::List { ordered, items, .. } => return self.list(*ordered, items),
            Block::Table { rows, .. } => self.table(rows),
            Block::Code { language, code, .. } => code_block(language.as_deref(), code),
            Block::Picture { src, alt, .. } => match self.asset(src, "picture") {
                Some(url) => format!("![{}]({url})", escape::text(alt, false)),
                None => String::new(),
            },
            Block::Tag(tag) => return self.tag(tag, false),
            Block::Region { .. } => String::new(),
        };
        if escape::needs_v_pre(&written) { wrap("div", &[], " v-pre", &written) } else { written }
    }

    fn list(&mut self, ordered: bool, items: &[Vec<Block>]) -> String {
        let mut out = String::new();
        for (index, item) in items.iter().enumerate() {
            let marker = if ordered { format!("{}. ", index + 1) } else { "- ".to_string() };
            let mut body = String::new();
            for (at, block) in item.iter().enumerate() {
                let written = self.block(block);
                if written.trim().is_empty() {
                    continue;
                }
                if at > 0 && !body.is_empty() {
                    // A list under the item's text keeps the list tight; anything else is a block of its own.
                    body.push_str(if matches!(block, Block::List { .. }) { "\n" } else { "\n\n" });
                }
                body.push_str(written.trim_end());
            }
            let indent = " ".repeat(marker.len());
            let mut lines = body.lines();
            out.push_str(marker.trim_end());
            if let Some(first) = lines.next().filter(|first| !first.is_empty()) {
                out.push(' ');
                out.push_str(first);
            }
            out.push('\n');
            for line in lines {
                if !line.is_empty() {
                    out.push_str(&indent);
                    out.push_str(line);
                }
                out.push('\n');
            }
        }
        out
    }

    fn table(&mut self, rows: &[Vec<Vec<Span>>]) -> String {
        let Some(header) = rows.first() else { return String::new() };
        let width = header.len().max(1);
        let mut out = String::new();
        for (index, row) in rows.iter().enumerate() {
            let cells: Vec<String> = (0..width).map(|column| row.get(column).map_or(String::new(), |spans| self.inline(spans, true))).collect();
            out.push_str(&format!("| {} |\n", cells.join(" | ")));
            if index == 0 {
                out.push_str(&format!("|{}\n", " --- |".repeat(width)));
            }
        }
        out
    }

    /// A paragraph's or heading's text, on one line. `in_table` is whether it's a cell's, where
    /// a `|` ends the cell even in code.
    fn inline(&mut self, spans: &[Span], in_table: bool) -> String {
        let mut out = String::new();
        for span in spans {
            for (index, piece) in span.text.split('\n').enumerate() {
                if index > 0 {
                    out.push_str("<br>");
                }
                if !piece.is_empty() {
                    let line_start = out.is_empty() && !in_table;
                    out.push_str(&self.span(piece, &span.style, line_start, in_table));
                }
            }
        }
        out
    }

    fn span(&mut self, text: &str, style: &Style, line_start: bool, in_table: bool) -> String {
        let core = text.trim_matches([' ', '\t']);
        let lead = if line_start { "" } else { &text[..text.len() - text.trim_start_matches([' ', '\t']).len()] };
        let trail = &text[text.trim_end_matches([' ', '\t']).len()..];
        if core.is_empty() {
            return format!("{lead}{}", if trail.is_empty() { "" } else { " " });
        }
        let styled = style.color.as_deref().and_then(colour).is_some() || style.size.is_some() || style.underline;
        let mut inner = if style.code {
            code_span(core, in_table)
        } else {
            escape::text(core, line_start && !styled && !style.bold && !style.italic && style.link.is_none())
        };
        if styled {
            let mut css = Vec::new();
            css.extend(style.color.as_deref().and_then(colour).map(|color| format!("color:{color}")));
            css.extend(style.size.map(|size| format!("font-size:{size}px")));
            if style.bold {
                css.push("font-weight:bold".into());
            }
            if style.italic {
                css.push("font-style:italic".into());
            }
            if style.underline {
                css.push("text-decoration:underline".into());
            }
            inner = format!("<span style=\"{}\">{inner}</span>", css.join(";"));
        } else if style.bold || style.italic {
            // Markdown's marks only hold where the text inside starts and ends with a letter or digit.
            let plain = core.chars().next().is_some_and(char::is_alphanumeric) && core.chars().last().is_some_and(char::is_alphanumeric) && !style.code;
            inner = match (style.bold, style.italic, plain) {
                (true, true, true) => format!("***{inner}***"),
                (true, false, true) => format!("**{inner}**"),
                (false, true, true) => format!("*{inner}*"),
                (true, true, false) => format!("<strong><em>{inner}</em></strong>"),
                (true, false, false) => format!("<strong>{inner}</strong>"),
                (false, _, false) => format!("<em>{inner}</em>"),
                (false, false, true) => inner,
            };
        }
        if let Some(url) = &style.link {
            match escape::link(url) {
                Some(url) if styled => inner = format!("<a href=\"{}\">{inner}</a>", escape::attribute(&url)),
                Some(url) => inner = format!("[{inner}]({url})"),
                None => self.notice(NoticeKind::LeftOut, format!("A link to {} could run code when it's followed, so only its text was kept.", shortened(url, 120))),
            }
        }
        format!("{lead}{inner}{trail}")
    }

    fn tag(&mut self, tag: &Tag, top: bool) -> String {
        let place = Place::of(tag);
        let positioned = top && place.is_positioned();
        let click = tag.attr("click").and_then(read_click);
        let enter = tag.attr("enter").and_then(read_enter).unwrap_or(Enter::Appear);
        let (attrs, timing_css) = match click {
            Some(click) => {
                self.clicked(click);
                timing(click, enter)
            }
            None => (String::new(), None),
        };
        let mut css = if positioned {
            placed_css(place.x.unwrap_or(0.0), place.y.unwrap_or(0.0), place.w, place.h, place.rotate.unwrap_or(0.0))
        } else {
            let mut css: Vec<String> = place.w.map(|w| format!("width:{}px", px(w))).into_iter().collect();
            css.extend(place.h.map(|h| format!("height:{}px", px(h))));
            css
        };
        let origin = (place.x.unwrap_or(0.0), place.y.unwrap_or(0.0));
        let turned = place.rotate.is_some_and(|rotate| rotate != 0.0);
        match tag.name.as_str() {
            "Box" => {
                let fill = tag.attr("fill").and_then(colour);
                let outline = tag.attr("outline").and_then(colour);
                let mut outline_width = number(tag.attr("outline-width")).unwrap_or(0.0).clamp(0.0, 256.0);
                if outline.is_some() && outline_width == 0.0 {
                    outline_width = 1.0;
                }
                let radius = number(tag.attr("radius")).unwrap_or(0.0).clamp(0.0, MAX_SIZE);
                let padding = number(tag.attr("padding"))
                    .unwrap_or(if fill.is_some() || outline.is_some() { DEFAULT_BOX_PADDING } else { 0.0 })
                    .clamp(0.0, MAX_SIZE);
                css.extend(fill.map(|fill| format!("background:{fill}")));
                css.extend(outline.as_ref().map(|outline| format!("border:{}px solid {outline}", px(outline_width))));
                if radius > 0.0 {
                    css.push(format!("border-radius:{}px", px(radius)));
                }
                if padding > 0.0 {
                    css.push(format!("padding:{}px", px(padding)));
                }
                css.push("box-sizing:border-box".into());
                css.extend(timing_css);
                let border = if outline.is_some() { outline_width } else { 0.0 };
                let children: Vec<&Block> = tag.children.iter().collect();
                let content = if positioned { self.inside((origin.0 + border, origin.1 + border), turned, |this| this.blocks(&children)) } else { self.blocks(&children) };
                wrap("div", &css, &attrs, &content)
            }
            "Shape" => {
                let kind = shapes::Kind::read(tag.attr("kind"));
                let outline = tag.attr("outline").and_then(colour);
                let mut outline_width = number(tag.attr("outline-width")).unwrap_or(0.0).clamp(0.0, 256.0);
                if outline.is_some() && outline_width == 0.0 {
                    outline_width = 1.0;
                }
                let look = shapes::Look { kind, fill: tag.attr("fill").and_then(colour), outline, outline_width, text: self.theme.text.clone() };
                let (w, h) = (place.w.unwrap_or(DEFAULT_SHAPE.0), place.h.unwrap_or(DEFAULT_SHAPE.1));
                self.arrows += usize::from(kind == shapes::Kind::Arrow);
                let id = format!("arrow-{}", self.arrows);
                let svg = shapes::svg(&look, w, h, &id);
                css.retain(|rule| !rule.starts_with("width:") && !rule.starts_with("height:"));
                if !positioned {
                    css.insert(0, "position:relative".into());
                }
                css.extend([format!("width:{}px", px(w)), format!("height:{}px", px(h))]);
                // What's in a shape sits in its middle, over the shape.
                css.extend(["display:flex", "flex-direction:column", "justify-content:center", "align-items:center", "isolation:isolate"].map(String::from));
                if kind == shapes::Kind::Callout {
                    css.push(format!("padding-bottom:{}px", px(shapes::CALLOUT_TAIL.min(h / 2.0))));
                    css.push("box-sizing:border-box".into());
                }
                css.extend(timing_css);
                let children: Vec<&Block> = tag.children.iter().collect();
                let content = if positioned { self.inside(origin, turned, |this| this.blocks(&children)) } else { self.blocks(&children) };
                let svg = svg.replacen("overflow:visible", "overflow:visible;z-index:-1", 1);
                let open = open("div", &css, &attrs);
                if content.trim().is_empty() { format!("{open}\n{svg}\n</div>\n") } else { format!("{open}\n{svg}\n\n{}\n\n</div>\n", content.trim_end()) }
            }
            "Text" => {
                let mut style = Style::default();
                for attr in &tag.attrs {
                    style.apply_text_attr(&attr.name, &attr.value);
                }
                css.extend(style.color.as_deref().and_then(colour).map(|color| format!("color:{color}")));
                css.extend(style.size.map(|size| format!("font-size:{size}px")));
                if style.bold {
                    css.push("font-weight:bold".into());
                }
                if style.italic {
                    css.push("font-style:italic".into());
                }
                if style.underline {
                    css.push("text-decoration:underline".into());
                }
                css.extend(timing_css);
                let children: Vec<&Block> = tag.children.iter().collect();
                let content = if positioned { self.inside(origin, turned, |this| this.blocks(&children)) } else { self.blocks(&children) };
                if css.is_empty() && attrs.is_empty() { content } else { wrap("div", &css, &attrs, &content) }
            }
            "Picture" => {
                let Some(url) = self.asset(tag.attr("src").unwrap_or_default(), "picture") else { return String::new() };
                css.extend(timing_css);
                format!("{}\n", open("img", &css, &format!(" src=\"{}\" alt=\"\"{attrs}", escape::attribute(&url))))
            }
            "Video" => self.video(tag, css, &attrs, timing_css),
            _ => self.baked(tag, &attrs, timing_css),
        }
    }

    /// The part the app drew for `tag`, which is baked, if it drew it.
    fn drawn(&mut self, tag: &Tag) -> Option<(usize, &'a Baked)> {
        let n = *self.slide.baked_n.get(&tag.range.start)?;
        self.baked.get(&(self.slide.number, n)).filter(|baked| baked.tag == tag.name).map(|baked| (n, *baked))
    }

    /// Where a baked part goes, in the box it's placed in.
    fn baked_css(&mut self, baked: &Baked) -> Vec<String> {
        if self.slide.turned {
            self.notice(NoticeKind::Approximation, format!("The {} is in a box that's turned, so it may not sit where it does on the slide.", shortened(&baked.tag, 80)));
        }
        let (x, y) = (baked.x - self.slide.origin.0, baked.y - self.slide.origin.1);
        placed_css(x, y, Some(baked.w.max(0.0)), Some(baked.h.max(0.0)), if baked.rotate.is_finite() { baked.rotate } else { 0.0 })
    }

    /// The picture `baked/<slide>/<file>`, put under `public/` as `baked-<slide>-<name>`.
    fn baked_file(&mut self, file: &str, name: &str, tag: &str) -> Option<String> {
        let from = format!("baked/{}/{file}", self.slide.number);
        let Some(&data) = self.files.get(from.as_str()) else {
            self.notice(NoticeKind::LeftOut, format!("The picture of the {} wasn't made, so it was left out.", shortened(tag, 80)));
            return None;
        };
        self.publish(&format!("baked-{}-{name}", self.slide.number), &from, data)
    }

    fn video(&mut self, tag: &Tag, mut css: Vec<String>, attrs: &str, timing_css: Option<String>) -> String {
        let Some(url) = self.asset(tag.attr("src").unwrap_or_default(), "video") else { return String::new() };
        let mut extra = format!(" src=\"{}\" controls", escape::attribute(&url));
        if tag.attr("loop").is_some_and(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "" | "true" | "yes" | "1")) {
            extra.push_str(" loop");
        }
        if let Some((n, baked)) = self.drawn(tag) {
            if let Some(poster) = self.baked_file(&format!("{n}.png"), &format!("{n}.png"), &tag.name) {
                extra.push_str(&format!(" poster=\"{}\"", escape::attribute(&poster)));
            }
            css = self.baked_css(baked);
        }
        css.extend(timing_css);
        format!("{}</video>\n", open("video", &css, &format!("{extra}{attrs}")))
    }

    fn baked(&mut self, tag: &Tag, attrs: &str, timing_css: Option<String>) -> String {
        let Some((n, baked)) = self.drawn(tag) else {
            self.notice(NoticeKind::LeftOut, format!("The {} wasn't drawn for the export, so it was left out.", shortened(&tag.name, 80)));
            return String::new();
        };
        let mut css = self.baked_css(baked);
        let mut steps: Vec<u32> = baked.steps.iter().copied().filter(|step| *step <= MAX_CLICK).collect();
        steps.sort_unstable();
        steps.dedup();
        if baked.animated || steps.len() < 2 {
            if baked.animated {
                self.notice(NoticeKind::Frozen, format!("The {} moves on its own, and in Slidev it is a picture of how it starts.", shortened(&tag.name, 80)));
            }
            let Some(url) = self.baked_file(&format!("{n}.png"), &format!("{n}.png"), &tag.name) else { return String::new() };
            css.extend(timing_css);
            return format!("{}\n", open("img", &css, &format!(" src=\"{}\" alt=\"\"{attrs}", escape::attribute(&url))));
        }
        self.slide.stepped = true;
        let mut pictures = Vec::new();
        for (index, step) in steps.iter().enumerate() {
            let Some(url) = self.baked_file(&format!("{n}@{step}.png"), &format!("{n}-{step}.png"), &tag.name) else { return String::new() };
            self.clicked(*step);
            // Each picture shows from its click until the next one's; the first from the start.
            let from = if index == 0 { 0 } else { *step };
            let shown = match steps.get(index + 1) {
                None => format!("$clicks >= {from}"),
                Some(next) if *next == from + 1 => format!("$clicks === {from}"),
                Some(next) if from == 0 => format!("$clicks < {next}"),
                Some(next) => format!("$clicks >= {from} && $clicks < {next}"),
            };
            pictures.push(open("img", &css, &format!(" v-if=\"{shown}\" src=\"{}\" alt=\"\"", escape::attribute(&url))));
        }
        css.clear();
        css.extend(timing_css);
        let mut out = open("div", &css, attrs);
        out.push('\n');
        for picture in pictures {
            out.push_str(&picture);
            out.push('\n');
        }
        out.push_str("</div>\n");
        out
    }
}

/// `code` in backticks enough that none of its own end it.
fn code_span(code: &str, in_table: bool) -> String {
    let longest = longest_run(code, '`');
    let fence = "`".repeat(longest + 1);
    let pad = code.starts_with('`') || code.ends_with('`') || (code.starts_with(' ') && code.ends_with(' ') && code.trim() != "");
    let code = if in_table { code.replace('|', "\\|") } else { code.to_string() };
    if pad { format!("{fence} {code} {fence}") } else { format!("{fence}{code}{fence}") }
}

fn longest_run(text: &str, mark: char) -> usize {
    let (mut longest, mut run) = (0, 0);
    for c in text.chars() {
        run = if c == mark { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    longest
}

fn code_block(language: Option<&str>, code: &str) -> String {
    let fence = "`".repeat(longest_run(code, '`').max(2) + 1);
    let language = language.map(escape::language).unwrap_or_default();
    if code.is_empty() { format!("{fence}{language}\n{fence}") } else { format!("{fence}{language}\n{code}\n{fence}") }
}
