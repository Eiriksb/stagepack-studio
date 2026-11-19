//! The Insert tab's tools: Picture, Video, the Shapes list, Text box, Icon, Table and Component. What
//! they put on the slide is [`crate::insert`]'s and [`crate::document::Document`]'s; the window
//! does what they ask. Icon opens the [`IconPicker`], whose pick the window hears of, Table the
//! [`TableGrid`], whose square clicked asks for a table that size, and Component the
//! [`ComponentList`], whose entry clicked asks for that component.

use gpui_kit::assets::IconName as Icons;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{Disableable as _, Sizable as _};
use gpui_kit::*;

use crate::icon_picker::IconPicker;
use crate::palette::ComponentList;
use crate::ribbon::group;
use crate::table_tools::TableGrid;

pub const INSERT_PICTURE_ID: &str = "insert-picture";
/// The Video tool, which asks for a video file to put on the slide.
pub const INSERT_VIDEO_ID: &str = "insert-video";
/// The Shapes button, which lists the shapes.
pub const INSERT_SHAPES_ID: &str = "insert-shapes";
pub const INSERT_TEXT_BOX_ID: &str = "insert-text-box";
/// The Icon tool, which opens the icon picker.
pub const INSERT_ICON_ID: &str = "insert-icon";
/// The Table tool, which opens the grid of table sizes.
pub const INSERT_TABLE_ID: &str = "insert-table";
/// The Component tool, which opens the list of components.
pub const INSERT_COMPONENT_ID: &str = "insert-component";

actions!(insert_tools, [InsertPicture, InsertVideo, InsertTextBox]);

/// Puts a shape of kind `kind`, the talk's name for it, on the slide.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = insert_tools, no_json)]
pub struct InsertShape {
    pub kind: &'static str,
}

/// The shapes the list offers: the talk's name for each, the name people know it by, its
/// icon, and the id of its row in the list.
const SHAPES: [(&str, &str, Icons, &str); 6] = [
    ("rect", "Rectangle", Icons::RectangleHorizontal, "insert-shape-rect"),
    ("rounded", "Rounded rectangle", Icons::SquareRoundCorner, "insert-shape-rounded"),
    ("ellipse", "Ellipse", Icons::Circle, "insert-shape-ellipse"),
    ("line", "Line", Icons::Minus, "insert-shape-line"),
    ("arrow", "Arrow", Icons::MoveRight, "insert-shape-arrow"),
    ("callout", "Callout", Icons::MessageSquare, "insert-shape-callout"),
];

/// The id of the Shapes list's row for the shape of kind `kind`, the talk's name for it. A kind
/// the list doesn't have has the rectangle's.
pub fn shape_id(kind: &str) -> &'static str {
    SHAPES.iter().find(|shape| shape.0 == kind).unwrap_or(&SHAPES[0]).3
}

/// The Insert tab's groups, for a window with a talk if `has_talk`, whose Icon tool opens
/// `icon_picker`, whose Table tool opens `table_grid` and whose Component tool opens
/// `component_list`.
pub fn insert_tab(
    has_talk: bool,
    icon_picker: &Entity<IconPicker>,
    table_grid: &Entity<TableGrid>,
    component_list: &Entity<ComponentList>,
    cx: &App,
) -> Vec<AnyElement> {
    let shapes = Button::new(INSERT_SHAPES_ID)
        .ghost()
        .small()
        .icon(Icons::Shapes)
        .label("Shapes")
        .tooltip("Put a shape on the slide")
        .disabled(!has_talk)
        .dropdown_menu(|menu, _, _| {
            SHAPES.iter().fold(menu, |menu, &(kind, name, icon, id)| {
                menu.menu_element_with_icon(icon, Box::new(InsertShape { kind }), move |_, _| {
                    div().id(id).test_support().aria_label(name).child(name)
                })
            })
        });
    vec![
        group(
            "Pictures",
            vec![
                tool(INSERT_PICTURE_ID, Icons::ImagePlus, "Picture")
                    .tooltip("Put a picture from a file on the slide")
                    .disabled(!has_talk)
                    .on_click(|_, window, cx| window.dispatch_action(InsertPicture.boxed_clone(), cx))
                    .into_any_element(),
            ],
            cx,
        ),
        group(
            "Video",
            vec![
                tool(INSERT_VIDEO_ID, Icons::Video, "Video")
                    .tooltip("Put a video from a file on the slide")
                    .disabled(!has_talk)
                    .on_click(|_, window, cx| window.dispatch_action(InsertVideo.boxed_clone(), cx))
                    .into_any_element(),
            ],
            cx,
        ),
        group(
            "Shapes",
            vec![
                shapes.into_any_element(),
                tool(INSERT_TEXT_BOX_ID, Icons::TextCursorInput, "Text box")
                    .tooltip("Put a box to type in on the slide")
                    .disabled(!has_talk)
                    .on_click(|_, window, cx| window.dispatch_action(InsertTextBox.boxed_clone(), cx))
                    .into_any_element(),
            ],
            cx,
        ),
        group("Icons", vec![icon_tool(has_talk, icon_picker, cx)], cx),
        group("Tables", vec![table_tool(has_talk, table_grid, cx)], cx),
        group("Components", vec![component_tool(has_talk, component_list, cx)], cx),
    ]
}

/// The Component tool, which opens `list` under it.
fn component_tool(has_talk: bool, list: &Entity<ComponentList>, cx: &App) -> AnyElement {
    let button = tool(INSERT_COMPONENT_ID, Icons::Blocks, "Component").tooltip("Put a chart or one of the talk's components on the slide");
    if !has_talk {
        return button.disabled(true).into_any_element();
    }
    let (opened, shown) = (list.clone(), list.clone());
    Popover::new("insert-component-list")
        .open(list.read(cx).is_open())
        .on_open_change(move |open, _, cx| opened.update(cx, |list, cx| list.set_open(*open, cx)))
        .trigger(button)
        .content(move |_, _, _| shown.clone())
        .into_any_element()
}

/// The Table tool, which opens `grid` under it.
fn table_tool(has_talk: bool, grid: &Entity<TableGrid>, cx: &App) -> AnyElement {
    let button = tool(INSERT_TABLE_ID, Icons::Table, "Table").tooltip("Put a table on the slide");
    if !has_talk {
        return button.disabled(true).into_any_element();
    }
    let (opened, shown) = (grid.clone(), grid.clone());
    Popover::new("insert-table-grid")
        .open(grid.read(cx).is_open())
        .on_open_change(move |open, _, cx| opened.update(cx, |grid, cx| grid.set_open(*open, cx)))
        .trigger(button)
        .content(move |_, _, _| shown.clone())
        .into_any_element()
}

/// The Icon tool, which opens `picker` under it.
fn icon_tool(has_talk: bool, picker: &Entity<IconPicker>, cx: &App) -> AnyElement {
    let button = tool(INSERT_ICON_ID, Icons::Star, "Icon").tooltip("Put an icon on the slide");
    if !has_talk {
        return button.disabled(true).into_any_element();
    }
    let focus = picker.read(cx).search_focus(cx);
    let (opened, shown) = (picker.clone(), picker.clone());
    Popover::new("insert-icon-picker")
        .open(picker.read(cx).is_open())
        .on_open_change(move |open, window, cx| opened.update(cx, |picker, cx| picker.set_open(*open, window, cx)))
        .track_focus(&focus)
        .trigger(button)
        .content(move |_, _, _| shown.clone())
        .into_any_element()
}

/// A tool: its icon, then its name.
fn tool(id: &'static str, icon: Icons, label: &'static str) -> Button {
    Button::new(id).ghost().small().icon(icon).label(label)
}
