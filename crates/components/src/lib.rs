//! A talk's own components: checking their source, compiling them to WebAssembly, and
//! running them in a sandbox. And the add-ons the app runs in a sandbox of the same kind.

pub mod addon;
mod compile;
mod index;
mod sandbox;
mod source_check;

pub use compile::{compile, compile_with_limit, Bundle, COMPILE_TIME_LIMIT};
pub use index::{safe_stem, source_hash, BuildEntry, BuildIndex, INDEX_FILE};
pub use sandbox::{
    Assets, ComponentError, WasmComponent, MAX_ASSET_PATH_BYTES, MAX_ELEMENTS, MAX_ELEMENT_DEPTH, MAX_MODULE_BYTES, MAX_OUTPUT_BYTES, MAX_PROPS,
    MAX_SCHEMA_STRING_BYTES, MAX_STATE_BYTES, MAX_TEXT_BYTES, MEMORY_LIMIT_BYTES, RENDER_TIME_LIMIT,
};
pub use source_check::{check_source, component_tag, SourceInfo};

/// The most characters a diagnostic's message keeps. A compiler's message quotes the
/// source, and a source can make it as long as it likes.
pub const MAX_DIAGNOSTIC_CHARS: usize = 500;

/// Something wrong with a component's source, at a line of it. A line of 0 is the file as a whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub file: String,
    pub line: u32,
    pub message: String,
}

impl Diagnostic {
    /// A diagnostic with its message held to `MAX_DIAGNOSTIC_CHARS`.
    pub fn new(file: impl Into<String>, line: u32, message: impl AsRef<str>) -> Diagnostic {
        Diagnostic { file: file.into(), line, message: shortened(message.as_ref(), MAX_DIAGNOSTIC_CHARS) }
    }
}

/// `text`, or its first `most - 1` characters and "…" if it has more than `most`.
pub fn shortened(text: &str, most: usize) -> String {
    match text.char_indices().nth(most) {
        None => text.to_string(),
        Some(_) => {
            let end = text.char_indices().nth(most.saturating_sub(1)).map_or(text.len(), |(at, _)| at);
            format!("{}…", &text[..end])
        }
    }
}
