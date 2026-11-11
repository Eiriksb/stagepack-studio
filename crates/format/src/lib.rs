//! The talk file format: Markdown in, a model with exact byte positions out.

mod attrs;
mod edit;
mod model;
mod parse;
mod settings;
mod slide_ops;
mod split;

pub use edit::{Change, SourceEdit, set_attr, set_setting};
pub use model::{Attr, Block, Notes, Slide, SlideError, Span, Style, Tag, Talk};
pub use parse::parse_talk;
pub use settings::{SettingEntry, Settings};
pub use slide_ops::{
    TALK_SETTINGS, delete_slide, duplicate_slide, insert_slide, is_hidden, move_slide, remove_setting, set_hidden, set_notes,
};
pub use split::{RawSlide, split_slides};
