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
use fields::{Choice, FIRST_FRAME, Kind, Spec, Typed, file_name, number_typed, stepped, text_typed};

pub use fields::{BOTH_QUOTES, NOT_A_NUMBER, NOT_WHOLE};
pub use labels::{model_facts, terminal_facts, video_facts};

pub const INSPECTOR_ID: &str = "inspector";
/// What names the thing selected, at the top of the panel.
pub const INSPECTOR_HEADING_ID: &str = "inspector-heading";
/// What the panel says in place of fields, when it has none to show.
pub const INSPECTOR_MESSAGE_ID: &str = "inspector-message";
/// The button of a file field, inside the field.
pub const INSPECTOR_CHOOSE_FILE_ID: &str = "inspector-choose-file";
/// The line saying how long a video selected is and how big, read off the window's thread.
pub const INSPECTOR_VIDEO_FACTS_ID: &str = "inspector-video-facts";
/// The sentence saying why a video selected can't be played, if it can't.
pub const INSPECTOR_VIDEO_PROBLEM_ID: &str = "inspector-video-problem";
/// The line saying how many triangles or points a 3D model selected has, read off the window's
/// thread.
pub const INSPECTOR_MODEL_FACTS_ID: &str = "inspector-model-facts";
/// The sentence saying why a 3D model selected can't be drawn, if it can't.
pub const INSPECTOR_MODEL_PROBLEM_ID: &str = "inspector-model-problem";
/// The line saying how big a terminal recording selected's screen is, how long it plays and how
/// many times it stops, read off the window's thread.
pub const INSPECTOR_TERMINAL_FACTS_ID: &str = "inspector-terminal-facts";
/// The sentence saying why a terminal recording selected can't be played, if it can't.
pub const INSPECTOR_TERMINAL_PROBLEM_ID: &str = "inspector-terminal-problem";
/// The line saying how many views a 3D model selected has, one a click in the show.
pub const INSPECTOR_MODEL_VIEWS_ID: &str = "inspector-model-views";
/// The button that adds the view a 3D model being turned is shown from to its views.
pub const INSPECTOR_ADD_VIEW_ID: &str = "inspector-add-view";
/// The button that takes away a 3D model's views.
pub const INSPECTOR_CLEAR_VIEWS_ID: &str = "inspector-clear-views";
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

/// The talk's pictures, for a video's still, as last read off the window's thread, and for which
/// thing picked and which files added they were read, or are being read.
#[derive(Default)]
struct Pictures {
    read_for: Option<(u64, u64)>,
    reading: Option<(u64, u64)>,
    found: Vec<String>,
}

pub struct Inspector {
    document: Option<Entity<Document>>,
    /// What every change is made through.
    workspace: WeakEntity<WorkspaceView>,
    form: Option<Form>,
    looked_up: Option<LookedUp>,
    pictures: Pictures,
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
        Inspector { document, workspace, form: None, looked_up: None, pictures: Pictures::default(), retired: Vec::new(), _subscriptions: subscriptions }
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
            Widget::Flag => labels::says_yes(self.form_tag(), self.talk_attr(name, cx).as_deref()).to_string(),
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

    /// Whether any of the form's fields has the keyboard, or what one of them opened, as a
    /// choice's list or the icon picker's search.
    pub fn has_any_keyboard(&self, window: &Window, cx: &App) -> bool {
        self.form.as_ref().is_some_and(|form| {
            form.fields.iter().any(|field| {
                let focus = match &field.widget {
                    Widget::Input(input) => input.read(cx).focus_handle(cx),
                    Widget::Choice { select, .. } => select.read(cx).focus_handle(cx),
                    Widget::Color { picker, .. } => picker.read(cx).focus_handle(cx),
                    Widget::Icon(picker) => picker.read(cx).search_focus(cx),
                    Widget::Flag | Widget::File => return false,
                };
                focus.contains_focused(window, cx)
            })
        })
    }

    /// Whether the text or number field for attribute `name` has the keyboard.
    pub fn has_keyboard(&self, name: &str, window: &Window, cx: &App) -> bool {
        match self.field(name).map(|field| &field.widget) {
            Some(Widget::Input(input)) => input.read(cx).focus_handle(cx).is_focused(window),
            _ => false,
        }
    }

    /// Writes `value`, one of the choices of the choice field for attribute `name`, as picking it
    /// from the field's list does. For a video's still, `value` is one of the talk's pictures, or
    /// empty for its first frame, which takes the still away.
    pub fn choose(&mut self, name: &str, value: &str, _window: &mut Window, cx: &mut Context<Self>) {
        let written = self.field(name).and_then(|field| match &field.spec.kind {
            Kind::Choice(options) => options.iter().any(|option| option == value).then_some(Some(value)),
            // Its first frame is no still of its own.
            Kind::Still(_) if value.is_empty() => Some(None),
            Kind::Still(pictures) => pictures.iter().any(|picture| picture == value).then_some(Some(value)),
            _ => None,
        });
        if let Some(written) = written {
            self.write(name, written, cx).ok();
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

    /// What the form says of the video it's for, if it's for one: how long it is and how big,
    /// once that's been read, and why it can't be played, if it can't. What's known is read off
    /// the window's thread, once for each version of its file, and the panel is drawn again once
    /// it's there.
    fn video_said(&mut self, cx: &mut Context<Self>) -> (Option<String>, Option<String>) {
        let (Some(form), Some(document)) = (&self.form, self.document.clone()) else {
            return (None, None);
        };
        let Subject::Thing(target) = &form.target else {
            return (None, None);
        };
        let target = target.clone();
        if form.shape.tag.as_deref() != Some("Video") {
            return (None, None);
        }
        let src = self.talk_attr("src", cx).map(|src| src.trim().to_string()).filter(|src| !src.is_empty());
        let Some(src) = src else {
            return (None, Some(slides_engine::VIDEO_NO_FILE.to_string()));
        };
        let (known, to_read, problem) = document.update(cx, |document, _| {
            let known = document.video_facts(&src);
            let to_read = if known.is_none() { document.video_facts_to_read(&src) } else { None };
            let problem = if target.slide == document.current() { document.video_problem(&target.source) } else { None };
            (known, to_read, problem)
        });
        if let Some(to_read) = to_read {
            let document = document.downgrade();
            cx.spawn(async move |this, cx| {
                let read = cx.background_spawn(async move { to_read.read() }).await;
                let known = document.update(cx, |document, _| document.take_video_facts(read)).unwrap_or(false);
                if known {
                    this.update(cx, |_, cx| cx.notify()).ok();
                }
            })
            .detach();
        }
        let facts = known.and_then(Result::ok).map(|facts| labels::video_facts(facts.seconds, facts.size));
        let problem = problem.or_else(|| known.and_then(Result::err).map(|why| why.to_string()));
        (facts, problem)
    }

    /// What the form says of the 3D model it's for, if it's for one: how many triangles or
    /// points it has, once that's been read off the window's thread from its headers, once for
    /// each version of its file; why it can't be drawn, if it can't; and its views.
    fn model_said(&mut self, cx: &mut Context<Self>) -> ModelSaid {
        let (Some(form), Some(document)) = (&self.form, self.document.clone()) else {
            return ModelSaid::default();
        };
        let Subject::Thing(target) = &form.target else {
            return ModelSaid::default();
        };
        let target = target.clone();
        if form.shape.tag.as_deref() != Some("Model") {
            return ModelSaid::default();
        }
        let turned = self
            .workspace
            .upgrade()
            .and_then(|workspace| workspace.read(cx).slide_editor().read(cx).model_turned())
            .is_some_and(|turned| turned == target.source);
        let written = self.talk_attr("views", cx);
        let views = Some(ModelViews { count: views_of(written.as_deref()), written: written.is_some(), turned });
        let src = self.talk_attr("src", cx).map(|src| src.trim().to_string()).filter(|src| !src.is_empty());
        let Some(src) = src else {
            return ModelSaid { facts: None, problem: Some(slides_engine::MODEL_NO_FILE.to_string()), views };
        };
        let (known, to_read, problem) = document.update(cx, |document, _| {
            let known = document.model_facts(&src);
            let to_read = if known.is_none() { document.model_facts_to_read(&src) } else { None };
            let problem = if target.slide == document.current() { document.selected_model_said() } else { None };
            (known, to_read, problem)
        });
        if let Some(to_read) = to_read {
            let document = document.downgrade();
            cx.spawn(async move |this, cx| {
                let read = cx.background_spawn(async move { to_read.read() }).await;
                let known = document.update(cx, |document, _| document.take_model_facts(read)).unwrap_or(false);
                if known {
                    this.update(cx, |_, cx| cx.notify()).ok();
                }
            })
            .detach();
        }
        let facts = known.and_then(Result::ok).map(|facts| labels::model_facts(facts.kind, facts.triangles, facts.points));
        // One drawn, but not from the view written, says that too.
        let view = self.talk_attr("view", cx).filter(|view| !view.trim().is_empty());
        let unread = view.is_some_and(|view| slides_model::View::parse(&view).is_none()).then(|| slides_engine::MODEL_VIEW_UNREAD.to_string());
        let problem = problem.or_else(|| known.and_then(Result::err).map(|why| why.to_string())).or(unread);
        ModelSaid { facts, problem, views }
    }

    /// What the form says of the terminal recording it's for, if it's for one: how big its screen
    /// is, how long it plays and how many times it stops, once that's been read off the window's
    /// thread, once for each version of its file, and why it can't be played, if it can't.
    fn terminal_said(&mut self, cx: &mut Context<Self>) -> (Option<String>, Option<String>) {
        let (Some(form), Some(document)) = (&self.form, self.document.clone()) else {
            return (None, None);
        };
        let Subject::Thing(target) = &form.target else {
            return (None, None);
        };
        let target = target.clone();
        if form.shape.tag.as_deref() != Some("Terminal") {
            return (None, None);
        }
        let src = self.talk_attr("src", cx).map(|src| src.trim().to_string()).filter(|src| !src.is_empty());
        let Some(src) = src else {
            // One that runs a command has no recording to say of; the engine says if it's wrong.
            let run = self.talk_attr("run", cx);
            let problem = match run {
                Some(_) => document.update(cx, |document, _| if target.slide == document.current() { document.terminal_problem_on(target.slide, &target.source) } else { None }),
                None => Some(slides_engine::TERMINAL_NO_FILE.to_string()),
            };
            return (None, problem);
        };
        let (known, to_read, problem) = document.update(cx, |document, _| {
            let known = document.recording_facts(&src);
            let to_read = if known.is_none() { document.recording_facts_to_read(&src) } else { None };
            let problem = if target.slide == document.current() { document.terminal_problem_on(target.slide, &target.source) } else { None };
            (known, to_read, problem)
        });
        if let Some(to_read) = to_read {
            let document = document.downgrade();
            cx.spawn(async move |this, cx| {
                let read = cx.background_spawn(async move { to_read.read() }).await;
                let known = document.update(cx, |document, _| document.take_recording_facts(read)).unwrap_or(false);
                if known {
                    this.update(cx, |_, cx| cx.notify()).ok();
                }
            })
            .detach();
        }
        // What can't be played has no facts to say, but why.
        let problem = problem.or_else(|| known.and_then(Result::err).map(|why| why.to_string()));
        let facts = known.and_then(Result::ok).filter(|_| problem.is_none()).map(labels::terminal_facts);
        (facts, problem)
    }

    /// Adds the view the 3D model the form is for is shown from, while it's turned, to its views.
    fn add_view(&mut self, cx: &mut Context<Self>) {
        if let Some(workspace) = self.workspace.upgrade() {
            let editor = workspace.read(cx).slide_editor().clone();
            editor.update(cx, |editor, cx| editor.add_view(cx));
        }
    }

    /// Takes away the views of the 3D model the form is for, as any setting is written: one
    /// being turned goes back to its still.
    fn clear_views(&mut self, cx: &mut Context<Self>) {
        self.write("views", None, cx).ok();
        cx.notify();
    }

    /// The tag of the thing the form is for, if it's one.
    fn form_tag(&self) -> &str {
        self.form.as_ref().and_then(|form| form.shape.tag.as_deref()).unwrap_or_default()
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

/// What the form says of a 3D model.
#[derive(Default)]
struct ModelSaid {
    facts: Option<String>,
    problem: Option<String>,
    views: Option<ModelViews>,
}

/// How many views a 3D model has, whether it has any written, read or not, and whether it's
/// being turned, so that the view it's shown from can be added.
struct ModelViews {
    count: usize,
    written: bool,
    turned: bool,
}

/// How many views a model whose `views` is `written` flies through, as the engine reads them.
fn views_of(written: Option<&str>) -> usize {
    slides_engine::read_model_views(written).0.len()
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
