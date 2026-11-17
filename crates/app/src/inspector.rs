//! The inspector: a form at the window's right for the thing selected on the slide, made from
//! its component's arguments, so that someone who doesn't write code can set it up. It's always
//! there while a talk is open, so the slide never moves as things are selected. Each field
//! shows what the talk says, and writes one attribute when it's changed.
//!
//! The form is worked out as the panel is drawn, from the talk and the thing selected; nothing
//! runs while the window waits. A field being typed in keeps what's typed until it's committed:
//! with Enter, by leaving it, or by a click anywhere outside the panel, which commits it before
//! the click does anything else, so a change never goes to the wrong thing.

mod colors;
mod commit;
mod fields;
mod form;
pub(crate) mod labels;
mod render;
mod widgets;

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
use gpui_kit::component::{ActiveTheme as _, Disableable as _, IndexPath, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use slides_engine::{ComponentInfo, is_positioned, talk_theme};
use slides_format::{Block, Span, find_tag, region_value};
use toolkit::ComponentSchema;

use crate::added_files::{FileKind, NotAdded};
use crate::document::{Document, EditRefused, Selected};
use crate::icon_picker::{IconPicked, IconPicker};
use crate::selection;
use crate::table_tools;
use crate::text_tools::written_color;
use crate::workspace_view::WorkspaceView;
use colors::color_of;
use fields::{Choice, Kind, Spec, Typed, file_name, number_typed, stepped, text_typed};

pub use fields::{BOTH_QUOTES, NOT_A_NUMBER, NOT_WHOLE};

pub const INSPECTOR_ID: &str = "inspector";
/// What names the thing selected, at the top of the panel.
pub const INSPECTOR_HEADING_ID: &str = "inspector-heading";
/// What the panel says in place of fields, when it has none to show.
pub const INSPECTOR_MESSAGE_ID: &str = "inspector-message";
/// The button of a file field, inside the field.
pub const INSPECTOR_CHOOSE_FILE_ID: &str = "inspector-choose-file";
/// The button that puts an area selected back where its layout has it.
pub const INSPECTOR_PUT_BACK_ID: &str = "inspector-put-back";

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

/// What a form is for.
#[derive(Clone, PartialEq)]
enum Subject {
    /// A thing on a slide, where it is in the talk now.
    Thing(Selected),
    /// An area of slide `slide`'s layout, and whether the slide's settings move it from where
    /// the layout has it.
    Area { slide: usize, name: String, moved: bool },
    /// Several things at once, which the form has no fields for.
    Several,
}

/// What the thing a form is for has as each of its fields' attributes, or its area's words.
#[derive(Default)]
struct Values(Vec<(String, String)>);

impl Values {
    fn get(&self, name: &str) -> Option<&str> {
        self.0.iter().find(|(held, _)| held == name).map(|(_, value)| value.as_str())
    }
}

struct Form {
    shape: Shape,
    /// What the form is for.
    target: Subject,
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

    /// What the thing the form is for has as attribute `name`, or its area as the word `name`.
    fn talk_attr(&self, name: &str, cx: &App) -> Option<String> {
        let document = self.document.as_ref()?.read(cx);
        match &self.form.as_ref()?.target {
            Subject::Thing(target) => find_tag(document.talk(), target.slide, &target.source)?.attr(name).map(str::to_string),
            Subject::Area { slide, name: area, .. } => region_value(document.talk().slides.get(*slide)?, area, name).map(str::to_string),
            Subject::Several => None,
        }
    }
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
