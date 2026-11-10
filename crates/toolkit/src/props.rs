use crate::color::{Color, Theme};
use crate::element::Element;
use std::collections::{BTreeMap, HashMap};

/// A tag's attributes, as written.
pub struct Props(pub BTreeMap<String, String>);

/// What a component can read about the moment it is drawn.
pub struct Ctx {
    pub click_step: u32,
    pub time: f32,
    pub slide_width: f32,
    pub slide_height: f32,
    pub theme: Theme,
}

/// A type a component argument can have. Bad or missing values never fail.
pub trait Prop: Sized {
    const TYPE_NAME: &'static str;
    fn from_attr(value: Option<&str>) -> Self;
}

impl Prop for String {
    const TYPE_NAME: &'static str = "String";
    fn from_attr(value: Option<&str>) -> Self {
        value.unwrap_or_default().to_string()
    }
}

impl Prop for u32 {
    const TYPE_NAME: &'static str = "u32";
    fn from_attr(value: Option<&str>) -> Self {
        value.and_then(|v| v.trim().parse().ok()).unwrap_or(0)
    }
}

impl Prop for f32 {
    const TYPE_NAME: &'static str = "f32";
    fn from_attr(value: Option<&str>) -> Self {
        value.and_then(|v| v.trim().parse().ok()).unwrap_or(0.0)
    }
}

impl Prop for bool {
    const TYPE_NAME: &'static str = "bool";
    fn from_attr(value: Option<&str>) -> Self {
        value == Some("true")
    }
}

impl Prop for Color {
    const TYPE_NAME: &'static str = "Color";
    fn from_attr(value: Option<&str>) -> Self {
        value.and_then(Color::parse).unwrap_or(Color::rgb(0, 0, 0))
    }
}

impl<T: Prop> Prop for Option<T> {
    const TYPE_NAME: &'static str = "Option";
    fn from_attr(value: Option<&str>) -> Self {
        value.map(|v| T::from_attr(Some(v)))
    }
}

pub struct PropInfo {
    pub name: &'static str,
    pub type_name: &'static str,
}

#[derive(Clone, Copy)]
pub struct NativeComponent {
    pub name: &'static str,
    pub schema: &'static [PropInfo],
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
