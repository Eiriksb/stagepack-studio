//! Drawing the editor: the slide, the marks over it, and the keys and mouse it takes.

use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};

use super::*;
use crate::arrange_tools::{order_items, reason};

/// How many sides the round turn handle is drawn with.
const HANDLE_ROUND: usize = 16;

impl Render for SlideEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = cx.entity().downgrade();
        let marks = self.marks(cx);
        // Each mark over the slide in the window theme's colour for it, or, where that doesn't
        // stand out from the slide's background, one that does.
        let background = match &self.document {
            Some(document) => document.read(cx).theme().background,
            None => toolkit::Theme::default().background,
        };
        let caret_color = standing_out_on(cx.theme().caret, background);
        let selection_color = cx.theme().selection.opacity(0.6);
        let (composition_color, composition_background) =
            (cx.theme().foreground, cx.theme().background);
        let caret_on = self.caret_on;
        let outline_color = standing_out_on(cx.theme().ring, background);
        // The things selected on the slide shown, each drawn around.
        let outlines: Vec<[(f32, f32); 4]> = match (self.selection_here(cx), self.document.clone()) {
            (Some(_), Some(document)) => document
                .update(cx, |document, _| document.selected_boxes())
                .iter()
                .map(|(_, rect, rotate)| outline_corners(*rect, *rotate))
                .collect(),
            _ => Vec::new(),
        };
        // A new box waiting for its first character: a dashed line around where it'll be.
        let new_box = self.new_box_here(cx);
        let new_box_color = standing_out_on(cx.theme().muted_foreground, background);
        // The area whose text is being typed: a dashed line around it, a press on which picks it.
        let frame = self.typed_region(cx).and_then(|(slide, name)| {
            let area = self.layout(slide, cx)?.region(&name)?.clone();
            Some((name, outline_corners(area.rect, area.rotate)))
        });
        let selecting = !outlines.is_empty() && !self.is_editing(cx);
        // A video selected: a button in its middle that plays it, or stops it while it plays.
        let video = match (selecting, self.document.clone()) {
            (true, Some(document)) => document.update(cx, |document, _| document.selected_video()),
            _ => None,
        };
        let video_playing = video.is_some() && self.video_playing(cx);
        // The lines what's dragged snaps to, while the button is down.
        let guides = self.guides().to_vec();
        // A thing being dragged out of the slide's text: a dashed line where it'll go.
        let lift_outline = self.lift_outline().map(|placed| outline_corners(placed.rect, placed.rotate));
        // The handles on the one thing selected, and what the pointer shows over them.
        let handles = self.handles(cx).map(|handles| (handles.spots, handles.rotate()));
        if handles.is_none() && !self.handle_drag() {
            self.pointer = None;
        }
        let pointer = self.pointer;
        let handle_color = cx.theme().background;
        let placed = {
            let canvas = self.canvas.read(cx);
            canvas.slide_bounds().zip(canvas.area())
        };
        let in_editor = |bounds: Bounds<Pixels>, area: Bounds<Pixels>| Bounds {
            origin: bounds.origin - area.origin,
            size: bounds.size,
        };
        // Where the caret is in the editor, for whoever looks for it.
        let caret_box = marks
            .as_ref()
            .and_then(|marks| marks.caret)
            .zip(placed)
            .map(|(caret, (slide, area))| in_editor(caret_box(&caret, slide), area));
        // Where each outline is in the editor, for whoever looks for it: the one picked last
        // has the id the editor's one outline always had.
        let boxed = |corners: &[(f32, f32); 4]| {
            placed.and_then(|(slide, area)| {
                let unit = f32::from(slide.size.width) / SLIDE_WIDTH;
                (unit > 0.0).then(|| in_editor(corners_box(corners, slide, unit), area))
            })
        };
        let outline_boxes: Vec<(SharedString, Bounds<Pixels>)> = outlines
            .iter()
            .enumerate()
            .filter_map(|(at, corners)| {
                let id = if at + 1 == outlines.len() { SELECTION_OUTLINE_ID.into() } else { selection_outline_id(at).into() };
                Some((id, boxed(corners)?))
            })
            .collect();
        // Where each handle is in the editor, for whoever looks for it.
        let handle_boxes: Vec<(SharedString, Bounds<Pixels>)> = handles
            .iter()
            .flat_map(|(spots, _)| spots.iter())
            .filter_map(|&(handle, (x, y))| {
                let (slide, area) = placed?;
                let unit = f32::from(slide.size.width) / SLIDE_WIDTH;
                let half = HANDLE_SIZE / 2.;
                let middle = slide.origin + point(px(x * unit), px(y * unit));
                let bounds = Bounds { origin: middle - point(px(half), px(half)), size: size(px(HANDLE_SIZE), px(HANDLE_SIZE)) };
                Some((SharedString::from(handle_id(handle)), in_editor(bounds, area)))
            })
            .collect();
        // Where each guide is in the editor, for whoever looks for it: the first has an id of its own.
        let guide_boxes: Vec<(SharedString, Bounds<Pixels>)> = guides
            .iter()
            .enumerate()
            .filter_map(|(at, guide)| {
                let (slide, area) = placed?;
                let unit = f32::from(slide.size.width) / SLIDE_WIDTH;
                let id = if at == 0 { SNAP_GUIDE_ID.into() } else { snap_guide_id(at).into() };
                (unit > 0.0).then(|| (id, in_editor(guide_bounds(guide, slide, unit), area)))
            })
            .collect();
        // On the Animate tab, the step each thing animated comes in on, at its top-left corner.
        let badges: Vec<(SharedString, Point<Pixels>, String)> = match (self.show_steps, self.document.clone(), placed) {
            (true, Some(document), Some((slide, area))) => {
                let unit = f32::from(slide.size.width) / SLIDE_WIDTH;
                document
                    .update(cx, |document, _| document.step_badges())
                    .into_iter()
                    .enumerate()
                    .filter(|_| unit > 0.0)
                    .map(|(at, badge)| {
                        let corner = in_editor(corners_box(&outline_corners(badge.rect, badge.rotate), slide, unit), area).origin;
                        (step_badge_id(at).into(), corner, badge.words)
                    })
                    .collect()
            }
            _ => Vec::new(),
        };
        let badge_color = standing_out_on(cx.theme().primary, background);
        let badge_text = standing_out_on(cx.theme().primary_foreground, shown_on(badge_color, background));
        // What each mark drawn this time is drawn in, as it shows on the slide.
        let shown = |drawn: bool, color: Hsla| drawn.then(|| shown_on(color, background));
        let painted = &mut self.painted;
        let caret_drawn = caret_on && marks.as_ref().is_some_and(|marks| marks.caret.is_some());
        painted.caret = shown(caret_drawn, caret_color).or(painted.caret);
        painted.frame = shown(frame.is_some() || new_box.is_some(), new_box_color).or(painted.frame);
        painted.outline = shown(!outlines.is_empty() || lift_outline.is_some(), outline_color).or(painted.outline);
        painted.handles = shown(handles.is_some(), outline_color).or(painted.handles);
        painted.guides = shown(!guides.is_empty(), outline_color).or(painted.guides);
        painted.badge = shown(!badges.is_empty(), badge_color).or(painted.badge);
        painted.badge_words = shown(!badges.is_empty(), badge_text).or(painted.badge_words);
        let video_button = video.as_ref().zip(placed).map(|(video, (slide, area))| {
            let unit = f32::from(slide.size.width) / SLIDE_WIDTH;
            let rect = video.on.rect;
            let middle = slide.origin + point(px((rect.x + rect.w / 2.) * unit), px((rect.y + rect.h / 2.) * unit));
            in_editor(Bounds { origin: middle, size: size(px(0.), px(0.)) }, area).origin
        });
        let lift_box = lift_outline.as_ref().and_then(|corners| Some((SharedString::from(LIFT_OUTLINE_ID), boxed(corners)?)));
        let frame_box = frame.as_ref().and_then(|(name, corners)| Some((SharedString::from(region_frame_id(name)), boxed(corners)?)));
        // Where the new box is in the editor, for whoever looks for it.
        let new_box_bounds = new_box.zip(placed).and_then(|((x, y, w, h), (slide, area))| {
            let unit = f32::from(slide.size.width) / SLIDE_WIDTH;
            let corners = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
            (unit > 0.0).then(|| in_editor(corners_box(&corners, slide, unit), area))
        });
        // What the system is composing, at the caret, where the selection it stands for starts.
        let composing = self.editing(cx).and_then(|editing| {
            let composing = editing.composing.as_ref()?;
            Some((composing.text.clone(), editing.edit.selection.start()))
        });
        let composition = composing.and_then(|(text, at)| {
            let caret = self.caret_in_window(at, cx)?;
            Some((text, in_editor(caret, placed?.1)))
        });
        // The system's input method places its window by the caret: it's told when that moves.
        let place = self
            .editing(cx)
            .map(|editing| (editing.edit.selection, caret_box, composition.clone()));
        if place != self.ime_place {
            if place.is_some() {
                window.invalidate_character_coordinates();
            }
            self.ime_place = place;
        }
        // While text is typed, the system's input method types it, and on a shape or a box
        // selected, where it starts the typing.
        let input = (self.is_editing(cx) || self.takes_typing(cx))
            .then(|| (self.focus_handle.clone(), cx.entity()));
        let handle_dragging = self.handle_drag();
        // The menu a right-click opens is worked out as it opens.
        let menu_editor = editor.clone();
        let menu_focus = self.focus_handle.clone();
        let overlay = canvas(
            // What the pointer shows over a handle shows only where nothing lies over the editor.
            |area, window, _| window.insert_hitbox(area, HitboxBehavior::Normal),
            move |area, hitbox, window, cx| {
                if let Some((focus, editor)) = &input {
                    window.handle_input(focus, ElementInputHandler::new(area, editor.clone()), cx);
                }
                // A drag goes on selecting wherever the mouse goes, off the slide and off the
                // editor too, until it's let go of anywhere.
                let moved = editor.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble {
                        moved
                            .update(cx, |editor, cx| editor.mouse_move(event, window, cx))
                            .ok();
                    }
                });
                window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble {
                        editor.update(cx, |editor, cx| editor.mouse_up(event, window, cx)).ok();
                    }
                });
                let slide = letterbox(area, window.scale_factor());
                let unit = f32::from(slide.size.width) / SLIDE_WIDTH;
                if unit <= 0.0 {
                    return;
                }
                window.with_content_mask(Some(ContentMask { bounds: slide }), |window| {
                    if let Some((_, corners)) = &frame {
                        paint_dashed(corners, slide, unit, new_box_color, window);
                    }
                    for corners in &outlines {
                        paint_outline(corners, slide, unit, outline_color, window);
                    }
                    if let Some(corners) = &lift_outline {
                        paint_dashed(corners, slide, unit, outline_color, window);
                    }
                    for guide in &guides {
                        window.paint_quad(fill(guide_bounds(guide, slide, unit), outline_color));
                    }
                    if let Some((x, y, w, h)) = new_box {
                        paint_dashed(&[(x, y), (x + w, y), (x + w, y + h), (x, y + h)], slide, unit, new_box_color, window);
                    }
                    let Some(marks) = marks else { return };
                    let SelectionRects {
                        rects,
                        rotate,
                        centre,
                    } = &marks.selection;
                    for rect in rects {
                        let corners = corners(rect.x, rect.y, rect.w, rect.h, *centre, *rotate);
                        paint_polygon(&corners, slide, unit, selection_color, window);
                    }
                    if let Some(caret) = marks.caret.filter(|_| caret_on) {
                        paint_polygon(
                            &caret_corners(&caret, unit),
                            slide,
                            unit,
                            caret_color,
                            window,
                        );
                    }
                });
                // Handles show past the slide's edge too, as the turn handle of a thing at its top.
                if let Some((spots, rotate)) = &handles {
                    let half = HANDLE_SIZE / 2. / unit;
                    for &(handle, (x, y)) in spots {
                        let points: Vec<(f32, f32)> = if handle == Handle::Turn {
                            (0..HANDLE_ROUND).map(|at| turned((x + half, y), (x, y), 360. * at as f32 / HANDLE_ROUND as f32)).collect()
                        } else {
                            corners(x - half, y - half, half * 2., half * 2., (x, y), *rotate).to_vec()
                        };
                        paint_polygon(&points, slide, unit, handle_color, window);
                        paint_outline(&points, slide, unit, outline_color, window);
                    }
                }
                // A handle dragged holds the pointer wherever it goes.
                match pointer {
                    Some(style) if handle_dragging => window.set_window_cursor_style(style),
                    Some(style) => window.set_cursor_style(style, &hitbox),
                    None => {}
                }
            },
        )
        .absolute()
        .size_full();
        let key_context = match (selecting, video.is_some()) {
            (true, true) => format!("{SELECTION_CONTEXT} {VIDEO_CONTEXT}"),
            (true, false) => SELECTION_CONTEXT.to_string(),
            (false, _) => EDITOR_CONTEXT.to_string(),
        };
        let (button_color, button_text) = (cx.theme().primary, cx.theme().primary_foreground);
        div()
            .id(SLIDE_EDITOR_ID)
            .test_support()
            .key_context(key_context.as_str())
            .track_focus(&self.focus_handle)
            .absolute()
            .size_full()
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            // Before the menu opens: it keeps the press from going further.
            .on_mouse_down(MouseButton::Right, cx.listener(Self::right_down))
            .on_action(cx.listener(|this, _: &MoveLeft, _, cx| {
                this.arrow(Go::By(Motion::Left), Beside::Before, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveRight, _, cx| {
                this.arrow(Go::By(Motion::Right), Beside::After, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveUp, _, cx| {
                this.arrow(Go::Line(LineMove::Up), Beside::Above, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveDown, _, cx| {
                this.arrow(Go::Line(LineMove::Down), Beside::Below, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveWordLeft, _, cx| {
                this.go(Go::By(Motion::WordLeft), false, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveWordRight, _, cx| {
                this.go(Go::By(Motion::WordRight), false, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveLineStart, _, cx| {
                this.go(Go::Line(LineMove::LineStart), false, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveLineEnd, _, cx| {
                this.go(Go::Line(LineMove::LineEnd), false, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveFlowStart, _, cx| {
                this.go(Go::By(Motion::FlowStart), false, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveFlowEnd, _, cx| {
                this.go(Go::By(Motion::FlowEnd), false, cx)
            }))
            .on_action(
                cx.listener(|this, _: &SelectLeft, _, cx| this.go(Go::By(Motion::Left), true, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SelectRight, _, cx| {
                    this.go(Go::By(Motion::Right), true, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &SelectUp, _, cx| this.go(Go::Line(LineMove::Up), true, cx)),
            )
            .on_action(cx.listener(|this, _: &SelectDown, _, cx| {
                this.go(Go::Line(LineMove::Down), true, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectWordLeft, _, cx| {
                this.go(Go::By(Motion::WordLeft), true, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectWordRight, _, cx| {
                this.go(Go::By(Motion::WordRight), true, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectLineStart, _, cx| {
                this.go(Go::Line(LineMove::LineStart), true, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectLineEnd, _, cx| {
                this.go(Go::Line(LineMove::LineEnd), true, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectFlowStart, _, cx| {
                this.go(Go::By(Motion::FlowStart), true, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectFlowEnd, _, cx| {
                this.go(Go::By(Motion::FlowEnd), true, cx)
            }))
            .on_action(
                cx.listener(|this, _: &SelectAll, _, cx| this.edit_with(TextEdit::select_all, cx)),
            )
            .on_action(
                cx.listener(|this, _: &Backspace, _, cx| this.edit_with(TextEdit::backspace, cx)),
            )
            .on_action(cx.listener(|this, _: &Delete, _, cx| this.edit_with(TextEdit::delete, cx)))
            .on_action(cx.listener(|this, _: &Enter, _, cx| this.enter(cx)))
            .on_action(cx.listener(|this, _: &LineBreak, _, cx| this.line_break(cx)))
            .on_action(cx.listener(|this, _: &Indent, _, cx| this.tab(false, cx)))
            .on_action(cx.listener(|this, _: &Outdent, _, cx| this.tab(true, cx)))
            .on_action(cx.listener(|this, _: &StopEditing, window, cx| this.stop_editing(window, cx)))
            .on_action(cx.listener(|this, _: &TakeAwaySelected, _, cx| this.take_away(cx)))
            .on_action(cx.listener(|this, _: &SelectNextThing, window, cx| {
                this.select_along(false, window, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectPreviousThing, window, cx| {
                this.select_along(true, window, cx)
            }))
            .on_action(cx.listener(|this, _: &LetGoOfSelected, _, cx| this.let_go(cx)))
            .on_action(cx.listener(|this, nudge: &Nudge, _, cx| this.nudge((nudge.dx, nudge.dy), cx)))
            .on_action(cx.listener(|this, _: &CopyToClipboard, _, cx| {
                this.copy_or_cut_typed(false, cx)
            }))
            .on_action(cx.listener(|this, _: &CutToClipboard, _, cx| {
                this.copy_or_cut_typed(true, cx)
            }))
            .on_action(cx.listener(|this, _: &DuplicateSelected, _, cx| this.duplicate(cx)))
            .on_action(cx.listener(|this, _: &SelectAllThings, window, cx| this.select_all_things(window, cx)))
            .on_action(cx.listener(|this, _: &PlayOrStopVideo, _, cx| this.play_or_stop(cx)))
            .on_action(cx.listener(|this, _: &ToggleBold, _, cx| {
                this.format(|edit| edit.toggle(StyleBit::Bold), cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleItalic, _, cx| {
                this.format(|edit| edit.toggle(StyleBit::Italic), cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleUnderline, _, cx| {
                this.format(|edit| edit.toggle(StyleBit::Underline), cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleBullets, _, cx| this.toggle_list(false, cx)))
            .on_action(cx.listener(|this, _: &ToggleNumbering, _, cx| this.toggle_list(true, cx)))
            .on_action(cx.listener(|this, action: &SetTextSize, _, cx| {
                this.set_text_size(action.size, cx)
            }))
            .child(self.canvas.clone())
            .child(overlay)
            .when_some(new_box_bounds, |this, bounds| {
                this.child(
                    div()
                        .id(NEW_BOX_ID)
                        .test_support()
                        .absolute()
                        .left(bounds.origin.x)
                        .top(bounds.origin.y)
                        .w(bounds.size.width)
                        .h(bounds.size.height),
                )
            })
            .children(frame_box.into_iter().chain(lift_box).chain(outline_boxes).chain(handle_boxes).chain(guide_boxes).map(|(id, bounds)| {
                div()
                    .id(id)
                    .test_support()
                    .absolute()
                    .left(bounds.origin.x)
                    .top(bounds.origin.y)
                    .w(bounds.size.width)
                    .h(bounds.size.height)
            }))
            .when_some(video_button, |this, middle| {
                let (id, label) = if video_playing { (VIDEO_STOP_ID, "Stop") } else { (VIDEO_PLAY_ID, "Play") };
                this.child(
                    div()
                        .absolute()
                        .left(middle.x)
                        .top(middle.y)
                        .child(
                            div()
                                .id(id)
                                .test_support()
                                .aria_label(label)
                                .absolute()
                                // Its middle at the video's.
                                .left(px(-32.))
                                .top(px(-16.))
                                .w(px(64.))
                                .h(px(32.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_md()
                                .bg(button_color)
                                .text_color(button_text)
                                .text_sm()
                                .cursor_pointer()
                                // The press is the button's, not the slide's beneath it.
                                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.play_or_stop(cx);
                                }))
                                .child(label),
                        ),
                )
            })
            .children(badges.into_iter().map(|(id, corner, words)| {
                div()
                    .id(id)
                    .test_support()
                    .aria_label(words.clone())
                    .absolute()
                    .left(corner.x)
                    .top(corner.y)
                    .px_1()
                    .rounded_sm()
                    .bg(badge_color)
                    .text_color(badge_text)
                    .text_xs()
                    .child(words)
            }))
            .when_some(caret_box, |this, caret| {
                this.child(
                    div()
                        .id(CARET_ID)
                        .test_support()
                        .absolute()
                        .left(caret.origin.x)
                        .top(caret.origin.y)
                        .w(caret.size.width)
                        .h(caret.size.height),
                )
            })
            .when_some(composition, |this, (text, at)| {
                let height = at.size.height;
                this.child(
                    div()
                        .id(COMPOSITION_ID)
                        .test_support()
                        .aria_label(text.clone())
                        .absolute()
                        .left(at.origin.x + at.size.width)
                        .top(at.origin.y)
                        .h(height)
                        .flex()
                        .items_center()
                        .bg(composition_background)
                        .text_color(composition_color)
                        .text_size(height * COMPOSITION_TEXT)
                        .line_height(height)
                        .underline()
                        .whitespace_nowrap()
                        .child(text),
                )
            })
            .context_menu(move |menu, _, cx| {
                let Some(editor) = menu_editor.upgrade() else {
                    return menu;
                };
                editor.read(cx).menu(menu.action_context(menu_focus.clone()), &menu_editor, cx)
            })
    }
}

impl SlideEditor {
    /// The menu a right-click opened, from `menu`, `editor` being this editor: for a thing, what
    /// can be done to it, and for the slide, or a thing no longer selected, Paste. Each item does
    /// what its key does, on the slide.
    fn menu(&self, menu: PopupMenu, editor: &WeakEntity<Self>, cx: &App) -> PopupMenu {
        let document = match (self.menu_on, &self.document) {
            (Some(MenuOn::Thing), Some(document)) if self.selection_here(cx).is_some() => document.read(cx),
            (Some(_), _) => return menu.menu("Paste", Box::new(PasteFromClipboard)),
            (None, _) => return menu,
        };
        let here = document.arrange_menu();
        // An area can't be cut, copied, duplicated or deleted.
        let areas = document.selection().is_some_and(|selection| selection.blocks().next().is_none());
        let item = |label: &'static str, action: Box<dyn Action>| thing_item(label, action, areas, editor.clone());
        let menu = menu
            .item(item("Cut", Box::new(CutToClipboard)))
            .item(item("Copy", Box::new(CopyToClipboard)))
            .menu("Paste", Box::new(PasteFromClipboard))
            .item(item("Duplicate", Box::new(DuplicateSelected)))
            .item(item("Delete", Box::new(TakeAwaySelected)))
            .separator();
        let menu = order_items(menu, &here);
        match here.order_refused {
            Some(why) => reason(menu.separator(), why),
            None => menu,
        }
    }
}

/// An item of a thing's menu named `label`, off with `disabled`, that does what `action` does on
/// the slide of `editor`, but only while something is selected there: the window's own Cut and
/// Copy act on the slide itself with nothing selected, and this menu never does.
fn thing_item(label: &'static str, action: Box<dyn Action>, disabled: bool, editor: WeakEntity<SlideEditor>) -> PopupMenuItem {
    PopupMenuItem::new(label).action(action.boxed_clone()).disabled(disabled).on_click(move |_, window, cx| {
        let Some(editor) = editor.upgrade() else {
            return;
        };
        if editor.read(cx).selection_here(cx).is_none() {
            return;
        }
        let focus = editor.read(cx).focus_handle.clone();
        focus.focus(window, cx);
        window.dispatch_action(action.boxed_clone(), cx);
    })
}
