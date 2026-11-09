//! The inspector, headless: a selection in, `InspectorEvent`s out. The
//! title-bar Undo and Redo are checked through a `WorkspaceView` whose
//! canvas-less test constructor records the commands it would have sent.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Entity, Point, TestAppContext, WindowBounds,
    WindowOptions, px, size,
};
use stagepack_studio::document_view::DocumentView;
use stagepack_studio::inspector::{Inspector, InspectorEvent};
use stagepack_studio::workspace_view::WorkspaceView;
use studio_protocol::{CommandBody, DragPos, PageMessage, SelectionInfo};

type Events = Rc<RefCell<Vec<InspectorEvent>>>;

fn options() -> WindowOptions {
    let bounds = Bounds {
        origin: Point::default(),
        size: size(px(1280.), px(800.)),
    };
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        ..Default::default()
    }
}

fn open_inspector(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Inspector>, Events) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_reduce_motion(true);
    });
    let events: Events = Rc::default();
    let (handle, inspector) = cx.update(|cx| {
        let (window, view) = gpui_kit::open_window(options(), cx, |window, cx| {
            cx.new(|cx| Inspector::new(window, cx))
        })
        .expect("open test window");
        (window, view)
    });
    let sink = events.clone();
    cx.update(|cx| {
        cx.subscribe(&inspector, move |_, event: &InspectorEvent, _| {
            sink.borrow_mut().push(event.clone());
        })
        .detach();
    });
    (handle, inspector, events)
}

fn select(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    inspector: &Entity<Inspector>,
    selection: Option<SelectionInfo>,
) {
    cx.update_window(handle, |_, window, cx| {
        inspector.update(cx, |inspector, cx| {
            inspector.set_selection(selection, window, cx)
        });
        window.render_frame(cx);
    })
    .unwrap();
}

fn positioned() -> SelectionInfo {
    SelectionInfo {
        slide: 1,
        range: Some([10, 20]),
        kind: "element".into(),
        tag: Some("div".into()),
        label: "Title box".into(),
        positioned: true,
        nested: false,
        pos: Some(DragPos {
            x: 120.0,
            y: 80.0,
            w: 200.0,
            h: None,
            rotate: 0.0,
        }),
        can_reorder: false,
    }
}

fn in_flow() -> SelectionInfo {
    SelectionInfo {
        positioned: false,
        pos: None,
        ..positioned()
    }
}

fn type_into(window: &mut gpui_kit::Window, id: &'static str, text: &str, cx: &mut gpui_kit::App) {
    window.click(id, cx);
    window.press("ctrl-a", cx);
    if text.is_empty() {
        window.press("backspace", cx);
    } else {
        window.input(text, cx);
    }
    window.press("enter", cx);
}

/// Subscribers run when the closure that caused the event returns, so what
/// they did shows from the next frame.
fn settle(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
}

#[gpui_kit::test]
fn empty_state_text(cx: &mut TestAppContext) {
    let (handle, inspector, _) = open_inspector(cx);
    select(cx, handle, &inspector, None);
    cx.update_window(handle, |_, window, _| {
        assert_eq!(
            window.find("inspector-empty").label(),
            Some("Click something on the slide to select it.")
        );
        assert!(window.try_find("inspector-label").is_none());
        assert!(window.try_find("duplicate").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn positioned_selection_shows_fields(cx: &mut TestAppContext) {
    let (handle, inspector, _) = open_inspector(cx);
    select(cx, handle, &inspector, Some(positioned()));
    cx.update_window(handle, |_, window, _| {
        assert_eq!(window.find("inspector-label").label(), Some("Title box"));
        assert!(window.try_find("inspector-empty").is_none());
        assert_eq!(window.find("pos-x").value(), Some("120"));
        assert_eq!(window.find("pos-y").value(), Some("80"));
        assert_eq!(window.find("pos-w").value(), Some("200"));
        assert_eq!(window.find("pos-h").value(), Some(""));
        assert_eq!(window.find("pos-rotate").value(), Some("0"));
        assert!(window.find("return-to-flow").visible());
        assert!(window.try_find("free-position").is_none());
        assert!(window.find("duplicate").visible());
        assert!(window.find("delete").visible());
    })
    .unwrap();
}

#[gpui_kit::test]
fn fractions_keep_their_digits(cx: &mut TestAppContext) {
    let (handle, inspector, _) = open_inspector(cx);
    let mut selection = positioned();
    selection.pos.as_mut().unwrap().x = 12.5;
    selection.pos.as_mut().unwrap().h = Some(90.0);
    select(cx, handle, &inspector, Some(selection));
    cx.update_window(handle, |_, window, _| {
        assert_eq!(window.find("pos-x").value(), Some("12.5"));
        assert_eq!(window.find("pos-h").value(), Some("90"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn enter_in_x_sends_set_pos(cx: &mut TestAppContext) {
    let (handle, inspector, events) = open_inspector(cx);
    select(cx, handle, &inspector, Some(positioned()));
    cx.update_window(handle, |_, window, cx| {
        type_into(window, "pos-x", "140", cx)
    })
    .unwrap();
    assert_eq!(
        *events.borrow(),
        vec![InspectorEvent::Send(CommandBody::SetPos {
            pos: DragPos {
                x: 140.0,
                y: 80.0,
                w: 200.0,
                h: None,
                rotate: 0.0
            }
        })]
    );
}

#[gpui_kit::test]
fn enter_in_h_sets_and_clears_the_height(cx: &mut TestAppContext) {
    let (handle, inspector, events) = open_inspector(cx);
    select(cx, handle, &inspector, Some(positioned()));
    cx.update_window(handle, |_, window, cx| type_into(window, "pos-h", "60", cx))
        .unwrap();
    // The page echoes the new position back.
    let mut echoed = positioned();
    echoed.pos.as_mut().unwrap().h = Some(60.0);
    select(cx, handle, &inspector, Some(echoed));
    cx.update_window(handle, |_, window, cx| type_into(window, "pos-h", "", cx))
        .unwrap();
    let pos = |h| {
        InspectorEvent::Send(CommandBody::SetPos {
            pos: DragPos {
                x: 120.0,
                y: 80.0,
                w: 200.0,
                h,
                rotate: 0.0,
            },
        })
    };
    assert_eq!(*events.borrow(), vec![pos(Some(60.0)), pos(None)]);
}

#[gpui_kit::test]
fn negative_x_is_allowed(cx: &mut TestAppContext) {
    let (handle, inspector, events) = open_inspector(cx);
    select(cx, handle, &inspector, Some(positioned()));
    cx.update_window(handle, |_, window, cx| {
        type_into(window, "pos-x", "-30", cx)
    })
    .unwrap();
    assert!(matches!(
        events.borrow().as_slice(),
        [InspectorEvent::Send(CommandBody::SetPos { pos })] if pos.x == -30.0
    ));
}

#[gpui_kit::test]
fn bad_text_sends_nothing_and_restores_the_field(cx: &mut TestAppContext) {
    let (handle, inspector, events) = open_inspector(cx);
    select(cx, handle, &inspector, Some(positioned()));
    for (id, text, restored) in [
        ("pos-x", "abc", "120"),
        ("pos-y", "", "80"),
        ("pos-h", "tall", ""),
    ] {
        cx.update_window(handle, |_, window, cx| type_into(window, id, text, cx))
            .unwrap();
        settle(cx, handle);
        cx.update_window(handle, |_, window, _| {
            assert_eq!(window.find(id).value(), Some(restored), "{id}");
        })
        .unwrap();
    }
    assert!(events.borrow().is_empty(), "{:?}", events.borrow());
}

#[gpui_kit::test]
fn an_echo_for_the_same_element_leaves_the_focused_field_alone(cx: &mut TestAppContext) {
    let (handle, inspector, _) = open_inspector(cx);
    select(cx, handle, &inspector, Some(positioned()));
    cx.update_window(handle, |_, window, cx| {
        window.click("pos-x", cx);
        window.press("ctrl-a", cx);
        window.input("14", cx);
    })
    .unwrap();
    let mut echoed = positioned();
    let pos = echoed.pos.as_mut().unwrap();
    pos.x = 999.0;
    pos.y = 90.0;
    select(cx, handle, &inspector, Some(echoed));
    cx.update_window(handle, |_, window, _| {
        assert_eq!(window.find("pos-x").value(), Some("14"), "being typed in");
        assert_eq!(window.find("pos-y").value(), Some("90"), "updated");
    })
    .unwrap();

    // A different element replaces every field.
    let mut other = positioned();
    other.label = "Subtitle".into();
    other.pos.as_mut().unwrap().x = 5.0;
    select(cx, handle, &inspector, Some(other));
    cx.update_window(handle, |_, window, _| {
        assert_eq!(window.find("pos-x").value(), Some("5"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn flow_selection_offers_free_position(cx: &mut TestAppContext) {
    let (handle, inspector, events) = open_inspector(cx);
    select(cx, handle, &inspector, Some(in_flow()));
    cx.update_window(handle, |_, window, cx| {
        assert!(window.try_find("pos-x").is_none());
        assert!(window.try_find("return-to-flow").is_none());
        let button = window.find("free-position");
        assert!(button.visible());
        assert_ne!(button.disabled(), Some(true));
        window.click("free-position", cx);
    })
    .unwrap();
    assert_eq!(
        *events.borrow(),
        vec![InspectorEvent::Send(CommandBody::FreePosition)]
    );
}

#[gpui_kit::test]
fn nested_disables_free_position(cx: &mut TestAppContext) {
    let (handle, inspector, events) = open_inspector(cx);
    let mut selection = in_flow();
    selection.nested = true;
    select(cx, handle, &inspector, Some(selection));
    cx.update_window(handle, |_, window, cx| {
        assert_eq!(window.find("free-position").disabled(), Some(true));
        window.click("free-position", cx);
    })
    .unwrap();
    assert!(events.borrow().is_empty());
}

#[gpui_kit::test]
fn return_to_flow_duplicate_and_delete_send_their_commands(cx: &mut TestAppContext) {
    let (handle, inspector, events) = open_inspector(cx);
    select(cx, handle, &inspector, Some(positioned()));
    cx.update_window(handle, |_, window, cx| {
        window.click("return-to-flow", cx);
        window.click("duplicate", cx);
        window.click("delete", cx);
    })
    .unwrap();
    assert_eq!(
        *events.borrow(),
        vec![
            InspectorEvent::Send(CommandBody::ReturnToFlow),
            InspectorEvent::Send(CommandBody::Duplicate),
            InspectorEvent::Send(CommandBody::Delete),
        ]
    );
}

#[gpui_kit::test]
fn delete_sends_delete(cx: &mut TestAppContext) {
    let (handle, inspector, events) = open_inspector(cx);
    select(cx, handle, &inspector, Some(in_flow()));
    cx.update_window(handle, |_, window, cx| window.click("delete", cx))
        .unwrap();
    assert_eq!(
        *events.borrow(),
        vec![InspectorEvent::Send(CommandBody::Delete)]
    );
}

fn open_workspace(
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<WorkspaceView>, Entity<DocumentView>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_reduce_motion(true);
    });
    cx.update(|cx| {
        let (window, view) = gpui_kit::open_window(options(), cx, |window, cx| {
            cx.new(|cx| {
                WorkspaceView::for_test_with_document(
                    PathBuf::from("/talks/hello.sptx"),
                    window,
                    cx,
                )
            })
        })
        .expect("open test window");
        let document = view.read(cx).document_view().expect("a document").clone();
        (window, view, document)
    })
}

#[gpui_kit::test]
fn undo_redo_follow_history(cx: &mut TestAppContext) {
    let (handle, workspace, document) = open_workspace(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        for id in ["undo", "redo"] {
            assert_eq!(window.find(id).disabled(), Some(true), "{id}");
        }
    })
    .unwrap();

    cx.update_window(handle, |_, window, cx| {
        document.update(cx, |doc, cx| {
            doc.on_page(
                &PageMessage::History {
                    can_undo: true,
                    can_redo: false,
                    last_action: None,
                },
                window,
                cx,
            )
        });
        window.render_frame(cx);
        assert_ne!(window.find("undo").disabled(), Some(true));
        assert_eq!(window.find("redo").disabled(), Some(true));
        window.click("undo", cx);
        document.update(cx, |doc, cx| {
            doc.on_page(
                &PageMessage::History {
                    can_undo: false,
                    can_redo: true,
                    last_action: None,
                },
                window,
                cx,
            )
        });
        window.render_frame(cx);
        assert_eq!(window.find("undo").disabled(), Some(true));
        window.click("redo", cx);
    })
    .unwrap();

    cx.update(|cx| {
        assert_eq!(
            workspace.read(cx).sent_commands(),
            &[CommandBody::Undo, CommandBody::Redo]
        );
    });
}

#[gpui_kit::test]
fn a_page_selection_reaches_the_inspector_and_its_edits_are_sent(cx: &mut TestAppContext) {
    let (handle, workspace, document) = open_workspace(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("inspector-empty").is_some());
        document.update(cx, |doc, cx| {
            doc.on_page(
                &PageMessage::Selection {
                    selection: Some(positioned()),
                },
                window,
                cx,
            )
        });
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("pos-x").value(), Some("120"));
        type_into(window, "pos-x", "140", cx);
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| window.click("delete", cx))
        .unwrap();
    cx.update(|cx| {
        let sent = workspace.read(cx).sent_commands();
        assert!(
            matches!(
                sent,
                [CommandBody::SetPos { pos }, CommandBody::Delete] if pos.x == 140.0
            ),
            "{sent:?}"
        );
    });
}

fn set_pos(x: f64, y: f64, w: f64) -> InspectorEvent {
    InspectorEvent::Send(CommandBody::SetPos {
        pos: DragPos {
            x,
            y,
            w,
            h: None,
            rotate: 0.0,
        },
    })
}

#[gpui_kit::test]
fn undo_echo_updates_the_focused_field_after_enter(cx: &mut TestAppContext) {
    let (handle, inspector, events) = open_inspector(cx);
    select(cx, handle, &inspector, Some(positioned()));
    cx.update_window(handle, |_, window, cx| {
        type_into(window, "pos-x", "140", cx)
    })
    .unwrap();
    settle(cx, handle);
    // Undo: the page puts the box back, and the field still has focus.
    let mut undone = positioned();
    undone.pos.as_mut().unwrap().x = 100.0;
    select(cx, handle, &inspector, Some(undone));
    cx.update_window(handle, |_, window, cx| {
        assert_eq!(window.find("pos-x").value(), Some("100"));
        type_into(window, "pos-y", "81", cx);
    })
    .unwrap();
    assert_eq!(events.borrow()[1], set_pos(100.0, 81.0, 200.0));
}

#[gpui_kit::test]
fn an_echo_that_changes_y_is_what_enter_in_w_sends(cx: &mut TestAppContext) {
    let (handle, inspector, events) = open_inspector(cx);
    select(cx, handle, &inspector, Some(positioned()));
    // Uncommitted typing in X, which the page never hears of.
    cx.update_window(handle, |_, window, cx| {
        window.click("pos-x", cx);
        window.press("ctrl-a", cx);
        window.input("14", cx);
    })
    .unwrap();
    let mut echoed = positioned();
    echoed.pos.as_mut().unwrap().y = 90.0;
    select(cx, handle, &inspector, Some(echoed));
    cx.update_window(handle, |_, window, cx| {
        type_into(window, "pos-w", "250", cx)
    })
    .unwrap();
    assert_eq!(*events.borrow(), vec![set_pos(120.0, 90.0, 250.0)]);
}
