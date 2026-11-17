//! Drawing the editor: the slide, the marks over it, and the keys and mouse it takes.

use super::*;

impl Render for SlideEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = cx.entity().downgrade();
        let marks = self.marks(cx);
        let caret_color = cx.theme().caret;
        let selection_color = cx.theme().selection.opacity(0.6);
        let (composition_color, composition_background) =
            (cx.theme().foreground, cx.theme().background);
        let caret_on = self.caret_on;
        let outline_color = cx.theme().ring;
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
        let new_box_color = cx.theme().muted_foreground;
        // The area whose text is being typed: a dashed line around it, a press on which picks it.
        let frame = self.typed_region(cx).and_then(|(slide, name)| {
            let area = self.layout(slide, cx)?.region(&name)?.clone();
            Some((name, outline_corners(area.rect, area.rotate)))
        });
        let selecting = !outlines.is_empty() && !self.is_editing(cx);
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
        let overlay = canvas(
            |_, _, _| {},
            move |area, _, window, cx| {
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
            },
        )
        .absolute()
        .size_full();
        div()
            .id(SLIDE_EDITOR_ID)
            .test_support()
            .key_context(if selecting { SELECTION_CONTEXT } else { EDITOR_CONTEXT })
            .track_focus(&self.focus_handle)
            .absolute()
            .size_full()
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
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
            .children(frame_box.into_iter().chain(outline_boxes).map(|(id, bounds)| {
                div()
                    .id(id)
                    .test_support()
                    .absolute()
                    .left(bounds.origin.x)
                    .top(bounds.origin.y)
                    .w(bounds.size.width)
                    .h(bounds.size.height)
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
    }
}
