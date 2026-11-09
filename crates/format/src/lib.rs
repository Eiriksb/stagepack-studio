//! The talk file format: Markdown in, a model with exact byte positions out.

mod settings;
mod split;

pub use settings::{SettingEntry, Settings};
pub use split::{RawSlide, split_slides};
