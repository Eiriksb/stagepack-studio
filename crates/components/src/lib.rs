//! A talk's own components: checking their source and compiling them to WebAssembly.

mod compile;
mod source_check;

pub use compile::{compile, compile_with_limit, Bundle, COMPILE_TIME_LIMIT};
pub use source_check::{check_source, component_tag, SourceInfo};

/// Something wrong with a component's source, at a line of it. A line of 0 is the file as a whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub file: String,
    pub line: u32,
    pub message: String,
}
