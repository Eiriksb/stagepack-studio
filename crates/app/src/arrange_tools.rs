//! The Home tab's Arrange tool: one menu that brings the thing selected forward or sends it
//! back, lines the things selected up, and spaces them out. Each item asks the window to do its
//! work, and is off when it can't be done, the menu saying why where there's a reason to say.
//!
//! A press on the tool leaves the keyboard where it is, and once the menu closes, the keyboard
//! is back on the slide with the things still selected.

use gpui_kit::assets::IconName as Icons;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{Disableable as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use slides_format::Order;

use crate::arrange::AlignTo;
use crate::document::{ArrangeHere, Document, EditRefused};
use crate::ribbon::group;

/// The Arrange tool, which opens its menu.
pub const ARRANGE_ID: &str = "arrange";

/// Lines the things selected up on a line: one on the slide's, several on each other's.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = arrange_tools, no_json)]
pub struct AlignThings {
    pub to: AlignTo,
}

/// Spaces the things selected out evenly, across or down.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = arrange_tools, no_json)]
pub struct DistributeThings {
    pub across: bool,
}

/// Brings the thing selected forward or sends it back.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = arrange_tools, no_json)]
pub struct ReorderThing {
    pub order: Order,
}

/// The four ways a thing goes in front or behind, by what the menus call them.
pub const ORDERS: [(Order, &str); 4] = [
    (Order::Front, "Bring to front"),
    (Order::Forward, "Bring forward"),
    (Order::Backward, "Send backward"),
    (Order::Back, "Send to back"),
];

/// The ways things line up, by what the menu calls them.
const ALIGNS: [(AlignTo, &str); 6] = [
    (AlignTo::Left, "Align left"),
    (AlignTo::Centre, "Align centre"),
    (AlignTo::Right, "Align right"),
    (AlignTo::Top, "Align top"),
    (AlignTo::Middle, "Align middle"),
    (AlignTo::Bottom, "Align bottom"),
];

/// `menu` with the four order items, each off where the thing selected can't go that way, and
/// why none of them can be used, if there's a reason to say.
pub fn order_items(menu: PopupMenu, here: &ArrangeHere) -> PopupMenu {
    ORDERS.iter().fold(menu, |menu, &(order, label)| {
        menu.menu_with_disabled(label, Box::new(ReorderThing { order }), !here.can_order(order))
    })
}

/// `menu` with a line that says why some of its items are off, `why`.
pub fn reason(menu: PopupMenu, why: EditRefused) -> PopupMenu {
    menu.item(PopupMenuItem::label(why.to_string()))
}

/// The Arrange group of `document`'s window, for what `here` says is selected. It's off while
/// nothing is. Its menu tries each way the thing can go as it opens. Once the menu closes, the
/// keyboard goes to `slide`, the slide's.
pub fn arrange_group(here: &ArrangeHere, document: Option<Entity<Document>>, slide: FocusHandle, cx: &App) -> AnyElement {
    let here = *here;
    let tool = Button::new(ARRANGE_ID)
        .ghost()
        .small()
        .icon(Icons::BringToFront)
        .label("Arrange")
        .dropdown_caret(true)
        .tooltip("Line things up, space them out, or bring them forward or back")
        .disabled(here.count == 0)
        .dropdown_menu(move |menu, _, cx| {
            let here = document.as_ref().map_or(here, |document| document.read(cx).arrange_menu());
            let menu = order_items(menu.action_context(slide.clone()), &here).separator();
            let menu = ALIGNS.iter().fold(menu, |menu, &(to, label)| {
                menu.menu_with_disabled(label, Box::new(AlignThings { to }), !here.can_align())
            });
            let off = !here.can_distribute();
            let menu = menu
                .separator()
                .menu_with_disabled("Space out across", Box::new(DistributeThings { across: true }), off)
                .menu_with_disabled("Space out down", Box::new(DistributeThings { across: false }), off);
            // Why what's off is off, each said once.
            let mut reasons: Vec<EditRefused> = Vec::new();
            for why in [here.align_refused(), here.order_refused, here.distribute_refused()].into_iter().flatten() {
                if !reasons.contains(&why) {
                    reasons.push(why);
                }
            }
            let menu = menu.when(!reasons.is_empty(), |menu| menu.separator());
            reasons.into_iter().fold(menu, reason)
        });
    div()
        .h_full()
        // A press on the tool, or on the group around it, leaves the keyboard where it is.
        .capture_any_mouse_down(|_, window, _| window.prevent_default())
        .child(group("Arrange", vec![tool.into_any_element()], cx))
        .into_any_element()
}
