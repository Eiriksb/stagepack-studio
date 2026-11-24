//! Gives the add-on's own stack room for the deepest slide the Markdown reader is handed. The
//! reader goes one level further down for every level a slide nests, and a slide as long as
//! the format allows can nest thousands of levels before it is refused, which takes far more
//! than the 1 MiB a WebAssembly module gets by default. Only the add-on's build is changed,
//! which a `.cargo` configuration couldn't keep to this crate.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        println!("cargo:rustc-cdylib-link-arg=-zstack-size=33554432");
    }
}
