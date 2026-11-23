//! `package`: the dev-only packaging tool.
//!
//!   package icon OUT_DIR       draws the icon (PNGs and .ico)
//!   package notices OUT_FILE   gathers every licence notice (and writes notices/rust-COPYRIGHT.html beside it)
//!   package linux [--staged DIR] [--bundle DIR]   packs the Linux .tar.zst into target/package (stages first unless DIR is given)
//!   package windows-folder OUT_DIR [--bundle DIR]   the same as stage, on Windows (refuses elsewhere)
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
        Some(command @ ("stage" | "windows-folder")) => {
            let usage = format!("usage: package {command} OUT_DIR [--bundle DIR]");
            let out = PathBuf::from(args.get(1).ok_or(usage.as_str())?);
            let bundle = match args.get(2).map(String::as_str) {
                None => None,
                Some("--bundle") if args.len() == 4 => Some(PathBuf::from(&args[3])),
                Some(_) => return Err(usage.clone()),
            };
            let bytes = if command == "windows-folder" {
                xtask::windows::windows_folder(&out, bundle.as_deref())?
            } else {
                xtask::stage::stage(&out, bundle.as_deref())?
            };
            println!("The app folder {} is {} MB.", out.display(), bytes.div_ceil(1024 * 1024));
            Ok(())
        }
        Some("linux") => {
            const USAGE: &str = "usage: package linux [--staged DIR] [--bundle DIR]";
            let (mut staged, mut bundle) = (None, None);
            let mut rest = args[1..].iter();
            while let Some(flag) = rest.next() {
                let value = PathBuf::from(rest.next().ok_or(USAGE)?);
                match flag.as_str() {
                    "--staged" => staged = Some(value),
                    "--bundle" => bundle = Some(value),
                    _ => return Err(USAGE.into()),
                }
            }
            let repo = xtask::stage::repo_root();
            let staged = match staged {
                Some(dir) => dir,
                None => {
                    let dir = repo.join("target/package-stage");
                    xtask::stage::stage(&dir, bundle.as_deref())?;
                    dir
                }
            };
            let tarball = xtask::linux::pack(&staged, &repo.join("target/package"))?;
            let bytes = std::fs::metadata(&tarball).map_err(|e| e.to_string())?.len();
            println!("{} is {} MB.", tarball.display(), bytes.div_ceil(1024 * 1024));
            Ok(())
        }
        Some(other) => Err(format!("unknown subcommand '{other}' (known: icon, notices, stage, windows-folder, linux)")),
        None => Err("usage: package <icon OUT_DIR | notices OUT_FILE | stage OUT_DIR [--bundle DIR] | windows-folder OUT_DIR [--bundle DIR] | linux [--staged DIR] [--bundle DIR]>".into()),
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
