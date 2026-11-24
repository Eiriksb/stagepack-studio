//! What a window shows while it has no talk: the templates a new presentation starts as, each a
//! tile with a picture of its first slide, the two ways to open a talk, and the new
//! presentations that weren't saved last time, to bring back or let go.
//!
//! The screen says what was asked of it with a [`StartEvent`], and the window does it. Showing
//! it reads the app's own data folder and nothing else, and writes nothing.

use std::sync::Arc;
use std::time::SystemTime;

use chrono::{DateTime, Datelike as _, Local};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use slides_engine::{Deck, Fonts, to_bgra_frame};

use crate::document::UntitledId;
use crate::recovery::{Recovery, UntitledCopy};
use crate::templates::{Template, memory_files, templates};
use crate::workspace_view::{self, OPEN_IN_STAGEPACK_ID};

pub const START_SCREEN_ID: &str = "start-screen";
pub const START_OPEN_FILE_ID: &str = "start-open-file";
pub const START_OPEN_FOLDER_ID: &str = "start-open-folder";
/// The line under the buttons saying why a talk couldn't be opened.
pub const START_PROBLEM_ID: &str = "start-problem";
/// The line saying how many presentations from last time couldn't be read.
pub const UNREADABLE_ID: &str = "start-unreadable";
pub const CONFIRM_DISCARD_ID: &str = "confirm-discard";
pub const CANCEL_DISCARD_ID: &str = "cancel-discard";

/// What Discard says of a presentation from last time that a window has open.
pub const OPEN_ELSEWHERE: &str = "It's open in another window.";

/// What Discard asks before a presentation from last time goes.
pub const DISCARD_QUESTION: &str = "Discard this presentation? It can't be brought back.";

/// The key context of a template's tile, where the arrow keys move along the row.
const TILES_CONTEXT: &str = "StartTiles";

/// How big a tile's picture is: the slide's shape, small, so that five fit in the narrowest
/// window.
const TILE_WIDTH: Pixels = px(160.);
const TILE_HEIGHT: Pixels = px(90.);

actions!(start_screen, [NextTile, PreviousTile, PickTile]);

/// The tiles' keys. The window's [`workspace_view::bind_keys`] binds them.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("right", NextTile, Some(TILES_CONTEXT)),
        KeyBinding::new("left", PreviousTile, Some(TILES_CONTEXT)),
        KeyBinding::new("enter", PickTile, Some(TILES_CONTEXT)),
    ]);
}

/// The id of the tile of the template known as `id`.
pub fn template_tile_id(id: &str) -> SharedString {
    format!("template-tile-{id}").into()
}

/// The id of the picture on the tile of the template known as `id`, once it's drawn.
pub fn template_picture_id(id: &str) -> SharedString {
    format!("template-picture-{id}").into()
}

/// The id of the row of the presentation `id` from last time.
pub fn unsaved_row_id(id: UntitledId) -> SharedString {
    format!("unsaved-{id}").into()
}

/// The id of the button that brings back the presentation `id` from last time.
pub fn unsaved_open_id(id: UntitledId) -> SharedString {
    format!("unsaved-open-{id}").into()
}

/// The id of the button that lets go of the presentation `id` from last time.
pub fn unsaved_discard_id(id: UntitledId) -> SharedString {
    format!("unsaved-discard-{id}").into()
}

/// What the screen asks the window to do.
#[derive(Clone, Debug, PartialEq)]
pub enum StartEvent {
    /// Make a new presentation from the template known by this id, here.
    Make(&'static str),
    /// Ask which talk file to open, and open it here.
    OpenFile,
    /// Ask which talk folder to open, and open it here.
    OpenFolder,
    /// Bring back the presentation from last time, here.
    BringBack(UntitledId),
    /// Open the file that couldn't be opened in Stagepack.
    OpenInStagepack,
    /// Opens the Add-ons dialog, for a file the Slidev add-on would open.
    OpenAddOns,
}

pub struct StartScreen {
    /// Where the presentations from last time are kept, if anywhere.
    recovery: Option<Recovery>,
    /// What gives each template's tile the keyboard, in the order of [`templates`].
    tiles: Vec<FocusHandle>,
    /// The presentations from last time, newest first.
    unsaved: Vec<UntitledCopy>,
    /// How many of them couldn't be read.
    unreadable: usize,
    /// Why the talk asked for couldn't be opened.
    problem: Option<SharedString>,
    /// Whether to offer opening it in Stagepack.
    offers_stagepack: bool,
    /// Whether it offers the Add-ons dialog beside the problem.
    offers_add_ons: bool,
    /// The read of the presentations from last time, while it runs.
    _reading: Option<Task<()>>,
    /// The list is read again as the window comes to the front, so that what's listed and when
    /// it was kept, *Today* or *Yesterday*, is as it is now. Nothing is read while it's left.
    _activated: Subscription,
}

impl EventEmitter<StartEvent> for StartScreen {}

impl StartScreen {
    /// The screen, listing the presentations from last time kept in `recovery`. With none it
    /// lists nothing and reads nothing.
    pub fn new(recovery: Option<Recovery>, window: &mut Window, cx: &mut Context<Self>) -> StartScreen {
        let tiles = templates().iter().map(|_| cx.focus_handle()).collect();
        ask_for_pictures(window, cx);
        let activated = cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active() {
                this.refresh(cx);
            }
        });
        let mut screen = StartScreen {
            _activated: activated,
            recovery,
            tiles,
            unsaved: Vec::new(),
            unreadable: 0,
            problem: None,
            offers_stagepack: false,
            offers_add_ons: false,
            _reading: None,
        };
        screen.refresh(cx);
        screen
    }

    /// Gives the first tile the keyboard.
    pub fn take_keyboard(&self, window: &mut Window, cx: &mut App) {
        if let Some(first) = self.tiles.first() {
            first.focus(window, cx);
        }
    }

    /// Where the presentations from last time are kept, if anywhere.
    pub fn recovery(&self) -> Option<&Recovery> {
        self.recovery.as_ref()
    }

    /// Says `problem` under the buttons, or nothing, offering to open the file in Stagepack
    /// too if `offers_stagepack`.
    pub fn set_problem(&mut self, problem: Option<SharedString>, offers_stagepack: bool, cx: &mut Context<Self>) {
        self.set_problem_offering(problem, offers_stagepack, false, cx);
    }

    /// [`Self::set_problem`], offering the Add-ons dialog too if `offers_add_ons`.
    pub fn set_problem_offering(&mut self, problem: Option<SharedString>, offers_stagepack: bool, offers_add_ons: bool, cx: &mut Context<Self>) {
        self.problem = problem;
        self.offers_stagepack = offers_stagepack;
        self.offers_add_ons = offers_add_ons;
        cx.notify();
    }

    /// Reads the presentations from last time again, off the window's thread. Those a window of
    /// this app has open aren't listed.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(recovery) = self.recovery.clone() else {
            return;
        };
        self._reading = Some(cx.spawn(async move |this, cx| {
            let (listed, unreadable) = cx.background_spawn(async move { recovery.untitled() }).await;
            this.update(cx, |this, cx| {
                let open = workspace_view::open_untitled_ids(cx);
                this.unsaved = listed.into_iter().filter(|copy| !open.contains(&copy.id)).collect();
                this.unreadable = unreadable;
                this._reading = None;
                cx.notify();
            })
            .ok();
        }));
    }

    /// Gives the keyboard to the tile `step` along from tile `from`, as far as the row goes.
    fn move_along(&mut self, from: usize, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        let to = from.saturating_add_signed(step).min(self.tiles.len() - 1);
        self.tiles[to].focus(window, cx);
        cx.notify();
    }

    /// Asks before the presentation `id` from last time goes.
    fn ask_to_discard(&mut self, id: UntitledId, window: &mut Window, cx: &mut Context<Self>) {
        let screen = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let screen = screen.clone();
            dialog
                .child(
                    div()
                        .id("discard-question")
                        .test_support()
                        .aria_label(DISCARD_QUESTION)
                        .child(DISCARD_QUESTION),
                )
                .footer(
                    h_flex()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new(CANCEL_DISCARD_ID)
                                .ghost()
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new(CONFIRM_DISCARD_ID)
                                .danger()
                                .label("Discard")
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    screen.update(cx, |screen, cx| screen.discard(id, cx)).ok();
                                }),
                        ),
                )
        });
    }

    /// Lets the presentation `id` from last time go, off the window's thread, and takes it off
    /// every start screen.
    fn discard(&mut self, id: UntitledId, cx: &mut Context<Self>) {
        let Some(recovery) = self.recovery.clone() else {
            return;
        };
        // Its old copy is still there for a moment after another window brings it back, and
        // that window's edits would go with it.
        if workspace_view::open_untitled_ids(cx).contains(&id) {
            self.set_problem(Some(OPEN_ELSEWHERE.into()), false, cx);
            self.refresh(cx);
            return;
        }
        self.unsaved.retain(|copy| copy.id != id);
        cx.notify();
        cx.spawn(async move |_, cx| {
            cx.background_spawn(async move { recovery.discard_untitled(id) }).await;
            cx.update(workspace_view::refresh_start_screens);
        })
        .detach();
    }

    fn tile(&self, index: usize, template: &'static Template, window: &Window, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let focus = &self.tiles[index];
        let has_keyboard = focus.is_focused(window);
        let id = template.id;
        let picture = tile_picture(id, cx);
        v_flex()
            .id(template_tile_id(id))
            .test_support()
            .track_focus(focus)
            .key_context(TILES_CONTEXT)
            .aria_label(template.name)
            .on_action(cx.listener(move |this, _: &NextTile, window, cx| this.move_along(index, 1, window, cx)))
            .on_action(cx.listener(move |this, _: &PreviousTile, window, cx| this.move_along(index, -1, window, cx)))
            .on_action(cx.listener(move |_, _: &PickTile, _, cx| cx.emit(StartEvent::Make(id))))
            .on_click(cx.listener(move |_, _, _, cx| cx.emit(StartEvent::Make(id))))
            .flex_none()
            // The picture's frame, with room around it for the ring that shows the keyboard.
            .w(TILE_WIDTH + px(22.))
            .gap_1()
            .p_2()
            .rounded(theme.radius)
            .border_2()
            .border_color(if has_keyboard { theme.ring } else { theme.transparent })
            .cursor_pointer()
            .hover(|this| this.bg(theme.list_hover))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_none()
                    .w(TILE_WIDTH + px(2.))
                    .h(TILE_HEIGHT + px(2.))
                    .overflow_hidden()
                    .rounded(px(3.))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.background)
                    .map(|frame| match picture {
                        Some(image) => frame.child(
                            div()
                                .id(template_picture_id(id))
                                .test_support()
                                .size_full()
                                .child(img(image).size_full()),
                        ),
                        // Its name on a plain panel, until its picture is drawn.
                        None => frame.text_sm().text_color(theme.muted_foreground).child(template.name),
                    }),
            )
            .child(div().text_sm().child(said(("tile-name", index), template.name)))
            .child(div().text_xs().text_color(theme.muted_foreground).child(said(("tile-says", index), template.says)))
    }

    fn unsaved_row(&self, copy: &UntitledCopy, now: SystemTime, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let id = copy.id;
        let slides = match (copy.slides, copy.slides_at_least) {
            (count, true) => format!("{count} or more slides"),
            (1, false) => "1 slide".to_string(),
            (count, false) => format!("{count} slides"),
        };
        h_flex()
            .id(unsaved_row_id(id))
            .test_support()
            .w_full()
            .gap_3()
            .px_2()
            .py_2()
            .items_center()
            .border_b_1()
            .border_color(theme.border)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_sm().truncate().child(said("unsaved-name", copy.name.clone())))
                    .when(!copy.first_words.is_empty(), |this| {
                        this.child(
                            div()
                                .text_xs()
                                .truncate()
                                .text_color(theme.muted_foreground)
                                .child(said("unsaved-words", copy.first_words.clone())),
                        )
                    }),
            )
            .child(div().flex_none().text_xs().text_color(theme.muted_foreground).child(said("unsaved-slides", slides)))
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(said("unsaved-when", when_words(copy.when, now))),
            )
            .child(
                Button::new(unsaved_open_id(id))
                    .small()
                    .primary()
                    .label("Open")
                    .on_click(cx.listener(move |_, _, _, cx| cx.emit(StartEvent::BringBack(id)))),
            )
            .child(
                Button::new(unsaved_discard_id(id))
                    .small()
                    .ghost()
                    .label("Discard")
                    .on_click(cx.listener(move |this, _, window, cx| this.ask_to_discard(id, window, cx))),
            )
    }
}

impl Render for StartScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let tiles: Vec<AnyElement> = templates()
            .iter()
            .enumerate()
            .map(|(index, template)| self.tile(index, template, window, cx).into_any_element())
            .collect();
        let now = SystemTime::now();
        let rows: Vec<AnyElement> =
            self.unsaved.iter().map(|copy| self.unsaved_row(copy, now, cx).into_any_element()).collect();
        let lists = !self.unsaved.is_empty() || self.unreadable > 0;
        let heading = |id: &'static str, words: &'static str| div().text_lg().child(said(id, words));
        div()
            .id(START_SCREEN_ID)
            .test_support()
            .size_full()
            .overflow_y_scroll()
            .bg(theme.background)
            .child(
                v_flex()
                    .mx_auto()
                    .max_w(px(960.))
                    .px_4()
                    .py_8()
                    .gap_4()
                    .child(heading("start-new", "New presentation"))
                    // Lined up at the top, as a line under a tile can take two lines.
                    .child(h_flex().gap_2().items_start().children(tiles))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new(START_OPEN_FILE_ID)
                                    .label("Open a talk file…")
                                    .on_click(cx.listener(|_, _, _, cx| cx.emit(StartEvent::OpenFile))),
                            )
                            .child(
                                Button::new(START_OPEN_FOLDER_ID)
                                    .label("Open a talk folder…")
                                    .on_click(cx.listener(|_, _, _, cx| cx.emit(StartEvent::OpenFolder))),
                            ),
                    )
                    .when_some(self.problem.clone(), |this, problem| {
                        this.child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(div().text_sm().text_color(theme.danger).child(said(START_PROBLEM_ID, problem)))
                                .when(self.offers_stagepack, |this| {
                                    this.child(
                                        Button::new(OPEN_IN_STAGEPACK_ID)
                                            .small()
                                            .label("Open in Stagepack")
                                            .on_click(cx.listener(|_, _, _, cx| cx.emit(StartEvent::OpenInStagepack))),
                                    )
                                })
                                .when(self.offers_add_ons, |this| {
                                    this.child(
                                        Button::new(workspace_view::OFFER_ADD_ONS_ID)
                                            .small()
                                            .label("Add-ons…")
                                            .on_click(cx.listener(|_, _, _, cx| cx.emit(StartEvent::OpenAddOns))),
                                    )
                                }),
                        )
                    })
                    .when(lists, |this| {
                        this.child(div().pt_4().child(heading("start-unsaved", "Not saved last time")))
                            .child(v_flex().w_full().children(rows))
                            .when(self.unreadable > 0, |this| {
                                let line = format!("{} couldn't be read.", self.unreadable);
                                this.child(div().text_xs().text_color(theme.muted_foreground).child(said(UNREADABLE_ID, line)))
                            })
                    }),
            )
    }
}

/// Words shown, by `id`, with the same words as their name for anything that reads them out.
fn said(id: impl Into<ElementId>, words: impl Into<SharedString>) -> impl IntoElement {
    let words = words.into();
    div().id(id).test_support().aria_label(words.clone()).child(words)
}

/// How the time `when` reads, at `now`, in this computer's time zone: *Today 14:05*,
/// *Yesterday*, or the day, *12 November*, with its year if it isn't this one.
pub fn when_words(when: SystemTime, now: SystemTime) -> String {
    let (when, now): (DateTime<Local>, DateTime<Local>) = (when.into(), now.into());
    let (day, today) = (when.date_naive(), now.date_naive());
    if day == today {
        format!("Today {}", when.format("%H:%M"))
    } else if today.pred_opt() == Some(day) {
        "Yesterday".to_string()
    } else if day.year() == today.year() {
        day.format("%-d %B").to_string()
    } else {
        day.format("%-d %B %Y").to_string()
    }
}

/// The pictures of the templates' first slides, drawn once while the app runs, for every start
/// screen.
#[derive(Default)]
struct TilePictures {
    /// Each template's, in the order of [`templates`], once they're drawn.
    drawn: Option<Vec<Option<Arc<RenderImage>>>>,
    /// Whether they're being drawn.
    drawing: bool,
    /// The screens waiting for them.
    waiting: Vec<WeakEntity<StartScreen>>,
}

impl Global for TilePictures {}

/// The picture of the first slide of the template known as `id`, once it's drawn.
pub fn tile_picture(id: &str, cx: &App) -> Option<Arc<RenderImage>> {
    let at = templates().iter().position(|template| template.id == id)?;
    cx.try_global::<TilePictures>()?.drawn.as_ref()?.get(at)?.clone()
}

/// Every tile's picture, to let the window go of them once the screen no longer shows.
pub fn tile_pictures(cx: &App) -> Vec<Arc<RenderImage>> {
    cx.try_global::<TilePictures>()
        .and_then(|pictures| pictures.drawn.as_ref())
        .map(|drawn| drawn.iter().flatten().cloned().collect())
        .unwrap_or_default()
}

/// Has the tiles' pictures drawn, if they aren't yet, on one background task, at the size of a
/// tile in `window`. The screen is drawn again once they are.
fn ask_for_pictures(window: &Window, cx: &mut Context<StartScreen>) {
    let screen = cx.weak_entity();
    let pictures = cx.default_global::<TilePictures>();
    if pictures.drawn.is_some() {
        return;
    }
    pictures.waiting.push(screen);
    if std::mem::replace(&mut pictures.drawing, true) {
        return;
    }
    let width = (f32::from(TILE_WIDTH) * window.scale_factor()).round() as u32;
    cx.spawn(async move |_, cx| {
        let drawn = cx.background_spawn(async move { draw_pictures(width) }).await;
        cx.update(|cx| {
            let pictures = cx.default_global::<TilePictures>();
            pictures.drawn = Some(drawn);
            pictures.drawing = false;
            for screen in std::mem::take(&mut pictures.waiting) {
                screen.update(cx, |_, cx| cx.notify()).ok();
            }
        });
    })
    .detach();
}

/// The first slide of each template, `width` pixels wide, drawn by one deck with the bundled
/// fonts, which are loaded once for them all.
fn draw_pictures(width: u32) -> Vec<Option<Arc<RenderImage>>> {
    let mut deck: Option<Deck> = None;
    templates()
        .iter()
        .map(|template| {
            let files = Box::new(memory_files(template));
            let source = template.slides.to_string();
            match deck.as_mut() {
                Some(deck) => {
                    deck.set_files(files);
                    deck.update(source);
                }
                None => deck = Some(Deck::open(files, Fonts::bundled_only(), source)),
            }
            let pixmap = deck.as_mut()?.thumbnail(0, width)?;
            Some(Arc::new(RenderImage::new(vec![to_bgra_frame(&pixmap)])))
        })
        .collect()
}
