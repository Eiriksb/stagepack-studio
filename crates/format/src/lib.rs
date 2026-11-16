//! The talk file format: Markdown in, a model with exact byte positions out.

mod attrs;
mod edit;
mod model;
mod notes;
mod parse;
mod reparse;
mod rich;
mod rich_edit;
mod rich_write;
mod settings;
mod slide_ops;
mod split;

pub use edit::{Change, SourceEdit, set_attr, set_setting};
pub use model::{Attr, Block, Notes, Slide, SlideError, Span, Style, Tag, Talk};
pub use parse::{MAX_BODY as MAX_SLIDE_BYTES, parse_talk, slides_parsed_on_this_thread};
pub use reparse::reparse;
pub use rich::{FlowPlace, MAX_LEVEL, Para, ParaKind, ParaSource, TextFlow, text_flows, uneditable_reason};
pub use rich_edit::{Motion, Pos, Selection, Shared, StyleBit, StyleState, TextEdit, is_bidi_control};
pub use rich_write::{storable, write_flow, write_inline, write_into_region};
pub use settings::{SettingEntry, Settings};
pub use slide_ops::{
    TALK_SETTINGS, delete_slide, duplicate_slide, insert_slide, is_hidden, move_slide, remove_setting, set_align, set_hidden, set_notes,
    slide_text_for_copy,
};
pub use split::{RawSlide, split_slides};
