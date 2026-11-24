//! A Slidev talk as a talk of our own: its slides as our Markdown, and the files it shows as
//! the talk's `assets/`. Slides are found as Slidev's own parser finds them, front matter is
//! read as YAML, and clicks are numbered as Slidev numbers them. Nothing is ever run: what
//! can't be carried over, a component, raw HTML, a script, is a labelled box that holds its
//! original text, and is in the report under its slide.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;

use addon_abi::{safe_name, Bundle, Notice, NoticeKind, Report, MANIFEST};
use serde::Deserialize;
use slides_format::{layout_names, parse_talk, split_slides, LayoutNames};

use crate::escape;
use crate::export::{colour, is_code_name, px, BAKED_FOLDER, SLIDES};
use crate::headmatter::{self, Value};
use crate::html::{self, OpenTag};

/// What an import is asked to do, from the operation's `.args.json`.
#[derive(Clone, Debug, Deserialize)]
pub struct ImportArgs {
    /// The talk's Markdown, the manifest's `entry`.
    #[serde(default = "default_entry")]
    pub entry: String,
}

fn default_entry() -> String {
    SLIDES.to_string()
}

impl Default for ImportArgs {
    fn default() -> Self {
        ImportArgs { entry: default_entry() }
    }
}

/// The most notices a report holds.
const MAX_NOTICES: usize = 10_000;
/// The most characters of a notice.
const MAX_NOTICE_CHARS: usize = 500;
/// How many elements deep a slide is read; what's deeper is a labelled box.
const MAX_NESTING: usize = 24;
/// How much inline formatting can be open on one line.
const MAX_INLINE_DEPTH: usize = 32;
/// The most `src:` imports followed in all, and the most they bring in. Imports that import
/// each other many times over could otherwise go on for longer than any run lasts.
const MAX_IMPORTS: usize = 1000;
const MAX_IMPORTED_BYTES: usize = 64 * 1024 * 1024;
/// The most of an import's slide a labelled box holds.
const MAX_PLACEHOLDER_SLIDE: usize = 4096;
/// The most parts of a `src:` range that are read.
const MAX_RANGE_PARTS: usize = 256;
/// How many levels of a list a `<v-clicks depth>` is followed through.
const MAX_LIST_LEVELS: usize = 16;
/// How many files deep `src:` imports are followed.
const MAX_IMPORT_DEPTH: usize = 16;
/// The most slides an import makes. Imports that import each other twice over could make
/// more than any talk has.
const MAX_SLIDES: usize = 5000;
/// As the engine holds clicks.
const MAX_CLICK: i64 = 999;
/// Our slides are this wide; Slidev's are `canvasWidth` wide, 980 unless it says.
const OUR_WIDTH: f32 = 1920.0;
const SLIDEV_WIDTH: f32 = 980.0;
/// The talk's files the importer never reads.
const VENDOR: &str = "vendor/";
const STAGEPACK_MANIFEST: &str = "stagepack.json";
/// The outline of a labelled box.
const PLACEHOLDER_OUTLINE: &str = "#94a3b8";

// ---------------------------------------------------------------------------------------
// Slides, as Slidev's parser finds them.

/// One slide as Slidev's parser finds it in a file.
#[derive(Clone, Debug)]
struct SlidevSlide {
    /// The slide as written, for a labelled box.
    raw: String,
    /// Its front matter as YAML, or why it can't be read.
    front: Result<Rc<Value>, String>,
    /// How long its front matter is, as it counts against an import's budget.
    front_size: usize,
    content: String,
    note: Option<String>,
}

/// JavaScript's `trim`, which takes a byte order mark as white space too.
fn js_trim(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}

/// Whether a line leaves Slidev's splitter inside an HTML comment, as its
/// `advanceHtmlCommentState` has it.
fn comment_state(line: &str, mut inside: bool) -> bool {
    let mut cursor = 0;
    while cursor < line.len() {
        if inside {
            match line[cursor..].find("-->") {
                None => return true,
                Some(end) => {
                    inside = false;
                    cursor += end + 3;
                }
            }
        } else {
            let Some(start) = line[cursor..].find("<!--") else { return false };
            let start = cursor + start;
            match line[start + 4..].find("-->") {
                None => return true,
                Some(end) => cursor = start + 4 + end + 3,
            }
        }
    }
    inside
}

fn is_js_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

/// JavaScript's `trimStart`, and its `\s`.
fn js_trim_start(text: &str) -> &str {
    text.trim_start_matches(is_js_space)
}

/// Where the fences of a file close, as Slidev's splitter finds them: at the next line that
/// starts with the same white space and as many backticks. Lines are kept by the white space
/// they start with, and each search remembers what it found, so a file of fences that never
/// close is read in one pass rather than one pass for each.
struct Fences<'l> {
    /// The lines that start with white space and a backtick, by that white space: each line's
    /// index and how many backticks start it.
    by_lead: HashMap<&'l str, Vec<(usize, usize)>>,
    /// For each of those lists, the most backticks of any line from each place on, so that
    /// a fence nothing can close is known at once.
    most_after: HashMap<&'l str, Vec<usize>>,
    /// For a lead and a number of backticks, the last line searched after and what was found.
    found: HashMap<(&'l str, usize), (usize, Option<usize>)>,
}

impl<'l> Fences<'l> {
    fn new(lines: &[&'l str]) -> Fences<'l> {
        let mut by_lead: HashMap<&'l str, Vec<(usize, usize)>> = HashMap::new();
        for (index, line) in lines.iter().enumerate() {
            let rest = js_trim_start(line);
            if rest.starts_with('`') {
                let lead = &line[..line.len() - rest.len()];
                let ticks = rest.bytes().take_while(|byte| *byte == b'`').count();
                by_lead.entry(lead).or_default().push((index, ticks));
            }
        }
        let most_after = by_lead
            .iter()
            .map(|(lead, lines)| {
                let mut most = vec![0; lines.len()];
                let mut so_far = 0;
                for (index, (_, ticks)) in lines.iter().enumerate().rev() {
                    so_far = so_far.max(*ticks);
                    most[index] = so_far;
                }
                (*lead, most)
            })
            .collect();
        Fences { by_lead, most_after, found: HashMap::new() }
    }

    /// The first line after `after` that starts with `lead` and at least `ticks` backticks.
    fn close(&mut self, lead: &'l str, ticks: usize, after: usize) -> Option<usize> {
        if let Some(&(from, found)) = self.found.get(&(lead, ticks))
            && from <= after
            && found.is_none_or(|line| line > after)
        {
            return found;
        }
        let lines = self.by_lead.get(lead)?;
        let start = lines.partition_point(|(index, _)| *index <= after);
        if self.most_after.get(lead).and_then(|most| most.get(start)).is_none_or(|most| *most < ticks) {
            return None;
        }
        let found = lines[start..].iter().find(|(_, run)| *run >= ticks).map(|(index, _)| *index);
        self.found.insert((lead, ticks), (after, found));
        found
    }
}

/// The slides of `markdown`, as Slidev's `parse` splits them: on lines that start with
/// `---`, past code fences and comments, each with the front matter block that follows.
fn split(markdown: &str) -> Vec<SlidevSlide> {
    let lines: Vec<&str> = markdown.split('\n').collect();
    let mut slides = Vec::new();
    let mut start = 0;
    let mut in_comment = false;
    // As Slidev's `slice`: nothing is made, and `start` stays, when there's nothing to slice.
    let slice = |start: &mut usize, end: usize, slides: &mut Vec<SlidevSlide>| {
        if *start == end {
            return;
        }
        slides.push(parse_slide(lines[*start..end].join("\n")));
        *start = end + 1;
    };
    let mut fences = Fences::new(&lines);
    let mut i = 0;
    while i < lines.len() {
        let raw = lines[i];
        let line = raw.trim_end();
        if in_comment {
            in_comment = comment_state(raw, true);
        } else if line.starts_with("---") {
            slice(&mut start, i, &mut slides);
            let next = lines.get(i + 1);
            if line.as_bytes().get(3) != Some(&b'-') && next.is_some_and(|next| !next.trim().is_empty()) {
                start = i;
                i += 1;
                while i < lines.len() && lines[i].trim_end() != "---" {
                    i += 1;
                }
            }
        } else if js_trim_start(line).starts_with("```") {
            let blank = line.len() - js_trim_start(line).len();
            let ticks = line[blank..].bytes().take_while(|byte| *byte == b'`').count();
            if let Some(close) = fences.close(&line[..blank], ticks, i) {
                i = close;
            }
        } else {
            in_comment = comment_state(raw, false);
        }
        i += 1;
    }
    if start < lines.len() {
        slice(&mut start, lines.len(), &mut slides);
    }
    slides
}

/// One slide's text as Slidev's `parseSlide` reads it: its front matter, its content and
/// its notes, the comment that ends it.
fn parse_slide(raw: String) -> SlidevSlide {
    let (yaml, rest) = front_matter(&raw);
    let mut content = js_trim(&rest).to_string();
    let mut note = None;
    if let Some((at, inner)) = last_comment(&content) {
        note = Some(js_trim(&inner).to_string());
        content = js_trim(&content[..at]).to_string();
    }
    let front_size = yaml.as_ref().map_or(0, String::len);
    let front = match yaml {
        Some(yaml) => headmatter::read(&yaml).map(Rc::new),
        None => Ok(Rc::new(Value::Map(Vec::new()))),
    };
    SlidevSlide { raw, front, front_size, content, note }
}

/// The YAML of the slide's front matter, as `/^---.*\r?\n([\s\S]*?)---/` or a `yaml` code
/// block at its start finds it, and the rest of its text.
fn front_matter(raw: &str) -> (Option<String>, String) {
    if let Some(after) = raw.strip_prefix("---")
        && let Some(newline) = after.find('\n')
        && !after[..newline].contains(['\u{2028}', '\u{2029}'])
    {
        let body = 3 + newline + 1;
        if let Some(close) = raw[body..].find("---") {
            return (Some(raw[body..body + close].to_string()), raw[body + close + 3..].to_string());
        }
    }
    let lead = raw.len() - js_trim_start(raw).len();
    let rest = &raw[lead..];
    for opening in ["```yaml", "```yml"] {
        if let Some(after) = rest.strip_prefix(opening)
            && let Some(close) = after.find("```")
        {
            let end = lead + opening.len() + close + 3;
            return (Some(after[..close].to_string()), raw[end..].to_string());
        }
    }
    (None, raw.to_string())
}

/// Where the last comment of `content` starts, and what's in it, if it ends the content.
fn last_comment(content: &str) -> Option<(usize, String)> {
    let mut last = None;
    let mut from = 0;
    while let Some(start) = content[from..].find("<!--") {
        let start = from + start;
        let Some(end) = content[start + 4..].find("-->") else { break };
        let end = start + 4 + end;
        last = Some((start, end));
        from = end + 3;
    }
    let (start, end) = last?;
    (end + 3 >= content.len()).then(|| (start, content[start + 4..end].to_string()))
}

/// The slides a `#range` names, as Slidev's `parseRangeString` reads it, of `total`: as
/// sorted runs `from..=to` that don't overlap, so that a range of many parts, or one that
/// names a slide many times, costs no more than the slides it names. No more than
/// [`MAX_RANGE_PARTS`] parts are read; the second value says whether there were more.
fn range_of(total: usize, range: Option<&str>) -> (Vec<(usize, usize)>, bool) {
    let range = range.unwrap_or("");
    if range.is_empty() || range == "all" || range == "*" {
        return (if total == 0 { Vec::new() } else { vec![(1, total)] }, false);
    }
    if range == "none" {
        return (Vec::new(), false);
    }
    let number = |text: &str| -> Option<usize> {
        let text = text.trim();
        if text.is_empty() {
            return Some(0);
        }
        text.parse::<f64>().ok().filter(|number| number.is_finite() && *number >= 0.0).map(|number| number.min((total + 1) as f64) as usize)
    };
    let mut runs = Vec::new();
    let mut parts = range.split([',', ';']);
    for part in parts.by_ref().take(MAX_RANGE_PARTS) {
        let (from, to) = match part.split_once('-') {
            None => match number(part) {
                Some(index) => (index, index),
                None => continue,
            },
            Some((from, to)) => {
                let from = number(from).unwrap_or(0);
                let to = if to.trim().is_empty() { total } else { number(to.split('-').next().unwrap_or("")).unwrap_or(0) };
                (from, to)
            }
        };
        let (from, to) = (from.max(1), to.min(total));
        if from <= to {
            runs.push((from, to));
        }
    }
    let more = parts.next().is_some();
    runs.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(runs.len());
    for (from, to) in runs {
        match merged.last_mut() {
            Some(last) if from <= last.1 + 1 => last.1 = last.1.max(to),
            _ => merged.push((from, to)),
        }
    }
    (merged, more)
}

// ---------------------------------------------------------------------------------------
// Loading the talk, with its imports.

/// Why an import's slides can't be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unread {
    Outside,
    Missing,
    Loop,
    TooDeep,
    NotText,
    /// Past the most an import follows.
    TooMuch,
}

/// A slide the talk shows, as Slidev loads it.
#[derive(Clone, Debug)]
struct Loaded {
    front: Front,
    /// Why the slide's own front matter can't be read.
    front_error: Option<String>,
    content: String,
    note: Option<String>,
    /// The file it is in.
    file: String,
    /// An import that couldn't be read: what it named, why, and the slide that named it.
    unread: Option<(String, Unread, String)>,
}

/// A slide's front matter as Slidev merges it, `{ ...own, ...importer, ...its importer }`, kept
/// as the blocks it's merged from, each shared, so that an import's front matter isn't copied
/// into each slide it brings in. The slide's own block is first; later ones win.
#[derive(Clone, Debug, Default)]
struct Front {
    layers: Vec<Rc<Value>>,
}

impl Front {
    fn get(&self, key: &str) -> Option<&Value> {
        // An importer's own `src` is the import itself, which Slidev takes out.
        self.layers.iter().enumerate().rev().filter(|(index, _)| *index == 0 || key != "src").find_map(|(_, layer)| layer.get(key))
    }

    fn text(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::text)
    }
}

fn get<'v>(front: &'v Front, key: &str) -> Option<&'v Value> {
    front.get(key)
}

fn get_text<'v>(front: &'v Front, key: &str) -> Option<&'v str> {
    front.text(key)
}

/// The folder of the file `path`, with its `/`, or nothing for the talk's own folder.
fn folder_of(path: &str) -> &str {
    path.rfind('/').map_or("", |at| &path[..=at])
}

/// `path`, made of `/`-separated parts, with `.` and `..` read. `None` if it leaves the talk.
fn normalized(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

// ---------------------------------------------------------------------------------------
// Clicks, as Slidev counts them.

/// When something comes in, as Slidev's `at` says: so many clicks after the ones before it,
/// or at a click of its own.
#[derive(Clone, Copy, Debug, PartialEq)]
enum At {
    After(i64),
    On(i64),
}

/// A slide's clicks as Slidev's clicks context counts them: each thing that comes in is
/// registered in the order it's mounted, and moves the count on by as much as it's after
/// the last.
#[derive(Default)]
struct Clicks {
    offset: i64,
}

impl Clicks {
    /// Registers something of `size` clicks at `at`, and gives the click it starts at.
    fn since(&mut self, at: At, size: i64) -> i64 {
        let (start, delta) = match at {
            At::After(value) => (self.offset + value, value + size - 1),
            At::On(value) => (value, 0),
        };
        self.offset = (self.offset + delta).clamp(-1_000_000, 1_000_000);
        start
    }

    /// The gap a `<v-clicks>` leaves after its pieces.
    fn gap(&mut self, size: i64) {
        self.offset = (self.offset + size).clamp(-1_000_000, 1_000_000);
    }
}

/// A number as JavaScript's `+` reads it, rounded up to a whole click.
fn js_number(text: &str) -> Option<f64> {
    let text = text.trim();
    if text.is_empty() {
        return Some(0.0);
    }
    text.parse::<f64>().ok().filter(|number| number.is_finite()).map(|number| number.clamp(-1e6, 1e6))
}

/// An `at` as Slidev's `normalizeSingleAtValue` reads it. `Err` for one it can't read, and
/// `Ok(None)` for one that turns it off.
fn at_value(text: &str) -> Result<Option<At>, ()> {
    match text {
        "false" => return Ok(None),
        "true" => return Ok(Some(At::After(1))),
        _ => {}
    }
    // Slidev looks at the first character as written: ` +1` is the number 1.
    if text.starts_with(['+', '-']) {
        return js_number(text).map(|value| Some(At::After(value.ceil() as i64))).ok_or(());
    }
    let value = js_number(text).ok_or(())?;
    Ok(Some(At::On(if value <= 0.0 { 1 } else { value.ceil() as i64 })))
}

// ---------------------------------------------------------------------------------------
// What a slide's content is made into.

/// How a part comes in when its click arrives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Enter {
    Appear,
    Fade,
    FlyUp,
}

impl Enter {
    fn attr(self) -> Option<&'static str> {
        match self {
            Enter::Appear => None,
            Enter::Fade => Some("fade"),
            Enter::FlyUp => Some("fly-up"),
        }
    }
}

/// How a `<v-clicks>` numbers what's in it, as `VClicks.ts` does: each thing takes the next
/// index, shown at `at + ceil(index / every) - 1`, and is registered as so many clicks after the
/// one registered before it, or at its own click for an `at` that isn't relative.
struct Counter {
    at: i64,
    every: f64,
    relative: bool,
    count: i64,
    done: i64,
}

impl Counter {
    /// The click the next thing is shown at.
    fn index(&mut self) -> i64 {
        self.count += 1;
        self.at + (self.count as f64 / self.every).ceil() as i64 - 1
    }

    /// Registers a thing shown at `show`, and gives the click it starts at.
    fn register(&mut self, show: i64, clicks: &mut Clicks) -> i64 {
        let value = if self.relative { At::After(show - self.done) } else { At::On(show) };
        self.done = show;
        clicks.since(value, 1)
    }

    fn next(&mut self, clicks: &mut Clicks) -> i64 {
        let show = self.index();
        self.register(show, clicks)
    }

    /// The size of the gap `VClickGap` leaves after them.
    fn gap(&self) -> i64 {
        self.at + (self.count as f64 / self.every).ceil() as i64 - 1 - self.done
    }
}

/// Numbers the items of the lists under `item`, at `level`, and those under them up to
/// `levels`, as `VClicks` does. Whether there were any.
fn count_nested(item: &str, level: usize, levels: usize, counter: &mut Counter, clicks: &mut Clicks) -> bool {
    let Some(list) = nested_list(item) else { return false };
    for sub in list_items(list) {
        let show = counter.index();
        if level < levels {
            count_nested(sub, level + 1, levels, counter, clicks);
        }
        counter.register(show, clicks);
    }
    true
}

/// The first list under a list item: from its first line after the item's own that starts
/// with a list marker, however far in.
fn nested_list(item: &str) -> Option<&str> {
    let mut at = line_at(item, 0).1;
    while at < item.len() {
        let (end, next) = line_at(item, at);
        if marker_length(&item[at..end]).is_some() {
            return Some(item[at..].trim_end_matches('\n'));
        }
        at = next;
    }
    None
}

/// A `<v-clicks>`: its pieces as one, for a region shown a piece at a time, and each in a box
/// of its own, at its click.
#[derive(Clone, Debug)]
struct Each {
    whole: String,
    boxed: String,
    /// Whether its pieces come in at clicks 1, 2, 3 and on, as a region's do.
    from_one: bool,
    /// What's said when it's written in boxes.
    boxed_notice: Option<String>,
}

/// One block of a slide, written in our Markdown.
#[derive(Clone, Debug)]
enum Item {
    Region(String),
    Heading(String),
    /// `counts` is whether Slidev takes it for one element, as a `<v-clicks>` counts them.
    Block { text: String, counts: bool },
    List { text: String },
    Each(Each),
}

impl Item {
    fn counts(&self) -> bool {
        match self {
            Item::Region(_) => false,
            Item::Block { counts, .. } => *counts,
            _ => true,
        }
    }

    /// The item as written inside something else, where no region is shown a piece at a time.
    fn text(&self) -> &str {
        match self {
            Item::Region(_) => "",
            Item::Heading(text) | Item::Block { text, .. } | Item::List { text } => text,
            Item::Each(each) => &each.boxed,
        }
    }
}

fn block(text: String) -> Item {
    Item::Block { text, counts: true }
}

/// Items, each on its own, as written inside something else.
fn written(items: &[Item]) -> String {
    items.iter().map(Item::text).filter(|text| !text.trim().is_empty()).collect::<Vec<_>>().join("\n\n")
}

/// A tag of ours: its attributes, bare ones with no value, around `children`.
fn our_tag(name: &str, attrs: &[(&str, Option<String>)], children: &str) -> String {
    let mut open = format!("<{name}");
    for (attr, value) in attrs {
        match value {
            Some(value) => open.push_str(&format!(" {attr}=\"{}\"", attr_value(value))),
            None => open.push_str(&format!(" {attr}")),
        }
    }
    if children.trim().is_empty() { format!("{open} />") } else { format!("{open}>\n\n{}\n\n</{name}>", children.trim_end()) }
}

/// A value as an attribute of ours holds it: on one line, with no quote to end it.
fn attr_value(value: &str) -> String {
    value.chars().map(|c| if c.is_control() { ' ' } else { c }).filter(|c| *c != '"').collect()
}

/// A setting's value, on one line.
fn setting_value(value: &str) -> String {
    let line: String = value.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    line.trim().to_string()
}

/// `text`, in the middle of a line, as Markdown text that reads as itself: what would start
/// a tag, code, a link, emphasis or a character reference escaped.
fn plain_markdown(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '!' | '|' | '~' | '&' | '$' | '#') {
            out.push('\\');
            out.push(c);
        } else if c.is_control() {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}

fn shortened(text: &str, most: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= most {
        return text;
    }
    let mut short: String = text.chars().take(most.saturating_sub(1)).collect();
    short.push('…');
    short
}

fn longest_run(text: &str, mark: char) -> usize {
    let (mut longest, mut run) = (0, 0);
    for c in text.chars() {
        run = if c == mark { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    longest
}

/// `code` as a fenced code block, with a fence none of its own lines can close.
fn fenced(indent: &str, language: &str, code: &str) -> String {
    let fence = "`".repeat(longest_run(code, '`').max(2) + 1);
    let mut out = format!("{indent}{fence}{language}\n");
    if !code.is_empty() {
        out.push_str(code);
        out.push('\n');
    }
    out.push_str(indent);
    out.push_str(&fence);
    out
}

/// `code` in backticks enough that none of its own end it.
fn code_span(code: &str) -> String {
    let code = code.replace('\n', " ");
    if code.trim().is_empty() {
        return plain_markdown(&code);
    }
    let fence = "`".repeat(longest_run(&code, '`') + 1);
    let pad = code.starts_with('`') || code.ends_with('`') || (code.starts_with(' ') && code.ends_with(' '));
    if pad { format!("{fence} {code} {fence}") } else { format!("{fence}{code}{fence}") }
}

/// The notes as written inside `<Notes>`, as the format's own `escape_notes` writes them, so
/// that the format reads back what was written.
fn notes_text(notes: &str) -> String {
    let mut out = String::with_capacity(notes.len() + notes.len() / 8);
    for (n, line) in notes.split('\n').enumerate() {
        if n > 0 {
            out.push('\n');
        }
        if line.trim_end_matches(['\r', ' ']) == "---" {
            out.push('\\');
        }
        let chars: Vec<char> = line.chars().collect();
        let last_bar = chars.iter().rposition(|&c| !matches!(c, ' ' | '\r')).filter(|&at| chars[at] == '|');
        for (i, &c) in chars.iter().enumerate() {
            let starts_a_fence = matches!(c, '`' | '~') && (i == 0 || chars[i - 1] != c) && chars.get(i + 1) == Some(&c) && chars.get(i + 2) == Some(&c);
            if matches!(c, '\\' | '<') || starts_a_fence || last_bar == Some(i) {
                out.push('\\');
            }
            out.push(c);
        }
    }
    out
}

// ---------------------------------------------------------------------------------------
// The blocks of a slide's content.

/// One block of a slide's content, as Markdown-it would find it.
#[derive(Debug)]
enum Raw<'t> {
    Fence { indent: &'t str, info: &'t str, code: String },
    Slot(&'t str),
    Element { tag: OpenTag, inner: Option<&'t str>, source: &'t str },
    Unclosed { tag: OpenTag, source: &'t str },
    StrayClose(&'t str),
    Comment,
    Markdown(&'t str),
}

/// The end of the line at `at`, and where the next one starts.
fn line_at(text: &str, at: usize) -> (usize, usize) {
    match text[at..].find('\n') {
        Some(end) => (at + end, at + end + 1),
        None => (text.len(), text.len()),
    }
}

/// How far in a line's text starts, a tab going on to the next multiple of four.
fn indent_of(line: &str) -> (usize, usize) {
    let mut column = 0;
    let mut bytes = 0;
    for byte in line.bytes() {
        match byte {
            b' ' => column += 1,
            b'\t' => column += 4 - column % 4,
            _ => break,
        }
        bytes += 1;
    }
    (column, bytes)
}

/// The fence a line opens: its mark, how many, and what follows them.
fn fence_opens(line: &str) -> Option<(u8, usize, &str)> {
    let (column, lead) = indent_of(line);
    if column > 3 {
        return None;
    }
    let rest = &line[lead..];
    let mark = *rest.as_bytes().first().filter(|mark| matches!(mark, b'`' | b'~'))?;
    let length = rest.bytes().take_while(|byte| *byte == mark).count();
    let info = &rest[length..];
    (length >= 3 && (mark == b'~' || !info.contains('`'))).then_some((mark, length, info))
}

fn fence_closes(line: &str, mark: u8, length: usize) -> bool {
    let (column, lead) = indent_of(line);
    let rest = &line[lead..];
    let run = rest.bytes().take_while(|byte| *byte == mark).count();
    column <= 3 && run >= length && rest[run..].trim().is_empty()
}

/// The name of the slot a line marks, as Slidev's `::name::` is read.
fn slot_name(line: &str) -> Option<&str> {
    let inner = line.strip_prefix("::")?.trim_end().strip_suffix("::")?.trim();
    (!inner.is_empty() && inner.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-' | b':'))).then_some(inner)
}

/// The marker of a list item that starts `line`, no more than three columns in: its length.
fn list_marker(line: &str) -> Option<usize> {
    if indent_of(line).0 > 3 {
        return None;
    }
    marker_length(line)
}

/// The marker of a list item that starts `line`, however far in: its length.
fn marker_length(line: &str) -> Option<usize> {
    let (_, lead) = indent_of(line);
    let rest = &line.as_bytes()[lead..];
    let digits = rest.iter().take_while(|byte| byte.is_ascii_digit()).count();
    let length = match (digits, rest.get(digits)) {
        (0, Some(b'-' | b'*' | b'+')) => 1,
        (1..=9, Some(b'.' | b')')) => digits + 1,
        _ => return None,
    };
    matches!(rest.get(length), None | Some(b' ' | b'\t')).then_some(length)
}

fn is_atx_heading(line: &str) -> bool {
    let (column, lead) = indent_of(line);
    let rest = &line[lead..];
    let hashes = rest.bytes().take_while(|byte| *byte == b'#').count();
    column <= 3 && (1..=6).contains(&hashes) && matches!(rest.as_bytes().get(hashes), None | Some(b' ' | b'\t'))
}

/// Inline elements, which Markdown-it reads in a paragraph's text.
fn is_inline(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "a" | "abbr" | "b" | "bdi" | "bdo" | "br" | "cite" | "code" | "data" | "dfn" | "em" | "font" | "i" | "kbd" | "mark" | "q" | "s" | "samp" | "small" | "span"
            | "strong" | "sub" | "sup" | "time" | "u" | "var" | "wbr" | "del" | "ins"
    )
}

/// Elements whose text is never read for tags.
fn is_raw_text(name: &str) -> bool {
    matches!(name.to_ascii_lowercase().as_str(), "script" | "style" | "textarea")
}

/// Where the element opened by `tag`, at `start` in `text`, ends: where what's in it ends,
/// and just past its closing tag. `None` when it's never closed.
fn element_end(text: &str, tag: &OpenTag, runs: &mut Runs) -> Option<(usize, usize)> {
    if tag.self_closing || html::is_void(&tag.name) {
        return Some((tag.end, tag.end));
    }
    let bytes = text.as_bytes();
    if is_raw_text(&tag.name) {
        let close = format!("</{}", tag.name.to_ascii_lowercase());
        let mut from = tag.end;
        while let Some(at) = find_folded(text, from, &close) {
            if let Some((_, end)) = html::close_tag(text, at) {
                return Some((at, end));
            }
            from = at + close.len();
        }
        return None;
    }
    let mut depth = 1;
    let mut at = tag.end;
    let mut line_start = false;
    while at < bytes.len() {
        if line_start {
            line_start = false;
            let (end, next) = line_at(text, at);
            if let Some((mark, length, _)) = fence_opens(&text[at..end]) {
                // Past the fenced code, to the line after its closing fence.
                let mut line = next;
                loop {
                    if line >= bytes.len() {
                        return None;
                    }
                    let (end, after) = line_at(text, line);
                    let closes = fence_closes(&text[line..end], mark, length);
                    line = after;
                    if closes {
                        break;
                    }
                }
                at = line;
                line_start = true;
                continue;
            }
        }
        match bytes[at] {
            b'\n' => {
                line_start = true;
                at += 1;
            }
            b'`' => {
                let run = bytes[at..].iter().take_while(|byte| **byte == b'`').count();
                at += run;
                if let Some(close) = runs.closing(text, at, run) {
                    at = close + run;
                }
            }
            b'<' if text[at..].starts_with("<!--") => at = at + 4 + text[at + 4..].find("-->")? + 3,
            b'<' => {
                if let Some((name, end)) = html::close_tag(text, at)
                    && html::same_name(&name, &tag.name)
                {
                    depth -= 1;
                    if depth == 0 {
                        return Some((at, end));
                    }
                    at = end;
                } else if let Some(open) = html::open_tag(text, at)
                    && html::same_name(&open.name, &tag.name)
                {
                    if !open.self_closing {
                        depth += 1;
                    }
                    at = open.end;
                } else {
                    at += 1;
                }
            }
            _ => at += 1,
        }
    }
    None
}

/// A search for one pattern in one text, which remembers what it found: a search from a later
/// place gives the same answer without looking again while that answer still holds. Each of
/// a slide's unclosed constructs then costs one pass, not one pass for each.
#[derive(Default)]
struct Memo {
    last: Option<(usize, Option<usize>)>,
}

impl Memo {
    fn find(&mut self, text: &str, from: usize, pattern: &str) -> Option<usize> {
        self.find_with(from, |from| text.get(from..).and_then(|rest| rest.find(pattern)).map(|at| from + at))
    }

    /// As [`Memo::find`], with ASCII letters in any case.
    fn find_folded(&mut self, text: &str, from: usize, pattern: &str) -> Option<usize> {
        self.find_with(from, |from| find_folded(text, from, pattern))
    }

    fn find_with(&mut self, from: usize, search: impl FnOnce(usize) -> Option<usize>) -> Option<usize> {
        if let Some((searched, found)) = self.last
            && searched <= from
            && found.is_none_or(|at| at >= from)
        {
            return found;
        }
        let found = search(from);
        self.last = Some((from, found));
        found
    }
}

/// Where `pattern`, which is ASCII, is next in `text` from `from`, in any case, without
/// copying the text.
fn find_folded(text: &str, from: usize, pattern: &str) -> Option<usize> {
    let (bytes, pattern) = (text.as_bytes(), pattern.as_bytes());
    if pattern.is_empty() || bytes.len() < pattern.len() {
        return None;
    }
    (from..=bytes.len() - pattern.len()).find(|at| bytes[*at..*at + pattern.len()].eq_ignore_ascii_case(pattern))
}

/// Where each run of backticks in one text starts, by its length, made the first time it's
/// asked for, so that finding the run that closes a code span never reads the text again.
#[derive(Default)]
struct Runs {
    starts: Option<HashMap<usize, Vec<usize>>>,
}

impl Runs {
    /// The first run of exactly `run` backticks from `from`, which is never
    /// in the middle of a run.
    fn closing(&mut self, text: &str, from: usize, run: usize) -> Option<usize> {
        let starts = self.starts.get_or_insert_with(|| {
            let bytes = text.as_bytes();
            let mut starts: HashMap<usize, Vec<usize>> = HashMap::new();
            let mut at = 0;
            while at < bytes.len() {
                if bytes[at] == b'`' {
                    let length = bytes[at..].iter().take_while(|byte| **byte == b'`').count();
                    starts.entry(length).or_default().push(at);
                    at += length;
                } else {
                    at += 1;
                }
            }
            starts
        });
        let list = starts.get(&run)?;
        list.get(list.partition_point(|start| *start < from)).copied()
    }
}

/// The searches one run of text makes, each remembered.
#[derive(Default)]
struct Searches {
    comment: Memo,
    braces: Memo,
    code: Memo,
    label: Memo,
    alt_line: Memo,
    paren: Memo,
    target_line: Memo,
    runs: Runs,
}


/// The blocks of `text`, a slide's content or what's in an element.
fn scan(text: &str) -> Vec<Raw<'_>> {
    let mut blocks = Vec::new();
    let mut runs = Runs::default();
    let mut at = 0;
    while at < text.len() {
        let (end, next) = line_at(text, at);
        let line = &text[at..end];
        if line.trim().is_empty() {
            at = next;
            continue;
        }
        let (column, lead) = indent_of(line);
        let start = at + lead;
        if column <= 3 {
            if let Some((mark, length, info)) = fence_opens(line) {
                let mut code = Vec::new();
                let mut line_at_ = next;
                while line_at_ < text.len() {
                    let (end, next) = line_at(text, line_at_);
                    let code_line = &text[line_at_..end];
                    line_at_ = next;
                    if fence_closes(code_line, mark, length) {
                        break;
                    }
                    code.push(code_line);
                }
                blocks.push(Raw::Fence { indent: &line[..lead], info: info.trim(), code: code.join("\n") });
                at = line_at_;
                continue;
            }
            if column == 0
                && let Some(name) = slot_name(line)
            {
                blocks.push(Raw::Slot(name));
                at = next;
                continue;
            }
            if text[start..].starts_with("<!--") {
                blocks.push(Raw::Comment);
                at = match text[start + 4..].find("-->") {
                    Some(close) => after_element(text, start + 4 + close + 3),
                    None => text.len(),
                };
                continue;
            }
            if let Some((_, close_end)) = html::close_tag(text, start) {
                blocks.push(Raw::StrayClose(&text[start..close_end]));
                at = after_element(text, close_end);
                continue;
            }
            if let Some(tag) = html::open_tag(text, start) {
                let inline = is_inline(&tag.name) || (tag.name.eq_ignore_ascii_case("img") && !text[tag.end..line_at(text, tag.end).0].trim().is_empty());
                if !inline {
                    match element_end(text, &tag, &mut runs) {
                        Some((inner_end, end)) => {
                            let inner = (inner_end > tag.end).then(|| &text[tag.end..inner_end]);
                            let source = &text[start..end];
                            blocks.push(Raw::Element { tag, inner, source });
                            at = after_element(text, end);
                        }
                        None => {
                            blocks.push(Raw::Unclosed { tag, source: &text[start..] });
                            at = text.len();
                        }
                    }
                    continue;
                }
            }
        }
        let chunk_end = markdown_end(text, at);
        blocks.push(Raw::Markdown(&text[at..chunk_end]));
        at = chunk_end;
    }
    blocks
}

/// Where the next block starts after an element that ends at `end`: the next line, if
/// nothing but white space follows it on its own.
fn after_element(text: &str, end: usize) -> usize {
    let (line_end, next) = line_at(text, end);
    if text[end..line_end].trim().is_empty() { next } else { end }
}

/// Where code by indentation that goes on at `line` ends: at the first line indented less
/// than four columns, as such code has no lazy lines. Blank lines inside it are its own.
fn indented_code_end(text: &str, mut line: usize) -> usize {
    let mut end = line;
    while line < text.len() {
        let (line_end, next) = line_at(text, line);
        let this = &text[line..line_end];
        if !this.trim().is_empty() {
            if indent_of(this).0 < 4 {
                break;
            }
            end = next;
        }
        line = next;
    }
    end
}

/// `code`'s lines with the four columns that make it code taken off.
fn dedented(code: &str) -> String {
    code.split('\n')
        .map(|line| {
            let mut column = 0;
            let mut cut = 0;
            for byte in line.bytes() {
                if column >= 4 {
                    break;
                }
                match byte {
                    b' ' => column += 1,
                    b'\t' => column += 4 - column % 4,
                    _ => break,
                }
                cut += 1;
            }
            &line[cut..]
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Where a run of Markdown that starts at `at` ends: at a blank line, but for one inside a
/// list, or at a line that starts a block of another kind.
fn markdown_end(text: &str, at: usize) -> usize {
    let (first_end, first_next) = line_at(text, at);
    let first = &text[at..first_end];
    if is_atx_heading(first) {
        return first_next;
    }
    if indent_of(first).0 >= 4 {
        return indented_code_end(text, first_next);
    }
    let list = list_marker(first).is_some();
    let mut end = first_next;
    let mut line = first_next;
    while line < text.len() {
        let (line_end, next) = line_at(text, line);
        let this = &text[line..line_end];
        if this.trim().is_empty() {
            if !list {
                break;
            }
            // A list goes on past a blank line to an item, or to what's indented under one.
            let mut ahead = next;
            while ahead < text.len() {
                let (ahead_end, ahead_next) = line_at(text, ahead);
                if !text[ahead..ahead_end].trim().is_empty() {
                    break;
                }
                ahead = ahead_next;
            }
            if ahead >= text.len() {
                break;
            }
            let ahead_line = &text[ahead..line_at(text, ahead).0];
            if indent_of(ahead_line).0 >= 2 || list_marker(ahead_line).is_some() {
                line = ahead;
                continue;
            }
            break;
        }
        let (column, lead) = indent_of(this);
        if column <= 3 {
            let rest = &this[lead..];
            let starts_block = fence_opens(this).is_some()
                || (column == 0 && slot_name(this).is_some())
                || rest.starts_with("<!--")
                || is_atx_heading(this)
                || html::open_tag(this, lead).is_some_and(|tag| html::starts_a_block(&tag.name))
                || html::close_tag(this, lead).is_some_and(|(name, _)| html::starts_a_block(&name));
            if starts_block {
                break;
            }
        }
        line = next;
        end = next;
    }
    end
}

// ---------------------------------------------------------------------------------------
// The import.

/// The talk's look, as the style block Stagepack's export writes says.
#[derive(Clone, Debug, Default)]
struct Look {
    background: Option<String>,
    text: Option<String>,
    accent: Option<String>,
    font: Option<String>,
    mono: Option<String>,
}

/// What the import keeps track of across the talk.
struct Run<'a> {
    files: BTreeMap<&'a str, &'a [u8]>,
    notices: Vec<Notice>,
    seen: HashSet<(Option<usize>, NoticeKind, String)>,
    dropped: usize,
    /// The slide being made, counting from 1, or 0 for the talk.
    slide: usize,
    /// Each file under `assets/`, and the file of the talk it comes from.
    assets: BTreeMap<String, String>,
    /// The other way round: each file of the talk under `assets/`, and its name there.
    asset_of: HashMap<String, String>,
    /// The files read: the Markdown and the pictures.
    used: BTreeSet<String>,
    look: Option<Look>,
    /// How many dry runs are under way, which say nothing.
    quiet: usize,
}

impl Run<'_> {
    fn notice(&mut self, kind: NoticeKind, text: String) {
        if self.quiet > 0 {
            return;
        }
        let text = shortened(&text, MAX_NOTICE_CHARS);
        let slide = Some(self.slide).filter(|slide| *slide > 0);
        if !self.seen.insert((slide, kind, text.clone())) {
            return;
        }
        if self.notices.len() < MAX_NOTICES - 1 {
            self.notices.push(Notice { slide, kind, text });
        } else {
            self.dropped += 1;
        }
    }

    /// The name under `assets/` for the talk's file `source`, which is copied there.
    fn asset(&mut self, source: &str) -> Option<String> {
        if let Some(name) = self.asset_of.get(source) {
            return Some(name.clone());
        }
        // A name a Markdown link can hold as it is: what would end it, or be read as more
        // than a name, is a `-`.
        let rest: String = source
            .strip_prefix("public/")
            .or_else(|| source.strip_prefix("assets/"))
            .unwrap_or(source)
            .chars()
            .map(|c| if matches!(c, ' ' | '(' | ')' | '[' | ']' | '&' | '#' | '?' | '%' | '`' | '{' | '}') || c.is_whitespace() { '-' } else { c })
            .collect();
        let rest = rest.as_str();
        let (stem, extension) = match rest.rsplit_once('.') {
            Some((stem, extension)) if !stem.is_empty() && !extension.contains('/') => (stem, format!(".{extension}")),
            _ => (rest, String::new()),
        };
        for number in 1..=100 {
            let name = if number == 1 { format!("assets/{rest}") } else { format!("assets/{stem}-{number}{extension}") };
            if !safe_name(&name) {
                return None;
            }
            if self.assets.contains_key(&name) {
                continue;
            }
            self.assets.insert(name.clone(), source.to_string());
            self.asset_of.insert(source.to_string(), name.clone());
            self.used.insert(source.to_string());
            return Some(name);
        }
        None
    }
}

/// Why a picture can't be shown.
enum Miss {
    Web,
    Missing,
}

/// Imports the Slidev talk in `files` whose Markdown is `args.entry`.
pub fn import(args: &ImportArgs, files: &Bundle) -> Result<(Bundle, Report), String> {
    let entry = normalized(args.entry.trim()).filter(|entry| safe_name(entry)).ok_or_else(|| format!("The talk's entry, {}, isn't a file in the talk.", shortened(&args.entry, 120)))?;
    let mut run = Run {
        files: files.iter().map(|(name, data)| (name.as_str(), data.as_slice())).collect(),
        notices: Vec::new(),
        seen: HashSet::new(),
        dropped: 0,
        slide: 0,
        assets: BTreeMap::new(),
        asset_of: HashMap::new(),
        used: BTreeSet::new(),
        look: None,
        quiet: 0,
    };
    let data = run.files.get(entry.as_str()).copied().ok_or_else(|| format!("The Slidev talk has no {}.", shortened(&entry, 120)))?;
    let source = text_of(data).ok_or_else(|| format!("The Slidev talk's {} isn't UTF-8 text.", shortened(&entry, 120)))?;
    run.used.insert(entry.clone());

    let mut loader = Loader { run: &mut run, pages: BTreeMap::new(), slides: Vec::new(), full: false, calls: 0, bytes: 0, over_budget: false, long_range: false };
    loader.pages.insert(entry.clone(), split(&source));
    loader.load(&entry, None, &[], &mut Vec::new());
    let Loader { pages, slides, full, over_budget, long_range, .. } = loader;
    if long_range {
        run.notice(NoticeKind::LeftOut, format!("A src: range has more than {MAX_RANGE_PARTS} parts, so only its first {MAX_RANGE_PARTS} were read."));
    }
    if full {
        run.notice(NoticeKind::LeftOut, format!("The talk imports more than {MAX_SLIDES} slides, so the rest were left out."));
    }
    if over_budget {
        run.notice(NoticeKind::LeftOut, format!("The talk's imports go past the most that's followed ({MAX_IMPORTS} imports, {} MiB), so those past it are labelled boxes.", MAX_IMPORTED_BYTES / (1024 * 1024)));
    }
    let headmatter = Front { layers: pages.get(&entry).and_then(|page| page.first()).and_then(|slide| slide.front.as_ref().ok()).cloned().into_iter().collect() };
    let markdown = write_talk(&mut run, &slides, &headmatter);

    let mut bundle: Bundle = vec![(SLIDES.to_string(), markdown.into_bytes())];
    // Every file under `public/` is the talk's, as Slidev serves them all.
    let public: Vec<&str> = run.files.keys().copied().filter(|name| name.starts_with("public/") && safe_name(name) && !is_code_name(name)).collect();
    for name in public {
        run.asset(name);
    }
    run.slide = 0;
    if run.files.keys().any(|name| name.starts_with(VENDOR)) {
        run.notice(NoticeKind::LeftOut, "The talk's vendor folder was left out.".to_string());
    }
    let left: Vec<&str> = run
        .files
        .keys()
        .copied()
        .filter(|name| !run.used.contains(*name) && !name.starts_with(VENDOR) && *name != STAGEPACK_MANIFEST && *name != MANIFEST && !name.starts_with('.'))
        .collect();
    if !left.is_empty() {
        let mut names: Vec<String> = left.iter().take(5).map(|name| shortened(name, 80)).collect();
        if left.len() > 5 {
            names.push(format!("and {} more", left.len() - 5));
        }
        let text = if left.len() == 1 {
            format!("1 file of the Slidev talk isn't slides or a picture, so it was left out: {}.", names.join(", "))
        } else {
            format!("{} files of the Slidev talk aren't slides or pictures, so they were left out: {}.", left.len(), names.join(", "))
        };
        run.notice(NoticeKind::LeftOut, text);
    }
    for (name, source) in &run.assets {
        if let Some(data) = run.files.get(source.as_str()) {
            bundle.push((name.clone(), data.to_vec()));
        }
    }
    let mut notices = run.notices;
    if run.dropped > 0 {
        notices.push(Notice { slide: None, kind: NoticeKind::Approximation, text: format!("And {} more things that couldn't be carried over as they were.", run.dropped) });
    }
    Ok((bundle, Report { notices }))
}

/// A file's text, as Slidev reads it: UTF-8, with no byte order mark, and with its line
/// endings and NULs made ones the format can hold. A lone `\r` is made a line ending too,
/// where Slidev's splitter sees none: Markdown, ours as well as Markdown-it, ends a line
/// there, and a line our splitter can't see could otherwise end a slide or a fence.
fn text_of(data: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(data).ok()?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    Some(text.replace("\r\n", "\n").replace('\r', "\n").replace('\0', "\u{fffd}"))
}

/// Loads the talk's slides as Slidev's `load` does, following `src:` imports.
struct Loader<'r, 'a> {
    run: &'r mut Run<'a>,
    pages: BTreeMap<String, Vec<SlidevSlide>>,
    slides: Vec<Loaded>,
    full: bool,
    /// How many imports have been followed, and how much they brought in.
    calls: usize,
    bytes: usize,
    over_budget: bool,
    /// Whether a range had more parts than are read.
    long_range: bool,
}

impl Loader<'_, '_> {
    fn load(&mut self, path: &str, range: Option<&str>, over: &[Rc<Value>], chain: &mut Vec<String>) {
        let total = self.pages.get(path).map_or(0, Vec::len);
        let imported = !chain.is_empty();
        let (runs, more) = range_of(total, range);
        if more {
            self.long_range = true;
        }
        for index in runs.into_iter().flat_map(|(from, to)| from..=to) {
            if self.slides.len() >= MAX_SLIDES {
                self.full = true;
                return;
            }
            let Some(slide) = self.pages.get(path).and_then(|page| page.get(index - 1)) else { continue };
            let (own, front_error) = match &slide.front {
                Ok(front) => (Rc::clone(front), None),
                Err(error) => (Rc::new(Value::Map(Vec::new())), Some(error.clone())),
            };
            let front = Front { layers: std::iter::once(Rc::clone(&own)).chain(over.iter().cloned()).collect() };
            let Some(src) = own.get("src").and_then(Value::text).filter(|src| !src.trim().is_empty()).map(str::to_string) else {
                let size = slide.content.len() + slide.note.as_ref().map_or(0, String::len) + slide.front_size;
                if imported && self.bytes + size > MAX_IMPORTED_BYTES {
                    let raw = capped(&slide.raw);
                    self.over_budget = true;
                    self.slides.push(Loaded { front, front_error, content: String::new(), note: None, file: path.to_string(), unread: Some((path.to_string(), Unread::TooMuch, raw)) });
                    continue;
                }
                if imported {
                    self.bytes += size;
                }
                let (content, note) = (slide.content.clone(), slide.note.clone());
                self.slides.push(Loaded { front, front_error, content, note, file: path.to_string(), unread: None });
                continue;
            };
            // What a labelled box would hold, kept short: the import itself is at its start.
            let raw = capped(&slide.raw);
            if imported {
                self.bytes += slide.front_size;
            }
            // What the imported slides are given: this slide's block, under its importers'.
            let over: Vec<Rc<Value>> = std::iter::once(own).chain(over.iter().cloned()).collect();
            let mut parts = src.split('#');
            let raw_path = parts.next().unwrap_or("").trim();
            let part_range = parts.next();
            let target = if let Some(rooted) = raw_path.strip_prefix('/') { normalized(rooted) } else { normalized(&format!("{}{raw_path}", folder_of(path))) };
            let unread = match &target {
                None => Some(Unread::Outside),
                Some(target) if !safe_name(target) => Some(Unread::Outside),
                Some(target) if target == path || chain.iter().any(|file| file == target) => Some(Unread::Loop),
                Some(_) if chain.len() >= MAX_IMPORT_DEPTH => Some(Unread::TooDeep),
                Some(_) if self.calls >= MAX_IMPORTS || self.bytes > MAX_IMPORTED_BYTES => Some(Unread::TooMuch),
                Some(target) if !self.run.files.contains_key(target.as_str()) => Some(Unread::Missing),
                Some(target) => {
                    if !self.pages.contains_key(target) {
                        match self.run.files.get(target.as_str()).and_then(|data| text_of(data)) {
                            Some(text) => {
                                self.pages.insert(target.clone(), split(&text));
                            }
                            None => {
                                self.slides.push(Loaded { front, front_error, content: String::new(), note: None, file: path.to_string(), unread: Some((src, Unread::NotText, raw)) });
                                continue;
                            }
                        }
                    }
                    self.run.used.insert(target.clone());
                    None
                }
            };
            match (unread, target) {
                (None, Some(target)) => {
                    self.calls += 1;
                    chain.push(path.to_string());
                    self.load(&target, part_range, &over, chain);
                    chain.pop();
                }
                (why, _) => {
                    let why = why.unwrap_or(Unread::Missing);
                    self.over_budget |= why == Unread::TooMuch;
                    self.slides.push(Loaded { front, front_error, content: String::new(), note: None, file: path.to_string(), unread: Some((src, why, raw)) });
                }
            }
        }
    }
}

/// `text` as a labelled box holds an import's slide: no more than its first few KiB.
fn capped(text: &str) -> String {
    if text.len() <= MAX_PLACEHOLDER_SLIDE {
        return text.to_string();
    }
    let mut end = MAX_PLACEHOLDER_SLIDE;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n…", &text[..end])
}

/// Our layout for Slidev's `layout`, and what's said when it isn't the same.
fn our_layout(slidev: &str, has_image: bool) -> (&'static str, Option<String>) {
    match slidev {
        "cover" | "intro" => ("title", None),
        "default" | "center" => ("content", None),
        "two-cols" | "two-cols-header" => ("two-columns", None),
        "section" => ("section", None),
        "none" | "full" => ("blank", None),
        "image-right" | "image-left" | "image" if has_image => ("content", Some(format!("Slidev's layout {slidev} has no match here, so this slide uses content, with its picture at the end."))),
        other => ("content", Some(format!("Slidev's layout {} has no match here, so this slide uses content.", shortened(other, 80)))),
    }
}

/// Our transition for Slidev's, and whether it is the same.
fn our_transition(slidev: &str) -> (&'static str, bool) {
    // `forward | backward`: the way forward is the one a talk plays.
    let forward = slidev.split('|').next().unwrap_or("").trim();
    match forward {
        "" | "none" => ("none", true),
        "fade" => ("fade", true),
        name if name.starts_with("slide-") => ("push", true),
        _ => ("none", false),
    }
}

/// The first heading's text, as Slidev's parser takes a slide's title from it.
fn first_heading(content: &str) -> Option<String> {
    let mut in_fence = false;
    for line in content.lines() {
        if line.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let hashes = line.bytes().take_while(|byte| *byte == b'#').count();
        if hashes > 0 && line.as_bytes().get(hashes) == Some(&b' ') {
            return Some(line[hashes + 1..].trim().to_string());
        }
    }
    None
}

/// One of our slides, before it's written.
struct OurSlide {
    settings: Vec<(String, String)>,
    body: String,
    notes: Option<String>,
}

fn write_talk(run: &mut Run, slides: &[Loaded], headmatter: &Front) -> String {
    // The headmatter: what the talk is, and how it looks.
    run.slide = 1;
    if let Some(theme) = get_text(headmatter, "theme").filter(|theme| !matches!(theme.trim(), "" | "default" | "none")) {
        run.notice(NoticeKind::Approximation, format!("The theme {} isn't carried over, so the talk has Stagepack's own look.", shortened(theme, 80)));
    }
    if get(headmatter, "themeConfig").is_some_and(|config| !config.entries().is_empty()) {
        run.notice(NoticeKind::Approximation, "The headmatter's colours (themeConfig) aren't carried over.".to_string());
    }
    if let Some(addons) = get(headmatter, "addons").and_then(Value::texts).filter(|addons| !addons.is_empty()) {
        run.notice(NoticeKind::LeftOut, format!("The headmatter's add-ons ({}) run code, so they aren't carried over.", shortened(&addons, 120)));
    }
    let width = get_text(headmatter, "canvasWidth").and_then(|width| width.trim().parse::<f32>().ok()).filter(|width| width.is_finite() && *width > 0.0).unwrap_or(SLIDEV_WIDTH);
    let scale = (OUR_WIDTH / width).clamp(0.01, 100.0);
    if let Some(ratio) = get_text(headmatter, "aspectRatio") {
        let read = ratio.split(['/', ':', 'x']).map(|part| part.trim().parse::<f32>().ok()).collect::<Vec<_>>();
        let value = match read.as_slice() {
            [Some(w), Some(h)] if *h != 0.0 => Some(w / h),
            [Some(value)] => Some(*value),
            _ => None,
        };
        if value.is_none_or(|value| (value - 16.0 / 9.0).abs() > 0.01) {
            run.notice(NoticeKind::Approximation, format!("The slides are {} in Slidev and 16/9 here, so things may sit differently.", shortened(ratio, 40)));
        }
    }
    let default_layout = get(headmatter, "defaults").and_then(|defaults| defaults.get("layout")).and_then(Value::text).map(str::to_string);
    let talk_transition = get_text(headmatter, "transition").unwrap_or("").to_string();
    let is_hidden = |slide: &Loaded| get(&slide.front, "hide").is_some_and(Value::is_truthy) || get(&slide.front, "disabled").is_some_and(Value::is_truthy);
    let first_shown = slides.iter().position(|slide| !is_hidden(slide));

    let mut ours = Vec::with_capacity(slides.len());
    // The Slidev slide each transition said, to say once that it isn't ours.
    let mut said: HashSet<usize> = HashSet::new();
    let mut last_shown: Option<usize> = None;
    for (index, slide) in slides.iter().enumerate() {
        run.slide = index + 1;
        let hidden = is_hidden(slide);
        if let Some(error) = &slide.front_error {
            run.notice(NoticeKind::Approximation, format!("This slide's settings can't be read as YAML ({}), so they were ignored.", shortened(error, 200)));
        }
        let has_image = get_text(&slide.front, "image").is_some();
        let layout_name = get_text(&slide.front, "layout").filter(|layout| !layout.trim().is_empty()).map(str::to_string).or_else(|| default_layout.clone().filter(|layout| !layout.trim().is_empty())).unwrap_or_else(|| if Some(index) == first_shown { "cover".to_string() } else { "default".to_string() });
        let (layout, layout_notice) = our_layout(layout_name.trim(), has_image);
        if let Some(text) = layout_notice {
            run.notice(NoticeKind::Approximation, text);
        }
        // How it's arrived at: as the slide before it, the last one shown, says it's left.
        let before = if hidden { index.checked_sub(1) } else { last_shown };
        let transition = match before {
            None => "none",
            Some(before) => {
                let own = get_text(&slides[before].front, "transition").filter(|value| !value.trim().is_empty());
                let (value, from) = match own {
                    Some(value) => (value.to_string(), before),
                    None => (talk_transition.clone(), usize::MAX),
                };
                let (ours, same) = our_transition(&value);
                if !same && said.insert(from) {
                    run.notice(NoticeKind::Approximation, format!("The transition {} has no match here, so this slide has none.", shortened(value.trim(), 80)));
                }
                ours
            }
        };
        if !hidden {
            last_shown = Some(index);
        }
        if get(&slide.front, "background").is_some() {
            run.notice(NoticeKind::Approximation, "The slide's background isn't carried over.".to_string());
        }

        let names = layout_names(layout).unwrap_or(&slides_format::LAYOUT_NAMES[1]);
        let mut convert = Convert { run: &mut *run, clicks: Clicks::default(), scale, folder: folder_of(&slide.file).to_string(), v_pre: false, layout: names, most: 0 };
        let mut items = match &slide.unread {
            Some((src, why, raw)) => {
                let what = match why {
                    Unread::Outside => "are outside the talk",
                    Unread::Missing => "aren't in the talk",
                    Unread::Loop => "import themselves",
                    Unread::TooDeep => "are imported too many files deep",
                    Unread::NotText => "aren't UTF-8 text",
                    Unread::TooMuch => "are past the most the import follows",
                };
                let src = shortened(src, 120);
                vec![convert.placeholder(&format!("the slides of {src}"), raw, &format!("The slides of {src} {what}"), true)]
            }
            None => convert.content(&slide.content, 0, true),
        };
        if has_image && matches!(layout_name.trim(), "image-right" | "image-left" | "image") {
            let image = get_text(&slide.front, "image").unwrap_or_default().to_string();
            items.push(convert.picture_tag(&image, "", &[], &format!("image: {image}")));
        }
        let mut settings = vec![("layout".to_string(), layout.to_string()), ("transition".to_string(), transition.to_string())];
        if hidden {
            settings.push(("hidden".to_string(), "true".to_string()));
        }
        let body = convert.write(items, &mut settings);
        let most = convert.most;
        if let Some(asked) = get_text(&slide.front, "clicks").and_then(|clicks| clicks.trim().parse::<u32>().ok())
            && i64::from(asked) > most
        {
            run.notice(NoticeKind::Approximation, format!("Slidev gives this slide {asked} clicks, and here it takes {most}."));
        }
        let notes = slide.note.as_deref().map(html::decode).filter(|notes| !notes.trim().is_empty());
        ours.push(OurSlide { settings, body, notes });
    }
    if ours.is_empty() {
        ours.push(OurSlide { settings: vec![("layout".to_string(), "content".to_string())], body: String::new(), notes: None });
    }

    // The talk's own settings, on its first slide.
    let mut talk = Vec::new();
    let title = get_text(headmatter, "title")
        .map(str::to_string)
        .or_else(|| slides.iter().find(|slide| !is_hidden(slide)).and_then(|slide| get_text(&slide.front, "title").or_else(|| get_text(&slide.front, "name")).map(str::to_string).or_else(|| first_heading(&slide.content))));
    if let Some(title) = title.map(|title| setting_value(&title)).filter(|title| !title.is_empty()) {
        talk.push(("title".to_string(), title));
    }
    let look = run.look.clone().unwrap_or_default();
    let fonts = get(headmatter, "fonts");
    let font_of = |key: &str| fonts.and_then(|fonts| fonts.get(key)).and_then(Value::texts).and_then(|names| names.split(',').next().map(escape::font_name)).filter(|name| !name.is_empty());
    for (key, value) in [
        ("color.background", look.background.clone()),
        ("color.text", look.text.clone()),
        ("color.accent", look.accent.clone()),
        ("font", font_of("sans").or(look.font.as_deref().map(escape::font_name))),
        ("font.mono", font_of("mono").or(look.mono.as_deref().map(escape::font_name))),
    ] {
        if let Some(value) = value.map(|value| setting_value(&value)).filter(|value| !value.is_empty()) {
            talk.push((key.to_string(), value));
        }
    }
    if run.look.is_none() && get_text(headmatter, "colorSchema").is_some_and(|schema| schema.trim() == "dark") {
        run.slide = 1;
        run.notice(NoticeKind::Approximation, "The headmatter's colours (colorSchema: dark) aren't carried over.".to_string());
    }
    if let Some(first) = ours.first_mut() {
        talk.append(&mut first.settings);
        first.settings = talk;
    }
    assemble(run, ours)
}

/// One slide as our Markdown.
fn slide_text(slide: &OurSlide) -> String {
    let mut out = String::from("---\n");
    for (key, value) in &slide.settings {
        out.push_str(&format!("{key}: {value}\n"));
    }
    out.push_str("---\n");
    let body = slide.body.trim_end();
    if !body.is_empty() {
        out.push('\n');
        out.push_str(body);
        out.push('\n');
    }
    if let Some(notes) = &slide.notes {
        out.push_str(&format!("\n<Notes>\n{}\n</Notes>\n", notes_text(notes.trim())));
    }
    out
}

/// The slides, written, with any the format can't read as written made so it can, and said.
fn assemble(run: &mut Run, mut slides: Vec<OurSlide>) -> String {
    // Each slide must end where it's written to, whatever comes after it.
    for (index, slide) in slides.iter_mut().enumerate() {
        let text = slide_text(slide);
        let with_next = format!("{text}\n---\nlayout: content\n---\n");
        let split = split_slides(&with_next);
        if split.len() != 2 || split[0].settings.is_none() || split[1].settings.is_none() {
            run.slide = index + 1;
            run.notice(NoticeKind::LeftOut, "This slide can't be carried over as it was written, so what's on it was left out.".to_string());
            slide.body = "This slide couldn't be carried over from Slidev.".to_string();
            slide.notes = None;
        }
    }
    let markdown = slides.iter().map(slide_text).collect::<Vec<_>>().join("\n");
    let talk = parse_talk(&markdown);
    let broken: Vec<(usize, String)> = if talk.slides.len() == slides.len() {
        talk.slides.iter().enumerate().filter_map(|(index, slide)| slide.error.as_ref().map(|error| (index, error.message.clone()))).collect()
    } else {
        // Never seen: the slides are each checked above. Should it happen, none is trusted.
        (0..slides.len()).map(|index| (index, "it runs into the slides around it".to_string())).collect()
    };
    if broken.is_empty() {
        return markdown;
    }
    for (index, message) in broken {
        run.slide = index + 1;
        run.notice(NoticeKind::LeftOut, format!("This slide can't be carried over as it was ({message}), so what's on it was left out."));
        if let Some(slide) = slides.get_mut(index) {
            slide.body = "This slide couldn't be carried over from Slidev.".to_string();
            slide.notes = None;
        }
    }
    // Each slide is now a sentence and settings, which always read.
    slides.iter().map(slide_text).collect::<Vec<_>>().join("\n")
}

// ---------------------------------------------------------------------------------------
// A slide's content.

/// What a slide's conversion keeps track of.
struct Convert<'r, 'a> {
    run: &'r mut Run<'a>,
    clicks: Clicks,
    /// Our slide's width over Slidev's.
    scale: f32,
    /// The folder of the file the slide is in, for pictures named from it.
    folder: String,
    /// Whether Vue reads what's here as text, inside `v-pre`.
    v_pre: bool,
    layout: &'static LayoutNames,
    /// The most clicks anything written asks for.
    most: i64,
}

/// A stack entry of the inline formatting open on a line.
struct Inline {
    name: String,
    /// What opens and closes it in our Markdown, or nothing.
    open: String,
    close: String,
    written: bool,
}

/// How a part of the slide looks, from its inline CSS.
#[derive(Default)]
struct Css {
    positioned: bool,
    x: Option<f32>,
    y: Option<f32>,
    w: Option<f32>,
    h: Option<f32>,
    rotate: Option<f32>,
    fill: Option<String>,
    outline: Option<String>,
    outline_width: Option<f32>,
    radius: Option<f32>,
    padding: Option<f32>,
    color: Option<String>,
    size: Option<f32>,
    bold: bool,
    italic: bool,
    underline: bool,
    appear: bool,
}

impl Css {
    fn has_box(&self) -> bool {
        self.fill.is_some() || self.outline.is_some() || self.radius.is_some() || self.padding.is_some()
    }

    fn has_text(&self) -> bool {
        self.color.is_some() || self.size.is_some() || self.bold || self.italic || self.underline
    }

    fn place(&self) -> Vec<(&'static str, Option<String>)> {
        if !self.positioned {
            return Vec::new();
        }
        let mut attrs = vec![("x", Some(px(self.x.unwrap_or(0.0)))), ("y", Some(px(self.y.unwrap_or(0.0))))];
        attrs.extend(self.w.map(|w| ("w", Some(px(w)))));
        attrs.extend(self.h.map(|h| ("h", Some(px(h)))));
        attrs.extend(self.rotate.filter(|rotate| *rotate != 0.0).map(|rotate| ("rotate", Some(px(rotate)))));
        attrs
    }

    fn text_attrs(&self) -> Vec<(&'static str, Option<String>)> {
        let mut attrs = Vec::new();
        attrs.extend(self.color.clone().map(|color| ("color", Some(color))));
        attrs.extend(self.size.map(|size| ("size", Some(format!("{}", size.round().clamp(1.0, 1000.0) as u32)))));
        for (on, name) in [(self.bold, "bold"), (self.italic, "italic"), (self.underline, "underline")] {
            if on {
                attrs.push((name, None));
            }
        }
        attrs
    }

    fn box_attrs(&self) -> Vec<(&'static str, Option<String>)> {
        let mut attrs = Vec::new();
        attrs.extend(self.fill.clone().map(|fill| ("fill", Some(fill))));
        attrs.extend(self.outline.clone().map(|outline| ("outline", Some(outline))));
        attrs.extend(self.outline.as_ref().and(self.outline_width).map(|width| ("outline-width", Some(px(width)))));
        attrs.extend(self.radius.map(|radius| ("radius", Some(px(radius)))));
        attrs.extend(self.padding.map(|padding| ("padding", Some(px(padding)))));
        attrs
    }
}

/// When a part comes in, as its attributes say, and whether it's hidden on its click.
struct Timing {
    at: Option<At>,
    hide: bool,
    /// A `v-motion` click, which Slidev doesn't count among the slide's.
    motion: Option<i64>,
    enter: Enter,
}

/// The attributes a part can have and still be carried over.
fn is_known_attr(name: &str) -> bool {
    let base = name.split('.').next().unwrap_or(name);
    matches!(base, "style" | "v-click" | "v-after" | "v-click-hide" | "v-motion" | ":initial" | ":enter" | "v-pre") || (base.starts_with(":click-") && base.len() > ":click-".len())
}

impl Convert<'_, '_> {
    fn notice(&mut self, kind: NoticeKind, text: String) {
        self.run.notice(kind, text);
    }

    /// Notes that `click` is one a part of the slide comes in at.
    fn clicked(&mut self, click: i64) {
        self.most = self.most.max(click);
    }

    /// The click to write for one Slidev starts at: none for the start, and no more than 999.
    fn our_click(&mut self, start: i64) -> Option<String> {
        if start < 1 {
            return None;
        }
        let click = start.min(MAX_CLICK);
        self.clicked(click);
        Some(click.to_string())
    }

    /// A labelled box holding `original`, which couldn't be carried over, and says so.
    fn placeholder(&mut self, label: &str, original: &str, sentence: &str, counts: bool) -> Item {
        self.notice(NoticeKind::Placeholder, format!("{sentence}, so a labelled box holds its original text."));
        let title = format!("**From Slidev, not converted: {}**", plain_markdown(&shortened(label, 100)));
        let code = fenced("", "text", original.trim_matches('\n'));
        let text = our_tag("Box", &[("outline", Some(PLACEHOLDER_OUTLINE.to_string())), ("outline-width", Some("2".into())), ("radius", Some("8".into())), ("padding", Some("16".into()))], &format!("{title}\n\n{code}"));
        Item::Block { text, counts }
    }

    /// What `name` is, to say so: one of HTML's elements, or a component.
    fn describe(name: &str) -> String {
        if html::is_html(name) && !name.bytes().any(|byte| byte.is_ascii_uppercase()) { format!("the HTML element {name}") } else { format!("the component {name}") }
    }

    /// A labelled box for the element `name`, whose source is `source`, with its clicks
    /// counted as Slidev would count them, so that those after it are numbered right.
    fn element_placeholder(&mut self, name: &str, inner: Option<&str>, source: &str, depth: usize, counts: bool) -> Item {
        if let Some(inner) = inner.filter(|inner| inner.contains("v-click") || inner.contains("v-after") || inner.contains("click-"))
            && depth + 1 < MAX_NESTING
        {
            // Counted, and nothing else: what's said and what's written in it is let go.
            self.run.quiet += 1;
            let most = self.most;
            let _ = self.content(inner, depth + 1, false);
            self.most = most;
            self.run.quiet -= 1;
        }
        let what = Self::describe(name);
        self.placeholder(&what, source, &format!("{} isn't converted", capitalised(&what)), counts)
    }

    /// The items of `text`, a slide's content or what an element holds. `top` is whether it's
    /// the slide's own, where slots are.
    fn content(&mut self, text: &str, depth: usize, top: bool) -> Vec<Item> {
        let mut items = Vec::new();
        for raw in scan(text) {
            items.extend(self.raw(raw, depth, top));
        }
        items
    }

    fn raw(&mut self, raw: Raw, depth: usize, top: bool) -> Vec<Item> {
        match raw {
            Raw::Fence { indent, info, code } => vec![self.fence(indent, info, &code)],
            Raw::Slot(name) => {
                if !top {
                    self.notice(NoticeKind::Approximation, format!("The slot {} is inside something else, so it is ignored.", shortened(name, 80)));
                    return Vec::new();
                }
                let layout = self.layout;
                let region = if layout.regions.contains(&name) {
                    name.to_string()
                } else {
                    if name != "default" {
                        self.notice(NoticeKind::Approximation, format!("The slot {} has no place in the {} layout, so what's in it is in the {}.", shortened(name, 80), layout.name, layout.main));
                    }
                    layout.main.to_string()
                };
                vec![Item::Region(region)]
            }
            Raw::Comment => {
                self.notice(NoticeKind::LeftOut, "A comment in the slide, which Slidev never shows, was left out.".to_string());
                Vec::new()
            }
            Raw::StrayClose(source) => {
                self.notice(NoticeKind::LeftOut, format!("The closing tag {} closes nothing, so it was left out.", shortened(source, 80)));
                Vec::new()
            }
            Raw::Unclosed { tag, source } => {
                let what = Self::describe(&tag.name);
                vec![self.placeholder(&what, source, &format!("{} isn't closed", capitalised(&what)), true)]
            }
            Raw::Element { tag, inner, source } => self.element(tag, inner, source, depth),
            Raw::Markdown(chunk) => self.markdown(chunk),
        }
    }

    fn fence(&mut self, indent: &str, info: &str, code: &str) -> Item {
        let language = escape::language(info.split_whitespace().next().unwrap_or(""));
        let lower = info.to_ascii_lowercase();
        if let Some(open) = info.find('{')
            && let Some(close) = info[open..].find('}')
        {
            let inside = &info[open + 1..open + close];
            if inside.contains("monaco") {
                self.notice(NoticeKind::Approximation, "A code block can be edited or run in Slidev, and here it is only shown.".to_string());
            } else {
                let ranges = inside.split('|').count() as i64;
                if ranges > 1 {
                    self.clicks.since(At::After(1), ranges - 1);
                    self.notice(NoticeKind::Approximation, "A code block's lines are lit up one click at a time in Slidev, and here it is shown all at once.".to_string());
                }
            }
        }
        if lower.starts_with("mermaid") || lower.starts_with("plantuml") {
            self.notice(NoticeKind::Approximation, "A diagram Slidev draws from code is shown as its code.".to_string());
        }
        block(fenced(indent, &language, code))
    }

    // ----- Markdown.

    fn markdown(&mut self, chunk: &str) -> Vec<Item> {
        let chunk = chunk.trim_end_matches('\n');
        let lines: Vec<&str> = chunk.split('\n').collect();
        let mut extras = Vec::new();
        let first = lines.first().copied().unwrap_or("");
        let item = if is_atx_heading(first) {
            let (text, more) = self.inline(chunk, true);
            extras = more;
            Item::Heading(text)
        } else if lines.len() > 1 && lines.last().is_some_and(|last| is_setext(last)) && list_marker(first).is_none() {
            // A heading underlined with `=` or `-`, written with `#`.
            let level = if lines.last().is_some_and(|last| last.trim().starts_with('=')) { "#" } else { "##" };
            let text = lines[..lines.len() - 1].iter().map(|line| line.trim()).collect::<Vec<_>>().join(" ");
            let (text, more) = self.inline(&text, true);
            extras = more;
            Item::Heading(format!("{level} {text}"))
        } else if indent_of(first).0 >= 4 {
            // Code by its indent, written fenced, so it's read as code whatever is around it.
            block(fenced("", "", &dedented(chunk)))
        } else if list_marker(first).is_some() {
            let mut text = String::new();
            for (index, item) in list_items(chunk).iter().enumerate() {
                if index > 0 {
                    text.push('\n');
                }
                let (written, more) = self.inline(item, false);
                text.push_str(&written);
                extras.extend(more);
            }
            Item::List { text: no_breaks(&text) }
        } else {
            let (text, more) = self.inline(chunk, false);
            extras = more;
            block(no_breaks(&text))
        };
        let mut items = vec![item];
        items.extend(extras);
        items
    }

    /// Text in Markdown, with what isn't ours made ours: HTML formatting as ours, a `<` that
    /// isn't a tag escaped, a component or a `{{ }}` a labelled box after the block. Each line
    /// is whole, with whatever it opens closed by its end. `one_line` is a heading's or a
    /// table cell's, where a line can't be broken.
    fn inline(&mut self, text: &str, one_line: bool) -> (String, Vec<Item>) {
        let bytes = text.as_bytes();
        let mut out = String::with_capacity(text.len() + 16);
        let mut extras = Vec::new();
        let mut stack: Vec<Inline> = Vec::new();
        let mut searches = Searches::default();
        let mut at = 0;
        while at < bytes.len() {
            match bytes[at] {
                b'\\' if bytes.get(at + 1).is_some_and(u8::is_ascii_punctuation) => {
                    write_open(&mut out, &mut stack);
                    out.push_str(&text[at..at + 2]);
                    at += 2;
                }
                b'`' => {
                    let run = bytes[at..].iter().take_while(|byte| **byte == b'`').count();
                    match searches.runs.closing(text, at + run, run) {
                        Some(close) => {
                            let span = &text[at..close + run];
                            if span.contains('\n') {
                                close_all(&mut out, &mut stack);
                            }
                            write_open(&mut out, &mut stack);
                            out.push_str(span);
                            at = close + run;
                        }
                        None => {
                            write_open(&mut out, &mut stack);
                            out.push_str(&text[at..at + run]);
                            at += run;
                        }
                    }
                }
                b'\n' => {
                    close_all(&mut out, &mut stack);
                    out.push('\n');
                    at += 1;
                }
                b'{' if !self.v_pre && bytes.get(at + 1) == Some(&b'{') => match searches.braces.find(text, at + 2, "}}") {
                    Some(end) => {
                        let expression = &text[at..end + 2];
                        let short = shortened(expression, 100);
                        extras.push(self.placeholder(&format!("the expression {short}"), expression, &format!("The expression {short} isn't converted"), false));
                        at = end + 2;
                    }
                    None => {
                        write_open(&mut out, &mut stack);
                        out.push_str("{{");
                        at += 2;
                    }
                },
                b'<' => at = self.inline_tag(text, at, one_line, &mut out, &mut stack, &mut extras, &mut searches),
                b'!' if bytes.get(at + 1) == Some(&b'[') => match self.inline_image(text, at, &mut extras, &mut searches) {
                    Some((written, end)) => {
                        write_open(&mut out, &mut stack);
                        out.push_str(&written);
                        at = end;
                    }
                    None => {
                        write_open(&mut out, &mut stack);
                        out.push('!');
                        at += 1;
                    }
                },
                _ => {
                    let c = text[at..].chars().next().unwrap_or(' ');
                    if !c.is_whitespace() {
                        write_open(&mut out, &mut stack);
                    }
                    // A line that's `---` would end the slide.
                    out.push(c);
                    at += c.len_utf8();
                }
            }
        }
        close_all(&mut out, &mut stack);
        (out, extras)
    }

    /// Reads the `<` at `at`, writes what it stands for, and gives where reading goes on.
    #[allow(clippy::too_many_arguments)]
    fn inline_tag(&mut self, text: &str, at: usize, one_line: bool, out: &mut String, stack: &mut Vec<Inline>, extras: &mut Vec<Item>, searches: &mut Searches) -> usize {
        let rest = &text[at..];
        if rest.starts_with("<!--")
            && let Some(end) = searches.comment.find(text, at + 4, "-->")
        {
            self.notice(NoticeKind::LeftOut, "A comment in the slide, which Slidev never shows, was left out.".to_string());
            return end + 3;
        }
        // An autolink, written as a link of ours. It holds no space or `<`, so it's looked for
        // no further than one.
        let autolink_end = rest.bytes().skip(1).position(|byte| matches!(byte, b'>' | b' ' | b'<' | b'\n' | b'\t')).map(|at| at + 1).filter(|end| rest.as_bytes()[*end] == b'>');
        if let Some(end) = autolink_end
            && let Some(url) = rest.get(1..end)
            && !url.contains([' ', '<', '\n', '\t'])
            && (url.split_once(':').is_some_and(|(scheme, _)| scheme.len() >= 2 && scheme.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'.' | b'-'))) || (url.contains('@') && !url.starts_with('@')))
        {
            write_open(out, stack);
            let target = if url.contains(':') { url.to_string() } else { format!("mailto:{url}") };
            match escape::link(&target) {
                Some(link) => out.push_str(&format!("[{}]({link})", plain_markdown(url))),
                None => out.push_str(&plain_markdown(url)),
            }
            return at + end + 1;
        }
        if let Some((name, end)) = html::close_tag(text, at) {
            close_named(out, stack, &name);
            return end;
        }
        let Some(tag) = html::open_tag(text, at) else {
            write_open(out, stack);
            out.push_str("\\<");
            return at + 1;
        };
        let lower = tag.name.to_ascii_lowercase();
        // Something that comes in on a click, in the middle of the text.
        let timing = self.timing(&tag);
        let comes_in = matches!(lower.as_str(), "v-click" | "v-after" | "vclick" | "vafter") || timing.at.is_some() || timing.motion.is_some();
        if comes_in {
            let at_value = if matches!(lower.as_str(), "v-click" | "vclick") {
                self.at_prop(&tag)
            } else if matches!(lower.as_str(), "v-after" | "vafter") {
                Some(At::After(0))
            } else {
                timing.at
            };
            if let Some(at_value) = at_value {
                self.clicks.since(at_value, 1);
            }
            self.notice(NoticeKind::Approximation, "Something in a line of text comes in on a click in Slidev, and here it is shown from the start.".to_string());
        }
        let push = |stack: &mut Vec<Inline>, open: String, close: String| {
            // No line holds more than this; past it a tag's formatting is let go.
            if !tag.self_closing && stack.len() < MAX_INLINE_DEPTH {
                stack.push(Inline { name: tag.name.clone(), open, close, written: false });
            }
        };
        match lower.as_str() {
            "br" => {
                if one_line {
                    out.push(' ');
                } else {
                    close_all(out, stack);
                    out.push_str("\\\n");
                }
            }
            "strong" | "b" => push(stack, "<Text bold>".into(), "</Text>".into()),
            "em" | "i" => push(stack, "<Text italic>".into(), "</Text>".into()),
            "u" | "ins" => push(stack, "<Text underline>".into(), "</Text>".into()),
            "span" | "font" | "v-click" | "v-after" | "vclick" | "vafter" => {
                let css = self.css(tag.attr("style"), false);
                let mut attrs = css.text_attrs();
                if lower == "font"
                    && let Some(color) = tag.attr("color").and_then(colour)
                {
                    attrs.insert(0, ("color", Some(color)));
                }
                if attrs.is_empty() {
                    push(stack, String::new(), String::new());
                } else {
                    let open = our_tag("Text", &attrs, "x");
                    let open = open.split('\n').next().unwrap_or("<Text>").to_string();
                    push(stack, open, "</Text>".into());
                }
            }
            "code" => {
                if let Some(close) = searches.code.find_folded(text, tag.end, "</code>") {
                    let inner = strip_tags(&text[tag.end..close]);
                    write_open(out, stack);
                    out.push_str(&code_span(&html::decode(&inner)));
                    return close + "</code>".len();
                }
                push(stack, String::new(), String::new());
            }
            "a" => match tag.attr("href").and_then(escape::link) {
                Some(link) => push(stack, "[".into(), format!("]({link})")),
                None => push(stack, String::new(), String::new()),
            },
            "img" => {
                let source = &text[at..tag.end];
                match tag.attr("src").map(|src| self.resolve(src)) {
                    Some(Ok(name)) => {
                        write_open(out, stack);
                        out.push_str(&format!("![{}]({})", plain_markdown(tag.attr("alt").unwrap_or("")), name));
                    }
                    Some(Err(miss)) => extras.push(self.missing_picture(tag.attr("src").unwrap_or(""), source, miss)),
                    None => extras.push(self.element_placeholder("img", None, source, 0, false)),
                }
            }
            "sup" | "sub" | "small" | "mark" | "kbd" | "s" | "del" | "abbr" | "cite" | "q" | "var" | "samp" | "dfn" | "time" | "bdi" | "bdo" | "data" | "wbr" => {
                self.notice(NoticeKind::Approximation, format!("The formatting <{lower}> is shown as plain text."));
                push(stack, String::new(), String::new());
            }
            _ => {
                // A component or an element that isn't ours: what's in it is kept as text, and
                // its tag is in a labelled box after the block.
                let source = &text[at..tag.end];
                extras.push(self.element_placeholder(&tag.name, None, source, MAX_NESTING, false));
                push(stack, String::new(), String::new());
            }
        }
        tag.end
    }

    /// The image `![alt](url)` at `at`, written with its picture's name under `assets/`, and
    /// where it ends. One whose picture isn't in the talk is a labelled box.
    fn inline_image(&mut self, text: &str, at: usize, extras: &mut Vec<Item>, searches: &mut Searches) -> Option<(String, usize)> {
        let close = searches.label.find(text, at + 2, "](")?;
        if searches.alt_line.find(text, at + 2, "\n").is_some_and(|line| line < close) {
            return None;
        }
        let alt = &text[at + 2..close];
        let target_start = close + 2;
        let target_end = searches.paren.find(text, target_start, ")")?;
        if searches.target_line.find(text, target_start, "\n").is_some_and(|line| line < target_end) {
            return None;
        }
        let target = text[target_start..target_end].trim();
        let (url, title) = match target.split_once(char::is_whitespace) {
            Some((url, title)) => (url, Some(title.trim())),
            None => (target, None),
        };
        let source = &text[at..=target_end];
        let end = target_end + 1;
        match self.resolve(url) {
            Ok(name) => {
                let title = match title.map(link_title) {
                    None => String::new(),
                    Some(Some(title)) => format!(" {title}"),
                    Some(None) => {
                        self.notice(NoticeKind::LeftOut, "A picture's title can't be read as one, so it was left out.".to_string());
                        String::new()
                    }
                };
                Some((format!("![{}]({name}{title})", self.inline(alt, true).0), end))
            }
            Err(miss) => {
                extras.push(self.missing_picture(url, source, miss));
                Some((String::new(), end))
            }
        }
    }

    fn missing_picture(&mut self, url: &str, source: &str, miss: Miss) -> Item {
        let url = shortened(url, 120);
        let why = match miss {
            Miss::Web => "is on the web, which isn't read",
            Miss::Missing => "isn't in the talk",
        };
        self.placeholder(&format!("the picture {url}"), source, &format!("The picture {url} {why}"), true)
    }

    /// The name under `assets/` of the picture Slidev finds at `url`.
    fn resolve(&mut self, url: &str) -> Result<String, Miss> {
        let url = url.trim().trim_start_matches('<').trim_end_matches('>');
        let lower = url.to_ascii_lowercase();
        if lower.starts_with("http:") || lower.starts_with("https:") || lower.starts_with("//") {
            return Err(Miss::Web);
        }
        let path = url.split(['?', '#']).next().unwrap_or("");
        let path = html::percent_decode(path);
        let candidates: Vec<String> = if let Some(rooted) = path.strip_prefix('/') {
            [format!("public/{rooted}"), rooted.to_string()].into_iter().filter_map(|path| normalized(&path)).collect()
        } else if lower.contains(':') {
            Vec::new()
        } else {
            normalized(&format!("{}{path}", self.folder)).into_iter().collect()
        };
        for candidate in candidates {
            if !candidate.is_empty()
                && safe_name(&candidate)
                && !is_code_name(&candidate)
                && self.run.files.contains_key(candidate.as_str())
                && let Some(name) = self.run.asset(&candidate)
            {
                // A part the app drew for an export comes back as the picture it was drawn as.
                if candidate.strip_prefix("public/").is_some_and(|rest| rest.starts_with(BAKED_FOLDER)) {
                    let file = candidate.rsplit('/').next().unwrap_or(&candidate);
                    self.notice(
                        NoticeKind::Frozen,
                        format!("The picture {} is of a part Stagepack Studio drew for Slidev, so it comes back as a picture that can't be edited as the part was.", shortened(file, 80)),
                    );
                }
                return Ok(name);
            }
        }
        Err(Miss::Missing)
    }

    // ----- Elements.

    /// When the element opened by `tag` comes in, as its attributes say.
    fn timing(&mut self, tag: &OpenTag) -> Timing {
        let mut timing = Timing { at: None, hide: false, motion: None, enter: Enter::Fade };
        for (name, value) in &tag.attrs {
            let mut parts = name.split('.');
            let base = parts.next().unwrap_or("");
            let modifiers: Vec<&str> = parts.collect();
            match base {
                "v-click" => {
                    timing.at = self.directive(value.as_deref(), "v-click");
                    timing.hide |= modifiers.contains(&"hide");
                }
                "v-click-hide" => {
                    timing.at = self.directive(value.as_deref(), "v-click-hide");
                    timing.hide = true;
                }
                "v-after" => {
                    timing.at = Some(At::After(0));
                    timing.hide |= modifiers.contains(&"hide");
                }
                _ => {}
            }
        }
        if tag.has("v-motion") {
            let motion = tag.attrs.iter().filter_map(|(name, _)| name.strip_prefix(":click-")).filter_map(|click| click.split('-').next()?.trim().parse::<i64>().ok()).min();
            if let Some(click) = motion {
                timing.motion = Some(click.clamp(0, MAX_CLICK));
                timing.enter = if tag.attr(":initial").is_some_and(|initial| initial.contains('y')) { Enter::FlyUp } else { Enter::Fade };
            }
        }
        if tag.attr("style").is_some_and(|style| html::style(style).iter().any(|(property, value)| property == "transition" && value == "none")) {
            timing.enter = Enter::Appear;
        }
        timing
    }

    /// A `v-click` directive's value: a JavaScript expression, read only as far as a number
    /// or a string of one.
    fn directive(&mut self, value: Option<&str>, name: &str) -> Option<At> {
        let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else { return Some(At::After(1)) };
        let quoted = ['\'', '"', '`'].iter().find_map(|quote| value.strip_prefix(*quote).and_then(|rest| rest.strip_suffix(*quote)));
        if let Some(inner) = quoted {
            return self.read_at(inner, name);
        }
        match value {
            "true" => return Some(At::After(1)),
            "false" => return None,
            _ => {}
        }
        if let Some(number) = js_number(value) {
            return Some(At::On(if number <= 0.0 { 1 } else { number.ceil() as i64 }));
        }
        if let Some(inner) = value.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
            let first = inner.split(',').next().unwrap_or("").trim();
            self.notice(NoticeKind::Approximation, format!("{name}=\"{}\" shows something for a few clicks in Slidev, and here it stays.", shortened(value, 80)));
            return self.directive(Some(first), name);
        }
        self.notice(NoticeKind::Approximation, format!("{name}=\"{}\" isn't a click this can read, so it is taken as the next click.", shortened(value, 80)));
        Some(At::After(1))
    }

    /// A component's `at`, as written or bound (`:at`, `v-bind:at`), or `+1`.
    fn at_prop(&mut self, tag: &OpenTag) -> Option<At> {
        if let Some(value) = tag.attr("at") {
            return self.read_at(value, "at");
        }
        match tag.attr(":at").or_else(|| tag.attr("v-bind:at")) {
            Some(value) => self.directive(Some(value), "at"),
            None => Some(At::After(1)),
        }
    }

    /// An `at` written as text, or a notice that it can't be read and nothing.
    fn read_at(&mut self, value: &str, name: &str) -> Option<At> {
        match at_value(value) {
            Ok(at) => at,
            Err(()) => {
                self.notice(NoticeKind::Approximation, format!("{name}=\"{}\" isn't a click Slidev can read, so this is shown from the start.", shortened(value, 80)));
                None
            }
        }
    }

    /// Registers the click a part comes in at, as its timing says, and gives our `click` and
    /// `enter` for it.
    fn register(&mut self, timing: &Timing) -> Vec<(&'static str, Option<String>)> {
        let start = match (timing.at, timing.motion) {
            (Some(at), _) => Some(self.clicks.since(at, 1)),
            (None, Some(motion)) => Some(motion),
            (None, None) => None,
        };
        let Some(start) = start else { return Vec::new() };
        if timing.hide {
            self.notice(NoticeKind::Approximation, "Something here is hidden on a click in Slidev, and here it is always shown.".to_string());
            return Vec::new();
        }
        let Some(click) = self.our_click(start) else { return Vec::new() };
        let mut attrs = vec![("click", Some(click))];
        attrs.extend(timing.enter.attr().map(|enter| ("enter", Some(enter.to_string()))));
        attrs
    }

    fn element(&mut self, tag: OpenTag, inner: Option<&str>, source: &str, depth: usize) -> Vec<Item> {
        if depth >= MAX_NESTING {
            let what = Self::describe(&tag.name);
            return vec![self.placeholder(&what, source, &format!("{} is nested too deeply", capitalised(&what)), true)];
        }
        let lower = tag.name.to_ascii_lowercase();
        let inner_text = inner.unwrap_or("");
        match lower.as_str() {
            "v-click" | "vclick" => self.v_click(&tag, inner_text, depth),
            "v-after" | "vafter" => self.v_after(inner_text, depth),
            "v-clicks" | "vclicks" => self.v_clicks(&tag, inner_text, depth),
            "style" => {
                if inner_text.contains(":global(.slidev-page)") && inner_text.contains("--slidev-theme-primary") {
                    if self.run.look.is_none() {
                        self.run.look = Some(read_look(inner_text));
                    }
                    return Vec::new();
                }
                vec![self.placeholder("a style block", source, "The style block isn't converted", false)]
            }
            "script" => vec![self.placeholder("a script", source, "The script isn't converted", false)],
            "img" => {
                if tag.attrs.iter().any(|(name, _)| !is_known_attr(name) && !matches!(name.as_str(), "src" | "alt" | "width" | "height")) {
                    return vec![self.element_placeholder("img", None, source, depth, true)];
                }
                let timing = self.timing(&tag);
                let attrs = self.register(&timing);
                vec![self.picture_tag(tag.attr("src").unwrap_or(""), tag.attr("alt").unwrap_or(""), &[("style", tag.attr("style").unwrap_or("").to_string())], source).with_attrs(attrs)]
            }
            "video" => self.video(&tag, inner_text, source),
            "pre" => {
                let code = inner_text.trim_matches('\n');
                let code = match html::open_tag(code, 0).filter(|open| open.name.eq_ignore_ascii_case("code")) {
                    Some(open) => code[open.end..].trim_end().strip_suffix("</code>").unwrap_or(&code[open.end..]),
                    None => code,
                };
                vec![block(fenced("", "", &html::decode(&strip_tags(code))))]
            }
            "hr" if tag.attrs.is_empty() => vec![block("***".to_string())],
            "br" if tag.attrs.is_empty() => Vec::new(),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" if tag.attrs.is_empty() => {
                let level = usize::from(lower.as_bytes()[1] - b'0');
                let (text, extras) = self.inline(&inner_text.replace('\n', " "), true);
                let mut items = vec![Item::Heading(format!("{} {}", "#".repeat(level), text.trim()))];
                items.extend(extras);
                items
            }
            "div" | "p" | "section" | "center" | "article" | "header" | "footer" | "main" | "aside" | "figure" => self.container(&tag, inner, source, depth),
            _ => vec![self.element_placeholder(&tag.name, inner, source, depth, true)],
        }
    }

    /// An element that holds the slide's own content, as a box or text of ours.
    fn container(&mut self, tag: &OpenTag, inner: Option<&str>, source: &str, depth: usize) -> Vec<Item> {
        if tag.attrs.iter().any(|(name, _)| !is_known_attr(name)) {
            return vec![self.element_placeholder(&tag.name, inner, source, depth, true)];
        }
        let was = self.v_pre;
        self.v_pre |= tag.has("v-pre");
        let children = self.content(inner.unwrap_or(""), depth + 1, false);
        self.v_pre = was;
        // Mounted after what's in it, so counted after it.
        let timing = self.timing(tag);
        let timing_attrs = self.register(&timing);
        let css = self.css(tag.attr("style"), true);
        let mut attrs = css.place();
        if timing_attrs.is_empty() && attrs.is_empty() && !css.has_box() && !css.has_text() {
            // An element that changes nothing: what's in it, as one element.
            return match children.len() {
                0 => Vec::new(),
                1 => children,
                _ => vec![block(written(&children))],
            };
        }
        let content = written(&children);
        let text = if css.has_box() || !css.has_text() {
            attrs.extend(css.box_attrs());
            let content = if css.has_text() { our_tag("Text", &css.text_attrs(), &content) } else { content };
            attrs.extend(timing_attrs);
            our_tag("Box", &attrs, &content)
        } else {
            attrs.extend(css.text_attrs());
            attrs.extend(timing_attrs);
            our_tag("Text", &attrs, &content)
        };
        vec![block(text)]
    }

    /// `<v-click>`: what's in it comes in on one click.
    fn v_click(&mut self, tag: &OpenTag, inner: &str, depth: usize) -> Vec<Item> {
        let children = self.content(inner, depth + 1, false);
        let at = self.at_prop(tag);
        if !children.iter().any(Item::counts) {
            return children;
        }
        let timing = Timing { at, hide: tag.has("hide"), motion: None, enter: Enter::Fade };
        let attrs = self.register(&timing);
        if attrs.is_empty() {
            return children;
        }
        vec![block(our_tag("Box", &attrs, &written(&children)))]
    }

    /// `<v-after>`: each thing in it comes in with the click before.
    fn v_after(&mut self, inner: &str, depth: usize) -> Vec<Item> {
        let children = self.content(inner, depth + 1, false);
        if !children.iter().any(Item::counts) {
            return children;
        }
        let timing = Timing { at: Some(At::After(0)), hide: false, motion: None, enter: Enter::Fade };
        let attrs = self.register(&timing);
        if attrs.is_empty() {
            return children;
        }
        vec![block(our_tag("Box", &attrs, &written(&children)))]
    }

    /// `<v-clicks>`: each thing in it, or each item of a list alone in it, comes in on a
    /// click of its own, numbered as Slidev's `VClicks` numbers them.
    fn v_clicks(&mut self, tag: &OpenTag, inner: &str, depth: usize) -> Vec<Item> {
        let at = self.at_prop(tag);
        let every_text = number_prop(tag, "every");
        let every = match every_text.as_deref().map(js_number) {
            None => 1.0,
            Some(Some(every)) if every >= 1.0 => every,
            Some(_) => {
                self.notice(NoticeKind::Approximation, format!("every=\"{}\" isn't a number of things to show at once, so they come in one at a time.", shortened(every_text.as_deref().unwrap_or(""), 40)));
                1.0
            }
        };
        // How many levels of a list come in a click at a time, as `+depth` reads it.
        let levels = number_prop(tag, "depth").as_deref().and_then(js_number).map_or(1, |levels| levels.ceil().clamp(1.0, MAX_LIST_LEVELS as f64) as usize);
        let hide = tag.has("hide");
        let raws: Vec<Raw> = scan(inner).into_iter().filter(|raw| !matches!(raw, Raw::Comment)).collect();
        let Some(at) = at else {
            let mut items = Vec::new();
            for raw in raws {
                items.extend(self.raw(raw, depth + 1, false));
            }
            return items;
        };
        let (relative, at_number) = match at {
            At::After(value) => (true, value),
            At::On(value) => (false, value),
        };
        // Each piece, the items that make it up, and the click it starts at.
        let mut pieces: Vec<(Vec<Item>, i64)> = Vec::new();
        let mut counter = Counter { at: at_number, every, relative, count: 0, done: 0 };
        let list = match raws.as_slice() {
            [Raw::Markdown(chunk)] if list_marker(chunk.split('\n').next().unwrap_or("")).is_some() && indent_of(chunk).0 < 4 => Some(*chunk),
            _ => None,
        };
        let table = match raws.as_slice() {
            [Raw::Markdown(chunk)] if is_table(chunk) => Some(*chunk),
            _ => None,
        };
        let mut whole_list = None;
        let mut nested = false;
        if let Some(chunk) = list {
            let mut text = String::new();
            let mut extras = Vec::new();
            for (index, item) in list_items(chunk.trim_end_matches('\n')).iter().enumerate() {
                let (written, more) = self.inline(item, false);
                if index > 0 {
                    text.push('\n');
                }
                text.push_str(&written);
                // As `VClicks` numbers a list `depth` levels deep: each item takes its number
                // before the items under it, and is mounted, and counted, after them.
                let show = counter.index();
                if levels > 1 {
                    nested |= count_nested(item, 2, levels, &mut counter, &mut self.clicks);
                }
                let start = counter.register(show, &mut self.clicks);
                pieces.push((vec![Item::List { text: no_breaks(written.trim_end()) }], start));
                extras.extend(more);
            }
            if nested {
                self.notice(NoticeKind::Approximation, "The items under a list's items come in a click at a time in Slidev, and here with the item they're under.".to_string());
            }
            whole_list = Some(no_breaks(&text));
            if let Some(last) = pieces.last_mut() {
                last.0.extend(extras);
            }
        } else if let Some(chunk) = table {
            let items = self.markdown(chunk);
            let rows = chunk.trim_end().lines().count().saturating_sub(2);
            let starts: Vec<i64> = (0..rows).map(|_| counter.next(&mut self.clicks)).collect();
            pieces.push((items, starts.first().copied().unwrap_or(0)));
            let from_one = !hide && rows as i64 <= MAX_CLICK && starts.iter().enumerate().all(|(index, start)| *start == index as i64 + 1);
            let size = counter.gap();
            self.clicks.gap(size);
            let whole = written(&pieces[0].0);
            let boxed = match self.our_click(starts.first().copied().unwrap_or(0)) {
                Some(click) if !hide => our_tag("Box", &[("click", Some(click)), ("enter", Some("fade".into()))], &whole),
                _ => whole.clone(),
            };
            if from_one {
                self.clicked(rows as i64);
            }
            return vec![Item::Each(Each { whole, boxed, from_one, boxed_notice: Some("A table's rows come in a click at a time in Slidev, and here the whole table comes in at once.".to_string()) })];
        } else {
            for raw in raws {
                let items = self.raw(raw, depth + 1, false);
                if items.is_empty() {
                    continue;
                }
                let counted = items.iter().filter(|item| item.counts()).count();
                if counted == 0 {
                    match pieces.last_mut() {
                        Some(last) => last.0.extend(items),
                        None => pieces.push((items, 0)),
                    }
                    continue;
                }
                // Each element is a piece, with what isn't one after it.
                let mut current: Vec<Item> = Vec::new();
                for item in items {
                    if item.counts() && !current.is_empty() {
                        let start = counter.next(&mut self.clicks);
                        pieces.push((std::mem::take(&mut current), start));
                    }
                    current.push(item);
                }
                if !current.is_empty() {
                    let start = counter.next(&mut self.clicks);
                    pieces.push((current, start));
                }
            }
        }
        let size = counter.gap();
        self.clicks.gap(size);
        if pieces.is_empty() {
            return Vec::new();
        }
        if hide {
            self.notice(NoticeKind::Approximation, "Something here is hidden on a click in Slidev, and here it is always shown.".to_string());
            let items: Vec<Item> = pieces.into_iter().flat_map(|(items, _)| items).collect();
            return vec![block(written(&items))];
        }
        let from_one = !nested && pieces.len() as i64 <= MAX_CLICK && pieces.iter().enumerate().all(|(index, (items, start))| *start == index as i64 + 1 && items.len() == 1);
        let whole = match whole_list {
            Some(list) => {
                let extras: Vec<Item> = pieces.iter().flat_map(|(items, _)| items.iter().skip(1).cloned()).collect();
                if extras.is_empty() { list } else { format!("{list}\n\n{}", written(&extras)) }
            }
            None => written(&pieces.iter().flat_map(|(items, _)| items.iter().cloned()).collect::<Vec<_>>()),
        };
        let mut boxed = Vec::new();
        for (items, start) in &pieces {
            let content = written(items);
            match self.our_click(*start) {
                Some(click) => boxed.push(our_tag("Box", &[("click", Some(click)), ("enter", Some("fade".into()))], &content)),
                None => boxed.push(content),
            }
        }
        vec![Item::Each(Each { whole, boxed: boxed.join("\n\n"), from_one, boxed_notice: None })]
    }

    fn video(&mut self, tag: &OpenTag, inner: &str, source: &str) -> Vec<Item> {
        let known = |name: &str| is_known_attr(name) || matches!(name, "src" | "controls" | "loop" | "autoplay" | "muted" | "poster" | "playsinline" | "width" | "height" | "preload");
        if tag.attrs.iter().any(|(name, _)| !known(name)) {
            return vec![self.element_placeholder("video", Some(inner), source, 0, true)];
        }
        let src = tag.attr("src").map(str::to_string).or_else(|| {
            let at = inner.find("<source")?;
            html::open_tag(inner, at)?.attr("src").map(str::to_string)
        });
        let Some(src) = src else { return vec![self.element_placeholder("video", Some(inner), source, 0, true)] };
        match self.resolve(&src) {
            Ok(name) => {
                let timing = self.timing(tag);
                let timing_attrs = self.register(&timing);
                let css = self.css(tag.attr("style"), true);
                let mut attrs = vec![("src", Some(name))];
                attrs.extend(css.place());
                if tag.has("loop") {
                    attrs.push(("loop", None));
                }
                attrs.extend(timing_attrs);
                vec![block(our_tag("Video", &attrs, ""))]
            }
            Err(miss) => vec![self.missing_picture(&src, source, miss)],
        }
    }

    /// A `<Picture>` of the picture at `url`, as `extra` places it, or a labelled box.
    fn picture_tag(&mut self, url: &str, alt: &str, extra: &[(&str, String)], source: &str) -> Item {
        match self.resolve(url) {
            Ok(name) => {
                let style = extra.iter().find(|(name, _)| *name == "style").map(|(_, style)| style.as_str()).filter(|style| !style.is_empty());
                let css = self.css(style, true);
                let mut attrs = vec![("src", Some(name)), ("alt", Some(alt.to_string()))];
                if alt.is_empty() {
                    attrs.pop();
                }
                attrs.extend(css.place());
                block(our_tag("Picture", &attrs, ""))
            }
            Err(miss) => self.missing_picture(url, source, miss),
        }
    }

    /// The look inline CSS gives, at our slide's size. `placed` is whether the part can be
    /// placed on the slide, which a run of text can't.
    fn css(&mut self, style: Option<&str>, placed: bool) -> Css {
        let mut css = Css::default();
        let Some(style) = style else { return css };
        let scale = self.scale;
        let length = |value: &str| html::pixels(value).map(|pixels| pixels * scale);
        let mut ignored = Vec::new();
        let declarations = html::style(style);
        let absolute = declarations.iter().any(|(property, value)| property == "position" && matches!(value.as_str(), "absolute" | "fixed"));
        for (property, value) in &declarations {
            let read = match property.as_str() {
                "position" => {
                    css.positioned = placed && absolute;
                    true
                }
                "left" | "top" | "width" | "height" if placed && absolute => {
                    let read = length(value);
                    match property.as_str() {
                        "left" => css.x = read,
                        "top" => css.y = read,
                        "width" => css.w = read,
                        _ => css.h = read,
                    }
                    read.is_some()
                }
                "transform" if placed && absolute => {
                    css.rotate = html::rotation(value);
                    css.rotate.is_some()
                }
                "background" | "background-color" if placed => {
                    css.fill = colour(value);
                    css.fill.is_some()
                }
                "border" if placed => {
                    for word in value.split_whitespace() {
                        if let Some(width) = length(word) {
                            css.outline_width = Some(width);
                        } else if let Some(colour) = colour(word) {
                            css.outline = Some(colour);
                        }
                    }
                    css.outline.is_some()
                }
                "border-color" if placed => {
                    css.outline = colour(value);
                    css.outline.is_some()
                }
                "border-width" if placed => {
                    css.outline_width = length(value);
                    css.outline_width.is_some()
                }
                "border-radius" if placed => {
                    css.radius = length(value);
                    css.radius.is_some()
                }
                "padding" if placed => {
                    css.padding = value.split_whitespace().next().and_then(length);
                    css.padding.is_some()
                }
                "color" => {
                    css.color = colour(value);
                    css.color.is_some()
                }
                "font-size" => {
                    css.size = length(value);
                    css.size.is_some()
                }
                "font-weight" => {
                    css.bold = matches!(value.as_str(), "bold" | "bolder") || value.parse::<u32>().is_ok_and(|weight| weight >= 600);
                    true
                }
                "font-style" => {
                    css.italic = matches!(value.as_str(), "italic" | "oblique");
                    true
                }
                "text-decoration" | "text-decoration-line" => {
                    css.underline = value.contains("underline");
                    true
                }
                "transition" => {
                    css.appear = value == "none";
                    value == "none"
                }
                // How the box lays out what's in it, which ours does its own way.
                "display" | "flex-direction" | "flex" | "justify-content" | "align-items" | "align-self" | "isolation" | "box-sizing" | "overflow" | "z-index" | "gap" => true,
                _ => false,
            };
            if !read {
                ignored.push(property.clone());
            }
        }
        for property in ignored {
            self.notice(NoticeKind::Approximation, format!("The style {} isn't carried over.", shortened(&property, 60)));
        }
        css
    }

    // ----- The slide, written.

    /// The slide's items as its body, with a region whose only content is a `<v-clicks>`
    /// from the first click shown a piece at a time, as `settings` then says.
    fn write(&mut self, items: Vec<Item>, settings: &mut Vec<(String, String)>) -> String {
        let layout = self.layout;
        let has_title = layout.regions.contains(&"title");
        // The items of each region, by the region they're written in.
        let mut region = layout.main.to_string();
        let mut regions: Vec<(String, Vec<usize>)> = Vec::new();
        let mut title_taken = !has_title;
        for (index, item) in items.iter().enumerate() {
            match item {
                Item::Region(name) => {
                    region = name.clone();
                    title_taken = true;
                }
                Item::Heading(_) if !title_taken => title_taken = true,
                _ => match regions.iter_mut().find(|(name, _)| *name == region) {
                    Some((_, indexes)) => indexes.push(index),
                    None => regions.push((region.clone(), vec![index])),
                },
            }
        }
        let mut whole: HashSet<usize> = HashSet::new();
        for (name, indexes) in &regions {
            if let [only] = indexes.as_slice()
                && let Item::Each(each) = &items[*only]
                && each.from_one
            {
                whole.insert(*only);
                settings.push((format!("region.{name}"), "click=each enter=fade".to_string()));
            }
        }
        let mut out = Vec::new();
        for (index, item) in items.into_iter().enumerate() {
            match item {
                Item::Region(name) => out.push(format!("<Region name=\"{}\" />", attr_value(&name))),
                Item::Each(each) if whole.contains(&index) => out.push(each.whole),
                Item::Each(each) => {
                    if let Some(text) = each.boxed_notice {
                        self.notice(NoticeKind::Approximation, text);
                    }
                    out.push(each.boxed);
                }
                item => {
                    if !item.text().trim().is_empty() {
                        out.push(item.text().to_string());
                    }
                }
            }
        }
        out.join("\n\n")
    }
}

impl Item {
    /// The item with `attrs` added to the tag it is, a `<Picture>`'s.
    fn with_attrs(self, attrs: Vec<(&'static str, Option<String>)>) -> Item {
        match self {
            Item::Block { text, counts } if text.starts_with("<Picture ") && text.ends_with(" />") && !attrs.is_empty() => {
                let mut more = String::new();
                for (attr, value) in attrs {
                    match value {
                        Some(value) => more.push_str(&format!(" {attr}=\"{}\"", attr_value(&value))),
                        None => more.push_str(&format!(" {attr}")),
                    }
                }
                Item::Block { text: format!("{}{more} />", &text[..text.len() - 3]), counts }
            }
            item => item,
        }
    }
}

fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// A link's title, as written after its target: in double or single quotes, or in brackets,
/// written again in double quotes with what could end it or start a tag escaped. `None` for
/// one that isn't a title.
fn link_title(title: &str) -> Option<String> {
    let inner = [('"', '"'), ('\'', '\''), ('(', ')')].iter().find_map(|(open, close)| title.strip_prefix(*open)?.strip_suffix(*close).filter(|_| title.len() >= 2))?;
    let mut out = String::from("\"");
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            // An escape is kept as written.
            '\\' => match chars.next() {
                Some(next) if next.is_ascii_punctuation() => {
                    out.push('\\');
                    out.push(next);
                }
                next => {
                    out.push_str("\\\\");
                    if let Some(next) = next.filter(|next| !next.is_control()) {
                        out.push(next);
                    }
                }
            },
            '"' | '<' | '`' | '&' => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    Some(out)
}

/// `text` with any tags taken out, as the text of what they mark.
fn strip_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while at < text.len() {
        if text.as_bytes()[at] == b'<'
            && let Some(end) = html::open_tag(text, at).map(|tag| tag.end).or_else(|| html::close_tag(text, at).map(|(_, end)| end))
        {
            at = end;
            continue;
        }
        let c = text[at..].chars().next().unwrap_or(' ');
        out.push(c);
        at += c.len_utf8();
    }
    out
}

/// `text` with no line that would end the slide: a `---` line is a rule written `***`.
fn no_breaks(text: &str) -> String {
    if !text.contains("---") {
        return text.to_string();
    }
    text.split('\n').map(|line| if line.trim_end_matches(' ') == "---" { "***" } else { line }).collect::<Vec<_>>().join("\n")
}

/// A number a component is given, as written or bound to a number or a string of one.
fn number_prop(tag: &OpenTag, name: &str) -> Option<String> {
    if let Some(value) = tag.attr(name) {
        return Some(value.to_string());
    }
    let bound = tag.attr(&format!(":{name}")).or_else(|| tag.attr(&format!("v-bind:{name}")))?.trim();
    let unquoted = ['\'', '"', '`'].iter().find_map(|quote| bound.strip_prefix(*quote).and_then(|rest| rest.strip_suffix(*quote)));
    Some(unquoted.unwrap_or(bound).to_string())
}

/// Whether a line underlines a heading.
fn is_setext(line: &str) -> bool {
    let (column, lead) = indent_of(line);
    let rest = line[lead..].trim_end();
    column <= 3 && !rest.is_empty() && (rest.bytes().all(|byte| byte == b'=') || rest.bytes().all(|byte| byte == b'-'))
}

/// Whether `chunk` is a table: a header row, then a row of `-` and `|`.
fn is_table(chunk: &str) -> bool {
    let mut lines = chunk.lines();
    let (Some(header), Some(rule)) = (lines.next(), lines.next()) else { return false };
    header.contains('|') && rule.contains('-') && rule.trim().bytes().all(|byte| matches!(byte, b'|' | b'-' | b':' | b' ' | b'\t'))
}

/// The items of a list, each as written, the lines under it with it.
fn list_items(chunk: &str) -> Vec<&str> {
    let base = indent_of(chunk).0;
    let mut items = Vec::new();
    let mut start = 0;
    let mut at = 0;
    while at < chunk.len() {
        let (end, next) = line_at(chunk, at);
        let line = &chunk[at..end];
        if at > start && marker_length(line).is_some() && indent_of(line).0 <= base {
            items.push(chunk[start..at].trim_end_matches('\n'));
            start = at;
        }
        at = next;
    }
    items.push(chunk[start..].trim_end_matches('\n'));
    items
}

/// Opens what's on the stack that hasn't been yet, as text is about to be written in it.
fn write_open(out: &mut String, stack: &mut [Inline]) {
    // What isn't open yet is always at the top.
    if stack.last().is_none_or(|open| open.written) {
        return;
    }
    for open in stack.iter_mut().filter(|open| !open.written) {
        out.push_str(&open.open);
        open.written = true;
    }
}

/// Closes all that's open, as a line ends.
fn close_all(out: &mut String, stack: &mut Vec<Inline>) {
    while let Some(open) = stack.pop() {
        if open.written {
            out.push_str(&open.close);
        }
    }
}

/// Closes what's open up to the element `name`, as its closing tag says. A closing tag
/// with nothing open to close is let go of.
fn close_named(out: &mut String, stack: &mut Vec<Inline>, name: &str) {
    let Some(at) = stack.iter().rposition(|open| html::same_name(&open.name, name)) else { return };
    while stack.len() > at {
        if let Some(open) = stack.pop()
            && open.written
        {
            out.push_str(&open.close);
        }
    }
}

/// The talk's look, from the style block Stagepack's export writes on each slide.
fn read_look(css: &str) -> Look {
    let value_of = |from: &str, property: &str| -> Option<String> {
        let at = from.find(&format!("{property}:"))?;
        let rest = &from[at + property.len() + 1..];
        let end = rest.find([';', '}', '\n']).unwrap_or(rest.len());
        Some(rest[..end].trim().to_string())
    };
    let first_family = |value: String| value.split(',').next().map(|name| name.trim().trim_matches(['"', '\'']).to_string()).filter(|name| !name.is_empty());
    let page = css.find(":global(.slidev-page) {").map(|at| {
        let rest = &css[at..];
        &rest[..rest.find('}').unwrap_or(rest.len())]
    });
    Look {
        background: page.and_then(|page| value_of(page, "background")).and_then(|value| colour(&value)),
        text: page.and_then(|page| value_of(page, "\n  color")).and_then(|value| colour(&value)),
        accent: value_of(css, "--slidev-theme-primary").and_then(|value| colour(&value)),
        font: page.and_then(|page| value_of(page, "font-family")).and_then(first_family),
        mono: value_of(css, "--slidev-code-font-family").and_then(first_family),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slides_split_as_slidevs_parser_splits_them() {
        let slides = split("---\ntitle: x\n---\n\n# One\n\n---\n\n# Two\n\n```\n---\n```\n\n<!--\n---\n-->\n---\nlayout: center\n---\n# Three");
        assert_eq!(slides.len(), 3);
        assert_eq!(slides[0].content, "# One");
        assert!(slides[1].content.contains("```\n---\n```"));
        assert_eq!(slides[1].note.as_deref(), Some("---"));
        assert_eq!(slides[2].front.as_ref().unwrap().get("layout").and_then(Value::text), Some("center"));
        // A file that opens with `---` and a blank line keeps the line in its first slide.
        assert_eq!(split("---\n\nx")[0].content, "---\n\nx");
        assert_eq!(split("").len(), 1);
    }

    #[test]
    fn ranges_are_read_as_slidevs() {
        assert_eq!(range_of(5, None), (vec![(1, 5)], false));
        assert_eq!(range_of(5, Some("2,4-")), (vec![(2, 2), (4, 5)], false));
        assert_eq!(range_of(5, Some("0-2;9")), (vec![(1, 2)], false));
        assert_eq!(range_of(5, Some("3,1-2,2-3,5")), (vec![(1, 3), (5, 5)], false));
        assert_eq!(range_of(3, Some("1-99999999999")), (vec![(1, 3)], false));
        assert_eq!(range_of(3, Some("none")), (Vec::new(), false));
        assert_eq!(range_of(3, Some("x,1-2-3")), (vec![(1, 2)], false));
        let many = vec!["1-9999999"; MAX_RANGE_PARTS + 1].join(",");
        assert_eq!(range_of(3, Some(&many)), (vec![(1, 3)], true));
    }

    #[test]
    fn ats_are_read_as_slidevs() {
        assert_eq!(at_value("+2"), Ok(Some(At::After(2))));
        assert_eq!(at_value("3"), Ok(Some(At::On(3))));
        assert_eq!(at_value("0"), Ok(Some(At::On(1))));
        assert_eq!(at_value("false"), Ok(None));
        assert_eq!(at_value("soon"), Err(()));
    }

    #[test]
    fn blocks_are_found() {
        let blocks = scan("# H\n\ntext\nmore\n\n- a\n\n- b\n  more\n\nafter\n\n<div v-click>\n\nin\n\n</div> tail\n\n```js\nx\n```\n::right::\n<!-- c -->\n</div>\n<Counter");
        let kinds: Vec<String> = blocks
            .iter()
            .map(|block| match block {
                Raw::Markdown(text) => format!("md:{}", text.trim_end()),
                Raw::Element { tag, .. } => format!("el:{}", tag.name),
                Raw::Fence { code, .. } => format!("fence:{code}"),
                Raw::Slot(name) => format!("slot:{name}"),
                Raw::Comment => "comment".into(),
                Raw::StrayClose(_) => "close".into(),
                Raw::Unclosed { tag, .. } => format!("unclosed:{}", tag.name),
            })
            .collect();
        assert_eq!(kinds, ["md:# H", "md:text\nmore", "md:- a\n\n- b\n  more", "md:after", "el:div", "md: tail", "fence:x", "slot:right", "comment", "close", "md:<Counter"]);
    }
}
