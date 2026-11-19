//! Drawing the panel.

use super::*;

impl Render for Inspector {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let subject = self.subject(cx);
        let values = match subject {
            Some((shape, target, values)) => {
                match &mut self.form {
                    Some(form) if form.shape == shape => form.target = target,
                    _ => {
                        self.drop_form(cx);
                        self.form = Some(self.build_form(shape, target, window, cx));
                    }
                }
                values
            }
            None => {
                self.drop_form(cx);
                Values::default()
            }
        };
        self.follow_talk(&values, window, cx);
        let (facts, video_problem) = self.video_said(cx);
        for image in self.retired.drain(..) {
            window.drop_image(image).ok();
        }
        let panel = v_flex()
            .id(INSPECTOR_ID)
            .test_support()
            .key_context(INSPECTOR_CONTEXT)
            .w(INSPECTOR_WIDTH)
            .h_full()
            .flex_none()
            .overflow_y_scroll()
            .p_3()
            .gap_3()
            .border_l_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            // What's typed in a field is written before a click anywhere else does anything.
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.commit_typing(window, cx)))
            // Tab goes from field to field.
            .capture_action(cx.listener(|this, _: &input::IndentInline, window, cx| {
                if this.focus_along(false, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &input::OutdentInline, window, cx| {
                if this.focus_along(true, window, cx) {
                    cx.stop_propagation();
                }
            }));
        let Some(form) = &self.form else {
            return panel.child(
                div()
                    .id(INSPECTOR_MESSAGE_ID)
                    .test_support()
                    .aria_label(NOTHING_SELECTED)
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(NOTHING_SELECTED),
            );
        };
        let heading = SharedString::from(form.shape.heading.clone());
        let message = form.shape.message.clone().map(SharedString::from);
        // A table's rows and columns, while it's selected or the caret is in one of its cells.
        let table = self.document.as_ref().and_then(|document| {
            let document = document.read(cx);
            document.table_here().filter(|here| here.slide == document.current())
        });
        let table = table.map(|here| table_tools::inspector_group(&here, cx));
        let mut settings = Vec::new();
        let mut place = Vec::new();
        for (index, field) in form.fields.iter().enumerate() {
            let element = self.field_element(index, field, &values, window, cx);
            if field.spec.place { place.push(element) } else { settings.push(element) }
        }
        let muted = cx.theme().muted_foreground;
        // An area moved from where its layout has it can be put back.
        let put_back = match &form.target {
            Subject::Area { moved, .. } => Some(
                Button::new(INSPECTOR_PUT_BACK_ID)
                    .small()
                    .label("Put back")
                    .disabled(!moved)
                    .on_click(cx.listener(|this, _, _, cx| this.put_area_back(cx))),
            ),
            _ => None,
        };
        panel
            .child(
                div()
                    .id(INSPECTOR_HEADING_ID)
                    .test_support()
                    .aria_label(heading.clone())
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(heading),
            )
            .when_some(message, |this, message| {
                this.child(
                    div()
                        .id(INSPECTOR_MESSAGE_ID)
                        .test_support()
                        .aria_label(message.clone())
                        .text_sm()
                        .text_color(muted)
                        .child(message),
                )
            })
            .when_some(facts.map(SharedString::from), |this, facts| {
                this.child(
                    div()
                        .id(INSPECTOR_VIDEO_FACTS_ID)
                        .test_support()
                        .aria_label(facts.clone())
                        .text_sm()
                        .text_color(muted)
                        .child(facts),
                )
            })
            .when_some(video_problem.map(SharedString::from), |this, problem| {
                this.child(
                    div()
                        .id(INSPECTOR_VIDEO_PROBLEM_ID)
                        .test_support()
                        .aria_label(problem.clone())
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .child(problem),
                )
            })
            .children(table)
            .children(settings)
            .when(!place.is_empty(), |this| {
                this.child(div().pt_2().text_xs().font_weight(FontWeight::SEMIBOLD).child("Place"))
                    .children(place)
            })
            .children(put_back)
    }
}
