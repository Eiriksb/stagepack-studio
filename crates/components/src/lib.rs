//! A talk's own components: checking their source, compiling them to WebAssembly, and
//! running them in a sandbox.

mod compile;
mod sandbox;
mod source_check;

pub use compile::{compile, compile_with_limit, Bundle, COMPILE_TIME_LIMIT};
pub use sandbox::{
    Assets, ComponentError, WasmComponent, MAX_ELEMENTS, MAX_ELEMENT_DEPTH, MAX_MODULE_BYTES, MAX_OUTPUT_BYTES, MEMORY_LIMIT_BYTES,
    RENDER_TIME_LIMIT,
};
pub use source_check::{check_source, component_tag, SourceInfo};

/// Something wrong with a component's source, at a line of it. A line of 0 is the file as a whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub file: String,
    pub line: u32,
    pub message: String,
}
