//! Working out the form: the thing selected, its component's arguments, the fields made from
//! them, and keeping them showing what the talk says.

use super::*;

impl Inspector {
    /// What the panel shows for the thing selected on the slide shown, if one is, or else the
    /// table the caret is in a cell of: the form's shape, the thing, and its tag, if it's one.
    pub(super) fn subject(&mut self, cx: &mut Context<Self>) -> Option<(Shape, Selected, Option<Tag>)> {
        let document = self.document.clone()?;
        let (selected, pick, block, heading) = {
            let document = document.read(cx);
            let in_cell = || {
                let here = document.table_here().filter(|here| here.caret)?;
                Some(Selected { slide: here.slide, source: here.table })
            };
            let selected = document.selected().cloned().or_else(in_cell).filter(|selected| selected.slide == document.current())?;
            let talk = document.talk();
            let block = selection::block_at(&talk.slides.get(selected.slide)?.blocks, &selected.source)?.clone();
            let heading = selection::name_of(talk, selected.slide, &selected.source)?;
            (selected, document.selection_pick(), block, heading)
        };
        let mut shape = Shape { pick, tag: None, heading, message: None, specs: Vec::new() };
        let tag = match block {
            Block::Picture { src, .. } => {
                shape.message = Some(file_name(&src).to_string());
                None
            }
            Block::Table { rows, .. } => {
                shape.message = Some(table_size(&rows));
                None
            }
            Block::Tag(tag) => {
                // Text boxes are the format's own, with no component.
                if tag.name != "Text" {
                    match self.schema(&document, pick, &tag.name, cx) {
                        Some(schema) => {
                            let ordered = labels::ordered(&tag.name, &schema.props);
                            shape.specs.extend(ordered.iter().map(|(prop, label)| Spec::of(&tag.name, prop, label.clone())));
                        }
                        None => {
                            let own = document.read(cx).components().iter().any(|info| info.tag == tag.name);
                            shape.message = Some(if own { NEEDS_BUILD } else { NO_SUCH_COMPONENT }.to_string());
                        }
                    }
                }
                if shape.message.is_none() && is_positioned(&tag) {
                    shape.specs.extend(Spec::place());
                }
                shape.tag = Some(tag.name.clone());
                Some(tag)
            }
            _ => return None,
        };
        Some((shape, selected, tag))
    }

    /// The schema of `tag`, the tag of the thing picked `pick`, looked up again only once
    /// something about it could have changed.
    fn schema(&mut self, document: &Entity<Document>, pick: u64, tag: &str, cx: &mut Context<Self>) -> Option<ComponentSchema> {
        let components = document.read(cx).components().to_vec();
        if let Some(looked_up) = &self.looked_up
            && looked_up.pick == pick
            && looked_up.tag == tag
            && looked_up.components == components
        {
            return looked_up.schema.clone();
        }
        let schema = document.update(cx, |document, _| document.schema_of_selected());
        self.looked_up = Some(LookedUp { pick, tag: tag.to_string(), components, schema: schema.clone() });
        schema
    }

    /// The form for `shape`: its fields, and what they hear from.
    pub(super) fn build_form(&mut self, shape: Shape, target: Selected, window: &mut Window, cx: &mut Context<Self>) -> Form {
        let mut subscriptions = Vec::new();
        let mut fields = Vec::new();
        let tag = shape.tag.clone().unwrap_or_default();
        for (index, spec) in shape.specs.iter().enumerate() {
            let widget = match &spec.kind {
                Kind::Text | Kind::Number { .. } => {
                    // A number field takes whatever's typed, a comma for the decimal mark too, and
                    // says what it can't take once it's committed.
                    let input = cx.new(|cx| InputState::new(window, cx).mask_pattern(MaskPattern::None));
                    subscriptions.push(cx.subscribe_in(&input, window, move |this, _, event: &InputEvent, window, cx| {
                        match event {
                            InputEvent::PressEnter { .. } => this.commit(index, true, window, cx),
                            InputEvent::Blur => this.commit(index, false, window, cx),
                            // Typing in it again takes back what was said of it.
                            InputEvent::Change => this.clear_problem(index, cx),
                            _ => {}
                        }
                    }));
                    if matches!(spec.kind, Kind::Number { .. }) {
                        subscriptions.push(cx.subscribe_in(&input, window, move |this, _, event: &NumberInputEvent, window, cx| {
                            let NumberInputEvent::Step(action) = event;
                            this.step(index, *action == StepAction::Increment, window, cx);
                        }));
                    }
                    Widget::Input(input)
                }
                Kind::Flag => Widget::Flag,
                Kind::Color => {
                    let picker = cx.new(|cx| ColorPickerState::new(window, cx));
                    let attr = spec.attr.clone();
                    let optional = spec.optional;
                    subscriptions.push(cx.subscribe_in(&picker, window, move |this, picker, event: &ColorPickerEvent, _, cx| {
                        let ColorPickerEvent::Change(color) = event;
                        match color {
                            // A drag of the sliders is one change, given once the picker closes.
                            Some(color) if picker.read(cx).is_open() => this.hold_color(&attr, Some(written_color(*color)), cx),
                            Some(color) => {
                                this.hold_color(&attr, None, cx);
                                this.write_color(&attr, &written_color(*color), cx);
                            }
                            None if optional => {
                                this.write(&attr, None, cx).ok();
                            }
                            None => {}
                        }
                        cx.notify();
                    }));
                    let attr = spec.attr.clone();
                    subscriptions.push(cx.observe(&picker, move |this, picker, cx| {
                        if !picker.read(cx).is_open() {
                            this.give_held_color(&attr, cx);
                        }
                    }));
                    Widget::Color { picker, held: None }
                }
                Kind::Choice(options) => {
                    let choices: Vec<Choice> = options
                        .iter()
                        .map(|option| Choice { value: option.clone(), title: labels::choice(&tag, &spec.attr, option).into() })
                        .collect();
                    let items = choices.clone();
                    let select = cx.new(|cx| SelectState::new(items, None, window, cx));
                    let attr = spec.attr.clone();
                    subscriptions.push(cx.subscribe_in(&select, window, move |this, _, event: &SelectEvent<Vec<Choice>>, window, cx| {
                        let SelectEvent::Confirm(Some(value)) = event else {
                            return;
                        };
                        this.choose(&attr, value, window, cx);
                    }));
                    Widget::Choice { select, choices }
                }
                Kind::File(_) => Widget::File,
                Kind::Icon => {
                    let picker = cx.new(|cx| IconPicker::new(window, cx));
                    let attr = spec.attr.clone();
                    subscriptions.push(cx.subscribe(&picker, move |this, _, picked: &IconPicked, cx| {
                        this.write(&attr, Some(picked.0), cx).ok();
                        cx.notify();
                    }));
                    // It opens and closes in the panel's drawing.
                    subscriptions.push(cx.observe(&picker, |_, _, cx| cx.notify()));
                    Widget::Icon(picker)
                }
            };
            fields.push(Field { spec: spec.clone(), widget, shown: String::new(), problem: None });
        }
        Form { shape, target, fields, _subscriptions: subscriptions }
    }

    /// Shows in every field what `tag` has, but in a text or number field that has the keyboard.
    pub(super) fn follow_talk(&mut self, tag: Option<&Tag>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &mut self.form else {
            return;
        };
        for field in &mut form.fields {
            let value = tag.and_then(|tag| tag.attr(&field.spec.attr));
            match &field.widget {
                Widget::Input(input) => {
                    // What was typed and refused stays, with why, until it's typed in again or
                    // Escape puts the talk's value back; what was typed and not yet committed,
                    // as the keyboard leaves the field, stays for the commit.
                    let typed = input.read(cx).value().as_ref() != field.shown;
                    if field.problem.is_some() || typed || input.read(cx).focus_handle(cx).is_focused(window) {
                        continue;
                    }
                    let wanted = value.unwrap_or_default();
                    if input.read(cx).value().as_ref() != wanted {
                        let wanted = SharedString::from(wanted.to_string());
                        input.update(cx, |input, cx| input.set_value(wanted, window, cx));
                    }
                    field.shown = wanted.to_string();
                    field.problem = None;
                }
                Widget::Color { picker, held } => {
                    if picker.read(cx).is_open() || held.is_some() {
                        continue;
                    }
                    let wanted = value.and_then(color_of);
                    if picker.read(cx).value().map(written_color) != wanted.map(written_color) {
                        picker.update(cx, |picker, cx| match wanted {
                            Some(wanted) => picker.set_value(wanted, window, cx),
                            None => picker.clear_value(window, cx),
                        });
                    }
                }
                Widget::Choice { select, choices } => {
                    // An unknown value of a choice that has to be given draws as its first.
                    let at = value
                        .and_then(|value| choices.iter().position(|choice| choice.value == value))
                        .or_else(|| (!field.spec.optional && !choices.is_empty()).then_some(0))
                        .map(IndexPath::new);
                    if select.read(cx).selected_index(cx) != at {
                        select.update(cx, |select, cx| select.set_selected_index(at, window, cx));
                    }
                }
                Widget::Flag | Widget::File | Widget::Icon(_) => {}
            }
        }
    }
}

/// How big a table with `rows` is, as the Insert tab's grid counts: the heading row's columns,
/// and the rows under it, such as "3 columns, 4 rows", or "3 columns, no rows" for a heading
/// row alone.
fn table_size(rows: &[Vec<Vec<Span>>]) -> String {
    let columns = rows.first().map_or(0, Vec::len);
    let counted = |count: usize, one: &str, many: &str| match count {
        0 => format!("no {many}"),
        1 => format!("1 {one}"),
        count => format!("{count} {many}"),
    };
    format!("{}, {}", counted(columns, "column", "columns"), counted(rows.len().saturating_sub(1), "row", "rows"))
}
