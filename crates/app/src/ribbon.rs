//! The tools across the top of the window, on tabs, each a row of groups with a caption under
//! them, as an office app has them.

use gpui_kit::assets::IconName as Icons;
use gpui_kit::component::button::{Button, ButtonVariants as _, DropdownButton};
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Selectable as _, Sizable as _, h_flex,
    v_flex,
};
use gpui_kit::*;

use crate::document::Document;
use crate::workspace_view::{
    AddSlide, DeleteSlide, DuplicateSlide, KEY_CONTEXT, NewSlide, Present, Redo, Rehearse,
    ToggleHidden, ToggleMarkdown, ToggleNotes, ToggleSlideList, Undo,
};

pub const RIBBON_ID: &str = "ribbon";
pub const RIBBON_TABS_ID: &str = "ribbon-tabs";
pub const TAB_HOME_ID: &str = "tab-home";
pub const TAB_INSERT_ID: &str = "tab-insert";
pub const TAB_ANIMATE_ID: &str = "tab-animate";
pub const TAB_SLIDE_SHOW_ID: &str = "tab-slide-show";
pub const TAB_VIEW_ID: &str = "tab-view";
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
    tab: RibbonTab,
    panels: Panels,
    _document_changed: Option<Subscription>,
}

impl Ribbon {
    pub fn new(document: Option<Entity<Document>>, panels: Panels, cx: &mut Context<Self>) -> Self {
        let document_changed = document
            .as_ref()
            .map(|document| cx.observe(document, |_, _, cx| cx.notify()));
        Ribbon {
            document,
            tab: RibbonTab::default(),
            panels,
            _document_changed: document_changed,
        }
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
        let undo = document.and_then(Document::undo_label);
        let redo = document.and_then(Document::redo_label);
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
fn group(caption: &'static str, tools: Vec<AnyElement>, cx: &App) -> AnyElement {
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
fn divider(cx: &App) -> AnyElement {
    div()
        .flex_none()
        .w(px(1.))
        .h(TOOLS_HEIGHT - px(16.))
        .bg(cx.theme().border)
        .into_any_element()
}

impl Render for Ribbon {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let groups = match self.tab {
            RibbonTab::Home => self.home(cx),
            RibbonTab::SlideShow => self.slide_show(cx),
            RibbonTab::View => self.view(cx),
            RibbonTab::Insert | RibbonTab::Animate => Self::on_the_way(cx),
        };
        let mut tools = Vec::with_capacity(groups.len() * 2);
        for (index, group) in groups.into_iter().enumerate() {
            if index > 0 {
                tools.push(divider(cx));
            }
            tools.push(group);
        }
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
                h_flex()
                    .w_full()
                    .h(TOOLS_HEIGHT)
                    .px_1()
                    .items_center()
                    .children(tools),
            )
    }
}
