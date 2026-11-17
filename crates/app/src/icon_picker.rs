//! The icon picker: a search field, and under it a grid of the talk's icons to choose one from.
//! It opens from the Insert tab's Icon tool, to put an icon on the slide, and from an icon's Icon
//! field in the inspector, to change which icon it is. Whoever opened it hears which was picked.
//!
//! The icons are drawn only for the rows on screen, in the theme's text colour, and kept while
//! they're near them; the picker lets go of them all once it's closed. Nothing is drawn while
//! it sits still.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use gpui_kit::component::input::{self, Input, InputEvent, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, VirtualListScrollHandle, h_flex, v_flex, v_virtual_list,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use slides_engine::{icon_names, icon_preview, to_bgra_frame};

/// The search field at the top of the picker.
pub const ICON_SEARCH_ID: &str = "icon-search";
/// The grid of icons under it.
pub const ICON_GRID_ID: &str = "icon-grid";
/// What the picker says in place of the grid when no icon matches.
pub const NO_ICONS_ID: &str = "icon-none";
pub const NO_MATCHES: &str = "No icons match.";

/// The key context of the picker.
pub const PICKER_CONTEXT: &str = "IconPicker";

/// How many icons a row of the grid holds.
pub const ACROSS: usize = 8;
/// How big an icon is drawn in the grid, in pixels.
pub const PREVIEW_SIZE: f32 = 32.;
/// How big a cell of the grid is.
const CELL: Pixels = px(40.);
/// How many rows the grid shows at a time.
const ROWS_SHOWN: f32 = 8.;
/// How many rows either side of those on screen keep their pictures once drawn.
const KEPT_AROUND: usize = 16;
/// The most pixels across an icon is drawn with, however dense the screen.
const MOST_PIXELS: u32 = 128;

/// The id of the cell of the grid that shows icon `name`.
pub fn icon_id(name: &str) -> String {
    format!("icon-pick-{name}")
}

/// The names of the icons that match `query`, as the picker lists them: those holding every word
/// of it, case and `-` ignored; those that start with it first, then the rest, each lot in name
/// order. An empty query gives every name.
pub fn matching(query: &str) -> Vec<&'static str> {
    let names = icon_names();
    let folded = |text: &str| text.to_lowercase().replace('-', "");
    // The names as they're searched, worked out once.
    static FOLDED: OnceLock<Vec<String>> = OnceLock::new();
    let folded_names = FOLDED.get_or_init(|| names.iter().map(|name| folded(name)).collect());
    let mut words: Vec<String> = query.split_whitespace().map(folded).filter(|word| !word.is_empty()).collect();
    if words.is_empty() {
        return names.to_vec();
    }
    let start = words.concat();
    words.sort_unstable();
    words.dedup();
    // A word longer than every name is in none of them.
    let longest = folded_names.iter().map(String::len).max().unwrap_or(0);
    if words.iter().any(|word| word.len() > longest) {
        return Vec::new();
    }
    let (mut first, mut rest) = (Vec::new(), Vec::new());
    for (&name, name_folded) in names.iter().zip(folded_names) {
        if !words.iter().all(|word| name_folded.contains(word.as_str())) {
            continue;
        }
        if name_folded.starts_with(&start) { first.push(name) } else { rest.push(name) }
    }
    first.extend(rest);
    first
}

/// An icon was picked: the name of the one picked.
pub struct IconPicked(pub &'static str);

/// How the icons are drawn: their colour as red, green, blue and alpha, and how many pixels
/// across.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Look {
    color: [u8; 4],
    pixels: u32,
}

pub struct IconPicker {
    search: Entity<InputState>,
    open: bool,
    /// The names of the icons the grid shows, in order.
    matches: Vec<&'static str>,
    /// The size of each row of the grid.
    rows: Rc<Vec<Size<Pixels>>>,
    scroll: VirtualListScrollHandle,
    /// The icons drawn, kept while they're near the rows on screen.
    previews: HashMap<&'static str, Arc<RenderImage>>,
    /// How the icons kept were drawn, and how they're to be drawn.
    drawn_as: Option<Look>,
    look: Look,
    /// The icon an inspector's Icon field shows, as asked for by its name.
    chosen: Option<(String, Look, Option<Arc<RenderImage>>)>,
    /// Pictures let go of, which the window lets go of once it's drawn again.
    retired: Vec<Arc<RenderImage>>,
    /// How many icons have been drawn for the grid.
    made: usize,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<IconPicked> for IconPicker {}

impl IconPicker {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search icons"));
        let subscriptions = vec![cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                this.search_changed(cx);
            }
        })];
        let matches = icon_names().to_vec();
        IconPicker {
            search,
            open: false,
            rows: rows_for(matches.len()),
            matches,
            scroll: VirtualListScrollHandle::new(),
            previews: HashMap::new(),
            drawn_as: None,
            look: Look { color: [0, 0, 0, 255], pixels: PREVIEW_SIZE as u32 },
            chosen: None,
            retired: Vec::new(),
            made: 0,
            _subscriptions: subscriptions,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// What the search field holds.
    pub fn query(&self, cx: &App) -> String {
        self.search.read(cx).value().to_string()
    }

    /// The names of the icons the grid shows, in order.
    pub fn matches(&self) -> &[&'static str] {
        &self.matches
    }

    /// Whether the search field has the keyboard.
    pub fn has_keyboard(&self, window: &Window, cx: &App) -> bool {
        self.search.read(cx).focus_handle(cx).is_focused(window)
    }

    /// What the search field holds the keyboard by, which it's given as the picker opens.
    pub fn search_focus(&self, cx: &App) -> FocusHandle {
        self.search.read(cx).focus_handle(cx)
    }

    /// How many icons have been drawn for the grid.
    pub fn previews_made(&self) -> usize {
        self.made
    }

    /// How many drawn icons are kept.
    pub fn previews_kept(&self) -> usize {
        self.previews.len()
    }

    /// Whether icon `name` is drawn and kept.
    pub fn has_preview(&self, name: &str) -> bool {
        self.previews.contains_key(name)
    }

    /// How many bytes the icons kept take.
    pub fn preview_bytes(&self) -> usize {
        let pixels = self.drawn_as.map_or(0, |look| look.pixels as usize);
        self.previews.len() * pixels * pixels * 4
    }

    /// The colour the icons kept were drawn in, as red, green, blue and alpha.
    pub fn preview_color(&self) -> Option<[u8; 4]> {
        self.drawn_as.map(|look| look.color)
    }

    /// Opens the picker afresh, with an empty search and the grid at its top, or closes it.
    pub fn set_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        if open == self.open {
            return;
        }
        if open {
            self.search.update(cx, |search, cx| search.set_value("", window, cx));
            self.show(icon_names().to_vec());
            self.open = true;
            cx.notify();
        } else {
            self.close(cx);
        }
    }

    /// Closes the picker, and lets go of the icons drawn.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        self.open = false;
        self.retire_all();
        cx.notify();
    }

    /// Lets go of what the window was drawing that the picker no longer shows. Whoever shows the
    /// picker calls this as it's drawn, so that this happens though the picker itself isn't.
    pub fn release(&mut self, window: &mut Window) {
        for image in self.retired.drain(..) {
            window.drop_image(image).ok();
        }
    }

    /// The picture of icon `name` for an Icon field that shows it, drawn in the theme's text
    /// colour; none if there's no such icon.
    pub fn image_of(&mut self, name: &str, window: &Window, cx: &App) -> Option<Arc<RenderImage>> {
        let look = look_of(window, cx);
        if let Some((shown, drawn, image)) = &self.chosen
            && shown == name
            && *drawn == look
        {
            return image.clone();
        }
        let image = icon_preview(name, look.pixels, look.color).map(|pixmap| Arc::new(RenderImage::new(vec![to_bgra_frame(&pixmap)])));
        if let Some((_, _, Some(old))) = self.chosen.take() {
            self.retired.push(old);
        }
        self.chosen = Some((name.to_string(), look, image.clone()));
        image
    }

    /// Lets go of every picture the picker holds, the Icon field's too, as the picker's owner
    /// is going: whoever it was drawn in lets go of them in the window.
    pub fn take_images(&mut self) -> Vec<Arc<RenderImage>> {
        self.retire_all();
        if let Some((_, _, Some(image))) = self.chosen.take() {
            self.retired.push(image);
        }
        std::mem::take(&mut self.retired)
    }

    fn retire_all(&mut self) {
        self.retired.extend(self.previews.drain().map(|(_, image)| image));
    }

    /// Shows `matches` in the grid, from its top.
    fn show(&mut self, matches: Vec<&'static str>) {
        self.rows = rows_for(matches.len());
        self.matches = matches;
        self.scroll.set_offset(Point::default());
    }

    fn search_changed(&mut self, cx: &mut Context<Self>) {
        let matches = matching(&self.query(cx));
        if matches != self.matches {
            self.show(matches);
            cx.notify();
        }
    }

    /// Picks icon `name`: the picker closes, and whoever opened it hears which.
    fn pick(&mut self, name: &'static str, cx: &mut Context<Self>) {
        self.close(cx);
        cx.emit(IconPicked(name));
    }

    /// Picks the first icon that matches, if one does.
    fn pick_first(&mut self, cx: &mut Context<Self>) {
        if let Some(&first) = self.matches.first() {
            self.pick(first, cx);
        }
    }

    /// The rows of the grid `rows` covers, each with the icons drawn for it so far.
    fn grid_rows(&mut self, rows: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let hover = cx.theme().accent;
        let radius = cx.theme().radius;
        rows.map(|row| {
            let names = self.matches.get(row * ACROSS..((row + 1) * ACROSS).min(self.matches.len())).unwrap_or_default();
            h_flex()
                .h(CELL)
                .children(names.iter().map(|&name| {
                    div()
                        .id(SharedString::from(icon_id(name)))
                        .test_support()
                        .aria_label(name)
                        .size(CELL)
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(radius)
                        .cursor_pointer()
                        .hover(move |cell| cell.bg(hover))
                        .tooltip(move |window, cx| Tooltip::new(name).build(window, cx))
                        .on_click(cx.listener(move |this, _, _, cx| this.pick(name, cx)))
                        .when_some(self.previews.get(name).cloned(), |cell, image| {
                            cell.child(img(image).size(px(PREVIEW_SIZE)))
                        })
                }))
                .into_any_element()
        })
        .collect()
    }

    /// Takes in which rows of the grid are on screen: their icons are drawn, those far from
    /// them let go of. The grid is drawn again only if that changed anything.
    fn rows_on_screen(&mut self, rows: Range<usize>, cx: &mut Context<Self>) {
        let count = self.rows.len();
        let kept = rows.start.saturating_sub(KEPT_AROUND)..(rows.end + KEPT_AROUND).min(count);
        let kept: HashSet<&'static str> = self.names_in(kept).iter().copied().collect();
        let gone: Vec<&'static str> = self.previews.keys().copied().filter(|name| !kept.contains(name)).collect();
        let mut changed = !gone.is_empty();
        for name in gone {
            if let Some(image) = self.previews.remove(name) {
                self.retired.push(image);
            }
        }
        let look = self.look;
        let shown: Vec<&'static str> = self.names_in(rows).to_vec();
        for name in shown {
            if self.previews.contains_key(name) {
                continue;
            }
            self.made += 1;
            changed = true;
            if let Some(pixmap) = icon_preview(name, look.pixels, look.color) {
                self.previews.insert(name, Arc::new(RenderImage::new(vec![to_bgra_frame(&pixmap)])));
            }
        }
        if changed {
            cx.notify();
        }
    }

    /// The names of the icons in grid rows `rows`.
    fn names_in(&self, rows: Range<usize>) -> &[&'static str] {
        let len = self.matches.len();
        let (start, end) = ((rows.start * ACROSS).min(len), (rows.end * ACROSS).min(len));
        &self.matches[start..end.max(start)]
    }
}

/// The rows of a grid of `count` icons.
fn rows_for(count: usize) -> Rc<Vec<Size<Pixels>>> {
    Rc::new(vec![size(CELL * ACROSS as f32, CELL); count.div_ceil(ACROSS)])
}

/// How icons are drawn in `window` now: in the theme's text colour, sharp on its screen.
fn look_of(window: &Window, cx: &App) -> Look {
    let color = Rgba::from(cx.theme().foreground);
    let color = [color.r, color.g, color.b, color.a].map(|part| (part.clamp(0., 1.) * 255.).round() as u8);
    let pixels = ((PREVIEW_SIZE * window.scale_factor()).round() as u32).clamp(1, MOST_PIXELS);
    Look { color, pixels }
}

impl Render for IconPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Icons drawn in another colour, as before the theme changed, or for another screen,
        // are drawn again.
        self.look = look_of(window, cx);
        if self.drawn_as != Some(self.look) {
            self.retire_all();
            self.drawn_as = Some(self.look);
        }
        self.release(window);
        let grid: AnyElement = if self.matches.is_empty() {
            div()
                .id(NO_ICONS_ID)
                .test_support()
                .aria_label(NO_MATCHES)
                .h(CELL * ROWS_SHOWN)
                .w(CELL * ACROSS as f32)
                .p_2()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(NO_MATCHES)
                .into_any_element()
        } else {
            let scroll = self.scroll.clone();
            let picker = cx.entity().downgrade();
            let row_count = self.rows.len();
            div()
                .id(ICON_GRID_ID)
                .test_support()
                .relative()
                .h(CELL * ROWS_SHOWN)
                .w(CELL * ACROSS as f32 + px(12.))
                .child(
                    v_virtual_list(cx.entity(), "icon-rows", self.rows.clone(), |this, rows, _, cx| {
                        this.grid_rows(rows, cx)
                    })
                    .track_scroll(&self.scroll)
                    .size_full(),
                )
                // Laid out after the grid: which rows are on screen is worked out from where it's
                // scrolled to, never from the rows the grid asks for, as it asks for one to
                // measure too.
                .child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, _, cx| {
                            let top = (-scroll.offset().y).max(Pixels::ZERO);
                            let first = (top / CELL).floor() as usize;
                            let last = ((top + bounds.size.height) / CELL).ceil() as usize;
                            let rows = first.min(row_count)..last.min(row_count);
                            picker.update(cx, |picker, cx| picker.rows_on_screen(rows, cx)).ok();
                        },
                    )
                    .absolute()
                    .size_full(),
                )
                .into_any_element()
        };
        v_flex()
            .id("icon-picker")
            .key_context(PICKER_CONTEXT)
            .gap_2()
            // Enter picks the first icon that matches; Escape closes the picker. Neither goes on
            // to the slide or to the popover around the picker.
            .capture_action(cx.listener(|this, _: &input::Enter, _, cx| {
                cx.stop_propagation();
                this.pick_first(cx);
            }))
            .capture_action(cx.listener(|this, _: &input::Escape, _, cx| {
                cx.stop_propagation();
                this.close(cx);
            }))
            .child(
                Input::new(&self.search)
                    .id(ICON_SEARCH_ID)
                    .aria_label("Search icons")
                    .small()
                    .prefix(Icon::new(IconName::Search).small()),
            )
            .child(grid)
    }
}

/// The picker's keys. While its search field has the keyboard, the window's own keys for slides
/// are off.
pub fn bind_keys(cx: &mut App) {
    let context = Some(PICKER_CONTEXT);
    cx.bind_keys(
        ["left", "right", "up", "down", "pageup", "pagedown", "ctrl-m", "ctrl-d"]
            .map(|key| KeyBinding::new(key, NoAction, context)),
    );
}
