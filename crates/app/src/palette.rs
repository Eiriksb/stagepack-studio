//! The component palette: the list the Insert tab's Component tool opens, from which a chart or
//! one of the talk's own components goes on the slide. What the list holds is the document's
//! ([`crate::document::Document::palette`]), listed again only when the talk's components
//! change; what a new one is written with is [`default_attrs`]'s. Opening the list loads the
//! builds not loaded yet, one at a time with the window drawn between, for at most
//! [`LOADING_BUDGET`]; an entry still waiting says so. A click on an entry asks the window to
//! put it on the slide. Nothing here builds anything.

use std::future::Future;
use std::pin::Pin;
use std::task::Poll;
use std::time::{Duration, Instant};

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use slides_engine::ComponentState;
use toolkit::{ComponentSchema, PropKind, Theme};

use crate::document::Document;
use crate::insert::hex;
use crate::inspector::labels::attribute;

/// The list the Component tool opens.
pub const COMPONENT_LIST_ID: &str = "component-list";
/// The line the list has in place of a talk's own components when it has none.
pub const NO_COMPONENTS_ID: &str = "component-list-none";
pub const NO_COMPONENTS_OF_ITS_OWN: &str = "This talk has no components of its own.";
/// What an entry for a component with no build says when it's pointed at.
pub const NEEDS_BUILDING_FIRST: &str = "Build this talk's components first.";
/// What an entry for a component whose build is older than its source says beside its name.
pub const OUT_OF_DATE: &str = "out of date";
/// What an entry whose build isn't loaded yet says beside its name, and when pointed at.
pub const STILL_LOADING: &str = "Still loading…";

/// The most time an opening of the palette spends loading its components' builds.
pub const LOADING_BUDGET: Duration = Duration::from_secs(1);

/// How big a new component is, in slide units.
pub const COMPONENT_SIZE: (f32, f32) = (640.0, 360.0);

/// The built-in components the palette lists. The rest have tools of their own.
pub(crate) const LISTED_BUILT_INS: [&str; 1] = ["Chart"];

/// What a new chart is written with, so that it shows something at once.
const NEW_CHART: [(&str, &str); 3] = [("kind", "bar"), ("labels", "A, B, C"), ("values", "3, 5, 2")];

/// A component the palette offers.
#[derive(Clone, Debug, PartialEq)]
pub struct PaletteEntry {
    /// Its tag, as the talk writes it.
    pub tag: String,
    /// What the list calls it.
    pub label: String,
    /// Whether it's one of the talk's own, rather than a built-in.
    pub ours: bool,
    /// How built it is, for one of the talk's own.
    pub state: Option<ComponentState>,
    /// Whether it draws what's written between its tags, so that it's written with a body and
    /// typing with it selected puts words in it. Known once its build is loaded.
    pub takes_content: bool,
    /// Whether its build is still to be loaded in this opening of the palette, which it can't
    /// be put on the slide till it is.
    pub loading: bool,
}

impl PaletteEntry {
    /// Whether it can go on the slide: it has a build to run, or is a built-in, and isn't
    /// waiting for its build to load.
    pub fn can_insert(&self) -> bool {
        self.state != Some(ComponentState::NeedsBuilding) && !self.loading
    }
}

/// The id of the list's entry for `tag`.
pub fn entry_id(tag: &str) -> String {
    format!("component-pick-{tag}")
}

/// The id of what the entry for `tag` says beside its name, when it says anything.
pub fn note_id(tag: &str) -> String {
    format!("component-note-{tag}")
}

/// What the list calls `tag`: the tag with a space before each capital but the first, so
/// "ScoreCard" is "Score Card".
pub fn label(tag: &str) -> String {
    crate::selection::spaced(tag)
}

/// What `entry` says beside its name: that its build is still loading, or out of date.
pub fn note(entry: &PaletteEntry) -> Option<&'static str> {
    if entry.loading {
        return Some(STILL_LOADING);
    }
    (entry.state == Some(ComponentState::OutOfDate)).then_some(OUT_OF_DATE)
}

/// What `entry` says when pointed at: why it can't go on the slide, if it can't.
pub fn tooltip(entry: &PaletteEntry) -> Option<&'static str> {
    if entry.state == Some(ComponentState::NeedsBuilding) {
        return Some(NEEDS_BUILDING_FIRST);
    }
    entry.loading.then_some(STILL_LOADING)
}

/// The attributes a new tag of the component `schema` describes is written with, in the order
/// it gives its arguments: each required argument at a starting value, an empty text, 0 for a
/// number, the theme's accent for a colour and a choice's first option; a flag, a file and an
/// optional argument aren't written. An argument's name is written with `-` for `_`. A name or
/// value the talk couldn't hold isn't written. A chart is given three labels and their values,
/// so that it shows bars at once.
pub fn default_attrs(schema: &ComponentSchema, theme: &Theme) -> Vec<(String, String)> {
    if schema.name == "Chart" {
        return NEW_CHART.iter().map(|(name, value)| (name.to_string(), value.to_string())).collect();
    }
    schema
        .props
        .iter()
        .filter(|prop| !prop.optional)
        .filter_map(|prop| {
            let value = match &prop.kind {
                PropKind::Whole | PropKind::Number => "0".to_string(),
                PropKind::Color => hex(theme.accent),
                PropKind::Choice(options) => options.first()?.clone(),
                PropKind::Flag | PropKind::File(_) => return None,
                // Text, and any kind a later interface adds, which takes text.
                _ => String::new(),
            };
            let name = attribute(&prop.name);
            (is_attr_name(&name) && is_writable(&value)).then_some((name, value))
        })
        .collect()
}

/// Whether `name` is one the talk can write an attribute as.
fn is_attr_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Whether `value` can be written as an attribute's value: in one kind of quote mark, on one
/// line, with nothing that doesn't draw.
fn is_writable(value: &str) -> bool {
    !(value.contains('"') && value.contains('\'')) && !value.chars().any(char::is_control)
}

/// An entry of the list was clicked: the component with tag `0` is to go on the slide.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct InsertComponent(pub String);

/// The list the Insert tab's Component tool opens: the document's palette, an entry each.
pub struct ComponentList {
    document: Option<Entity<Document>>,
    open: bool,
    /// Loading the builds of the components listed, while it's open and some are still to load.
    loading: Option<Task<()>>,
    _document_changed: Option<Subscription>,
}

impl EventEmitter<InsertComponent> for ComponentList {}

impl ComponentList {
    /// The list of `document`'s palette, which shows the palette as it is whenever it's drawn:
    /// a build that finishes while it's open shows in it.
    pub fn new(document: Option<Entity<Document>>, cx: &mut Context<Self>) -> Self {
        let document_changed = document.as_ref().map(|document| {
            cx.observe(document, |list, _, cx| {
                if list.open {
                    cx.notify();
                }
            })
        });
        ComponentList { document, open: false, loading: None, _document_changed: document_changed }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens the list, starting to load the builds of the components in it not loaded yet, or
    /// closes it, stopping that.
    pub fn set_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.open == open {
            return;
        }
        self.open = open;
        self.loading = None;
        if open && let Some(document) = self.document.clone() {
            document.update(cx, |document, _| document.begin_palette());
            let document = document.downgrade();
            self.loading = Some(cx.spawn(async move |_, cx| load_builds(document, cx).await));
        }
        cx.notify();
    }

    fn pick(&mut self, tag: String, cx: &mut Context<Self>) {
        cx.emit(InsertComponent(tag));
        self.set_open(false, cx);
    }
}

/// Loads the builds of `document`'s palette one at a time, letting the window draw between
/// each, until they're all loaded or [`LOADING_BUDGET`] is spent. The list hears of each.
async fn load_builds(document: WeakEntity<Document>, cx: &mut AsyncApp) {
    let started = Instant::now();
    loop {
        Yield(false).await;
        if started.elapsed() >= LOADING_BUDGET {
            document.update(cx, |document, cx| {
                document.give_up_palette();
                cx.notify();
            }).ok();
            return;
        }
        let more = document.update(cx, |document, cx| {
            let more = document.palette_step();
            cx.notify();
            more
        });
        if !matches!(more, Ok(true)) {
            return;
        }
    }
}

/// Gives the executor back once, so that the window can draw.
struct Yield(bool);

impl Future for Yield {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<()> {
        if self.0 {
            return Poll::Ready(());
        }
        self.0 = true;
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

impl Render for ComponentList {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entries = match (&self.document, self.open) {
            (Some(document), true) => document.update(cx, |document, _| document.palette()),
            _ => Vec::new(),
        };
        let (muted, hover) = (cx.theme().muted_foreground, cx.theme().accent);
        let none_of_its_own = self.open && !entries.iter().any(|entry| entry.ours);
        let rows = entries.into_iter().map(|entry| {
            let can_insert = entry.can_insert();
            let note = note(&entry);
            let tip = tooltip(&entry);
            let tag = entry.tag.clone();
            div()
                .id(SharedString::from(entry_id(&entry.tag)))
                .test_support()
                .aria_label(SharedString::from(entry.label.clone()))
                .flex()
                .flex_row()
                .gap_2()
                .px_2()
                .py_1()
                .rounded_sm()
                .text_sm()
                .child(entry.label)
                .when_some(note, |row, note| {
                    row.child(
                        div()
                            .id(SharedString::from(note_id(&tag)))
                            .test_support()
                            .aria_label(note)
                            .text_color(muted)
                            .child(note),
                    )
                })
                .when(can_insert, |row| {
                    row.cursor_pointer()
                        .hover(move |row| row.bg(hover))
                        .on_click(cx.listener(move |this, _, _, cx| this.pick(tag.clone(), cx)))
                })
                .when(!can_insert, |row| row.text_color(muted))
                .when_some(tip, |row, tip| row.tooltip(move |window, cx| Tooltip::new(tip).build(window, cx)))
        });
        v_flex()
            .id(COMPONENT_LIST_ID)
            .test_support()
            .min_w(px(200.))
            .max_h(px(420.))
            .overflow_y_scroll()
            .p_1()
            .gap(px(2.))
            .children(rows)
            .when(none_of_its_own, |list| {
                list.child(
                    div()
                        .id(NO_COMPONENTS_ID)
                        .test_support()
                        .aria_label(NO_COMPONENTS_OF_ITS_OWN)
                        .px_2()
                        .py_1()
                        .text_sm()
                        .text_color(muted)
                        .child(NO_COMPONENTS_OF_ITS_OWN),
                )
            })
    }
}
