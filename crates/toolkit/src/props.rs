use crate::color::{Color, Theme};
use crate::element::Element;
use crate::interface::{ComponentSchema, CtxData, PropKind, RenderOutput};
use std::cell::Cell;
use std::collections::{BTreeMap, HashMap};

/// A tag's attributes, as written.
pub struct Props(pub BTreeMap<String, String>);

/// What a component can read about the moment it is drawn. It also notes what the
/// component read, so that a slide is only redrawn for what can change it.
pub struct Ctx<'a> {
    data: CtxData,
    assets: &'a dyn Fn(&str) -> Option<Vec<u8>>,
    steps: Cell<u32>,
    stepped: Cell<bool>,
    animated: Cell<bool>,
}

impl<'a> Ctx<'a> {
    pub fn new(data: CtxData, assets: &'a dyn Fn(&str) -> Option<Vec<u8>>) -> Ctx<'a> {
        Ctx { data, assets, steps: Cell::new(0), stepped: Cell::new(false), animated: Cell::new(false) }
    }

    /// How many clicks the slide has had. To add clicks to it, use `steps`.
    pub fn click_step(&self) -> u32 {
        self.stepped.set(true);
        self.data.click_step
    }

    /// Asks the slide for `count` clicks, and gives how many of them have happened.
    pub fn steps(&self, count: u32) -> u32 {
        self.stepped.set(true);
        self.steps.set(self.steps.get().max(count));
        self.data.click_step.min(count)
    }

    /// Seconds since the slide appeared.
    pub fn time(&self) -> f32 {
        self.animated.set(true);
        self.data.time
    }

    pub fn slide_width(&self) -> f32 {
        self.data.slide_width
    }

    pub fn slide_height(&self) -> f32 {
        self.data.slide_height
    }

    /// The width of the box the tag sits in.
    pub fn width(&self) -> f32 {
        self.data.width
    }

    /// The height of the box the tag sits in, or 0 if it grows with its content.
    pub fn height(&self) -> f32 {
        self.data.height
    }

    pub fn theme(&self) -> &Theme {
        &self.data.theme
    }

    /// The bytes of a file in the talk's `assets` folder.
    pub fn asset(&self, path: &str) -> Option<Vec<u8>> {
        (self.assets)(path)
    }

    /// A file in the talk's `assets` folder, if it is text.
    pub fn asset_text(&self, path: &str) -> Option<String> {
        String::from_utf8(self.asset(path)?).ok()
    }

    /// `element`, with what the component read to make it.
    pub fn finish(self, element: Element) -> RenderOutput {
        RenderOutput::new(element, self.steps.get(), self.stepped.get(), self.animated.get())
    }
}

/// A type a component argument can have. Bad or missing values never fail.
pub trait Prop: Sized {
    fn kind() -> PropKind;
    const OPTIONAL: bool = false;
    fn from_attr(value: Option<&str>) -> Self;
}

impl Prop for String {
    fn kind() -> PropKind {
        PropKind::Text
    }
    fn from_attr(value: Option<&str>) -> Self {
        value.unwrap_or_default().to_string()
    }
}

impl Prop for u32 {
    fn kind() -> PropKind {
        PropKind::Whole
    }
    fn from_attr(value: Option<&str>) -> Self {
        value.and_then(|v| v.trim().parse().ok()).unwrap_or(0)
    }
}

impl Prop for i32 {
    fn kind() -> PropKind {
        PropKind::Whole
    }
    fn from_attr(value: Option<&str>) -> Self {
        value.and_then(|v| v.trim().parse().ok()).unwrap_or(0)
    }
}

impl Prop for f32 {
    fn kind() -> PropKind {
        PropKind::Number
    }
    /// A value that isn't finite, such as `inf` or `1e999`, is 0 too.
    fn from_attr(value: Option<&str>) -> Self {
        value.and_then(|v| v.trim().parse::<f32>().ok()).filter(|v| v.is_finite()).unwrap_or(0.0)
    }
}

impl Prop for bool {
    fn kind() -> PropKind {
        PropKind::Flag
    }
    fn from_attr(value: Option<&str>) -> Self {
        value == Some("true")
    }
}

impl Prop for Color {
    fn kind() -> PropKind {
        PropKind::Color
    }
    fn from_attr(value: Option<&str>) -> Self {
        value.and_then(Color::parse).unwrap_or(Color::rgb(0, 0, 0))
    }
}

/// The path of a picture in the talk.
#[derive(Clone, Debug, PartialEq)]
pub struct PictureFile(pub String);

impl Prop for PictureFile {
    fn kind() -> PropKind {
        PropKind::File(["png", "jpg", "jpeg", "svg", "webp"].map(String::from).to_vec())
    }
    fn from_attr(value: Option<&str>) -> Self {
        PictureFile(String::from_attr(value))
    }
}

/// The path of a table of data in the talk.
#[derive(Clone, Debug, PartialEq)]
pub struct DataFile(pub String);

impl Prop for DataFile {
    fn kind() -> PropKind {
        PropKind::File(vec!["csv".to_string()])
    }
    fn from_attr(value: Option<&str>) -> Self {
        DataFile(String::from_attr(value))
    }
}

impl<T: Prop> Prop for Option<T> {
    fn kind() -> PropKind {
        T::kind()
    }
    const OPTIONAL: bool = true;
    fn from_attr(value: Option<&str>) -> Self {
        value.map(|v| T::from_attr(Some(v)))
    }
}

#[derive(Clone, Copy)]
pub struct NativeComponent {
    pub name: &'static str,
    pub schema: fn() -> ComponentSchema,
    pub render: fn(&Props, &Ctx) -> Element,
}

#[derive(Default)]
pub struct Registry {
    components: HashMap<&'static str, NativeComponent>,
}

impl Registry {
    pub fn add(&mut self, component: NativeComponent) {
        self.components.insert(component.name, component);
    }

    pub fn get(&self, name: &str) -> Option<NativeComponent> {
        self.components.get(name).copied()
    }
}
