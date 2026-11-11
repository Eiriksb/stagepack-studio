//! Building a component with the bundled compiler.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::source_check::check_source;
use crate::Diagnostic;

pub const COMPILE_TIME_LIMIT: Duration = Duration::from_secs(60);

const RUSTC: &str = if cfg!(windows) { "rustc.exe" } else { "rustc" };
const SOURCE_NAME: &str = "component.rs";
const OUTPUT_NAME: &str = "component.wasm";
/// More than any real list of errors, as the compiler can be made to write without end.
const MAX_ERROR_OUTPUT: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bundle {
    pub root: PathBuf,
}

impl Bundle {
    /// `STUDIO_BUNDLE` if set, or else a `bundle` folder beside the executable.
    pub fn find() -> Option<Bundle> {
        let root = match std::env::var_os("STUDIO_BUNDLE") {
            Some(root) => PathBuf::from(root),
            None => std::env::current_exe().ok()?.parent()?.join("bundle"),
        };
        Bundle::in_folder(root)
    }

    fn in_folder(root: PathBuf) -> Option<Bundle> {
        root.join("bin").join(RUSTC).is_file().then_some(Bundle { root })
    }
}

/// Compiles a component's source to WebAssembly. `file` is the talk-relative name used in
/// diagnostics, such as `components/score.rs`.
pub fn compile(bundle: &Bundle, file: &str, source: &str) -> Result<Vec<u8>, Vec<Diagnostic>> {
    compile_with_limit(bundle, file, source, COMPILE_TIME_LIMIT)
}

pub fn compile_with_limit(bundle: &Bundle, file: &str, source: &str, limit: Duration) -> Result<Vec<u8>, Vec<Diagnostic>> {
    check_source(file, source)?;
    let failed = |message: String| vec![Diagnostic::new(file, 0, message)];

    let scratch = tempfile::Builder::new()
        .prefix("component-build")
        .tempdir()
        .map_err(|error| failed(format!("A folder to build in couldn't be made: {error}")))?;
    let root = std::path::absolute(&bundle.root).map_err(|error| failed(format!("The compiler couldn't be found: {error}")))?;
    std::fs::write(scratch.path().join(SOURCE_NAME), source)
        .map_err(|error| failed(format!("The source couldn't be copied to build it: {error}")))?;

    let mut command = rustc_command(&root, scratch.path());
    let (status, errors) = match run_for(&mut command, limit) {
        Ok(Some(finished)) => finished,
        Ok(None) => {
            return Err(failed(format!("Building this took longer than {} seconds, so it was stopped", limit.as_secs_f32())));
        }
        Err(error) => return Err(failed(format!("The compiler couldn't be started: {error}"))),
    };
    if !status.success() {
        let mut found = diagnostics(file, &errors);
        if found.is_empty() {
            found = failed(format!("The compiler stopped without giving a reason ({status})"));
        }
        return Err(found);
    }
    std::fs::read(scratch.path().join(OUTPUT_NAME)).map_err(|error| failed(format!("The compiler's output couldn't be read: {error}")))
}

fn joined(start: &str, path: &Path, end: &str) -> OsString {
    let mut text = OsString::from(start);
    text.push(path);
    text.push(end);
    text
}

/// Everything from this computer stays out: the environment is cleared, and the source and
/// output are named relative to the scratch folder it runs in. That also keeps the scratch
/// folder's own path from the linker, which takes a `%` in its output path as a placeholder.
fn rustc_command(root: &Path, scratch: &Path) -> Command {
    let toolkit = root.join("toolkit");
    let mut command = Command::new(root.join("bin").join(RUSTC));
    command
        .current_dir(scratch)
        .env_clear()
        .env("TMPDIR", scratch)
        .env("TMP", scratch)
        .env("TEMP", scratch)
        .arg("--sysroot")
        .arg(root)
        .args(["--crate-name", "component", "--edition", "2024", "--crate-type", "cdylib"])
        .args(["--target", "wasm32-unknown-unknown"])
        .args(["-C", "opt-level=1", "-C", "strip=debuginfo"])
        .arg("--remap-path-prefix")
        .arg(joined("", scratch, "=components"))
        .arg("-L")
        .arg(joined("dependency=", &toolkit.join("deps"), ""))
        .arg("--extern")
        .arg(joined("toolkit=", &toolkit.join("libtoolkit.rlib"), ""))
        .arg("--error-format=json")
        .args(["-o", OUTPUT_NAME, SOURCE_NAME])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SYSTEMROOT") {
        command.env("SYSTEMROOT", root);
    }
    command
}

/// Reads to the end on a thread of its own, so a full pipe can't stall the compiler.
fn read_in_background(mut pipe: impl Read + Send + 'static) -> mpsc::Receiver<String> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut text = Vec::new();
        let _ = pipe.by_ref().take(MAX_ERROR_OUTPUT).read_to_end(&mut text);
        let _ = std::io::copy(&mut pipe, &mut std::io::sink());
        let _ = sender.send(String::from_utf8_lossy(&text).into_owned());
    });
    receiver
}

/// How the command ended and what it wrote to standard error, or `None` if it was killed
/// at the limit.
fn run_for(command: &mut Command, limit: Duration) -> std::io::Result<Option<(ExitStatus, String)>> {
    let mut child = command.spawn()?;
    let errors = child.stderr.take().map(read_in_background);
    let deadline = Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Not forever: something the compiler started may still hold the pipe open.
                let text = errors.and_then(|errors| errors.recv_timeout(Duration::from_secs(1)).ok());
                return Ok(Some((status, text.unwrap_or_default())));
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            outcome => {
                let _ = child.kill();
                let _ = child.wait();
                return outcome.map(|_| None);
            }
        }
    }
}

fn diagnostics(file: &str, stderr: &str) -> Vec<Diagnostic> {
    let mut found = Vec::new();
    for line in stderr.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            // A line cut short at `MAX_ERROR_OUTPUT` says nothing worth showing.
            if !line.starts_with('{') {
                found.push(Diagnostic::new(file, 0, line));
            }
            continue;
        };
        // An internal error has a longer level than plain `error`.
        if !value["level"].as_str().is_some_and(|level| level.starts_with("error")) {
            continue;
        }
        let message = value["message"].as_str().unwrap_or_default().to_string();
        let primary = value["spans"].as_array().and_then(|spans| spans.iter().find(|span| span["is_primary"] == true));
        match primary {
            Some(span) => {
                let name = span["file_name"].as_str().unwrap_or_default();
                let names_the_source = Path::new(name).file_name().is_some_and(|last| last == SOURCE_NAME);
                let line = span["line_start"].as_u64().unwrap_or(0) as u32;
                found.push(Diagnostic::new(if names_the_source { file } else { name }, line, message));
            }
            // This one only counts the others, but something like a linker failure has no span either.
            None if message.starts_with("aborting due to") => {}
            None => found.push(Diagnostic::new(file, 0, message)),
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_without_a_compiler_is_not_a_bundle() {
        let folder = tempfile::tempdir().unwrap();
        assert_eq!(Bundle::in_folder(folder.path().to_path_buf()), None);
        std::fs::create_dir(folder.path().join("bin")).unwrap();
        std::fs::write(folder.path().join("bin").join(RUSTC), "").unwrap();
        let found = Bundle::in_folder(folder.path().to_path_buf());
        assert_eq!(found, Some(Bundle { root: folder.path().to_path_buf() }));
    }
}
