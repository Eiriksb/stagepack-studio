//! `package`: the dev-only packaging tool.
//!
//!   package icon OUT_DIR       draws the icon (PNGs and .ico)
//!   package notices OUT_FILE   gathers every licence notice (and writes notices/rust-COPYRIGHT.html beside it)
//!   package linux [--staged DIR] [--bundle DIR]   packs the Linux .tar.zst into target/package (stages first unless DIR is given)
//!   package windows-folder OUT_DIR [--bundle DIR]   the same as stage, on Windows (refuses elsewhere)
//!   package stage OUT_DIR [--bundle DIR]   stages the installed app folder from shipped release builds (keep OUT_DIR inside target/;
//!                                          STUDIO_PACKAGE_TARGET picks the build folder, inside target/, default target/package-build)
//!   package glibc-check DIR    prints the glibc each ELF file under DIR needs, and fails if one is above 2.35
//!   package addon --key FILE [--out DIR]   builds the Slidev add-on for WebAssembly, signs it with the key in FILE and
//!                                          writes slidev-<version>.spaddon in DIR (default target/package)
//!   package addon-release [--package FILE]   writes index-abi<abi>-slidev<major>.json beside the signed package (default target/package/slidev-<version>.spaddon)
//!                                          for its release, and prints the steps to publish it
//!   package addon-keygen FILE  makes a new signing key in FILE (mode 0600, never written over) and prints its public half

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
        Some("glibc-check") => {
            let dir = PathBuf::from(args.get(1).filter(|_| args.len() == 2).ok_or("usage: package glibc-check DIR")?);
            glibc_check(&dir)
        }
        Some("addon") => {
            const USAGE: &str = "usage: package addon --key FILE [--out DIR]";
            let (mut key, mut out) = (None, None);
            let mut rest = args[1..].iter();
            while let Some(flag) = rest.next() {
                let value = PathBuf::from(rest.next().ok_or(USAGE)?);
                match flag.as_str() {
                    "--key" => key = Some(value),
                    "--out" => out = Some(value),
                    _ => return Err(USAGE.into()),
                }
            }
            let key = key.ok_or(USAGE)?;
            let out = out.unwrap_or_else(|| xtask::stage::repo_root().join("target/package"));
            let (file, sha256) = xtask::addon::package_addon(&key, &out)?;
            println!("{}", file.display());
            println!("sha256 {sha256}");
            if !xtask::addon::is_built_in(&key)? {
                eprintln!("package: note: this key isn't the one built into the app, so the app will refuse this package");
            }
            Ok(())
        }
        Some("addon-release") => {
            const USAGE: &str = "usage: package addon-release [--package FILE]";
            let package = match args.len() {
                1 => xtask::addon::default_package()?,
                3 if args[1] == "--package" => PathBuf::from(&args[2]),
                _ => return Err(USAGE.into()),
            };
            let (file, index) = xtask::addon::write_release_index(&package, &xtask::addon::built_in_key()?)?;
            println!("{}", file.display());
            println!("{}", serde_json::to_string_pretty(&index).map_err(|e| e.to_string())?);
            println!("{}", xtask::addon::publishing_steps(&package, &file, &index));
            Ok(())
        }
        Some("addon-keygen") => {
            let file = PathBuf::from(args.get(1).filter(|_| args.len() == 2).ok_or("usage: package addon-keygen FILE")?);
            let public = xtask::addon::keygen(&file)?;
            println!("Made the signing key {}. Its public half, for crates/app/src/addons/key.rs:", file.display());
            println!("{}", xtask::addon::public_key_line(&public));
            Ok(())
        }
        Some(other) => Err(format!("unknown subcommand '{other}' (known: icon, notices, stage, windows-folder, linux, glibc-check, addon, addon-keygen)")),
        None => Err("usage: package <icon OUT_DIR | notices OUT_FILE | stage OUT_DIR [--bundle DIR] | windows-folder OUT_DIR [--bundle DIR] | linux [--staged DIR] [--bundle DIR] | glibc-check DIR | addon --key FILE [--out DIR] | addon-release [--package FILE] | addon-keygen FILE>".into()),
    }
}

/// One line per ELF file under `dir`, `<need> <path relative to dir>`, then the highest; an error names each file above the ceiling.
fn glibc_check(dir: &std::path::Path) -> Result<(), String> {
    use xtask::elf::{GLIBC_CEILING, elf_files, max_glibc, version_text};
    let files = elf_files(dir);
    if files.is_empty() {
        return Err(format!("no ELF files under {}", dir.display()));
    }
    let (mut highest, mut over) = (None, Vec::new());
    for file in &files {
        let need = max_glibc(file)?;
        let relative = file.strip_prefix(dir).unwrap_or(file).display().to_string();
        println!("{} {relative}", need.map_or("none".to_string(), version_text));
        if need > Some(GLIBC_CEILING) {
            over.push(relative);
        }
        highest = highest.max(need);
    }
    println!("highest: {}", highest.map_or("none".to_string(), version_text));
    if over.is_empty() {
        Ok(())
    } else {
        Err(format!("above glibc {}: {}", version_text(GLIBC_CEILING), over.join(", ")))
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
