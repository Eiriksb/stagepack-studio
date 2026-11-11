mod builtins;
mod chart;
mod deck;
mod display;
mod elements;
mod files;
mod flow;
mod highlight;
mod layouts;
mod native;
mod packed;
mod pictures;
mod placeholder;
mod render;
mod slide;
mod talk_components;
mod text;
mod timeline;
pub use builtins::builtin_registry;
pub use chart::ChartKind;
/// The chart's workings, for its tests.
#[doc(hidden)]
pub mod chart_parts {
    pub use crate::chart::{Axis, ChartData, axis_for, parse_csv, parse_inline, plot_area, series_color};
}
pub use deck::Deck;
pub use display::*;
pub use files::*;
pub use highlight::*;
pub use layouts::*;
pub use native::*;
pub use packed::*;
pub use pictures::PictureCache;
pub use render::*;
pub use slide::*;
pub use talk_components::{ComponentInfo, ComponentState, SLIDE_RENDER_BUDGET, TalkComponents};
pub use text::*;
pub use timeline::*;
