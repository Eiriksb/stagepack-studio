use std::path::Path;
use std::process::Command;

/// The Slidev add-on reads a talk with this crate inside WebAssembly, so it must build there.
/// Ignored, as it builds the crate again; the components step of test-all runs it.
#[test]
#[ignore = "builds slides-format for wasm32; run by the components step of test-all"]
fn slides_format_builds_for_wasm32() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .current_dir(&workspace)
        .args(["build", "-p", "slides-format", "--target", "wasm32-unknown-unknown", "--target-dir"])
        .arg(workspace.join("target/wasm-check"))
        .output()
        .expect("cargo couldn't be started");
    assert!(output.status.success(), "slides-format doesn't build for wasm32:\n{}", String::from_utf8_lossy(&output.stderr));
}
