//! Drawing one field of the form, as the kind of field it is.

use super::*;

impl Inspector {
    /// Field `index` as it's drawn: its label, what shows its value, and any problem under it.
    pub(super) fn field_element(&self, index: usize, field: &Field, values: &Values, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let attr = field.spec.attr.clone();
        let id = SharedString::from(field_id(&attr));
        let value = values.get(&attr).map(str::to_string);
        let label = SharedString::from(field.spec.label.clone());
        let control: AnyElement = match &field.widget {
            Widget::Input(input) => {
                let control: AnyElement = match field.spec.kind {
                    Kind::Number { .. } => div()
                        .id(id.clone())
                        .test_support()
                        .w_full()
                        .capture_action(cx.listener(move |this, _: &input::MoveUp, window, cx| {
                            cx.stop_propagation();
                            this.step(index, true, window, cx);
                        }))
                        .capture_action(cx.listener(move |this, _: &input::MoveDown, window, cx| {
                            cx.stop_propagation();
                            this.step(index, false, window, cx);
                        }))
                        .child(NumberInput::new(input).small().accessibility_label(label.clone()))
                        .into_any_element(),
                    _ => Input::new(input).id(id.clone()).aria_label(label.clone()).small().into_any_element(),
                };
                div()
                    .w_full()
                    // Escape leaves the field as the talk has it.
                    .capture_action(cx.listener(move |this, _: &input::Escape, window, cx| {
                        cx.stop_propagation();
                        this.put_back(index, window, cx);
                    }))
                    .child(control)
                    .into_any_element()
            }
            Widget::Flag => {
                let checked = value.as_deref() == Some("true");
                Checkbox::new(id.clone())
                    .checked(checked)
                    .accessibility_label(label.clone())
                    .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                        let value = if *checked { "true" } else { "false" };
                        this.write(&attr, Some(value), cx).ok();
                        cx.notify();
                    }))
                    .into_any_element()
            }
            Widget::Color { picker, .. } => {
                let none = field.spec.optional.then(|| {
                    let attr = attr.clone();
                    Button::new(SharedString::from(none_id(&attr)))
                        .ghost()
                        .xsmall()
                        .label("None")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.write(&attr, None, cx).ok();
                            cx.notify();
                        }))
                });
                h_flex()
                    .id(id.clone())
                    .test_support()
                    .gap_2()
                    .items_center()
                    .child(
                        ColorPicker::new(picker)
                            .featured_colors(self.featured_colors(cx))
                            .accessibility_label(label.clone())
                            .small(),
                    )
                    .children(none)
                    .into_any_element()
            }
            Widget::Choice { select, .. } => Select::new(select)
                .id(id.clone())
                .accessibility_label(label.clone())
                .small()
                .into_any_element(),
            Widget::File => {
                let name = value.as_deref().map(file_name).unwrap_or_default().to_string();
                h_flex()
                    .id(id.clone())
                    .test_support()
                    .gap_2()
                    .items_center()
                    .child(
                        Button::new(INSPECTOR_CHOOSE_FILE_ID)
                            .small()
                            .label("Choose file…")
                            .on_click(cx.listener(move |this, _, _, cx| this.ask_for_file(attr.clone(), cx))),
                    )
                    .child(div().min_w_0().text_sm().truncate().child(name))
                    .into_any_element()
            }
            Widget::Icon(picker) => icon_field(id.clone(), label.clone(), value.unwrap_or_default(), picker, window, cx),
        };
        v_flex()
            .w_full()
            .gap_1()
            .child(div().text_xs().text_color(cx.theme().muted_foreground).child(label))
            .child(control)
            .when_some(field.problem.clone(), |this, problem| {
                this.child(
                    div()
                        .id(SharedString::from(problem_id(&field.spec.attr)))
                        .test_support()
                        .aria_label(problem.clone())
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .child(problem),
                )
            })
            .into_any_element()
    }
}

/// An icon field, `id`, labelled `label`: a button showing icon `name` and its name, which opens
/// `picker` to choose another.
fn icon_field(
    id: SharedString,
    label: SharedString,
    name: String,
    picker: &Entity<IconPicker>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let image = picker.update(cx, |picker, cx| {
        picker.release(window);
        picker.image_of(&name, window, cx)
    });
    let focus = picker.read(cx).search_focus(cx);
    let (opened, shown) = (picker.clone(), picker.clone());
    let button = Button::new(id)
        .small()
        .w_full()
        .accessibility_label(SharedString::from(name.clone()))
        .tooltip(label)
        .child(
            h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .when_some(image, |this, image| this.child(img(image).flex_none().size(px(20.))))
                .child(div().min_w_0().truncate().child(SharedString::from(name.clone()))),
        );
    Popover::new("inspector-icon-picker")
        .open(picker.read(cx).is_open())
        .on_open_change(move |open, window, cx| opened.update(cx, |picker, cx| picker.set_open(*open, window, cx)))
        .track_focus(&focus)
        .trigger_style(StyleRefinement::default().w_full())
        .trigger(button)
        .content(move |_, _, _| shown.clone())
        .into_any_element()
}
