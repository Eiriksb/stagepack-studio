//! The panel beside the canvas: what is selected, where it sits, and the
//! things to do with it. It holds no truth of its own. The page owns the
//! selection; every edit here goes out as an `InspectorEvent::Send` and the
//! page's `selection` message brings the result back.

use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use studio_protocol::{CommandBody, DragPos, SelectionInfo};

pub const EMPTY_TEXT: &str = "Click something on the slide to select it.";

#[derive(Debug, Clone, PartialEq)]
pub enum InspectorEvent {
    Send(CommandBody),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    X,
    Y,
    W,
    H,
    Rotate,
}

impl Field {
    const ALL: [Field; 5] = [Field::X, Field::Y, Field::W, Field::H, Field::Rotate];

    fn index(self) -> usize {
        Self::ALL.iter().position(|f| *f == self).unwrap_or(0)
    }

    fn id(self) -> &'static str {
        match self {
            Field::X => "pos-x",
            Field::Y => "pos-y",
            Field::W => "pos-w",
            Field::H => "pos-h",
            Field::Rotate => "pos-rotate",
        }
    }

    fn caption(self) -> &'static str {
        match self {
            Field::X => "X",
            Field::Y => "Y",
            Field::W => "W",
            Field::H => "H",
            Field::Rotate => "Rotate",
        }
    }

    /// What the field shows for `pos`: no trailing `.0`, empty for no height.
    fn text(self, pos: &DragPos) -> String {
        match self {
            Field::X => number(pos.x),
            Field::Y => number(pos.y),
            Field::W => number(pos.w),
            Field::H => pos.h.map(number).unwrap_or_default(),
            Field::Rotate => number(pos.rotate),
        }
    }
}

fn number(value: f64) -> String {
    if value == 0.0 {
        "0".into() // not "-0"
    } else {
        value.to_string()
    }
}

/// `pos` with `field` set from what was typed; `None` when the text isn't a
/// value for it. Only H may be empty (it means "as tall as the content").
fn with_field(pos: &DragPos, field: Field, text: &str) -> Option<DragPos> {
    let text = text.trim();
    let mut pos = pos.clone();
    if field == Field::H && text.is_empty() {
        pos.h = None;
        return Some(pos);
    }
    let value: f64 = text.parse().ok().filter(|value: &f64| value.is_finite())?;
    match field {
        Field::X => pos.x = value,
        Field::Y => pos.y = value,
        Field::W => pos.w = value,
        Field::H => pos.h = Some(value),
        Field::Rotate => pos.rotate = value,
    }
    Some(pos)
}

pub struct Inspector {
    selection: Option<SelectionInfo>,
    x: Entity<InputState>,
    y: Entity<InputState>,
    w: Entity<InputState>,
    h: Entity<InputState>,
    rotate: Entity<InputState>,
    /// Per field (in `Field::ALL` order): it holds typing not yet committed.
    uncommitted: [bool; 5],
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<InspectorEvent> for Inspector {}

impl Inspector {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut subscriptions = Vec::new();
        let mut state = |field: Field| {
            let state = cx.new(|cx| InputState::new(window, cx));
            subscriptions.push(cx.subscribe_in(
                &state,
                window,
                move |this, _, event: &InputEvent, window, cx| {
                    match event {
                        // `set_value` emits nothing, so this is typing.
                        InputEvent::Change => this.uncommitted[field.index()] = true,
                        InputEvent::PressEnter { .. } => this.commit(field, window, cx),
                        _ => {}
                    }
                },
            ));
            state
        };
        let (x, y, w, h, rotate) = (
            state(Field::X),
            state(Field::Y),
            state(Field::W),
            state(Field::H),
            state(Field::Rotate),
        );
        Self {
            selection: None,
            x,
            y,
            w,
            h,
            rotate,
            uncommitted: [false; 5],
            _subscriptions: subscriptions,
        }
    }

    pub fn selection(&self) -> Option<&SelectionInfo> {
        self.selection.as_ref()
    }

    fn input(&self, field: Field) -> &Entity<InputState> {
        match field {
            Field::X => &self.x,
            Field::Y => &self.y,
            Field::W => &self.w,
            Field::H => &self.h,
            Field::Rotate => &self.rotate,
        }
    }

    /// Shows `selection`. For the element already shown, a focused field holding
    /// typing not yet committed keeps its text, so the page's echo of an edit doesn't fight the
    /// person typing; the other fields follow the page.
    pub fn set_selection(
        &mut self,
        selection: Option<SelectionInfo>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let same_element = matches!(
            (&self.selection, &selection),
            (Some(old), Some(new)) if old.slide == new.slide && old.label == new.label
        );
        if let Some(pos) = selection.as_ref().and_then(|s| s.pos.as_ref()) {
            for field in Field::ALL {
                let input = self.input(field).clone();
                let typing = self.uncommitted[field.index()]
                    && input.read(cx).focus_handle(cx).is_focused(window);
                if same_element && typing {
                    continue;
                }
                self.uncommitted[field.index()] = false;
                let text = field.text(pos);
                input.update(cx, |input, cx| input.set_value(text, window, cx));
            }
        }
        self.selection = selection;
        cx.notify();
    }

    /// Enter in `field`: send the position with that field changed, or put
    /// the field's last known value back if the text isn't one.
    fn commit(&mut self, field: Field, window: &mut Window, cx: &mut Context<Self>) {
        let Some(current) = self.selection.as_ref().and_then(|s| s.pos.clone()) else {
            return;
        };
        self.uncommitted[field.index()] = false;
        let text = self.input(field).read(cx).value();
        match with_field(&current, field, &text) {
            Some(pos) => cx.emit(InspectorEvent::Send(CommandBody::SetPos { pos })),
            None => {
                let last = field.text(&current);
                self.input(field)
                    .update(cx, |input, cx| input.set_value(last, window, cx));
            }
        }
    }

    fn send(&mut self, body: CommandBody, cx: &mut Context<Self>) {
        cx.emit(InspectorEvent::Send(body));
    }

    fn action(
        &self,
        id: &'static str,
        label: &'static str,
        body: CommandBody,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> Button {
        Button::new(id)
            .small()
            .label(label)
            .disabled(disabled)
            .on_click(cx.listener(move |this, _, _, cx| this.send(body.clone(), cx)))
    }

    fn render_fields(&self) -> impl IntoElement {
        v_flex().gap_2().children(Field::ALL.map(|field| {
            h_flex()
                .gap_2()
                .items_center()
                .child(div().w(px(56.)).text_sm().child(field.caption()))
                .child(
                    Input::new(self.input(field))
                        .id(field.id())
                        .aria_label(field.caption())
                        .flex_1(),
                )
        }))
    }
}

impl Render for Inspector {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(selection) = self.selection.clone() else {
            return div()
                .size_full()
                .p_3()
                .child(
                    div()
                        .id("inspector-empty")
                        .test_support()
                        .aria_label(EMPTY_TEXT)
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(EMPTY_TEXT),
                )
                .into_any_element();
        };

        let flow_button = if selection.positioned {
            self.action(
                "return-to-flow",
                "Return to flow",
                CommandBody::ReturnToFlow,
                false,
                cx,
            )
        } else {
            self.action(
                "free-position",
                "Free position",
                CommandBody::FreePosition,
                selection.nested,
                cx,
            )
        };

        v_flex()
            .size_full()
            .p_3()
            .gap_3()
            .child(
                div()
                    .id("inspector-label")
                    .test_support()
                    .aria_label(selection.label.clone())
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(selection.label.clone()),
            )
            .when(selection.positioned, |this| {
                this.child(self.render_fields())
            })
            .child(flow_button)
            .child(
                h_flex()
                    .gap_2()
                    .child(self.action("duplicate", "Duplicate", CommandBody::Duplicate, false, cx))
                    .child(self.action("delete", "Delete", CommandBody::Delete, false, cx)),
            )
            .into_any_element()
    }
}
