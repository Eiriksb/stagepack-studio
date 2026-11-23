//! `package`: the dev-only packaging tool.
//!
//!   package icon OUT_DIR       draws the icon (PNGs and .ico)
//!   package notices OUT_FILE   gathers every licence notice

use std::path::PathBuf;
use std::process::ExitCode;

fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("icon") => {
            let dir = args.get(1).ok_or("usage: package icon OUT_DIR")?;
            xtask::icon::write_icons(&PathBuf::from(dir))
        }
        Some("notices") => {
            let file = PathBuf::from(args.get(1).ok_or("usage: package notices OUT_FILE")?);
            let text = xtask::notices::notices_for_workspace()?;
            if let Some(parent) = file.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent).map_err(|e| format!("can't create {}: {e}", parent.display()))?;
            }
            std::fs::write(&file, text).map_err(|e| format!("can't write {}: {e}", file.display()))
        }
        Some(other) => Err(format!("unknown subcommand '{other}' (known: icon, notices)")),
        None => Err("usage: package <icon OUT_DIR | notices OUT_FILE>".into()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("package: {e}");
            ExitCode::FAILURE
        }
    }
}
