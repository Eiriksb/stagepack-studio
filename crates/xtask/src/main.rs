//! `package`: the dev-only packaging tool.
//!
//!   package icon OUT_DIR       draws the icon (PNGs and .ico)
//!   package notices OUT_FILE   gathers every licence notice (and writes notices/rust-COPYRIGHT.html beside it)
//!   package stage OUT_DIR [--bundle DIR]   stages the installed app folder from shipped release builds (keep OUT_DIR inside target/)

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
            xtask::notices::write_notices(&file)
        }
        Some("stage") => {
            const USAGE: &str = "usage: package stage OUT_DIR [--bundle DIR]";
            let out = PathBuf::from(args.get(1).ok_or(USAGE)?);
            let bundle = match args.get(2).map(String::as_str) {
                None => None,
                Some("--bundle") if args.len() == 4 => Some(PathBuf::from(&args[3])),
                Some(_) => return Err(USAGE.into()),
            };
            let bytes = xtask::stage::stage(&out, bundle.as_deref())?;
            println!("The app folder {} is {} MB.", out.display(), bytes.div_ceil(1024 * 1024));
            Ok(())
        }
        Some(other) => Err(format!("unknown subcommand '{other}' (known: icon, notices, stage)")),
        None => Err("usage: package <icon OUT_DIR | notices OUT_FILE | stage OUT_DIR [--bundle DIR]>".into()),
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
