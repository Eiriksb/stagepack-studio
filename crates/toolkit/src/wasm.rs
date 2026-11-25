//! A component's side of the interface, once it is compiled to WebAssembly: JSON in the
//! module's memory, and a pointer and length packed together as `(ptr << 32) | len`.

use crate::element::problem;
use crate::interface::{RenderInput, RenderOutput};
use crate::props::{Ctx, NativeComponent, Props};
use serde::Serialize;

#[link(wasm_import_module = "toolkit")]
unsafe extern "C" {
    /// The file at the path, placed through `alloc`, or 0 if it can't be had.
    fn read_asset(ptr: u32, len: u32) -> u64;
}

/// Room for `len` bytes, which the app fills and hands back.
pub fn alloc(len: u32) -> u32 {
    Box::into_raw(vec![0u8; len as usize].into_boxed_slice()) as *mut u8 as u32
}

/// # Safety
///
/// `ptr` and `len` are from one call of `alloc`, and are only taken once.
unsafe fn take(ptr: u32, len: u32) -> Vec<u8> {
    let bytes = std::ptr::slice_from_raw_parts_mut(ptr as *mut u8, len as usize);
    unsafe { Box::from_raw(bytes) }.into_vec()
}

/// The bytes stay allocated: the app reads them and then drops the instance.
fn answer(value: &impl Serialize) -> u64 {
    let bytes = serde_json::to_vec(value).expect("these types are always JSON").leak();
    ((bytes.as_ptr() as u64) << 32) | bytes.len() as u64
}

fn asset(path: &str) -> Option<Vec<u8>> {
    let packed = unsafe { read_asset(path.as_ptr() as u32, path.len() as u32) };
    // The app placed these bytes through `alloc`, and gives them to us once.
    (packed != 0).then(|| unsafe { take((packed >> 32) as u32, packed as u32) })
}

pub fn schema(component: NativeComponent) -> u64 {
    answer(&(component.schema)())
}

pub fn render(ptr: u32, len: u32, component: NativeComponent) -> u64 {
    // The app made this with `alloc`, and passes it once.
    let input = unsafe { take(ptr, len) };
    let output = match serde_json::from_slice::<RenderInput>(&input) {
        Ok(RenderInput { props, ctx, state, events }) => {
            let ctx = Ctx::with_input(ctx, state, events, &asset);
            let element = (component.render)(&Props(props), &ctx);
            ctx.finish(element)
        }
        Err(_) => RenderOutput::new(problem("This component can't read what the app sent it"), 0, false, false),
    };
    answer(&output)
}
