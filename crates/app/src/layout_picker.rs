//! The tiles a slide's layout is picked from: one for each layout, a small drawing of where its
//! areas are, with its name under it. The Home tab's Layout tool shows them for the slide shown,
//! and the New slide arrow for the slide it adds.

use std::rc::Rc;

use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use slides_engine::{LayoutDef, layouts};

/// Gives the slide shown the layout called `layout`, by the engine's name for it.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = layout_picker, no_json)]
pub struct SetLayout {
    pub layout: &'static str,
}

/// What holds the tiles.
pub const LAYOUT_TILES_ID: &str = "layout-tiles";

/// How big a tile's drawing is: the slide's shape, small.
const TILE_WIDTH: f32 = 96.;
const TILE_HEIGHT: f32 = 54.;
/// How wide the slide a layout is drawn on is, in its own units.
const SLIDE_WIDTH: f32 = 1920.;

/// The id of the tile for the layout called `layout`, by the engine's name for it.
pub fn layout_tile_id(layout: &str) -> SharedString {
    format!("layout-tile-{layout}").into()
}

/// The id of the rectangle the tile for `layout` draws for its area `index`, in the order the
/// layout has them.
pub fn layout_area_id(layout: &str, index: usize) -> SharedString {
    format!("layout-tile-{layout}-area-{index}").into()
}

/// The name people know the layout called `layout` by. A name that isn't a layout's is given
/// back as it is.
pub fn layout_name(layout: &str) -> &str {
    match layout {
        "title" => "Title",
        "content" => "Title and content",
        "two-columns" => "Two columns",
        "section" => "Section",
        "blank" => "Blank",
        other => other,
    }
}

/// A tile for each layout, in the engine's order, the one called `current` marked. A click on
/// one hands `on_pick` that layout's name.
pub fn layout_tiles(
    current: Option<&str>,
    on_pick: impl Fn(&'static str, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let on_pick = Rc::new(on_pick);
    h_flex()
        .id(LAYOUT_TILES_ID)
        .test_support()
        .gap_2()
        .children(layouts().iter().map(|layout| {
            let on_pick = on_pick.clone();
            tile(layout, current == Some(layout.name), move |window, cx| on_pick(layout.name, window, cx), cx)
        }))
        .into_any_element()
}

/// The tile for `layout`, marked if it's the `current` one, which a click on does `on_click`.
fn tile(
    layout: &'static LayoutDef,
    current: bool,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let scale = TILE_WIDTH / SLIDE_WIDTH;
    let name = layout_name(layout.name);
    let areas = layout.regions.iter().enumerate().map(|(index, region)| {
        let rect = region.rect;
        div()
            .id(layout_area_id(layout.name, index))
            .test_support()
            .absolute()
            .left(px(rect.x * scale))
            .top(px(rect.y * scale))
            .w(px(rect.w * scale))
            .h(px(rect.h * scale))
            .rounded(px(1.))
            .border_1()
            .border_color(theme.muted_foreground.opacity(0.5))
            .bg(theme.muted)
    });
    v_flex()
        .id(layout_tile_id(layout.name))
        .on_click(move |_, window, cx| on_click(window, cx))
        .test_support()
        .aria_label(name)
        .aria_selected(current)
        .items_center()
        .gap_1()
        .p_1()
        .rounded(theme.radius)
        .cursor_pointer()
        .when(current, |this| this.bg(theme.list_active))
        .when(!current, |this| this.hover(|this| this.bg(theme.list_hover)))
        .child(
            div()
                .relative()
                .flex_none()
                .w(px(TILE_WIDTH))
                .h(px(TILE_HEIGHT))
                .overflow_hidden()
                .rounded(px(2.))
                .border_1()
                .border_color(if current { theme.primary } else { theme.border })
                .bg(theme.background)
                .children(areas),
        )
        .child(
            div()
                .text_xs()
                .whitespace_nowrap()
                .text_color(if current { theme.foreground } else { theme.muted_foreground })
                .child(name),
        )
        .into_any_element()
}
