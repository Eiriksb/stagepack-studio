//! The talk file format: Markdown in, a model with exact byte positions out.

mod attrs;
mod model;
mod parse;
mod settings;
mod split;

pub use model::{Attr, Block, Notes, Slide, SlideError, Span, Style, Tag, Talk};
pub use parse::parse_talk;
pub use settings::{SettingEntry, Settings};
pub use split::{RawSlide, split_slides};
