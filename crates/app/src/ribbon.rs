//! The tools across the top of the window, on tabs, each a row of groups with a caption under
//! them, as an office app has them.

use gpui_kit::assets::IconName as Icons;
use gpui_kit::component::button::{Button, ButtonVariants as _, DropdownButton};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Selectable as _, Sizable as _, h_flex,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::clipboard_tools::ClipboardTools;
use crate::document::Document;
use crate::icon_picker::IconPicker;
use crate::palette::ComponentList;
use crate::table_tools::{self, TableGrid};
use crate::text_tools::TextTools;
use crate::workspace_view::{
    AddSlide, DeleteSlide, DuplicateSlide, KEY_CONTEXT, NewSlide, Present, Redo, Rehearse, Save,
    ToggleHidden, ToggleMarkdown, ToggleNotes, ToggleSlideList, Undo,
};

pub const RIBBON_ID: &str = "ribbon";
pub const RIBBON_TABS_ID: &str = "ribbon-tabs";
pub const TAB_HOME_ID: &str = "tab-home";
pub const TAB_INSERT_ID: &str = "tab-insert";
pub const TAB_ANIMATE_ID: &str = "tab-animate";
pub const TAB_SLIDE_SHOW_ID: &str = "tab-slide-show";
pub const TAB_VIEW_ID: &str = "tab-view";
pub const SAVE_ID: &str = "save";
pub const NEW_SLIDE_ID: &str = "new-slide";
/// The New slide button and the arrow beside it, which lists the layouts.
pub const NEW_SLIDE_LAYOUTS_ID: &str = "new-slide-layouts";
pub const DUPLICATE_ID: &str = "duplicate-slide";
pub const DELETE_ID: &str = "delete-slide";
pub const HIDE_ID: &str = "hide-slide";
pub const UNDO_ID: &str = "undo";
pub const REDO_ID: &str = "redo";
pub const PRESENT_ID: &str = "present";
pub const REHEARSE_ID: &str = "rehearse";
pub const NOTES_TOGGLE_ID: &str = "toggle-notes";
pub const SLIDE_LIST_TOGGLE_ID: &str = "toggle-slide-list";
pub const MARKDOWN_TOGGLE_ID: &str = "toggle-markdown";
pub const ON_THE_WAY_ID: &str = "on-the-way";
/// The row of tools under the tabs, which scrolls sideways when it's wider than the window.
pub const RIBBON_TOOLS_ID: &str = "ribbon-tools";
/// The arrows at the row's ends that show there are more tools that way, and scroll to them.
pub const MORE_LEFT_ID: &str = "ribbon-more-left";
pub const MORE_RIGHT_ID: &str = "ribbon-more-right";

/// How wide the arrows at the row's cut ends are.
const MORE_WIDTH: Pixels = px(28.);

/// What a tab whose tools aren't made yet says.
pub const ON_THE_WAY: &str = "Tools for this tab are on the way.";

/// The layouts the New slide list offers, by the names people know them by, and the engine's
/// names for them.
pub const LAYOUTS: [(&str, &str); 5] = [
    ("Title", "title"),
    ("Title and content", "content"),
    ("Section", "section"),
    ("Two columns", "two-columns"),
    ("Blank", "blank"),
];

/// The height of the row of tools under the tabs, the same on every tab.
const TOOLS_HEIGHT: Pixels = px(64.);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RibbonTab {
    #[default]
    Home,
    Insert,
    Animate,
    SlideShow,
    View,
}

impl RibbonTab {
    const ALL: [RibbonTab; 5] = [
        RibbonTab::Home,
        RibbonTab::Insert,
        RibbonTab::Animate,
        RibbonTab::SlideShow,
        RibbonTab::View,
    ];

    fn name(self) -> &'static str {
        match self {
            RibbonTab::Home => "Home",
            RibbonTab::Insert => "Insert",
            RibbonTab::Animate => "Animate",
            RibbonTab::SlideShow => "Slide Show",
            RibbonTab::View => "View",
        }
    }

    fn id(self) -> &'static str {
        match self {
            RibbonTab::Home => TAB_HOME_ID,
            RibbonTab::Insert => TAB_INSERT_ID,
            RibbonTab::Animate => TAB_ANIMATE_ID,
            RibbonTab::SlideShow => TAB_SLIDE_SHOW_ID,
            RibbonTab::View => TAB_VIEW_ID,
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0)
    }
}

/// Which of the window's panels are showing, as the View tab shows them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Panels {
    pub notes: bool,
    pub slide_list: bool,
    pub markdown: bool,
}

pub struct Ribbon {
    document: Option<Entity<Document>>,
    /// Paste, Cut and Copy, first on the Home tab.
    clipboard_tools: Entity<ClipboardTools>,
    /// The tools for text being typed, on the Home tab.
    text_tools: Entity<TextTools>,
    /// What the Insert tab's Icon tool opens.
    icon_picker: Entity<IconPicker>,
    /// What the Insert tab's Table tool opens.
    table_grid: Entity<TableGrid>,
    /// What the Insert tab's Component tool opens.
    component_list: Entity<ComponentList>,
    tab: RibbonTab,
    panels: Panels,
    /// How far the row of tools is scrolled.
    scroll: ScrollHandle,
    /// Whether the row goes on past its left end, and past its right, as last drawn.
    more: (bool, bool),
    _document_changed: Option<Subscription>,
    _picker_changed: Subscription,
    _grid_changed: Subscription,
    _list_changed: Subscription,
}

impl Ribbon {
    pub fn new(
        document: Option<Entity<Document>>,
        clipboard_tools: Entity<ClipboardTools>,
        text_tools: Entity<TextTools>,
        icon_picker: Entity<IconPicker>,
        panels: Panels,
        cx: &mut Context<Self>,
    ) -> Self {
        let document_changed = document
            .as_ref()
            .map(|document| cx.observe(document, |_, _, cx| cx.notify()));
        // The picker opens and closes in the tab's drawing.
        let picker_changed = cx.observe(&icon_picker, |_, _, cx| cx.notify());
        let table_grid = cx.new(|_| TableGrid::new());
        let grid_changed = cx.observe(&table_grid, |_, _, cx| cx.notify());
        let component_list = cx.new(|cx| ComponentList::new(document.clone(), cx));
        let list_changed = cx.observe(&component_list, |_, _, cx| cx.notify());
        Ribbon {
            document,
            clipboard_tools,
            text_tools,
            icon_picker,
            table_grid,
            component_list,
            tab: RibbonTab::default(),
            panels,
            scroll: ScrollHandle::new(),
            more: (false, false),
            _document_changed: document_changed,
            _picker_changed: picker_changed,
            _grid_changed: grid_changed,
            _list_changed: list_changed,
        }
    }

    pub fn text_tools(&self) -> &Entity<TextTools> {
        &self.text_tools
    }

    /// The picker the Insert tab's Icon tool opens.
    pub fn icon_picker(&self) -> &Entity<IconPicker> {
        &self.icon_picker
    }

    /// The grid of sizes the Insert tab's Table tool opens.
    pub fn table_grid(&self) -> &Entity<TableGrid> {
        &self.table_grid
    }

    /// The list of components the Insert tab's Component tool opens.
    pub fn component_list(&self) -> &Entity<ComponentList> {
        &self.component_list
    }

    pub fn tab(&self) -> RibbonTab {
        self.tab
    }

    /// Shows which panels are showing.
    pub fn set_panels(&mut self, panels: Panels, cx: &mut Context<Self>) {
        if self.panels != panels {
            self.panels = panels;
            cx.notify();
        }
    }

    fn tab_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = self.tab;
        TabBar::new(RIBBON_TABS_ID)
            .underline()
            .small()
            .px_2()
            .selected_index(selected.index())
            .on_click(cx.listener(|this, index: &usize, _, cx| {
                if let Some(tab) = RibbonTab::ALL.get(*index) {
                    this.tab = *tab;
                    this.scroll.set_offset(Point::default());
                    cx.notify();
                }
            }))
            .children(RibbonTab::ALL.map(|tab| {
                let name = tab.name();
                Tab::new().aria_label(name).child(
                    div()
                        .id(tab.id())
                        .test_support()
                        .aria_label(name)
                        .aria_selected(tab == selected)
                        .child(name),
                )
            }))
    }

    fn home(&self, cx: &App) -> Vec<AnyElement> {
        let document = self.document.as_ref().map(|document| document.read(cx));
        let has_talk = document.is_some();
        let several = document.is_some_and(|document| document.slide_count() > 1);
        let hidden = document.is_some_and(|document| document.is_hidden(document.current()));
        let unsaved = document.is_some_and(Document::has_unsaved_changes);
        let undo = document.and_then(Document::undo_label);
        let redo = document.and_then(Document::redo_label);
        // While the caret is in a cell, or a table is selected, its tools come last.
        let table = document.and_then(Document::table_here);
        let new_slide = DropdownButton::new(NEW_SLIDE_LAYOUTS_ID)
            .button(
                tool(NEW_SLIDE_ID, Icons::SquarePlus, "New slide")
                    .tooltip_with_action("Add a slide after this one", &NewSlide, Some(KEY_CONTEXT))
                    .on_click(|_, window, cx| window.dispatch_action(NewSlide.boxed_clone(), cx)),
            )
            .ghost()
            .small()
            .disabled(!has_talk)
            .menu_accessibility_label("Choose a layout")
            .dropdown_menu(|menu, _, _| {
                LAYOUTS.iter().fold(menu, |menu, &(name, layout)| {
                    menu.menu(name, Box::new(AddSlide { layout }))
                })
            });
        // "Slide" in both, so that Show isn't read as starting the show.
        let (hide_label, hide_icon) = if hidden {
            ("Show slide", IconName::Eye)
        } else {
            ("Hide slide", IconName::EyeOff)
        };
        let hide_tip = if hidden {
            "Put this slide back in the show"
        } else {
            "Leave this slide out of the show"
        };
        vec![
            self.clipboard_tools.clone().into_any_element(),
            group(
                "File",
                vec![
                    tool(SAVE_ID, Icons::Save, "Save")
                        .tooltip_with_action("Save the talk", &Save, Some(KEY_CONTEXT))
                        .disabled(!unsaved)
                        .on_click(dispatch(Save))
                        .into_any_element(),
                ],
                cx,
            ),
            group(
                "Slides",
                vec![
                    new_slide.into_any_element(),
                    tool(DUPLICATE_ID, Icons::CopyPlus, "Duplicate")
                        .tooltip_with_action(
                            "Put a copy of this slide after it",
                            &DuplicateSlide,
                            Some(KEY_CONTEXT),
                        )
                        .disabled(!has_talk)
                        .on_click(dispatch(DuplicateSlide))
                        .into_any_element(),
                    tool(DELETE_ID, Icons::Trash, "Delete")
                        .tooltip("Delete this slide")
                        .disabled(!several)
                        .on_click(dispatch(DeleteSlide))
                        .into_any_element(),
                    tool(HIDE_ID, hide_icon, hide_label)
                        .tooltip(hide_tip)
                        .disabled(!has_talk)
                        .on_click(dispatch(ToggleHidden))
                        .into_any_element(),
                ],
                cx,
            ),
            self.text_tools.clone().into_any_element(),
            group(
                "History",
                vec![
                    tool(UNDO_ID, IconName::Undo2, "Undo")
                        .tooltip_with_action(
                            undo.map_or_else(|| "Undo".to_string(), |step| format!("Undo {step}")),
                            &Undo,
                            Some(KEY_CONTEXT),
                        )
                        .disabled(undo.is_none())
                        .on_click(dispatch(Undo))
                        .into_any_element(),
                    tool(REDO_ID, IconName::Redo2, "Redo")
                        .tooltip_with_action(
                            redo.map_or_else(|| "Redo".to_string(), |step| format!("Redo {step}")),
                            &Redo,
                            Some(KEY_CONTEXT),
                        )
                        .disabled(redo.is_none())
                        .on_click(dispatch(Redo))
                        .into_any_element(),
                ],
                cx,
            ),
        ]
        .into_iter()
        .chain(table.map(|table| table_tools::ribbon_group(&table, cx)))
        .collect()
    }

    fn slide_show(&self, cx: &App) -> Vec<AnyElement> {
        let no_talk = self.document.is_none();
        vec![group(
            "Start",
            vec![
                tool(PRESENT_ID, Icons::Presentation, "From the start")
                    .tooltip_with_action(
                        "Show the talk from its first slide",
                        &Present,
                        Some(KEY_CONTEXT),
                    )
                    .disabled(no_talk)
                    .on_click(dispatch(Present))
                    .into_any_element(),
                tool(REHEARSE_ID, Icons::Timer, "Rehearse")
                    .tooltip_with_action(
                        "Run through the talk in a window, with the clock",
                        &Rehearse,
                        Some(KEY_CONTEXT),
                    )
                    .disabled(no_talk)
                    .on_click(dispatch(Rehearse))
                    .into_any_element(),
            ],
            cx,
        )]
    }

    fn view(&self, cx: &App) -> Vec<AnyElement> {
        let Panels {
            notes,
            slide_list,
            markdown,
        } = self.panels;
        let toggle = |id: &'static str, icon: Icons, label: &'static str, on: bool| {
            tool(id, icon, label).selected(on).toggled(on)
        };
        vec![group(
            "Show",
            vec![
                toggle(
                    SLIDE_LIST_TOGGLE_ID,
                    Icons::PanelLeft,
                    "Slide list",
                    slide_list,
                )
                .tooltip("Show or hide the list of slides")
                .on_click(dispatch(ToggleSlideList))
                .into_any_element(),
                toggle(NOTES_TOGGLE_ID, Icons::PanelBottom, "Notes", notes)
                    .tooltip("Show or hide the notes under the slide")
                    .on_click(dispatch(ToggleNotes))
                    .into_any_element(),
                toggle(MARKDOWN_TOGGLE_ID, Icons::CodeXml, "Markdown", markdown)
                    .tooltip_with_action(
                        "Show or hide the talk's Markdown",
                        &ToggleMarkdown,
                        Some(KEY_CONTEXT),
                    )
                    .on_click(dispatch(ToggleMarkdown))
                    .into_any_element(),
            ],
            cx,
        )]
    }

    fn on_the_way(cx: &App) -> Vec<AnyElement> {
        vec![
            div()
                .id(ON_THE_WAY_ID)
                .test_support()
                .aria_label(ON_THE_WAY)
                .px_3()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(ON_THE_WAY)
                .into_any_element(),
        ]
    }
}

/// Whether the row of tools `scroll` scrolls goes on past its left end, and past its right.
fn more_of(scroll: &ScrollHandle) -> (bool, bool) {
    let (offset, max) = (-scroll.offset().x, scroll.max_offset().x);
    (offset > px(0.5), offset < max - px(0.5))
}

impl Ribbon {
    /// The arrow at the row's left end, or its right with `right`, over a fade into the
    /// ribbon's background: there are more tools that way, and a click scrolls to them.
    fn more_arrow(&self, right: bool, cx: &mut Context<Self>) -> AnyElement {
        let background = cx.theme().background;
        let (id, icon, label, from, to) = if right {
            (MORE_RIGHT_ID, IconName::ChevronRight, "More tools to the right", background.opacity(0.), background)
        } else {
            (MORE_LEFT_ID, IconName::ChevronLeft, "More tools to the left", background, background.opacity(0.))
        };
        div()
            .id(id)
            .test_support()
            .aria_label(label)
            .absolute()
            .top_0()
            .bottom_0()
            .when(right, |this| this.right_0())
            .when(!right, |this| this.left_0())
            .w(MORE_WIDTH)
            .flex()
            .items_center()
            .justify_center()
            .bg(linear_gradient(90., linear_color_stop(from, 0.), linear_color_stop(to, 0.6)))
            .text_color(cx.theme().muted_foreground)
            .child(Icon::new(icon).small())
            // Over the tools it covers: a click or a hover is the arrow's alone.
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_click(cx.listener(move |this, _, _, cx| {
                // Most of the row's width at a time.
                let page = (this.scroll.bounds().size.width - MORE_WIDTH * 2.).max(px(40.));
                let offset = this.scroll.offset();
                let max = this.scroll.max_offset().x;
                let x = if right { (offset.x - page).max(-max) } else { (offset.x + page).min(px(0.)) };
                this.scroll.set_offset(point(x, offset.y));
                cx.notify();
            }))
            .into_any_element()
    }
}

/// A tool: its icon, then its name.
fn tool(id: &'static str, icon: impl Into<Icon>, label: &'static str) -> Button {
    Button::new(id)
        .ghost()
        .small()
        .icon(icon.into())
        .label(label)
}

/// What a click on a tool does: what its key does.
fn dispatch(action: impl Action) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    move |_, window, cx| window.dispatch_action(action.boxed_clone(), cx)
}

/// A group of tools with its caption under them.
pub(crate) fn group(caption: &'static str, tools: Vec<AnyElement>, cx: &App) -> AnyElement {
    v_flex()
        .h_full()
        .px_2()
        .pt_1()
        .child(h_flex().flex_1().items_center().gap_1().children(tools))
        .child(
            div()
                .id(SharedString::from(format!("group-{caption}")))
                .test_support()
                .aria_label(caption)
                .w_full()
                .pb_1()
                .text_center()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(caption),
        )
        .into_any_element()
}

/// The line between two groups.
pub(crate) fn divider(cx: &App) -> AnyElement {
    div()
        .flex_none()
        .w(px(1.))
        .h(TOOLS_HEIGHT - px(16.))
        .bg(cx.theme().border)
        .into_any_element()
}

impl Render for Ribbon {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // What the picker no longer shows, once it's closed, goes from the window.
        self.icon_picker.update(cx, |picker, _| picker.release(window));
        if self.tab != RibbonTab::Insert {
            self.icon_picker.update(cx, |picker, cx| picker.close(cx));
            self.table_grid.update(cx, |grid, cx| grid.set_open(false, cx));
            self.component_list.update(cx, |list, cx| list.set_open(false, cx));
        }
        let groups = match self.tab {
            RibbonTab::Home => self.home(cx),
            RibbonTab::SlideShow => self.slide_show(cx),
            RibbonTab::View => self.view(cx),
            RibbonTab::Insert => {
                crate::insert_tools::insert_tab(self.document.is_some(), &self.icon_picker, &self.table_grid, &self.component_list, cx)
            }
            RibbonTab::Animate => Self::on_the_way(cx),
        };
        let mut tools = Vec::with_capacity(groups.len() * 2);
        for (index, group) in groups.into_iter().enumerate() {
            if index > 0 {
                tools.push(divider(cx));
            }
            // A group keeps its width: the row scrolls instead.
            tools.push(div().flex_none().h_full().child(group).into_any_element());
        }
        let (more_left, more_right) = self.more;
        // Whether the row goes on past its ends is known once it's laid out: if that's not
        // what this drawing shows, it's drawn again, once.
        let scroll = self.scroll.clone();
        let ribbon = cx.entity().downgrade();
        let shown = self.more;
        let measure = canvas(
            |_, _, _| {},
            move |_, _, _, cx| {
                if more_of(&scroll) != shown {
                    let ribbon = ribbon.clone();
                    cx.defer(move |cx| {
                        ribbon
                            .update(cx, |ribbon, cx| {
                                let more = more_of(&ribbon.scroll);
                                if more != ribbon.more {
                                    ribbon.more = more;
                                    cx.notify();
                                }
                            })
                            .ok();
                    });
                }
            },
        )
        .absolute()
        .size_0();
        v_flex()
            .id(RIBBON_ID)
            .test_support()
            .w_full()
            .flex_none()
            .bg(cx.theme().background)
            .border_b_1()
            .border_color(cx.theme().border)
            .child(self.tab_bar(cx))
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(TOOLS_HEIGHT)
                    .child(
                        h_flex()
                            .id(RIBBON_TOOLS_ID)
                            .test_support()
                            .size_full()
                            .px_1()
                            .items_center()
                            .overflow_x_scroll()
                            .track_scroll(&self.scroll)
                            .children(tools),
                    )
                    .when(more_left, |this| this.child(self.more_arrow(false, cx)))
                    .when(more_right, |this| this.child(self.more_arrow(true, cx)))
                    .child(measure),
            )
    }
}
