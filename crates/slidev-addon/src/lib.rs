//! The Slidev add-on: a talk to a Slidev talk and back, as bundles of files. It never touches
//! a file itself: the app gives it the files and writes what it gives back.
//!
//! It builds as a native library, for the tests, and as a `wasm32-unknown-unknown` module,
//! which is the add-on the app runs.

mod escape;
pub mod export;
mod fonts;
mod shapes;
#[cfg(target_arch = "wasm32")]
mod wasm;

use addon_abi::{decode, encode, Bundle, ARGS, ERROR, OP, REPORT};

pub use export::{export, is_baked, is_code_name, Baked, ExportArgs};

/// The add-on's name, which the app knows it by.
pub const NAME: &str = "slidev";

/// What `info` says: the add-on's name, its version and its operations.
pub fn info_json() -> String {
    serde_json::json!({ "name": NAME, "version": env!("CARGO_PKG_VERSION"), "operations": ["export", "import"] }).to_string()
}

/// Does what the bundle `input` asks and gives back the bundle that answers it: the files and
/// the report, or only `.error`. The input is let go of as soon as it's read, as the files
/// can be large.
pub fn run(input: Vec<u8>) -> Vec<u8> {
    let answer = answer(input).unwrap_or_else(|sentence| vec![(ERROR.to_string(), sentence.into_bytes())]);
    encode(&answer)
}

fn answer(input: Vec<u8>) -> Result<Bundle, String> {
    let mut files = decode(&input)?;
    drop(input);
    let mut take = |name: &str| files.iter().position(|(file, _)| file == name).map(|at| files.remove(at).1);
    let op = take(OP).ok_or("The add-on was given no operation to do.")?;
    let args = take(ARGS).unwrap_or_else(|| b"{}".to_vec());
    match op.as_slice() {
        b"export" => {
            let args: ExportArgs = serde_json::from_slice(&args).map_err(|error| format!("The export's arguments can't be read: {error}."))?;
            let (mut out, report) = export(&args, &files)?;
            drop(files);
            out.push((REPORT.to_string(), serde_json::to_vec(&report).map_err(|error| error.to_string())?));
            Ok(out)
        }
        b"import" => Err("This version of the add-on can't import a Slidev talk yet.".to_string()),
        other => Err(format!("The add-on has no operation called {:?}.", String::from_utf8_lossy(other))),
    }
}
