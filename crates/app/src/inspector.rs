//! The inspector: a form at the window's right for the thing selected on the slide, made from
//! its component's arguments, so that someone who doesn't write code can set it up. It's always
//! there while a talk is open, so the slide never moves as things are selected. Each field
//! shows what the talk says, and writes one attribute when it's changed.
//!
//! The form is worked out as the panel is drawn, from the talk and the thing selected; nothing
//! runs while the window waits. A field being typed in keeps what's typed until it's committed:
//! with Enter, by leaving it, or by a click anywhere outside the panel, which commits it before
//! the click does anything else, so a change never goes to the wrong thing.

mod fields;
mod labels;

use std::path::Path;
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::input::{
    self, Input, InputEvent, InputState, MaskPattern, NumberInput, NumberInputEvent, StepAction,
};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::{ActiveTheme as _, IndexPath, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use slides_engine::{ComponentInfo, is_positioned, talk_theme};
use slides_format::{Block, Span, Tag, find_tag};
use toolkit::ComponentSchema;

use crate::added_files::{FileKind, NotAdded};
use crate::document::{Document, EditRefused, Selected};
use crate::icon_picker::{IconPicked, IconPicker};
use crate::selection;
use crate::table_tools;
use crate::text_tools::written_color;
use crate::workspace_view::WorkspaceView;
use fields::{Choice, Kind, Spec, Typed, file_name, number_typed, stepped, text_typed};

pub use fields::{BOTH_QUOTES, NOT_A_NUMBER, NOT_WHOLE};

pub const INSPECTOR_ID: &str = "inspector";
/// What names the thing selected, at the top of the panel.
pub const INSPECTOR_HEADING_ID: &str = "inspector-heading";
/// What the panel says in place of fields, when it has none to show.
pub const INSPECTOR_MESSAGE_ID: &str = "inspector-message";
/// The button of a file field, inside the field.
pub const INSPECTOR_CHOOSE_FILE_ID: &str = "inspector-choose-file";

/// How wide the panel is.
pub const INSPECTOR_WIDTH: Pixels = px(300.);

/// The key context of the panel.
pub const INSPECTOR_CONTEXT: &str = "Inspector";

/// What the panel says with nothing selected.
pub const NOTHING_SELECTED: &str = "Select something on the slide to set it up.";

/// What the panel says of a talk's own component that has no working build.
pub const NEEDS_BUILD: &str = "Build this talk's components to set this up.";
/// What the panel says of a tag that's no component the talk has.
pub const NO_SUCH_COMPONENT: &str = "This talk has no component of that name.";

/// The id of the field for attribute `name`.
pub fn field_id(name: &str) -> String {
    format!("inspector-field-{name}")
}

/// The id of what's said under the field for attribute `name` when it can't be written.
pub fn problem_id(name: &str) -> String {
    format!("inspector-problem-{name}")
}

/// The id of the button that takes away the optional colour `name`.
pub fn none_id(name: &str) -> String {
    format!("inspector-none-{name}")
}

/// The panel's keys. While a field has the keyboard, the window's own keys for slides are off.
pub fn bind_keys(cx: &mut App) {
    let context = Some(INSPECTOR_CONTEXT);
    cx.bind_keys(
        ["left", "right", "up", "down", "pageup", "pagedown", "ctrl-m", "ctrl-d"]
            .map(|key| KeyBinding::new(key, NoAction, context)),
    );
}

/// What the form is: for which pick of a thing, what it's called, what it says in place of
/// fields, and its fields. A form stays as long as this does, and only its values change.
#[derive(Clone, PartialEq)]
struct Shape {
    pick: u64,
    tag: Option<String>,
    heading: String,
    message: Option<String>,
    specs: Vec<Spec>,
}

/// What shows a field's value.
enum Widget {
    /// A text or number field.
    Input(Entity<InputState>),
    Flag,
    Color {
        picker: Entity<ColorPickerState>,
        /// A colour dragged on the sliders, given once the picker closes.
        held: Option<Held>,
    },
    Choice {
        select: Entity<SelectState<Vec<Choice>>>,
        choices: Vec<Choice>,
    },
    File,
    /// The icon picker that chooses which icon it is.
    Icon(Entity<IconPicker>),
}

/// A colour dragged on a picker's sliders, and the talk's text it was dragged on, by its hash:
/// once the text has changed since, by an undo, a redo or a delete, it's let go of unwritten.
struct Held {
    color: String,
    text: u64,
}

struct Field {
    spec: Spec,
    widget: Widget,
    /// What a text or number field was last given: what it holds until something's typed.
    shown: String,
    /// Why what's in it wasn't written, while it says so.
    problem: Option<SharedString>,
}

struct Form {
    shape: Shape,
    /// The thing the form is for, where it is in the talk now.
    target: Selected,
    fields: Vec<Field>,
    _subscriptions: Vec<Subscription>,
}

/// What the schema of the thing selected was found to be, and for what.
struct LookedUp {
    pick: u64,
    tag: String,
    components: Vec<ComponentInfo>,
    schema: Option<ComponentSchema>,
}

pub struct Inspector {
    document: Option<Entity<Document>>,
    /// What every change is made through.
    workspace: WeakEntity<WorkspaceView>,
    form: Option<Form>,
    looked_up: Option<LookedUp>,
    /// Pictures of a form let go of, which the window lets go of as the panel is drawn.
    retired: Vec<Arc<RenderImage>>,
    _subscriptions: Vec<Subscription>,
}

impl Inspector {
    /// The panel for the thing selected in `document`, which changes it through `workspace`.
    pub fn new(document: Option<Entity<Document>>, workspace: WeakEntity<WorkspaceView>, cx: &mut Context<Self>) -> Self {
        let subscriptions = document
            .iter()
            .map(|document| cx.observe(document, |_, _, cx| cx.notify()))
            .collect();
        Inspector { document, workspace, form: None, looked_up: None, retired: Vec::new(), _subscriptions: subscriptions }
    }

    /// The labels of the fields shown, in order.
    pub fn labels(&self) -> Vec<String> {
        self.form
            .iter()
            .flat_map(|form| form.fields.iter().map(|field| field.spec.label.clone()))
            .collect()
    }

    /// What the field for attribute `name` shows: its text, a choice's name, a colour as
    /// `#rrggbb` or nothing, `true` or `false` for a checkbox, and a file's name.
    pub fn shown(&self, name: &str, cx: &App) -> Option<String> {
        let field = self.field(name)?;
        Some(match &field.widget {
            Widget::Input(input) => input.read(cx).value().to_string(),
            Widget::Flag => (self.talk_attr(name, cx).as_deref() == Some("true")).to_string(),
            Widget::Color { picker, .. } => picker.read(cx).value().map(written_color).unwrap_or_default(),
            Widget::Choice { select, choices } => {
                let value = select.read(cx).selected_value().cloned();
                choices
                    .iter()
                    .find(|choice| Some(&choice.value) == value.as_ref())
                    .map(|choice| choice.title.to_string())
                    .unwrap_or_default()
            }
            Widget::File => self.talk_attr(name, cx).map(|path| file_name(&path).to_string()).unwrap_or_default(),
            Widget::Icon(_) => self.talk_attr(name, cx).unwrap_or_default(),
        })
    }

    /// The icon picker of the icon field for attribute `name`.
    pub fn icon_picker(&self, name: &str) -> Option<Entity<IconPicker>> {
        match &self.field(name)?.widget {
            Widget::Icon(picker) => Some(picker.clone()),
            _ => None,
        }
    }

    /// The colour picker of the colour field for attribute `name`.
    pub fn color_picker(&self, name: &str) -> Option<Entity<ColorPickerState>> {
        match &self.field(name)?.widget {
            Widget::Color { picker, .. } => Some(picker.clone()),
            _ => None,
        }
    }

    /// Whether the text or number field for attribute `name` has the keyboard.
    pub fn has_keyboard(&self, name: &str, window: &Window, cx: &App) -> bool {
        match self.field(name).map(|field| &field.widget) {
            Some(Widget::Input(input)) => input.read(cx).focus_handle(cx).is_focused(window),
            _ => false,
        }
    }

    /// Writes `value`, one of the choices of the choice field for attribute `name`, as picking it
    /// from the field's list does.
    pub fn choose(&mut self, name: &str, value: &str, _window: &mut Window, cx: &mut Context<Self>) {
        let known = self
            .field(name)
            .is_some_and(|field| matches!(&field.spec.kind, Kind::Choice(options) if options.iter().any(|option| option == value)));
        if known {
            self.write(name, Some(value), cx).ok();
            cx.notify();
        }
    }

    /// Adds the file at `path`, outside the talk, to the talk, and writes where it's kept to the
    /// file field for attribute `name`, as choosing it in the system's dialog does. A file of a
    /// kind the field doesn't take, or one that can't be added, is said under the field.
    pub fn chosen_file(&mut self, name: &str, path: &Path, cx: &mut Context<Self>) {
        let Some(Kind::File(extensions)) = self.field(name).map(|field| field.spec.kind.clone()) else {
            return;
        };
        let Some(document) = self.document.clone() else {
            return;
        };
        let added = file_kind(path, &extensions)
            .ok_or(NotAdded::NotThatKind)
            .and_then(|kind| {
                let bytes = Document::read_outside_file(path)?;
                let suggested = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                document.update(cx, |document, cx| {
                    let added = document.add_file(kind, &suggested, bytes);
                    if added.is_ok() {
                        // The working copy keeps the file once it hears of it.
                        cx.notify();
                    }
                    added
                })
            });
        let problem = match added {
            Ok(added) => self.write(name, Some(&added), cx).err().map(|refused| SharedString::from(refused.to_string())),
            Err(why) => Some(SharedString::from(why.to_string())),
        };
        if let Some(field) = self.field_mut(name) {
            field.problem = problem;
        }
        cx.notify();
    }

    fn field(&self, name: &str) -> Option<&Field> {
        self.form.as_ref()?.fields.iter().find(|field| field.spec.attr == name)
    }

    fn field_mut(&mut self, name: &str) -> Option<&mut Field> {
        self.form.as_mut()?.fields.iter_mut().find(|field| field.spec.attr == name)
    }

    /// What the thing the form is for has as attribute `name`.
    fn talk_attr(&self, name: &str, cx: &App) -> Option<String> {
        let target = &self.form.as_ref()?.target;
        let document = self.document.as_ref()?.read(cx);
        find_tag(document.talk(), target.slide, &target.source)?.attr(name).map(str::to_string)
    }

    /// Sets attribute `name` of the thing the form is for to `value`, or takes it away, or says
    /// why it wasn't.
    fn write(&mut self, name: &str, value: Option<&str>, cx: &mut Context<Self>) -> Result<(), EditRefused> {
        let Some(form) = &self.form else {
            return Err(EditRefused::CouldNotBeMade);
        };
        let (pick, target) = (form.shape.pick, form.target.clone());
        let made = self
            .workspace
            .update(cx, |workspace, cx| workspace.set_attr_from_inspector(pick, &target, name, value, cx))
            .unwrap_or(Err(EditRefused::CouldNotBeMade));
        // The thing's text has grown or shrunk: the form follows it there at once.
        let moved = self.document.as_ref().and_then(|document| {
            let document = document.read(cx);
            document.selected().filter(|_| document.selection_pick() == pick).cloned()
        });
        if let (Some(form), Some(moved)) = (&mut self.form, moved) {
            form.target = moved;
        }
        made
    }

    /// Writes what text or number field `index` holds, if something's been typed in it. What was
    /// refused and not typed in since is tried again only by Enter, `again`: leaving the field
    /// leaves it as it is, with why.
    fn commit(&mut self, index: usize, again: bool, window: &mut Window, cx: &mut Context<Self>) {
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
    fn commit_typing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed_in = self.form.iter().flat_map(|form| form.fields.iter().enumerate()).find_map(|(index, field)| match &field.widget {
            Widget::Input(input) if input.read(cx).focus_handle(cx).is_focused(window) => Some(index),
            _ => None,
        });
        if let Some(index) = typed_in {
            self.commit(index, false, window, cx);
        }
    }

    /// Shows in text or number field `index` what the talk has again, and forgets what was typed.
    fn put_back(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
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
    fn step(&mut self, index: usize, up: bool, window: &mut Window, cx: &mut Context<Self>) {
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
    fn clear_problem(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(field) = self.form.as_mut().and_then(|form| form.fields.get_mut(index))
            && field.problem.take().is_some()
        {
            cx.notify();
        }
    }

    /// Lets go of the form, as it's no longer for the thing selected. A colour still held in
    /// an open picker is written to the thing it was dragged for, once the drawing is done.
    fn drop_form(&mut self, cx: &mut Context<Self>) {
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
        let (pick, target, workspace) = (form.shape.pick, form.target, self.workspace.clone());
        cx.defer(move |cx| {
            for (name, color) in held {
                workspace
                    .update(cx, |workspace, cx| workspace.set_attr_from_inspector(pick, &target, &name, Some(&color), cx))
                    .ok();
            }
        });
    }

    /// Gives the next text or number field the keyboard, or the one before with `back`, from
    /// the one that has it. Says whether one had it.
    fn focus_along(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) -> bool {
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
    fn ask_for_file(&mut self, name: String, cx: &mut Context<Self>) {
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

    /// What the panel shows for the thing selected on the slide shown, if one is, or else the
    /// table the caret is in a cell of: the form's shape, the thing, and its tag, if it's one.
    fn subject(&mut self, cx: &mut Context<Self>) -> Option<(Shape, Selected, Option<Tag>)> {
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
    fn build_form(&mut self, shape: Shape, target: Selected, window: &mut Window, cx: &mut Context<Self>) -> Form {
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

    /// Keeps `color`, dragged in colour field `name`'s picker, for when the picker closes, or
    /// with `None` lets go of what's held. A drag is of the talk as it was when it began.
    fn hold_color(&mut self, name: &str, color: Option<String>, cx: &App) {
        let text = self.text_hash(cx);
        if let Some(Field { widget: Widget::Color { held, .. }, .. }) = self.field_mut(name) {
            *held = color.map(|color| Held { text: held.as_ref().map_or(text, |held| held.text), color });
        }
    }

    /// Writes the colour held for colour field `name`, if there is one and nothing has changed
    /// the talk's text since the drag began.
    fn give_held_color(&mut self, name: &str, cx: &mut Context<Self>) {
        let held = match self.field_mut(name) {
            Some(Field { widget: Widget::Color { held, .. }, .. }) => held.take(),
            _ => None,
        };
        if let Some(held) = held {
            if self.is_current(held.text, cx) {
                self.write_color(name, &held.color, cx);
            }
            cx.notify();
        }
    }

    /// A hash of the talk's text now. A build of its components or a save to another place
    /// leaves it as it is.
    fn text_hash(&self, cx: &App) -> u64 {
        let mut hasher = std::hash::DefaultHasher::new();
        if let Some(document) = &self.document {
            std::hash::Hash::hash(document.read(cx).source(), &mut hasher);
        }
        std::hash::Hasher::finish(&hasher)
    }

    /// Whether the talk's text is still what hashed to `text`.
    fn is_current(&self, text: u64, cx: &App) -> bool {
        self.text_hash(cx) == text
    }

    /// Writes `color`, as `#rrggbb`, to colour field `name`, unless it's the colour the talk
    /// gives already, however that's written: a colour named by hand stays as it's written.
    fn write_color(&mut self, name: &str, color: &str, cx: &mut Context<Self>) {
        let now = self.talk_attr(name, cx).as_deref().and_then(color_of).map(written_color);
        if now.as_deref() != Some(color) {
            self.write(name, Some(color), cx).ok();
        }
    }

    /// Shows in every field what `tag` has, but in a text or number field that has the keyboard.
    fn follow_talk(&mut self, tag: Option<&Tag>, window: &mut Window, cx: &mut Context<Self>) {
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

    fn featured_colors(&self, cx: &App) -> Vec<Hsla> {
        let Some(document) = &self.document else {
            return Vec::new();
        };
        let theme = talk_theme(document.read(cx).talk());
        [theme.text, theme.accent, theme.background]
            .into_iter()
            .map(|color| rgb(u32::from(color.r) << 16 | u32::from(color.g) << 8 | u32::from(color.b)).into())
            .collect()
    }

    /// Field `index` as it's drawn: its label, what shows its value, and any problem under it.
    fn field_element(&self, index: usize, field: &Field, tag: Option<&Tag>, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let attr = field.spec.attr.clone();
        let id = SharedString::from(field_id(&attr));
        let value = tag.and_then(|tag| tag.attr(&attr)).map(str::to_string);
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

/// The kind of file `path` is, for a field that takes `extensions`, if it's one it takes and one
/// a talk can have.
fn file_kind(path: &Path, extensions: &[String]) -> Option<FileKind> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    if !extensions.contains(&extension) {
        return None;
    }
    [FileKind::Picture, FileKind::Data]
        .into_iter()
        .find(|kind| kind.extensions().contains(&extension.as_str()))
}

/// A colour as the talk writes it, as the picker holds it, if the talk can read it.
fn color_of(value: &str) -> Option<Hsla> {
    let color = toolkit::Color::parse(value)?;
    Some(rgb(u32::from(color.r) << 16 | u32::from(color.g) << 8 | u32::from(color.b)).into())
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

impl Render for Inspector {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let subject = self.subject(cx);
        let tag = match subject {
            Some((shape, target, tag)) => {
                match &mut self.form {
                    Some(form) if form.shape == shape => form.target = target,
                    _ => {
                        self.drop_form(cx);
                        self.form = Some(self.build_form(shape, target, window, cx));
                    }
                }
                tag
            }
            None => {
                self.drop_form(cx);
                None
            }
        };
        self.follow_talk(tag.as_ref(), window, cx);
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
            let element = self.field_element(index, field, tag.as_ref(), window, cx);
            if field.spec.place { place.push(element) } else { settings.push(element) }
        }
        let muted = cx.theme().muted_foreground;
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
            .children(table)
            .children(settings)
            .when(!place.is_empty(), |this| {
                this.child(div().pt_2().text_xs().font_weight(FontWeight::SEMIBOLD).child("Place"))
                    .children(place)
            })
    }
}
