//! The functions the app calls, as add-on interface 1 has them. Each `u64` is a pointer and a
//! length, `(ptr << 32) | len`. What's handed back is never freed: each run is an instance
//! of its own, which goes once the run is over.

use addon_abi::ABI_VERSION;

fn handed_back(bytes: Vec<u8>) -> u64 {
    let bytes = Box::leak(bytes.into_boxed_slice());
    ((bytes.as_ptr() as usize as u64) << 32) | bytes.len() as u64
}

#[unsafe(no_mangle)]
pub extern "C" fn addon_abi() -> u32 {
    ABI_VERSION
}

/// Room for `len` bytes, which the app writes the input into and hands to [`run`].
#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: u32) -> u32 {
    let room = Box::leak(vec![0u8; len as usize].into_boxed_slice());
    room.as_mut_ptr() as usize as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn info() -> u64 {
    handed_back(crate::info_json().into_bytes())
}

/// Takes the bundle at `ptr`, which [`alloc`] made room for, and gives back the answer's.
#[unsafe(no_mangle)]
pub extern "C" fn run(ptr: u32, len: u32) -> u64 {
    // SAFETY: `alloc` made exactly this room, as a boxed slice of `len` bytes, and the app
    // hands it back once, to this run.
    let input = unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as usize as *mut u8, len as usize)) };
    handed_back(crate::run(input.into_vec()))
}
