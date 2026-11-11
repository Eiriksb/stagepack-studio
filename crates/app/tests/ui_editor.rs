//! The editor's window: the tool tabs across the top, the list of slides on the left and the
//! notes under the slide.

use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::test::{TestQueryExt, TestWindowExt};
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Bounds, Context, Entity, Point, RenderImage, TestAppContext,
    Window, WindowBounds, WindowOptions, px, size,
};
use slides_engine::{Fonts, pack_folder};
use slides_format::is_hidden;
use stagepack_studio::document::Document;
use stagepack_studio::notes_panel::{NOTES_ID, NOTES_PLACEHOLDER};
use stagepack_studio::ribbon::{
    DELETE_ID, DUPLICATE_ID, HIDE_ID, MARKDOWN_TOGGLE_ID, NEW_SLIDE_ID, NEW_SLIDE_LAYOUTS_ID,
    NOTES_TOGGLE_ID, ON_THE_WAY, ON_THE_WAY_ID, PRESENT_ID, REDO_ID, REHEARSE_ID, RIBBON_ID,
    SLIDE_LIST_TOGGLE_ID, TAB_ANIMATE_ID, TAB_HOME_ID, TAB_INSERT_ID, TAB_SLIDE_SHOW_ID,
    TAB_VIEW_ID, UNDO_ID,
};
use stagepack_studio::slide_strip::{STRIP_ID, strip_row_id};
use stagepack_studio::workspace_view::{self, CANVAS_ID, MARKDOWN_ID, TITLE_ID, WorkspaceView};

const THREE: &str = "---\ntitle: T\n---\n\n# One\n\n---\n\n# Two\n\n---\n\n# Three\n";
const TWO: &str = "---\ntitle: T\n---\n\n# One\n\n---\n\n# Two\n";

fn open_view(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut Window, &mut Context<WorkspaceView>) -> WorkspaceView + 'static,
) -> (AnyWindowHandle, Entity<WorkspaceView>) {
    let opened = cx.update(|cx: &mut App| {
        gpui_kit::init(cx);
        workspace_view::bind_keys(cx);
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
    cx.read(|cx| view.read(cx).slide_strip().read(cx).thumbnail(index))
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
        for id in [
            NEW_SLIDE_ID,
            DUPLICATE_ID,
            DELETE_ID,
            HIDE_ID,
            UNDO_ID,
            REDO_ID,
        ] {
            assert!(window.find(id).visible(), "{id}");
        }
        assert_eq!(window.find(NEW_SLIDE_ID).label(), Some("New slide"));
        assert_eq!(window.find(HIDE_ID).label(), Some("Hide"));
        assert!(
            window.find_by_label("Slides").visible(),
            "the group's caption"
        );
        assert!(
            window.find_by_label("History").visible(),
            "the group's caption"
        );
    });
}

#[gpui_kit::test]
fn each_tab_shows_its_own_tools(cx: &mut TestAppContext) {
    let (handle, _) = open(cx, THREE);
    for tab in [TAB_INSERT_ID, TAB_ANIMATE_ID] {
        click(cx, handle, tab);
        with_window(cx, handle, |window, _| {
            let line = window.find(ON_THE_WAY_ID);
            assert!(line.visible());
            assert_eq!(line.label(), Some(ON_THE_WAY));
            assert_eq!(ON_THE_WAY, "Tools for this tab are on the way.");
            assert!(
                window.try_find(NEW_SLIDE_ID).is_none(),
                "Home's tools are put away"
            );
            assert_eq!(window.find(tab).selected(), Some(true));
        });
    }
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
            Some("Show"),
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
        assert_eq!(window.find(HIDE_ID).label(), Some("Hide"));
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
    click(cx, handle, UNDO_ID);
    assert_eq!(count(cx, &view), 2);
    assert_eq!(state(cx), (true, false));
    click(cx, handle, REDO_ID);
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
    click(cx, handle, UNDO_ID);
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

    click(cx, handle, UNDO_ID);
    assert_eq!(
        notes_of(cx, &view, 0),
        None,
        "one undo takes all the typing back"
    );
    assert_eq!(source(cx, &view), TWO);
    assert_eq!(notes_text(cx, &view), "", "and the field shows it");
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
    // The canvas has them again once it's clicked.
    click(cx, handle, CANVAS_ID);
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
    // With both away, the slide has the room.
    click(cx, handle, NOTES_TOGGLE_ID);
    click(cx, handle, SLIDE_LIST_TOGGLE_ID);
    with_window(cx, handle, |window, _| {
        let canvas = window.find(CANVAS_ID).bounds();
        assert!(canvas.left() < px(40.), "{canvas:?}");
        assert!(canvas.right() > px(1240.), "{canvas:?}");
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
