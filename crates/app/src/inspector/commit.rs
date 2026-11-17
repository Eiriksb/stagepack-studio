//! Writing what's in a field to the talk: committing it, putting the talk's value back,
//! stepping a number, and moving the keyboard between fields.

use super::*;

impl Inspector {
    /// Sets attribute `name` of the thing the form is for to `value`, or takes it away, or says
    /// why it wasn't.
    pub(super) fn write(&mut self, name: &str, value: Option<&str>, cx: &mut Context<Self>) -> Result<(), EditRefused> {
        let Some(form) = &self.form else {
            return Err(EditRefused::CouldNotBeMade);
        };
        let (pick, target) = (form.shape.pick, form.target.clone());
        // An icon's size is its width and height too, where it has them of its own: they're
        // set to it in the same step, or they'd hold it at the size it was.
        let mut values = vec![(name, value)];
        if name == "size" && form.shape.tag.as_deref() == Some("Icon") && value.is_some() {
            values.extend(["w", "h"].into_iter().filter(|side| self.talk_attr(side, cx).is_some()).map(|side| (side, value)));
        }
        let made = match &target {
            Subject::Thing(target) => self
                .workspace
                .update(cx, |workspace, cx| workspace.set_attr_from_inspector(pick, target, &values, cx))
                .unwrap_or(Err(EditRefused::CouldNotBeMade)),
            Subject::Area { slide, name: area, .. } => self
                .workspace
                .update(cx, |workspace, cx| workspace.set_region_value_from_inspector(*slide, area, name, value, cx))
                .unwrap_or(Err(EditRefused::CouldNotBeMade)),
            Subject::Several => Err(EditRefused::CouldNotBeMade),
        };
        // The thing's text has grown or shrunk: the form follows it there at once.
        let moved = self.document.as_ref().and_then(|document| {
            let document = document.read(cx);
            document.selected().filter(|_| document.selection_pick() == pick).cloned()
        });
        if let (Some(form), Some(moved)) = (&mut self.form, moved)
            && matches!(form.target, Subject::Thing(_))
        {
            form.target = Subject::Thing(moved);
        }
        made
    }

    /// Puts the area the form is for back where its layout has it.
    pub(super) fn put_area_back(&mut self, cx: &mut Context<Self>) {
        let Some(Subject::Area { slide, name, .. }) = self.form.as_ref().map(|form| form.target.clone()) else {
            return;
        };
        self.workspace.update(cx, |workspace, cx| workspace.put_back_region_from_inspector(slide, &name, cx)).ok();
        cx.notify();
    }

    /// Writes what text or number field `index` holds, if something's been typed in it. What was
    /// refused and not typed in since is tried again only by Enter, `again`: leaving the field
    /// leaves it as it is, with why.
    pub(super) fn commit(&mut self, index: usize, again: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(field) = self.form.as_ref().and_then(|form| form.fields.get(index)) else {
            return;
        };
        if field.problem.is_some() && !again {
            return;
        }
        let Widget::Input(input) = &field.widget else {
            return;
        };
        let input = input.clone();
        let text = input.read(cx).value().to_string();
        if text == field.shown {
            self.form.as_mut().expect("a form").fields[index].problem = None;
            cx.notify();
            return;
        }
        let optional = field.spec.optional;
        let typed = match &field.spec.kind {
            Kind::Number { whole, range } => number_typed(&text, *whole, *range, optional),
            _ => text_typed(&text, optional),
        };
        let attr = field.spec.attr.clone();
        match typed {
            Typed::Write(value) => match self.write(&attr, value.as_deref(), cx) {
                Ok(()) => {
                    let shown = value.unwrap_or_default();
                    if text != shown {
                        input.update(cx, |input, cx| input.set_value(SharedString::from(shown.clone()), window, cx));
                    }
                    if let Some(field) = self.field_mut(&attr) {
                        field.shown = shown;
                        field.problem = None;
                    }
                }
                // Refused as a value that can't be written is: it stays, with why.
                Err(refused) => {
                    if let Some(field) = self.field_mut(&attr) {
                        field.problem = Some(refused.to_string().into());
                    }
                }
            },
            Typed::Refused(why) => {
                if let Some(field) = self.field_mut(&attr) {
                    field.problem = Some(why.into());
                }
            }
            Typed::PutBack => self.put_back(index, window, cx),
        }
        cx.notify();
    }

    /// Commits the text or number field that has the keyboard, if one does.
    pub(super) fn commit_typing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed_in = self.form.iter().flat_map(|form| form.fields.iter().enumerate()).find_map(|(index, field)| match &field.widget {
            Widget::Input(input) if input.read(cx).focus_handle(cx).is_focused(window) => Some(index),
            _ => None,
        });
        if let Some(index) = typed_in {
            self.commit(index, false, window, cx);
        }
    }

    /// Shows in text or number field `index` what the talk has again, and forgets what was typed.
    pub(super) fn put_back(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(field) = self.form.as_ref().and_then(|form| form.fields.get(index)) else {
            return;
        };
        let Widget::Input(input) = &field.widget else {
            return;
        };
        let input = input.clone();
        let value = self.talk_attr(&field.spec.attr, cx).unwrap_or_default();
        input.update(cx, |input, cx| input.set_value(SharedString::from(value.clone()), window, cx));
        let field = &mut self.form.as_mut().expect("a form").fields[index];
        field.shown = value;
        field.problem = None;
        cx.notify();
    }

    /// Steps number field `index` one up or down, and writes it.
    pub(super) fn step(&mut self, index: usize, up: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(field) = self.form.as_ref().and_then(|form| form.fields.get(index)) else {
            return;
        };
        let Widget::Input(input) = &field.widget else {
            return;
        };
        let (Kind::Number { whole, range }, attr) = (&field.spec.kind, field.spec.attr.clone()) else {
            return;
        };
        let (whole, range, input) = (*whole, *range, input.clone());
        let talk = self.talk_attr(&attr, cx).unwrap_or_default();
        match stepped(&input.read(cx).value(), &talk, up, whole, range) {
            Ok(next) => {
                input.update(cx, |input, cx| input.set_value(SharedString::from(next), window, cx));
                self.commit(index, true, window, cx);
            }
            // What's in the field stays, and nothing is written.
            Err(why) => {
                if let Some(field) = self.field_mut(&attr) {
                    field.problem = Some(why.into());
                }
                cx.notify();
            }
        }
    }

    /// Takes back what was said of field `index`, if anything was.
    pub(super) fn clear_problem(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(field) = self.form.as_mut().and_then(|form| form.fields.get_mut(index))
            && field.problem.take().is_some()
        {
            cx.notify();
        }
    }

    /// Lets go of the form, as it's no longer for the thing selected. A colour still held in
    /// an open picker is written to the thing it was dragged for, once the drawing is done.
    pub(super) fn drop_form(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.form.take() else {
            return;
        };
        for field in &form.fields {
            if let Widget::Icon(picker) = &field.widget {
                let images = picker.update(cx, |picker, _| picker.take_images());
                self.retired.extend(images);
            }
        }
        let held: Vec<(String, String)> = form
            .fields
            .into_iter()
            .filter_map(|field| match field.widget {
                Widget::Color { held: Some(held), .. } if self.is_current(held.text, cx) => Some((field.spec.attr, held.color)),
                _ => None,
            })
            .collect();
        if held.is_empty() {
            return;
        }
        let (pick, Subject::Thing(target), workspace) = (form.shape.pick, form.target, self.workspace.clone()) else {
            return;
        };
        cx.defer(move |cx| {
            for (name, color) in held {
                workspace
                    .update(cx, |workspace, cx| workspace.set_attr_from_inspector(pick, &target, &[(&name, Some(&color))], cx))
                    .ok();
            }
        });
    }

    /// Gives the next text or number field the keyboard, or the one before with `back`, from
    /// the one that has it. Says whether one had it.
    pub(super) fn focus_along(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let inputs: Vec<Entity<InputState>> = self
            .form
            .iter()
            .flat_map(|form| &form.fields)
            .filter_map(|field| match &field.widget {
                Widget::Input(input) => Some(input.clone()),
                _ => None,
            })
            .collect();
        let Some(at) = inputs.iter().position(|input| input.read(cx).focus_handle(cx).is_focused(window)) else {
            return false;
        };
        let next = if back { (at + inputs.len() - 1) % inputs.len() } else { (at + 1) % inputs.len() };
        // What's typed is written, or refused, before the keyboard goes on.
        self.commit_typing(window, cx);
        inputs[next].update(cx, |input, cx| input.focus(window, cx));
        true
    }

    /// Asks which file to put in file field `name`, and puts it there once it's chosen, if the
    /// form is still for the same thing by then.
    pub(super) fn ask_for_file(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(pick) = self.form.as_ref().map(|form| form.shape.pick) else {
            return;
        };
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = chosen.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            this.update(cx, |this, cx| {
                if this.form.as_ref().is_some_and(|form| form.shape.pick == pick) {
                    this.chosen_file(&name, &path, cx);
                }
            })
            .ok();
        })
        .detach();
    }
}
