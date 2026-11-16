//! The text tools on the Home tab: how the text being typed on the slide looks, and what its
//! paragraphs are. Each shows what the selection has, and changes it there. They're only for
//! text being typed, and using one leaves the keyboard in the text, so typing goes on.
//!
//! What they show is worked out as they're drawn, from the text being typed and where the
//! slide draws it; nothing runs while the window waits.

use gpui_kit::accesskit::Toggled;
use gpui_kit::assets::IconName as Icons;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::input::{self, Input, InputEvent, InputState};
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::{
    Disableable as _, Icon, IconName, Selectable as _, Sizable as _, h_flex, tooltip::Tooltip,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use slides_engine::{Align, talk_theme};
use slides_format::{ParaKind, Shared, StyleBit, TextEdit};

use crate::document::Document;
use crate::ribbon::{divider, group};
use crate::slide_editor::{EDITOR_CONTEXT, Indent, Outdent, SlideEditor, only_lists};
use crate::text_editing::EditingText;

pub const TEXT_TOOLS_ID: &str = "text-tools";
pub const BOLD_ID: &str = "bold";
pub const ITALIC_ID: &str = "italic";
pub const UNDERLINE_ID: &str = "underline";
pub const SIZE_ID: &str = "text-size";
/// The arrow beside the size, which lists the sizes.
pub const SIZE_LIST_ID: &str = "text-sizes";
/// The colour button, which opens the colour picker.
pub const COLOR_ID: &str = "text-color";
/// The button that gives the text its region's own colour back.
pub const AUTOMATIC_COLOR_ID: &str = "text-color-automatic";
pub const BULLETS_ID: &str = "bullets";
pub const NUMBERING_ID: &str = "numbering";
pub const INDENT_LESS_ID: &str = "indent-less";
pub const INDENT_MORE_ID: &str = "indent-more";
pub const ALIGN_LEFT_ID: &str = "align-left";
pub const ALIGN_CENTER_ID: &str = "align-center";
pub const ALIGN_RIGHT_ID: &str = "align-right";

/// What the alignment tools say when the text is all list items.
pub const LISTS_STAY_LEFT: &str = "Lists stay at the left for now.";

/// The sizes the list offers. Any other from [`MIN_SIZE`] to [`MAX_SIZE`] can be typed in.
pub const SIZES: [u32; 8] = [18, 24, 32, 40, 48, 64, 80, 96];
pub const MIN_SIZE: u32 = 1;
pub const MAX_SIZE: u32 = 512;

actions!(
    text_tools,
    [ToggleBold, ToggleItalic, ToggleUnderline, ToggleBullets, ToggleNumbering]
);

/// Gives the text being typed size `size`.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = text_tools, no_json)]
pub struct SetTextSize {
    pub size: u32,
}

/// The tools' keys, which apply while text is being typed.
pub fn bind_keys(cx: &mut App) {
    let context = Some(EDITOR_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("ctrl-b", ToggleBold, context),
        KeyBinding::new("ctrl-i", ToggleItalic, context),
        KeyBinding::new("ctrl-u", ToggleUnderline, context),
    ]);
}

/// A size as it's typed, held to the sizes text can have, or `None` if it isn't a number.
pub fn size_typed(text: &str) -> Option<u32> {
    let size = text.trim().parse::<f64>().ok().filter(|size| size.is_finite())?;
    Some(size.round().clamp(f64::from(MIN_SIZE), f64::from(MAX_SIZE)) as u32)
}

/// How a tool that's on or off shows the selection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pressed {
    Off,
    On,
    /// Some of the selection has it, and some doesn't.
    Mixed,
}

impl Pressed {
    fn of(shared: Shared<bool>) -> Pressed {
        match shared {
            Shared::All(true) => Pressed::On,
            Shared::Mixed => Pressed::Mixed,
            Shared::All(false) | Shared::None => Pressed::Off,
        }
    }
}

/// What the tools show for the text being typed.
struct Showing {
    title: bool,
    bold: Pressed,
    italic: Pressed,
    underline: Pressed,
    /// The size drawn, or `None` when the selection has several.
    size: Option<u32>,
    /// The colour given, `Some(None)` for none of its own, or `None` when there are several.
    color: Option<Option<String>>,
    bullets: Pressed,
    numbering: Pressed,
    align: Option<Align>,
    /// Whether the text is all list items, which stay at the left.
    only_lists: bool,
    /// Whether any selected paragraph is a list item, which can be indented.
    any_listed: bool,
}

/// What the tools show for the text being typed in `document`, if any is.
fn showing(document: &Entity<Document>, cx: &mut App) -> Option<Showing> {
    let (editing, flow) = {
        let document = document.read(cx);
        (document.text()?.clone(), document.text_flow())
    };
    let layout = document.update(cx, |document, _| document.text_layout(editing.slide));
    let region = editing.flow.region().map(str::to_string);
    let placeholder = || region.as_deref().and_then(|region| layout.placeholder_text(region));
    // What text with no size of its own is drawn at in paragraph `para`.
    let base = |para: usize| match flow {
        Some(flow) => layout.text_size(flow, para),
        None => placeholder().map(|(size, _)| size),
    };
    let edit = &editing.edit;
    let state = edit.style_at();
    let selection = edit.selection;
    let sizes: Vec<Option<u32>> = if selection.is_caret() {
        let size = match state.size {
            Shared::All(size) => size,
            _ => None,
        };
        vec![size.or_else(|| base(selection.head.para).map(round))]
    } else {
        let start = selection.start().para;
        edit.copied()
            .iter()
            .enumerate()
            .flat_map(|(index, para)| {
                let base = base(start + index).map(round);
                para.spans.iter().map(move |span| span.style.size.or(base)).collect::<Vec<_>>()
            })
            .collect()
    };
    let size = match Shared::of_all(sizes) {
        Shared::All(size) => size,
        _ => None,
    };
    let color = match state.color {
        Shared::All(color) => Some(color),
        Shared::None => Some(None),
        Shared::Mixed => None,
    };
    let (start, end) = (selection.start().para, selection.end().para);
    let kinds: Vec<ParaKind> = edit
        .paras
        .get(start..=end)
        .map(|paras| paras.iter().map(|para| para.kind).collect())
        .unwrap_or_default();
    let listed = |numbered: bool| {
        let is = |kind: &ParaKind| match kind {
            ParaKind::Bullet { .. } => !numbered,
            ParaKind::Numbered { .. } => numbered,
            _ => false,
        };
        Pressed::of(Shared::of_all(kinds.iter().map(is)))
    };
    // As it's set to be, whatever a list in it is drawn at.
    let align = document.read(cx).text_align();
    Some(Showing {
        title: editing.is_title(),
        bold: Pressed::of(state.bold),
        italic: Pressed::of(state.italic),
        underline: Pressed::of(state.underline),
        size,
        color,
        bullets: listed(false),
        numbering: listed(true),
        align,
        only_lists: only_lists(&edit.paras),
        any_listed: kinds
            .iter()
            .any(|kind| matches!(kind, ParaKind::Bullet { .. } | ParaKind::Numbered { .. })),
    })
}

fn round(size: f32) -> u32 {
    size.round().max(0.0) as u32
}

/// `Shared::of` for the app, which the format crate keeps to itself.
trait SharedOf<T> {
    fn of_all(values: impl IntoIterator<Item = T>) -> Shared<T>;
}

impl<T: PartialEq> SharedOf<T> for Shared<T> {
    fn of_all(values: impl IntoIterator<Item = T>) -> Shared<T> {
        let mut values = values.into_iter();
        let Some(first) = values.next() else {
            return Shared::None;
        };
        if values.all(|value| value == first) {
            Shared::All(first)
        } else {
            Shared::Mixed
        }
    }
}

/// `color` as the colour picker holds it, if it's one written as `#rgb` or `#rrggbb`.
fn picker_color(color: &str) -> Option<Hsla> {
    let hex = color.strip_prefix('#')?;
    let hex = match hex.len() {
        3 => hex.chars().flat_map(|c| [c, c]).collect(),
        6 => hex.to_string(),
        _ => return None,
    };
    let value = u32::from_str_radix(&hex, 16).ok()?;
    Some(rgb(value).into())
}

/// `color` as the talk writes it: `#rrggbb`.
fn written_color(color: Hsla) -> String {
    let Rgba { r, g, b, .. } = color.into();
    let byte = |channel: f32| (channel.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", byte(r), byte(g), byte(b))
}

pub struct TextTools {
    document: Option<Entity<Document>>,
    editor: Entity<SlideEditor>,
    /// Holds the tools, so that the editor can tell the keyboard is with them.
    focus_handle: FocusHandle,
    size_field: Entity<InputState>,
    color_picker: Entity<ColorPickerState>,
    /// The text being typed as the tools last heard of it.
    seen: Option<EditingText>,
    _subscriptions: Vec<Subscription>,
}

impl TextTools {
    /// The tools for the text `editor` types, which hold `focus_handle`.
    pub fn new(
        document: Option<Entity<Document>>,
        editor: Entity<SlideEditor>,
        focus_handle: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let size_field = cx.new(|cx| InputState::new(window, cx));
        let color_picker = cx.new(|cx| ColorPickerState::new(window, cx));
        let mut subscriptions = vec![
            cx.subscribe_in(&size_field, window, |this, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    this.size_entered(window, cx);
                }
            }),
            cx.subscribe_in(
                &color_picker,
                window,
                |this, picker, event: &ColorPickerEvent, _, cx| {
                    let ColorPickerEvent::Change(Some(color)) = event else {
                        return;
                    };
                    let color = written_color(*color);
                    let open = picker.read(cx).is_open();
                    this.editor.update(cx, |editor, cx| {
                        // A drag of the sliders is one change, given once the picker closes,
                        // however it does: the editor gives it before typing ends, too.
                        editor.hold_color(color, cx);
                        if !open {
                            editor.give_held_color(cx);
                        }
                    });
                },
            ),
            cx.observe(&color_picker, |this, picker, cx| {
                if !picker.read(cx).is_open() {
                    this.editor.update(cx, |editor, cx| editor.give_held_color(cx));
                }
            }),
            // The selection moves with the editor, and the text it's in with the talk. The
            // editor draws again as its caret blinks, which changes nothing here.
            cx.observe(&editor, |this, _, cx| {
                let seen = this.seen(cx);
                if seen != this.seen {
                    this.seen = seen;
                    cx.notify();
                }
            }),
        ];
        if let Some(document) = &document {
            subscriptions.push(cx.observe(document, |_, _, cx| cx.notify()));
        }
        TextTools {
            document,
            editor,
            focus_handle,
            size_field,
            color_picker,
            seen: None,
            _subscriptions: subscriptions,
        }
    }

    fn seen(&self, cx: &App) -> Option<EditingText> {
        self.document.as_ref()?.read(cx).text().cloned()
    }

    /// The colour picker, for tests.
    pub fn color_picker(&self) -> &Entity<ColorPickerState> {
        &self.color_picker
    }

    fn editor_focus(&self, cx: &App) -> FocusHandle {
        self.editor.read(cx).focus_handle().clone()
    }

    /// Gives the keyboard back to the text being typed.
    fn back_to_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor
            .update(cx, |editor, cx| editor.focus_text(window, cx));
        cx.notify();
    }

    fn size_entered(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.size_field.read(cx).value();
        self.back_to_text(window, cx);
        if let Some(size) = size_typed(&typed) {
            self.editor
                .update(cx, |editor, cx| editor.set_text_size(size, cx));
        }
    }

    /// A tool that's on or off, `(id, icon, label)`, showing `pressed`, whose key is `action`'s.
    fn toggle(
        &self,
        (id, icon, label): (&'static str, Icons, &'static str),
        pressed: Pressed,
        action: &dyn Action,
        enabled: bool,
        work: impl Fn(&mut SlideEditor, &mut Context<SlideEditor>) + 'static,
    ) -> AnyElement {
        let toggled = match pressed {
            Pressed::Off => Toggled::False,
            Pressed::On => Toggled::True,
            Pressed::Mixed => Toggled::Mixed,
        };
        let button = icon_tool(button_id(id), icon, label)
            .tooltip_with_action(label, action, Some(EDITOR_CONTEXT))
            .selected(pressed == Pressed::On)
            // Half-pressed: outlined, but not filled.
            .when(pressed == Pressed::Mixed, |button| button.outline())
            .disabled(!enabled)
            .on_click(self.with_editor(work));
        // The button can't say it's half-pressed, so what holds it says how it's pressed.
        div()
            .id(id)
            .test_support()
            .aria_label(label)
            .aria_toggled(toggled)
            .child(button)
            .into_any_element()
    }

    /// What a click on a tool does: `work`, to the editor.
    fn with_editor(
        &self,
        work: impl Fn(&mut SlideEditor, &mut Context<SlideEditor>) + 'static,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        let editor = self.editor.clone();
        move |_, window, cx| {
            editor.update(cx, |editor, cx| {
                editor.focus_text(window, cx);
                work(editor, cx);
            })
        }
    }

    fn text_group(&mut self, showing: Option<&Showing>, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let editing = showing.is_some();
        let pressed = |get: fn(&Showing) -> Pressed| showing.map_or(Pressed::Off, get);
        let size = showing.and_then(|showing| showing.size);
        // The field shows the size, unless it's being typed in.
        let field_focused = self.size_field.read(cx).focus_handle(cx).is_focused(window);
        if !field_focused {
            let wanted: SharedString = size.map(|size| size.to_string()).unwrap_or_default().into();
            if self.size_field.read(cx).value() != wanted {
                self.size_field
                    .update(cx, |field, cx| field.set_value(wanted, window, cx));
            }
        }
        let editor_focus = self.editor_focus(cx);
        let sizes = Button::new(SIZE_LIST_ID)
            .ghost()
            .xsmall()
            .icon(IconName::ChevronDown)
            .accessibility_label("Choose a size")
            .disabled(!editing)
            .dropdown_menu(move |menu, _, _| {
                SIZES.iter().fold(menu.action_context(editor_focus.clone()), |menu, &each| {
                    menu.menu_with_check(each.to_string(), size == Some(each), Box::new(SetTextSize { size: each }))
                })
            });
        let size_field = div()
            .id("text-size-tool")
            .tooltip(|window, cx| Tooltip::new("Text size").build(window, cx))
            .w(px(72.))
            // Escape in the field leaves the size as it was, and goes back to the text.
            .capture_action(cx.listener(|this, _: &input::Escape, window, cx| {
                cx.stop_propagation();
                this.back_to_text(window, cx);
            }))
            .child(
                Input::new(&self.size_field)
                    .id(SIZE_ID)
                    .aria_label("Size")
                    .small()
                    .disabled(!editing)
                    .suffix(sizes),
            );
        let color = showing.and_then(|showing| showing.color.clone());
        let picker_open = self.color_picker.read(cx).is_open();
        if !picker_open {
            let wanted = color.flatten().as_deref().and_then(picker_color);
            if self.color_picker.read(cx).value() != wanted {
                self.color_picker.update(cx, |picker, cx| match wanted {
                    Some(wanted) => picker.set_value(wanted, window, cx),
                    None => picker.clear_value(window, cx),
                });
            }
        }
        let color_tool = div()
            .id(COLOR_ID)
            .test_support()
            .aria_label("Colour")
            .tooltip(|window, cx| Tooltip::new("Text colour").build(window, cx))
            .child(if editing {
                ColorPicker::new(&self.color_picker)
                    .featured_colors(self.featured_colors(cx))
                    .accessibility_label("Colour")
                    .small()
                    .into_any_element()
            } else {
                icon_tool("text-color-off", IconName::Palette, "Colour")
                    .disabled(true)
                    .into_any_element()
            });
        let automatic = icon_tool(AUTOMATIC_COLOR_ID, Icons::DropletOff, "Automatic colour")
            .tooltip("Automatic colour: the text's own")
            .disabled(!editing)
            .on_click(self.with_editor(|editor, cx| {
                editor.format(|edit| edit.set_color(None), cx)
            }))
            .into_any_element();
        group(
            "Text",
            vec![
                self.toggle((BOLD_ID, Icons::Bold, "Bold"), pressed(|s| s.bold), &ToggleBold, editing, |editor, cx| {
                    editor.format(|edit| edit.toggle(StyleBit::Bold), cx)
                }),
                self.toggle((ITALIC_ID, Icons::Italic, "Italic"), pressed(|s| s.italic), &ToggleItalic, editing, |editor, cx| {
                    editor.format(|edit| edit.toggle(StyleBit::Italic), cx)
                }),
                self.toggle((UNDERLINE_ID, Icons::Underline, "Underline"), pressed(|s| s.underline), &ToggleUnderline, editing, |editor, cx| {
                    editor.format(|edit| edit.toggle(StyleBit::Underline), cx)
                }),
                size_field.into_any_element(),
                color_tool.into_any_element(),
                automatic,
            ],
            cx,
        )
    }

    /// The talk's own colours, which the picker offers first: its text's, its accent and its
    /// background.
    pub fn featured_colors(&self, cx: &App) -> Vec<Hsla> {
        let Some(document) = &self.document else {
            return Vec::new();
        };
        let theme = talk_theme(document.read(cx).talk());
        [theme.text, theme.accent, theme.background]
            .into_iter()
            .map(|color| {
                let value = u32::from(color.r) << 16 | u32::from(color.g) << 8 | u32::from(color.b);
                rgb(value).into()
            })
            .collect()
    }

    fn paragraph_group(&self, showing: Option<&Showing>, cx: &App) -> AnyElement {
        // A title is one paragraph, never a list.
        let lists = showing.is_some_and(|showing| !showing.title);
        let pressed = |get: fn(&Showing) -> Pressed| showing.map_or(Pressed::Off, get);
        let align = showing.and_then(|showing| showing.align);
        // Lists stay at the left, so text that's all list items can't be aligned.
        let alignable = showing.is_some_and(|showing| !showing.only_lists);
        let listed = showing.is_some_and(|showing| showing.any_listed);
        let aligner = |this: &Self, id: &'static str, icon: Icons, label: &'static str, to: Align| {
            let on = align == Some(to);
            let tip = if showing.is_some() && !alignable { LISTS_STAY_LEFT } else { label };
            icon_tool(id, icon, label)
                .tooltip(tip)
                .selected(on)
                .toggled(on)
                .disabled(!alignable)
                .on_click(this.with_editor(move |editor, cx| editor.align_text(to, cx)))
                .into_any_element()
        };
        let indent = |this: &Self, id: &'static str, icon: Icons, label: &'static str, action: &dyn Action, more: bool| {
            icon_tool(id, icon, label)
                .tooltip_with_action(label, action, Some(EDITOR_CONTEXT))
                .disabled(!lists || !listed)
                .on_click(this.with_editor(move |editor, cx| {
                    editor.format(if more { TextEdit::indent } else { TextEdit::outdent }, cx)
                }))
                .into_any_element()
        };
        let tools = vec![
            self.toggle((BULLETS_ID, Icons::List, "Bullets"), pressed(|s| s.bullets), &ToggleBullets, lists, |editor, cx| {
                editor.toggle_list(false, cx)
            }),
            self.toggle((NUMBERING_ID, Icons::ListOrdered, "Numbering"), pressed(|s| s.numbering), &ToggleNumbering, lists, |editor, cx| {
                editor.toggle_list(true, cx)
            }),
            indent(self, INDENT_LESS_ID, Icons::ListIndentDecrease, "Indent less", &Outdent, false),
            indent(self, INDENT_MORE_ID, Icons::ListIndentIncrease, "Indent more", &Indent, true),
            aligner(self, ALIGN_LEFT_ID, Icons::TextAlignStart, "Align left", Align::Left),
            aligner(self, ALIGN_CENTER_ID, Icons::TextAlignCenter, "Centre", Align::Center),
            aligner(self, ALIGN_RIGHT_ID, Icons::TextAlignEnd, "Align right", Align::Right),
        ];
        group("Paragraph", tools, cx)
    }
}

/// The button of the tool `tool` that's on or off, such as [`BOLD_ID`]. The tool's own id is
/// what holds it, and says how it's pressed.
pub fn button_id(tool: &str) -> SharedString {
    format!("{tool}-button").into()
}

/// A tool shown by its icon alone, named `label`.
fn icon_tool(id: impl Into<ElementId>, icon: impl Into<Icon>, label: &'static str) -> Button {
    Button::new(id)
        .ghost()
        .small()
        .icon(icon.into())
        .accessibility_label(label)
}

impl Render for TextTools {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let showing = match self.document.clone() {
            Some(document) => showing(&document, cx),
            None => None,
        };
        let text = self.text_group(showing.as_ref(), window, cx);
        let paragraph = self.paragraph_group(showing.as_ref(), cx);
        h_flex()
            .id(TEXT_TOOLS_ID)
            .test_support()
            .h_full()
            .items_center()
            .track_focus(&self.focus_handle)
            // A press on the tools between them leaves the keyboard where it is.
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .child(text)
            .child(divider(cx))
            .child(paragraph)
    }
}
