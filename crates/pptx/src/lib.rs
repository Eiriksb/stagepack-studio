//! PowerPoint export: a Rust port of the OOXML writer in `slidev-pptx-export`, whose XML forms
//! were measured against real PowerPoint. `tests/parts.rs` checks every part byte for byte
//! against the original's output, which is what keeps those forms intact.

pub mod master;
pub mod package;
pub mod presentation;
pub mod theme;
pub mod xml;
