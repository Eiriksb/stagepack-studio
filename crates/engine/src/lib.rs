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
mod text_geometry;
mod timeline;
pub use builtins::builtin_registry;
pub use chart::ChartKind;
/// The chart's workings, for its tests.
#[doc(hidden)]
pub mod chart_parts {
    pub use crate::chart::{Axis, ChartData, axis_for, parse_csv, parse_inline, plot_area, series_color};
}
pub use deck::Deck;
pub use elements::{MODEL_IN_TEXT, MODEL_NO_FILE, MODEL_VIEW_UNREAD, MOST_VIEWS as MOST_MODEL_VIEWS, VIDEO_IN_TEXT, model_views as read_model_views, VIDEO_NO_FILE};
/// A drawn picture, such as a slide's thumbnail.
pub use tiny_skia::Pixmap;
pub use display::*;
pub use files::*;
pub use highlight::*;
pub use layouts::*;
pub use native::*;
pub use packed::*;
pub use pictures::{DrawModel, MadeStill, MeasureVideo, PictureCache, PictureFormat, stills_decoded_on_this_thread, StillJob, StillWanted, icon_names, icon_preview, picture_as_png, picture_format};
pub use render::*;
pub use slide::*;
pub use talk_components::{
    COMPONENTS_FOLDER, ComponentInfo, ComponentState, MAX_ASSETS_REMEMBERED, MAX_COMPONENTS, MAX_INDEX_BYTES, MAX_SOURCE_BYTES, SLIDE_RENDER_BUDGET, SchemaLoad, TalkComponents,
};
pub use text::*;
pub use text_geometry::{CaretRect, ElementBox, Hit, LineMove, PlacedPara, RegionBox, SelectionRects, TextLayout};
pub use timeline::*;
