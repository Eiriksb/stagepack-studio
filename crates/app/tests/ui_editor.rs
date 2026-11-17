//! The editor's window: the tool tabs across the top, the list of slides on the left and the
//! notes under the slide.

use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::test::{TestQueryExt, TestWindowExt};
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Context, Entity, InputEvent as _, Modifiers,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, RenderImage,
    TestAppContext, Window, WindowBounds, WindowOptions, point, px, size,
};
use slides_engine::{Fonts, pack_folder};
use slides_format::is_hidden;
use stagepack_studio::document::Document;
use stagepack_studio::insert_tools::INSERT_PICTURE_ID;
use stagepack_studio::notes_panel::{NOTES_CAPTION_ID, NOTES_ID, NOTES_PLACEHOLDER};
use stagepack_studio::ribbon::{
    DELETE_ID, DUPLICATE_ID, HIDE_ID, MARKDOWN_TOGGLE_ID, NEW_SLIDE_ID, NEW_SLIDE_LAYOUTS_ID,
    MORE_RIGHT_ID, NOTES_TOGGLE_ID, ON_THE_WAY_ID, PRESENT_ID, REDO_ID, REHEARSE_ID, RIBBON_ID,
    SLIDE_LIST_TOGGLE_ID, TAB_ANIMATE_ID, TAB_HOME_ID, TAB_INSERT_ID, TAB_SLIDE_SHOW_ID,
    TAB_VIEW_ID, UNDO_ID,
};
use stagepack_studio::slide_strip::{
    DRAG_PICTURE_ID, DROP_LINE_ID, STRIP_ID, strip_number_id, strip_row_id,
};
use stagepack_studio::workspace_view::{self, CANVAS_ID, MARKDOWN_ID, TITLE_ID, WorkspaceView};

const THREE: &str = "---\ntitle: T\n---\n\n# One\n\n---\n\n# Two\n\n---\n\n# Three\n";
const TWO: &str = "---\ntitle: T\n---\n\n# One\n\n---\n\n# Two\n";

fn open_view(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut Window, &mut Context<WorkspaceView>) -> WorkspaceView + 'static,
) -> (AnyWindowHandle, Entity<WorkspaceView>) {
    let opened = cx.update(|cx: &mut App| {
        gpui_kit::init(cx);
        workspace_view::init(cx);
        let bounds = Bounds {
            origin: Point::default(),
            size: size(px(1280.), px(800.)),
        };
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            cx,
            |window, cx| cx.new(|cx| build(window, cx)),
        )
        .expect("open test window")
    });
    settle(cx, opened.0);
    opened
}

fn open(cx: &mut TestAppContext, source: &str) -> (AnyWindowHandle, Entity<WorkspaceView>) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), source).unwrap();
    let talk = Document::open(dir.path(), Fonts::bundled_only()).unwrap();
    open_view(cx, |window, cx| WorkspaceView::open_with(talk, window, cx))
}

/// Draws until what one frame asked for has reached the screen.
fn settle(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    for _ in 0..3 {
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        cx.run_until_parked();
    }
}

fn click(cx: &mut TestAppContext, handle: AnyWindowHandle, id: impl Into<gpui_kit::ElementId>) {
    let id = id.into();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    })
    .unwrap();
    settle(cx, handle);
}

/// Scrolls the row of tools by its right arrow until `id` is clear of the arrow, as a person
/// does in a window too narrow for the Home tab: at 1,280 pixels its History group is past it.
fn reach(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) {
    for _ in 0..20 {
        let clear = with_window(cx, handle, |window, _| {
            let arrow = window.try_find(MORE_RIGHT_ID).map(|arrow| arrow.bounds().left());
            let right = arrow.unwrap_or_else(|| window.find(RIBBON_ID).bounds().right());
            window.find(id).bounds().right() <= right
        });
        if clear {
            return;
        }
        click(cx, handle, MORE_RIGHT_ID);
    }
    panic!("{id} can't be reached");
}

/// Clicks the tool `id`, scrolling the row of tools to it first.
fn click_tool(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) {
    reach(cx, handle, id);
    click(cx, handle, id);
}

fn keys(cx: &mut TestAppContext, handle: AnyWindowHandle, keystrokes: &str) {
    cx.simulate_keystrokes(handle, keystrokes);
    settle(cx, handle);
}

/// Picks `row` of the New slide button's list of layouts.
fn pick_layout(cx: &mut TestAppContext, handle: AnyWindowHandle, row: usize) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.within(NEW_SLIDE_LAYOUTS_ID).click("popup", cx);
        window.render_frame(cx);
        window.within("popup-menu").click(row, cx);
    })
    .unwrap();
    settle(cx, handle);
}

fn document(cx: &TestAppContext, view: &Entity<WorkspaceView>) -> Entity<Document> {
    cx.read(|cx| view.read(cx).document().unwrap().clone())
}

fn count(cx: &TestAppContext, view: &Entity<WorkspaceView>) -> usize {
    document(cx, view).read_with(cx, |document, _| document.slide_count())
}

fn current(cx: &TestAppContext, view: &Entity<WorkspaceView>) -> usize {
    cx.read(|cx| view.read(cx).document().unwrap().read(cx).current())
}

fn source(cx: &TestAppContext, view: &Entity<WorkspaceView>) -> String {
    cx.read(|cx| {
        view.read(cx)
            .document()
            .unwrap()
            .read(cx)
            .source()
            .to_string()
    })
}

fn notes_of(cx: &TestAppContext, view: &Entity<WorkspaceView>, index: usize) -> Option<String> {
    cx.read(|cx| view.read(cx).document().unwrap().read(cx).notes_of(index))
}

fn layout_of(cx: &TestAppContext, view: &Entity<WorkspaceView>, index: usize) -> Option<String> {
    cx.read(|cx| {
        let document = view.read(cx).document().unwrap().read(cx);
        let settings = document.talk().slides[index].settings.as_ref()?;
        settings.get("layout").map(str::to_string)
    })
}

fn hidden(cx: &TestAppContext, view: &Entity<WorkspaceView>, index: usize) -> bool {
    cx.read(|cx| is_hidden(&view.read(cx).document().unwrap().read(cx).talk().slides[index]))
}

fn notes_text(cx: &TestAppContext, view: &Entity<WorkspaceView>) -> String {
    cx.read(|cx| view.read(cx).notes_panel().read(cx).text(cx))
}

fn thumbnail(
    cx: &TestAppContext,
    view: &Entity<WorkspaceView>,
    index: usize,
) -> Option<Arc<RenderImage>> {
    cx.read(|cx| view.read(cx).slide_strip().read(cx).thumbnail(index, cx))
}

fn with_window<R>(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    f: impl FnOnce(&mut Window, &mut App) -> R,
) -> R {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        f(window, cx)
    })
    .unwrap()
}

/// Whether the control `id` says it's disabled. One that isn't says nothing.
fn disabled(window: &Window, id: &'static str) -> bool {
    window.find(id).disabled() == Some(true)
}

fn title(cx: &mut TestAppContext, handle: AnyWindowHandle) -> String {
    with_window(cx, handle, |window, _| {
        window.find(TITLE_ID).label().unwrap().to_string()
    })
}

const TABS: [&str; 5] = [
    TAB_HOME_ID,
    TAB_INSERT_ID,
    TAB_ANIMATE_ID,
    TAB_SLIDE_SHOW_ID,
    TAB_VIEW_ID,
];

#[gpui_kit::test]
fn the_window_has_its_tabs_and_the_home_tab_is_open(cx: &mut TestAppContext) {
    let (handle, _) = open(cx, THREE);
    with_window(cx, handle, |window, _| {
        let ribbon = window.find(RIBBON_ID);
        let title = window.find(TITLE_ID);
        let canvas = window.find(CANVAS_ID);
        assert!(ribbon.visible());
        assert!(
            title.bounds().bottom() <= ribbon.bounds().top(),
            "under the title bar"
        );
        assert!(
            ribbon.bounds().bottom() <= canvas.bounds().top(),
            "over the slide"
        );
        let names: Vec<_> = TABS
            .iter()
            .map(|id| {
                let tab = window.find(*id);
                assert!(tab.visible(), "{tab:?}");
                (tab.label().unwrap().to_string(), tab.selected())
            })
            .collect();
        assert_eq!(
            names,
            [
                ("Home".to_string(), Some(true)),
                ("Insert".to_string(), Some(false)),
                ("Animate".to_string(), Some(false)),
                ("Slide Show".to_string(), Some(false)),
                ("View".to_string(), Some(false)),
            ]
        );
        let tabs: Vec<_> = TABS.iter().map(|id| window.find(*id).bounds()).collect();
        assert!(
            tabs.windows(2)
                .all(|pair| pair[0].right() <= pair[1].left()),
            "in a row"
        );
        // At 1,280 pixels the History group is past the right arrow, to be scrolled to.
        for id in [NEW_SLIDE_ID, DUPLICATE_ID, DELETE_ID, HIDE_ID] {
            assert!(window.find(id).visible(), "{id}");
        }
        assert!(window.find(MORE_RIGHT_ID).visible(), "an arrow says there's more");
        assert_eq!(window.find(NEW_SLIDE_ID).label(), Some("New slide"));
        // Each says it acts on the slide, as a thing on it can show as selected.
        assert_eq!(window.find(DUPLICATE_ID).label(), Some("Duplicate slide"));
        assert_eq!(window.find(DELETE_ID).label(), Some("Delete slide"));
        assert_eq!(window.find(HIDE_ID).label(), Some("Hide slide"));
        assert!(
            window.find_by_label("Slides").visible(),
            "the group's caption"
        );
        assert!(
            window.find_by_label("History").visible(),
            "the group's caption"
        );
    });
    for id in [UNDO_ID, REDO_ID] {
        reach(cx, handle, id);
        with_window(cx, handle, |window, _| assert!(window.find(id).visible(), "{id}"));
    }
}

#[gpui_kit::test]
fn each_tab_shows_its_own_tools(cx: &mut TestAppContext) {
    let (handle, _) = open(cx, THREE);
    click(cx, handle, TAB_INSERT_ID);
    with_window(cx, handle, |window, _| {
        assert_eq!(window.find(INSERT_PICTURE_ID).label(), Some("Picture"));
        assert!(window.try_find(ON_THE_WAY_ID).is_none(), "the Insert tab has its tools");
        assert!(window.try_find(NEW_SLIDE_ID).is_none(), "Home's tools are put away");
        assert_eq!(window.find(TAB_INSERT_ID).selected(), Some(true));
    });
    click(cx, handle, TAB_ANIMATE_ID);
    with_window(cx, handle, |window, _| {
        assert!(window.try_find(ON_THE_WAY_ID).is_none(), "the Animate tab has its tools");
        assert_eq!(window.find(stagepack_studio::animate_tools::TRANSITION_ALL_ID).label(), Some("Apply to all slides"));
        assert!(
            window.try_find(NEW_SLIDE_ID).is_none(),
            "Home's tools are put away"
        );
        assert_eq!(window.find(TAB_ANIMATE_ID).selected(), Some(true));
    });
    click(cx, handle, TAB_SLIDE_SHOW_ID);
    with_window(cx, handle, |window, _| {
        assert_eq!(window.find(PRESENT_ID).label(), Some("From the start"));
        assert_eq!(window.find(REHEARSE_ID).label(), Some("Rehearse"));
        assert!(window.try_find(ON_THE_WAY_ID).is_none());
    });
    click(cx, handle, TAB_VIEW_ID);
    with_window(cx, handle, |window, _| {
        assert_eq!(window.find(NOTES_TOGGLE_ID).label(), Some("Notes"));
        assert_eq!(
            window.find(SLIDE_LIST_TOGGLE_ID).label(),
            Some("Slide list")
        );
        assert_eq!(window.find(MARKDOWN_TOGGLE_ID).label(), Some("Markdown"));
    });
    click(cx, handle, TAB_HOME_ID);
    with_window(cx, handle, |window, _| {
        assert!(window.find(NEW_SLIDE_ID).visible());
    });
}

#[gpui_kit::test]
fn the_title_bar_keeps_file_and_export_and_present_moves_to_the_slide_show_tab(
    cx: &mut TestAppContext,
) {
    let (handle, _) = open(cx, THREE);
    with_window(cx, handle, |window, _| {
        assert!(window.find(workspace_view::FILE_MENU_ID).visible());
        assert!(window.find(workspace_view::EXPORT_MENU_ID).visible());
        assert!(
            window.try_find(PRESENT_ID).is_none(),
            "not in the title bar"
        );
    });
}

#[gpui_kit::test]
fn the_strip_has_a_row_for_each_slide_and_marks_the_current_one(cx: &mut TestAppContext) {
    let (handle, _) = open(cx, THREE);
    with_window(cx, handle, |window, _| {
        let strip = window.find(STRIP_ID);
        let canvas = window.find(CANVAS_ID);
        assert!(strip.visible());
        assert!(
            strip.bounds().right() <= canvas.bounds().left(),
            "on the left"
        );
        let width = f32::from(strip.bounds().size.width);
        assert!((210.0..=221.0).contains(&width), "220 wide: {width}");
        let rows: Vec<_> = (0..3)
            .map(|index| window.find(strip_row_id(index)))
            .collect();
        assert!(rows.iter().all(|row| row.visible()));
        assert!(
            rows.windows(2)
                .all(|pair| pair[0].bounds().bottom() <= pair[1].bounds().top())
        );
        assert_eq!(rows[0].label(), Some("Slide 1"));
        let selected: Vec<_> = rows.iter().map(|row| row.selected()).collect();
        assert_eq!(selected, [Some(true), Some(false), Some(false)]);
        assert!(window.try_find(strip_row_id(3)).is_none());
    });
}

#[gpui_kit::test]
fn clicking_a_row_shows_that_slide(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, THREE);
    click(cx, handle, strip_row_id(2));
    assert_eq!(current(cx, &view), 2);
    assert_eq!(title(cx, handle), "T · 3 / 3");
    with_window(cx, handle, |window, _| {
        assert_eq!(window.find(strip_row_id(2)).selected(), Some(true));
        assert_eq!(window.find(strip_row_id(0)).selected(), Some(false));
    });
}

#[gpui_kit::test]
fn new_slide_adds_one_after_the_current_and_selects_it(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, TWO);
    click(cx, handle, NEW_SLIDE_ID);
    assert_eq!(count(cx, &view), 3);
    assert_eq!(current(cx, &view), 1);
    assert_eq!(
        layout_of(cx, &view, 1),
        None,
        "the content layout, the one a slide has unless it says"
    );
    assert!(
        source(cx, &view).ends_with("# Two\n"),
        "the new one is between"
    );
    assert_eq!(title(cx, handle), "• T · 2 / 3");
    with_window(cx, handle, |window, _| {
        assert_eq!(window.find(strip_row_id(1)).selected(), Some(true));
    });
}

#[gpui_kit::test]
fn the_layout_list_adds_a_slide_of_that_layout(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, TWO);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.within(NEW_SLIDE_LAYOUTS_ID).click("popup", cx);
        window.render_frame(cx);
        let menu = window.within("popup-menu");
        for name in [
            "Title",
            "Title and content",
            "Section",
            "Two columns",
            "Blank",
        ] {
            assert!(menu.find_by_label(name).visible(), "{name}");
        }
        window.press("escape", cx);
    })
    .unwrap();
    settle(cx, handle);
    let expected = [
        (0, Some("title")),
        (1, None),
        (2, Some("section")),
        (3, Some("two-columns")),
        (4, Some("blank")),
    ];
    for (row, layout) in expected {
        pick_layout(cx, handle, row);
        let at = current(cx, &view);
        assert_eq!(layout_of(cx, &view, at).as_deref(), layout, "row {row}");
    }
    assert_eq!(count(cx, &view), 7);
}

#[gpui_kit::test]
fn duplicate_delete_and_hide_work_from_the_ribbon(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, TWO);
    click(cx, handle, DUPLICATE_ID);
    assert_eq!(count(cx, &view), 3);
    assert_eq!(current(cx, &view), 1);
    let talk = source(cx, &view);
    assert_eq!(talk.matches("# One").count(), 2, "{talk}");

    click(cx, handle, HIDE_ID);
    assert!(hidden(cx, &view, 1));
    with_window(cx, handle, |window, _| {
        assert_eq!(
            window.find(HIDE_ID).label(),
            Some("Show slide"),
            "a hidden slide is shown again"
        );
        assert_eq!(
            window.find(strip_row_id(1)).label(),
            Some("Slide 2, hidden")
        );
    });
    click(cx, handle, HIDE_ID);
    assert!(!hidden(cx, &view, 1));
    with_window(cx, handle, |window, _| {
        assert_eq!(window.find(HIDE_ID).label(), Some("Hide slide"));
        assert_eq!(window.find(strip_row_id(1)).label(), Some("Slide 2"));
    });

    click(cx, handle, DELETE_ID);
    assert_eq!(count(cx, &view), 2);
    assert_eq!(source(cx, &view).matches("# One").count(), 1);
}

#[gpui_kit::test]
fn delete_is_disabled_for_the_only_slide(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, "# Only\n");
    with_window(cx, handle, |window, _| assert!(disabled(window, DELETE_ID)));
    click(cx, handle, DELETE_ID);
    assert_eq!(count(cx, &view), 1);
    click(cx, handle, NEW_SLIDE_ID);
    with_window(cx, handle, |window, _| {
        assert!(!disabled(window, DELETE_ID))
    });
}

#[gpui_kit::test]
fn undo_and_redo_are_disabled_until_there_is_something(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, TWO);
    let state = |cx: &mut TestAppContext| {
        with_window(cx, handle, |window, _| {
            (disabled(window, UNDO_ID), disabled(window, REDO_ID))
        })
    };
    assert_eq!(state(cx), (true, true));
    click(cx, handle, NEW_SLIDE_ID);
    assert_eq!(state(cx), (false, true));
    click_tool(cx, handle, UNDO_ID);
    assert_eq!(count(cx, &view), 2);
    assert_eq!(state(cx), (true, false));
    // In this window the row of tools runs past the right edge, History and all.
    click_tool(cx, handle, REDO_ID);
    assert_eq!(count(cx, &view), 3);
    assert_eq!(state(cx), (false, true));
}

#[gpui_kit::test]
fn undo_names_what_it_will_undo(cx: &mut TestAppContext) {
    let (handle, _) = open(cx, TWO);
    click(cx, handle, DELETE_ID);
    with_window(cx, handle, |window, _| {
        assert_eq!(
            window.find(UNDO_ID).description(),
            Some("Undo Delete slide")
        );
    });
    click_tool(cx, handle, UNDO_ID);
    with_window(cx, handle, |window, _| {
        assert_eq!(
            window.find(REDO_ID).description(),
            Some("Redo Delete slide")
        );
    });
}

#[gpui_kit::test]
fn the_keys_do_what_the_buttons_do(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, TWO);
    keys(cx, handle, "ctrl-m");
    assert_eq!((count(cx, &view), current(cx, &view)), (3, 1), "New slide");
    keys(cx, handle, "ctrl-d");
    assert_eq!((count(cx, &view), current(cx, &view)), (4, 2), "Duplicate");
    keys(cx, handle, "ctrl-z");
    assert_eq!(count(cx, &view), 3, "Undo");
    keys(cx, handle, "ctrl-shift-z");
    assert_eq!(count(cx, &view), 4, "Redo");
    keys(cx, handle, "ctrl-z");
    keys(cx, handle, "ctrl-y");
    assert_eq!(count(cx, &view), 4, "Redo, the other way");
    keys(cx, handle, "ctrl-shift-m");
    with_window(cx, handle, |window, _| {
        assert!(window.find(MARKDOWN_ID).visible(), "the Markdown");
    });

    // With the list of slides in hand.
    click(cx, handle, strip_row_id(0));
    keys(cx, handle, "down");
    assert_eq!(current(cx, &view), 1);
    keys(cx, handle, "up");
    assert_eq!(current(cx, &view), 0);
    let slide_says = |cx: &TestAppContext, index: usize| {
        cx.read(|cx| {
            let document = view.read(cx).document().unwrap().read(cx);
            let talk = document.talk();
            talk.source[talk.slides[index].range.clone()].contains("# One")
        })
    };
    assert!(slide_says(cx, 0));
    keys(cx, handle, "ctrl-down");
    assert_eq!(
        current(cx, &view),
        1,
        "the slide moved, and is still the one in hand"
    );
    assert!(slide_says(cx, 1) && !slide_says(cx, 0));
    keys(cx, handle, "ctrl-up");
    assert_eq!(current(cx, &view), 0);
    assert!(slide_says(cx, 0));
    keys(cx, handle, "delete");
    assert_eq!(count(cx, &view), 3);
    keys(cx, handle, "backspace");
    assert_eq!(count(cx, &view), 2);
}

#[gpui_kit::test]
fn typing_in_notes_changes_the_slides_notes_and_one_undo_takes_it_back(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, TWO);
    with_window(cx, handle, |window, _| {
        let notes = window.find(NOTES_ID);
        assert!(notes.visible());
        assert!(
            notes.bounds().top() >= window.find(CANVAS_ID).bounds().bottom(),
            "under the slide"
        );
        let height = f32::from(notes.bounds().size.height);
        assert!((150.0..=161.0).contains(&height), "160 tall: {height}");
    });
    click(cx, handle, NOTES_ID);
    with_window(cx, handle, |window, cx| window.input("Say hello", cx));
    settle(cx, handle);
    assert_eq!(notes_of(cx, &view, 0).as_deref(), Some("Say hello"));
    assert_eq!(notes_of(cx, &view, 1), None);
    with_window(cx, handle, |window, _| {
        assert_eq!(window.find(UNDO_ID).description(), Some("Undo Edit notes"));
    });

    click_tool(cx, handle, UNDO_ID);
    assert_eq!(
        notes_of(cx, &view, 0),
        None,
        "one undo takes all the typing back"
    );
    assert_eq!(source(cx, &view), TWO);
    assert_eq!(notes_text(cx, &view), "", "and the field shows it");
}

#[gpui_kit::test]
fn the_notes_caret_stops_blinking_when_left_alone(cx: &mut TestAppContext) {
    // A blinking caret draws the whole window twice a second, for as long as the field has
    // the keyboard, so it stops after a while, as GTK's does.
    let (handle, view) = open(cx, TWO);
    click(cx, handle, NOTES_ID);
    let field = cx.read(|cx| view.read(cx).notes_panel().read(cx).field().clone());
    let redraws = std::rc::Rc::new(std::cell::Cell::new(0));
    let counter = redraws.clone();
    let _watching = cx.update(|cx| cx.observe(&field, move |_, _| counter.set(counter.get() + 1)));
    let wait = |cx: &mut TestAppContext, seconds: u64| {
        for _ in 0..seconds * 4 {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(250));
            cx.run_until_parked();
        }
    };
    wait(cx, 2);
    assert!(redraws.get() > 0, "it blinks at first");
    wait(cx, 10);
    redraws.set(0);
    wait(cx, 10);
    assert_eq!(redraws.get(), 0, "then it stays still");
    with_window(cx, handle, |window, cx| window.input("a", cx));
    settle(cx, handle);
    redraws.set(0);
    wait(cx, 2);
    assert!(redraws.get() > 0, "and typing starts it again");
}

#[gpui_kit::test]
fn the_slide_has_an_edge_on_the_desk(cx: &mut TestAppContext) {
    // A white slide on the light grey desk barely shows where it ends.
    let (handle, view) = open(cx, TWO);
    let canvas = cx.read(|cx| view.read(cx).slide_canvas().clone());
    let edge = cx
        .read(|cx| canvas.read(cx).edge())
        .expect("the slide has an edge");
    with_window(cx, handle, |window, _| {
        let area = window.find(CANVAS_ID).bounds();
        let slide = stagepack_studio::slide_canvas::letterbox(area, window.scale_factor());
        assert_eq!(edge, slide.dilate(px(1.)), "just outside the slide");
    });
}

#[gpui_kit::test]
fn the_notes_field_says_what_it_is_for(cx: &mut TestAppContext) {
    let (handle, _) = open(cx, TWO);
    assert_eq!(NOTES_PLACEHOLDER, "Notes for this slide");
    with_window(cx, handle, |window, _| {
        let placeholders: Vec<_> = window
            .within(NOTES_ID)
            .elements()
            .into_iter()
            .filter_map(|element| element.placeholder().map(str::to_string))
            .collect();
        assert_eq!(placeholders, [NOTES_PLACEHOLDER]);
    });
}

#[gpui_kit::test]
fn arrow_keys_in_the_notes_field_do_not_change_slide(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, THREE);
    click(cx, handle, strip_row_id(1));
    click(cx, handle, NOTES_ID);
    for key in ["right", "left", "pagedown", "pageup"] {
        keys(cx, handle, key);
        assert_eq!(current(cx, &view), 1, "{key}");
    }
    // The slide has them again once the desk around it is clicked: a click on the slide itself
    // types there.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click_at(CANVAS_ID, point(px(1.), px(1.)), cx);
    })
    .unwrap();
    settle(cx, handle);
    keys(cx, handle, "right");
    assert_eq!(current(cx, &view), 2);
}

#[gpui_kit::test]
fn changing_slide_swaps_the_notes(cx: &mut TestAppContext) {
    let talk = "# One\n\n<Notes>\nFirst words\n</Notes>\n\n---\n\n# Two\n\n<Notes>\nSecond words\n</Notes>\n\n---\n\n# Three\n";
    let (handle, view) = open(cx, talk);
    assert_eq!(notes_text(cx, &view), "First words");
    keys(cx, handle, "right");
    assert_eq!(notes_text(cx, &view), "Second words");
    click(cx, handle, strip_row_id(2));
    assert_eq!(notes_text(cx, &view), "", "a slide with none");
    click(cx, handle, strip_row_id(0));
    assert_eq!(notes_text(cx, &view), "First words");
    assert_eq!(source(cx, &view), talk, "looking is no edit");
}

#[gpui_kit::test]
fn the_view_tab_hides_and_shows_each_panel(cx: &mut TestAppContext) {
    let (handle, _) = open(cx, TWO);
    click(cx, handle, TAB_VIEW_ID);
    let shown = |cx: &mut TestAppContext, id: &'static str| {
        with_window(cx, handle, |window, _| {
            window.try_find(id).is_some_and(|panel| panel.visible())
        })
    };
    let toggled = |cx: &mut TestAppContext, id: &'static str| {
        with_window(cx, handle, |window, _| window.find(id).checked())
    };
    for (toggle, panel, at_first) in [
        (NOTES_TOGGLE_ID, NOTES_ID, true),
        (SLIDE_LIST_TOGGLE_ID, STRIP_ID, true),
        (MARKDOWN_TOGGLE_ID, MARKDOWN_ID, false),
    ] {
        assert_eq!(shown(cx, panel), at_first, "{panel}");
        assert_eq!(toggled(cx, toggle), Some(at_first), "{toggle}");
        click(cx, handle, toggle);
        assert_eq!(shown(cx, panel), !at_first, "{panel}");
        assert_eq!(toggled(cx, toggle), Some(!at_first), "{toggle}");
        click(cx, handle, toggle);
        assert_eq!(shown(cx, panel), at_first, "{panel}");
    }
    // With both away, the slide has the room, but for the inspector's, which is always there.
    click(cx, handle, NOTES_TOGGLE_ID);
    click(cx, handle, SLIDE_LIST_TOGGLE_ID);
    with_window(cx, handle, |window, _| {
        let canvas = window.find(CANVAS_ID).bounds();
        let inspector = window.find(stagepack_studio::inspector::INSPECTOR_ID).bounds();
        assert!(canvas.left() < px(40.), "{canvas:?}");
        assert!(canvas.right() > inspector.left() - px(40.), "{canvas:?} {inspector:?}");
    });
}

#[gpui_kit::test]
fn a_thumbnail_follows_an_edit_to_its_slide(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, THREE);
    let (zero, one) = (thumbnail(cx, &view, 0), thumbnail(cx, &view, 1));
    let (zero, one) = (zero.expect("drawn"), one.expect("drawn"));
    with_window(cx, handle, |window, _| {
        assert!(window.has_image_atlas_entry(&zero), "it's on screen");
    });

    click(cx, handle, NOTES_ID);
    with_window(cx, handle, |window, cx| window.input("A", cx));
    settle(cx, handle);

    let edited = thumbnail(cx, &view, 0).expect("drawn again");
    assert!(
        !Arc::ptr_eq(&zero, &edited),
        "the edited slide's is drawn again"
    );
    assert!(
        Arc::ptr_eq(&one, &thumbnail(cx, &view, 1).unwrap()),
        "the others keep theirs"
    );
    with_window(cx, handle, |window, _| {
        assert!(
            !window.has_image_atlas_entry(&zero),
            "the old one was let go"
        );
        assert!(window.has_image_atlas_entry(&edited));
    });
}

#[gpui_kit::test]
fn a_talk_of_a_thousand_slides_opens_without_making_a_thousand_thumbnails(cx: &mut TestAppContext) {
    let talk = (0..1000)
        .map(|n| format!("# Slide {n}\n"))
        .collect::<Vec<_>>()
        .join("\n---\n\n");
    let (handle, view) = open(cx, &talk);
    let drawn = cx.read(|cx| view.read(cx).slide_strip().read(cx).thumbnails_drawn());
    assert!(drawn > 0 && drawn < 30, "{drawn} drawn");
    with_window(cx, handle, |window, _| {
        assert!(window.find(strip_row_id(0)).visible());
        assert!(
            window.try_find(strip_row_id(999)).is_none(),
            "rows off screen aren't made"
        );
    });
    assert_eq!(count(cx, &view), 1000);
}

#[gpui_kit::test]
fn a_packed_talk_can_be_edited_the_same_way(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("talk");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("slides.md"), TWO).unwrap();
    let file: PathBuf = dir.path().join("talk.sptx");
    pack_folder(&folder, &file, "test").unwrap();
    let talk = Document::open_packed(&file, Fonts::bundled_only()).unwrap();
    let (handle, view) = open_view(cx, move |window, cx| {
        WorkspaceView::open_with(talk, window, cx)
    });

    click(cx, handle, NEW_SLIDE_ID);
    assert_eq!(count(cx, &view), 3);
    click(cx, handle, NOTES_ID);
    with_window(cx, handle, |window, cx| window.input("Packed", cx));
    settle(cx, handle);
    assert_eq!(notes_of(cx, &view, 1).as_deref(), Some("Packed"));
    click(cx, handle, HIDE_ID);
    assert!(hidden(cx, &view, 1));
    assert!(title(cx, handle).starts_with(workspace_view::UNSAVED_MARK));
    click(cx, handle, CANVAS_ID);
    keys(cx, handle, "ctrl-z");
    keys(cx, handle, "ctrl-z");
    keys(cx, handle, "ctrl-z");
    assert_eq!(source(cx, &view), TWO);
}

/// The rows the list has on screen.
fn rows_on_screen(cx: &mut TestAppContext, handle: AnyWindowHandle, count: usize) -> usize {
    with_window(cx, handle, |window, _| {
        (0..count)
            .filter(|index| window.try_find(strip_row_id(*index)).is_some())
            .count()
    })
}

fn drawn(cx: &TestAppContext, view: &Entity<WorkspaceView>) -> usize {
    cx.read(|cx| view.read(cx).slide_strip().read(cx).thumbnails_drawn())
}

fn many_slides(count: usize) -> String {
    (0..count)
        .map(|n| format!("# Slide {n}\n"))
        .collect::<Vec<_>>()
        .join("\n---\n\n")
}

#[gpui_kit::test]
fn a_full_memory_budget_does_not_keep_the_list_drawing(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("slides.md"), many_slides(30)).unwrap();
    let mut talk = Document::open(dir.path(), Fonts::bundled_only()).unwrap();
    // No room for more than the slide being drawn.
    talk.set_memory_budget(1);
    let (handle, view) = open_view(cx, |window, cx| WorkspaceView::open_with(talk, window, cx));
    // Opening has settled, so the drawing stopped.
    let rows = rows_on_screen(cx, handle, 30);
    assert!(rows > 2, "{rows} rows");
    assert!(
        drawn(cx, &view) <= rows,
        "{} drawn for {rows} rows",
        drawn(cx, &view)
    );
    for index in 0..rows {
        assert!(
            thumbnail(cx, &view, index).is_some(),
            "row {index} shows its slide"
        );
    }
}

#[gpui_kit::test]
fn thumbnails_are_drawn_once_at_the_size_the_list_shows_them(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, &many_slides(30));
    let rows = rows_on_screen(cx, handle, 30);
    assert_eq!(drawn(cx, &view), rows, "none drawn at a guessed size first");
    // 220 wide, less the row's padding, number, gap and frame, at a scale factor of 2,
    // rounded up to 16 device pixels.
    let image = thumbnail(cx, &view, 0).unwrap();
    assert_eq!(i32::from(image.size(0).width), 352);
}

#[gpui_kit::test]
fn a_row_shows_its_own_slide_after_one_before_it_is_deleted(cx: &mut TestAppContext) {
    let talk = "# One\n\n---\n\n# Two\n\n---\n\n<Box x=\"0\" y=\"0\" w=\"1920\" h=\"1080\" fill=\"#ff0000\" />\n\n---\n\n# Four\n\n---\n\n# Five\n";
    let (handle, view) = open(cx, talk);
    let third = thumbnail(cx, &view, 2).unwrap();
    click(cx, handle, strip_row_id(1));
    click(cx, handle, DELETE_ID);
    assert_eq!(count(cx, &view), 4);
    let second = thumbnail(cx, &view, 1).unwrap();
    assert_eq!(
        second.as_bytes(0),
        third.as_bytes(0),
        "row 2 shows what was slide 3"
    );
    let fourth = thumbnail(cx, &view, 3).unwrap();
    assert_ne!(fourth.as_bytes(0), third.as_bytes(0));
}

#[gpui_kit::test]
fn the_undo_keys_in_the_notes_field_act_on_its_text(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, TWO);
    keys(cx, handle, "ctrl-m");
    keys(cx, handle, "ctrl-z");
    assert_eq!(count(cx, &view), 2);
    click(cx, handle, NOTES_ID);
    for key in ["ctrl-shift-z", "ctrl-y", "ctrl-z"] {
        keys(cx, handle, key);
        assert_eq!(count(cx, &view), 2, "{key} left the talk alone");
        assert_eq!(source(cx, &view), TWO, "{key}");
    }
    with_window(cx, handle, |window, _| assert!(!disabled(window, REDO_ID)));
}

#[gpui_kit::test]
fn delete_and_backspace_leave_the_slides_alone_outside_the_list(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, THREE);
    click(cx, handle, CANVAS_ID);
    keys(cx, handle, "delete");
    keys(cx, handle, "backspace");
    assert_eq!(count(cx, &view), 3, "with the slide in hand");
    click(cx, handle, NOTES_ID);
    keys(cx, handle, "delete");
    keys(cx, handle, "backspace");
    assert_eq!(count(cx, &view), 3, "with the notes in hand");
    assert_eq!(source(cx, &view), THREE);
}

#[gpui_kit::test]
fn a_refused_notes_edit_is_said_and_the_field_shows_the_notes_again(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, TWO);
    // More than a slide can hold, as a paste might bring.
    let pasted = "words ".repeat(8 * 1024);
    let field = cx.read(|cx| view.read(cx).notes_panel().read(cx).field().clone());
    cx.update_window(handle, |_, window, cx| {
        field.update(cx, |field, cx| field.replace_all(pasted, window, cx))
    })
    .unwrap();
    settle(cx, handle);
    assert_eq!(source(cx, &view), TWO, "nothing was made");
    let status = with_window(cx, handle, |window, _| {
        window
            .find(workspace_view::STATUS_ID)
            .label()
            .map(str::to_string)
    });
    assert_eq!(
        status.as_deref(),
        Some("That change would have broken a slide, so it wasn't made.")
    );
    assert_eq!(
        notes_text(cx, &view),
        "",
        "the field shows the slide's notes again"
    );
}

#[gpui_kit::test]
fn typing_blank_lines_and_spaces_in_notes_keeps_the_caret(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, TWO);
    click(cx, handle, NOTES_ID);
    with_window(cx, handle, |window, cx| window.input("a", cx));
    for _ in 0..3 {
        with_window(cx, handle, |window, cx| window.press("enter", cx));
        settle(cx, handle);
    }
    with_window(cx, handle, |window, cx| window.input("  b", cx));
    settle(cx, handle);
    with_window(cx, handle, |window, cx| window.input("c", cx));
    settle(cx, handle);
    assert_eq!(notes_text(cx, &view), "a\n\n\n  bc");
    assert_eq!(notes_of(cx, &view, 0).as_deref(), Some("a\n\n\n  bc"));
}

/// Where in row `index` the pointer is, `down` of the way from its top.
fn in_row(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    index: usize,
    down: f32,
) -> Point<Pixels> {
    with_window(cx, handle, |window, _| {
        let row = window.find(strip_row_id(index)).bounds();
        point(row.center().x, row.top() + row.size.height * down)
    })
}

/// Presses on `from` and moves the pointer to `to` in steps, as a hand does, without letting
/// go.
fn press_and_move(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    from: Point<Pixels>,
    to: Point<Pixels>,
) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let event = |position| MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers: Modifiers::default(),
        };
        window.dispatch_event(event(from).to_platform_input(), cx);
        window.render_frame(cx);
        window.dispatch_event(
            MouseDownEvent {
                button: MouseButton::Left,
                position: from,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
    move_to(cx, handle, from, to);
}

/// Moves the pointer, held down, from `from` to `to` in steps.
fn move_to(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    from: Point<Pixels>,
    to: Point<Pixels>,
) {
    cx.update_window(handle, |_, window, cx| {
        for step in 1..=8 {
            let fraction = step as f32 / 8.;
            let position = point(
                from.x + (to.x - from.x) * fraction,
                from.y + (to.y - from.y) * fraction,
            );
            window.dispatch_event(
                MouseMoveEvent {
                    position,
                    pressed_button: Some(MouseButton::Left),
                    modifiers: Modifiers::default(),
                }
                .to_platform_input(),
                cx,
            );
            window.render_frame(cx);
        }
    })
    .unwrap();
    settle(cx, handle);
}

fn release(cx: &mut TestAppContext, handle: AnyWindowHandle, at: Point<Pixels>) {
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_event(
            MouseUpEvent {
                button: MouseButton::Left,
                position: at,
                modifiers: Modifiers::default(),
                click_count: 1,
            }
            .to_platform_input(),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
    settle(cx, handle);
}

/// Drags row `from` with the mouse and lets go `down` of the way into row `to`.
fn drag_row(cx: &mut TestAppContext, handle: AnyWindowHandle, from: usize, to: usize, down: f32) {
    let start = in_row(cx, handle, from, 0.5);
    let end = in_row(cx, handle, to, down);
    press_and_move(cx, handle, start, end);
    release(cx, handle, end);
}

/// The headings of the slides, in the order the talk has them.
fn order(cx: &TestAppContext, view: &Entity<WorkspaceView>) -> Vec<String> {
    cx.read(|cx| {
        let document = view.read(cx).document().unwrap().read(cx);
        let talk = document.talk();
        talk.slides
            .iter()
            .map(|slide| {
                talk.source[slide.range.clone()]
                    .lines()
                    .find_map(|line| line.strip_prefix("# "))
                    .unwrap_or_default()
                    .to_string()
            })
            .collect()
    })
}

fn can_undo(cx: &TestAppContext, view: &Entity<WorkspaceView>) -> bool {
    document(cx, view).read_with(cx, |document, _| document.can_undo())
}

#[gpui_kit::test]
fn dragging_a_row_down_moves_the_slide_there(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, THREE);
    drag_row(cx, handle, 0, 2, 0.75);
    assert_eq!(order(cx, &view), ["Two", "Three", "One"]);
    assert_eq!(current(cx, &view), 2, "the moved slide is the one in hand");
    assert_eq!(title(cx, handle), "• T · 3 / 3");
    with_window(cx, handle, |window, _| {
        assert_eq!(window.find(strip_row_id(2)).selected(), Some(true));
    });

    // Between two rows.
    drag_row(cx, handle, 0, 2, 0.25);
    assert_eq!(order(cx, &view), ["Three", "Two", "One"]);
    assert_eq!(current(cx, &view), 1);
}

#[gpui_kit::test]
fn dragging_a_row_up(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, THREE);
    drag_row(cx, handle, 2, 0, 0.25);
    assert_eq!(order(cx, &view), ["Three", "One", "Two"]);
    assert_eq!(current(cx, &view), 0);
    drag_row(cx, handle, 2, 1, 0.25);
    assert_eq!(order(cx, &view), ["Three", "Two", "One"]);
}

#[gpui_kit::test]
fn dropping_a_row_on_itself_changes_nothing(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, THREE);
    // A little way within itself, and to the line just under it: either way it lands
    // where it is.
    drag_row(cx, handle, 1, 1, 0.8);
    drag_row(cx, handle, 1, 2, 0.2);
    drag_row(cx, handle, 1, 0, 0.8);
    assert_eq!(source(cx, &view), THREE);
    assert!(!can_undo(cx, &view), "no step to undo");
    assert_eq!(title(cx, handle), "T · 1 / 3", "nothing unsaved");
}

#[gpui_kit::test]
fn the_line_shows_where_it_will_land(cx: &mut TestAppContext) {
    let (handle, _) = open(cx, THREE);
    with_window(cx, handle, |window, _| {
        assert!(window.try_find(DROP_LINE_ID).is_none(), "no drag, no line");
        assert!(window.try_find(DRAG_PICTURE_ID).is_none());
    });
    let start = in_row(cx, handle, 0, 0.5);
    let between = in_row(cx, handle, 2, 0.25);
    press_and_move(cx, handle, start, between);
    with_window(cx, handle, |window, _| {
        let line = window.find(DROP_LINE_ID).bounds();
        let row = window.find(strip_row_id(2)).bounds();
        assert!(
            (line.center().y - row.top()).abs() <= px(3.),
            "between the second row and the third: {line:?} {row:?}"
        );
        let picture = window.find(DRAG_PICTURE_ID);
        assert!(picture.visible());
        assert!(
            picture.bounds().contains(&between),
            "the picture is under the pointer: {:?}",
            picture.bounds()
        );
    });
    let after = in_row(cx, handle, 2, 0.75);
    move_to(cx, handle, between, after);
    with_window(cx, handle, |window, _| {
        let line = window.find(DROP_LINE_ID).bounds();
        let row = window.find(strip_row_id(2)).bounds();
        assert!(
            (line.center().y - row.bottom()).abs() <= px(3.),
            "after the last row: {line:?} {row:?}"
        );
    });
    // Back over where it is, it would land where it is.
    let home = in_row(cx, handle, 0, 0.6);
    move_to(cx, handle, after, home);
    with_window(cx, handle, |window, _| {
        assert!(window.try_find(DROP_LINE_ID).is_none());
        assert!(window.find(DRAG_PICTURE_ID).visible());
    });
    release(cx, handle, home);
    with_window(cx, handle, |window, _| {
        assert!(window.try_find(DROP_LINE_ID).is_none(), "gone when dropped");
        assert!(window.try_find(DRAG_PICTURE_ID).is_none());
    });
}

#[gpui_kit::test]
fn escape_cancels_a_drag(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, THREE);
    let start = in_row(cx, handle, 0, 0.5);
    let end = in_row(cx, handle, 2, 0.75);
    press_and_move(cx, handle, start, end);
    with_window(cx, handle, |window, _| {
        assert!(window.find(DROP_LINE_ID).visible());
    });
    keys(cx, handle, "escape");
    with_window(cx, handle, |window, _| {
        assert!(window.try_find(DROP_LINE_ID).is_none());
        assert!(window.try_find(DRAG_PICTURE_ID).is_none());
    });
    release(cx, handle, end);
    assert_eq!(source(cx, &view), THREE);
    assert!(!can_undo(cx, &view));
}

#[gpui_kit::test]
fn letting_go_outside_the_list_changes_nothing(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, THREE);
    let start = in_row(cx, handle, 0, 0.5);
    let outside = with_window(cx, handle, |window, _| {
        window.find(CANVAS_ID).bounds().center()
    });
    press_and_move(cx, handle, start, outside);
    with_window(cx, handle, |window, _| {
        assert!(window.try_find(DROP_LINE_ID).is_none(), "nowhere to land");
    });
    release(cx, handle, outside);
    assert_eq!(source(cx, &view), THREE);
    with_window(cx, handle, |window, _| {
        assert!(window.try_find(DRAG_PICTURE_ID).is_none());
    });
}

#[gpui_kit::test]
fn one_undo_puts_a_dragged_slide_back(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, THREE);
    drag_row(cx, handle, 0, 2, 0.75);
    assert_ne!(source(cx, &view), THREE);
    with_window(cx, handle, |window, _| {
        assert_eq!(window.find(UNDO_ID).description(), Some("Undo Move slide"));
    });
    keys(cx, handle, "ctrl-z");
    assert_eq!(source(cx, &view), THREE, "the bytes it had");
    assert!(!can_undo(cx, &view), "one step");
    assert_eq!(current(cx, &view), 0, "back where it was, in hand");
}

#[gpui_kit::test]
fn dragging_near_the_bottom_scrolls_the_list(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, &many_slides(30));
    let start = in_row(cx, handle, 0, 0.5);
    let bottom = with_window(cx, handle, |window, _| {
        let strip = window.find(STRIP_ID).bounds();
        point(strip.center().x, strip.bottom() - px(4.))
    });
    press_and_move(cx, handle, start, bottom);
    // Held there, it goes on scrolling to the end, a little at a time.
    for _ in 0..400 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(20));
        cx.run_until_parked();
    }
    settle(cx, handle);
    with_window(cx, handle, |window, _| {
        assert!(window.try_find(strip_row_id(0)).is_none(), "scrolled away");
        assert!(window.try_find(strip_row_id(29)).is_some(), "to the end");
        let line = window.find(DROP_LINE_ID).bounds();
        let last = window.find(strip_row_id(29)).bounds();
        assert!(
            (line.center().y - last.bottom()).abs() <= px(3.),
            "{line:?} {last:?}"
        );
    });
    release(cx, handle, bottom);
    assert_eq!(order(cx, &view).last().map(String::as_str), Some("Slide 0"));
    assert_eq!(current(cx, &view), 29);
}

/// Opens the menu of row `row`.
fn open_rows_menu(cx: &mut TestAppContext, handle: AnyWindowHandle, row: usize) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.right_click(strip_row_id(row), cx);
    })
    .unwrap();
    settle(cx, handle);
}

/// Opens the menu of row `row` and picks its item `item`.
fn pick_from_rows_menu(cx: &mut TestAppContext, handle: AnyWindowHandle, row: usize, item: usize) {
    open_rows_menu(cx, handle, row);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.within("popup-menu").click(item, cx);
    })
    .unwrap();
    settle(cx, handle);
}

/// The menu's items, a line between the last two groups.
const MENU: [&str; 10] = [
    "New slide",
    "Duplicate",
    "Delete",
    "Hide slide",
    "Move up",
    "Move down",
    "",
    "Cut",
    "Copy",
    "Paste",
];

#[gpui_kit::test]
fn the_rows_menu_does_each_thing(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, THREE);
    open_rows_menu(cx, handle, 1);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let menu = window.within("popup-menu");
        let items: Vec<_> = (0..MENU.len())
            .map(|item| menu.find(item).label().unwrap_or_default().to_string())
            .collect();
        assert_eq!(items, MENU);
        assert!(menu.try_find(MENU.len()).is_none(), "nothing else");
        window.press("escape", cx);
    })
    .unwrap();
    settle(cx, handle);
    assert_eq!(current(cx, &view), 1, "the row's slide is the one in hand");

    // Each acts on the row's own slide, not the one that was in hand.
    click(cx, handle, strip_row_id(0));
    pick_from_rows_menu(cx, handle, 1, 0);
    assert_eq!(order(cx, &view), ["One", "Two", "", "Three"], "New slide");
    assert_eq!(current(cx, &view), 2);

    pick_from_rows_menu(cx, handle, 3, 1);
    assert_eq!(
        order(cx, &view),
        ["One", "Two", "", "Three", "Three"],
        "Duplicate"
    );

    pick_from_rows_menu(cx, handle, 2, 2);
    assert_eq!(order(cx, &view), ["One", "Two", "Three", "Three"], "Delete");

    pick_from_rows_menu(cx, handle, 0, 3);
    assert!(hidden(cx, &view, 0), "Hide");
    open_rows_menu(cx, handle, 0);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.within("popup-menu").find(3usize).label(),
            Some("Show slide")
        );
        window.press("escape", cx);
    })
    .unwrap();
    settle(cx, handle);
    pick_from_rows_menu(cx, handle, 0, 3);
    assert!(!hidden(cx, &view, 0), "Show");

    pick_from_rows_menu(cx, handle, 1, 4);
    assert_eq!(
        order(cx, &view),
        ["Two", "One", "Three", "Three"],
        "Move up"
    );
    assert_eq!(current(cx, &view), 0);

    pick_from_rows_menu(cx, handle, 0, 5);
    assert_eq!(
        order(cx, &view),
        ["One", "Two", "Three", "Three"],
        "Move down"
    );
    assert_eq!(current(cx, &view), 1);
}

#[gpui_kit::test]
fn move_up_is_disabled_for_the_first_slide(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, THREE);
    pick_from_rows_menu(cx, handle, 0, 4);
    assert_eq!(source(cx, &view), THREE);
    assert!(!can_undo(cx, &view));
    with_window(cx, handle, |window, _| {
        assert!(
            window.try_find("popup-menu").is_some(),
            "a disabled item doesn't take the click, so the menu stays"
        );
    });
    keys(cx, handle, "escape");

    // And Move down for the last, and Delete for the only one.
    pick_from_rows_menu(cx, handle, 2, 5);
    assert_eq!(source(cx, &view), THREE);
    keys(cx, handle, "escape");
    let (handle, view) = open(cx, "# Only\n");
    pick_from_rows_menu(cx, handle, 0, 2);
    assert_eq!(count(cx, &view), 1);
    assert!(!can_undo(cx, &view));
}

#[gpui_kit::test]
fn a_slide_number_of_three_digits_stays_on_one_line(cx: &mut TestAppContext) {
    let (handle, _view) = open(cx, &many_slides(120));
    let one_line = with_window(cx, handle, |window, _| {
        window.find(strip_number_id(0)).bounds()
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.scroll(
            STRIP_ID,
            gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-100_000.))),
            cx,
        );
    })
    .unwrap();
    settle(cx, handle);
    with_window(cx, handle, |window, _| {
        let row = window.find(strip_row_id(119)).bounds();
        let number = window.find(strip_number_id(119)).bounds();
        assert_eq!(
            number.size.height, one_line.size.height,
            "120 is on one line"
        );
        assert!(number.left() >= row.left(), "{number:?} inside {row:?}");
    });
}

#[gpui_kit::test]
fn the_list_scrolled_to_its_end_draws_each_thumbnail_once(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, &many_slides(30));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.scroll(
            STRIP_ID,
            gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-10_000.))),
            cx,
        );
    })
    .unwrap();
    settle(cx, handle);
    assert!(rows_on_screen(cx, handle, 30) < 30);
    with_window(cx, handle, |window, _| {
        assert!(window.try_find(strip_row_id(29)).is_some(), "at the end");
    });
    assert!(thumbnail(cx, &view, 29).is_some());
    let drawn_at_the_end = drawn(cx, &view);
    settle(cx, handle);
    assert_eq!(drawn(cx, &view), drawn_at_the_end, "nothing drawn again");
    assert!(drawn_at_the_end <= 30, "{drawn_at_the_end}");
}

#[gpui_kit::test]
fn a_reload_during_a_drag_ends_it_and_the_drop_moves_nothing(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, THREE);
    let start = in_row(cx, handle, 0, 0.5);
    let end = in_row(cx, handle, 2, 0.75);
    press_and_move(cx, handle, start, end);
    // The talk changes on disk while the first slide is held: another is put before it.
    let changed =
        "---\ntitle: T\n---\n\n# Zero\n\n---\n\n# One\n\n---\n\n# Two\n\n---\n\n# Three\n";
    view.update(cx, |view, cx| {
        view.reload_for_test(Some(changed.to_string()), cx)
    });
    settle(cx, handle);
    with_window(cx, handle, |window, _| {
        assert!(window.try_find(DROP_LINE_ID).is_none(), "the drag is over");
        assert!(window.try_find(DRAG_PICTURE_ID).is_none());
    });
    release(cx, handle, end);
    assert_eq!(source(cx, &view), changed, "nothing moved");
    assert!(!can_undo(cx, &view));
}

#[gpui_kit::test]
fn an_undo_during_a_drag_ends_it_and_the_drop_moves_nothing(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, THREE);
    // A new slide after the first, so that undoing it puts another slide where row 1 is.
    keys(cx, handle, "ctrl-m");
    let before_undo = order(cx, &view);
    assert_eq!(before_undo, ["One", "", "Two", "Three"]);
    let start = in_row(cx, handle, 2, 0.5);
    let end = in_row(cx, handle, 3, 0.75);
    press_and_move(cx, handle, start, end);
    keys(cx, handle, "ctrl-z");
    assert_eq!(order(cx, &view), ["One", "Two", "Three"]);
    with_window(cx, handle, |window, _| {
        assert!(window.try_find(DROP_LINE_ID).is_none(), "the drag is over");
        assert!(window.try_find(DRAG_PICTURE_ID).is_none());
    });
    release(cx, handle, end);
    assert_eq!(order(cx, &view), ["One", "Two", "Three"], "nothing moved");
    assert_eq!(source(cx, &view), THREE);
    assert!(
        document(cx, &view).read_with(cx, |document, _| document.can_redo()),
        "the undone slide can still be had back"
    );
}

#[gpui_kit::test]
fn a_talk_cut_to_one_slide_shows_its_picture(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, TWO);
    assert!(thumbnail(cx, &view, 0).is_some() && thumbnail(cx, &view, 1).is_some());
    // The first is deleted, so the one row left shows another slide.
    click(cx, handle, DELETE_ID);
    assert_eq!(count(cx, &view), 1);
    settle(cx, handle);
    assert!(
        thumbnail(cx, &view, 0).is_some(),
        "the slide left has its picture"
    );
}

#[gpui_kit::test]
fn a_window_that_shows_one_row_draws_its_picture(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, THREE);
    assert!(rows_on_screen(cx, handle, 3) > 1);
    cx.simulate_window_resize(handle, size(px(1280.), px(200.)));
    settle(cx, handle);
    with_window(cx, handle, |window, _| {
        let strip = window.find(STRIP_ID).bounds();
        let row = window.find(strip_row_id(0)).bounds();
        assert!(
            strip.size.height < row.size.height,
            "only the first row shows: {strip:?} {row:?}"
        );
    });
    // The first slide changes, so its picture has to be drawn again.
    let changed = THREE.replace("# One", "# One, again");
    view.update(cx, |view, cx| view.reload_for_test(Some(changed), cx));
    settle(cx, handle);
    assert!(thumbnail(cx, &view, 0).is_some(), "drawn as it is now");
}

#[gpui_kit::test]
fn a_drop_during_scrolling_lands_where_the_line_was(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, &many_slides(30));
    let start = in_row(cx, handle, 0, 0.5);
    let bottom = with_window(cx, handle, |window, _| {
        let strip = window.find(STRIP_ID).bounds();
        point(strip.center().x, strip.bottom() - px(4.))
    });
    press_and_move(cx, handle, start, bottom);
    // Partway down the list.
    for _ in 0..60 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(20));
        cx.run_until_parked();
    }
    settle(cx, handle);
    let gap = with_window(cx, handle, |window, _| {
        let line = window.find(DROP_LINE_ID).bounds().center().y;
        (1..30)
            .find(|index| {
                window
                    .try_find(strip_row_id(*index))
                    .is_some_and(|row| (row.bounds().top() - line).abs() <= px(3.))
            })
            .expect("the line is at the top of a row")
    });
    assert!(gap > 6 && gap < 29, "partway: {gap}");
    release(cx, handle, bottom);
    let order = order(cx, &view);
    assert_eq!(
        order[gap - 1],
        "Slide 0",
        "just above the row the line was over"
    );
    assert_eq!(current(cx, &view), gap - 1);
}

#[gpui_kit::test]
fn the_rows_menu_names_delete_by_the_delete_key(cx: &mut TestAppContext) {
    let (handle, view) = open(cx, THREE);
    open_rows_menu(cx, handle, 1);
    let strip = cx.read(|cx| view.read(cx).slide_strip().read(cx).focus_handle().clone());
    cx.update_window(handle, |_, window, _| {
        let binding = window
            .highest_precedence_binding_for_action_in(&workspace_view::DeleteSlide, &strip)
            .expect("Delete has a key");
        let keys: Vec<String> = binding.keystrokes().iter().map(|key| key.key().to_string()).collect();
        assert_eq!(keys, ["delete"]);
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_notes_have_a_caption_above_them(cx: &mut TestAppContext) {
    let (handle, _view) = open(cx, THREE);
    with_window(cx, handle, |window, _| {
        let caption = window.find(NOTES_CAPTION_ID);
        assert_eq!(caption.label(), Some("Notes"));
        assert!(caption.visible(), "{caption:?}");
        let field = window
            .within(NOTES_ID)
            .elements()
            .into_iter()
            .find(|element| element.placeholder().is_some())
            .expect("the field");
        assert!(caption.bounds().bottom() <= field.bounds().top(), "{caption:?} {field:?}");
    });
}
