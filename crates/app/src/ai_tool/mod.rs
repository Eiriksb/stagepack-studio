//! Commands that use a talk without opening a window, for an AI or a script to run: what a
//! talk holds, said in text or JSON. Nothing here starts the window toolkit.

mod check;
mod describe;
mod guide;
mod mcp;
mod outline;
mod picture;
mod talk;

use std::ffi::OsString;
use std::io::Write as _;
use std::path::PathBuf;

use slides_engine::write_atomically;

use crate::permissions::BuildPermissions;

pub use check::{Checked, SlideProblems, check, check_text, slide_problems};
pub use describe::{Description, FieldInfo, LayoutInfo, SettingInfo, TagInfo, describe, describe_text};
pub use picture::{DEFAULT_WIDTH, LEAST_WIDTH, MOST_WIDTH, Picture, picture, picture_text};
pub use guide::GUIDE;
pub use mcp::serve;
pub use outline::{OutlineSlide, outline, outline_text};
pub use talk::{OpenTalk, TalkPlace, open_talk};

/// The command names, with what each does.
const COMMANDS: [(&str, &str); 6] = [
    ("describe", "describe [TALK] [--json]   what a talk may hold: layouts, settings and tags"),
    ("check", "check TALK [--json]   every complaint about a talk, by slide"),
    ("outline", "outline TALK [--json]   each slide's number, layout, first words and clicks"),
    ("picture", "picture TALK SLIDE [--step N] [--width W] --out FILE.png   a slide as a picture"),
    ("guide", "guide   the guide for writing talks"),
    ("mcp", "mcp TALK   the Model Context Protocol server for a talk, on standard input and output"),
];

/// A program of the windows subsystem starts with no console, so what a command prints would go nowhere.
/// Attaches to the terminal that ran it and, where the program was given no output of its own (a pipe or a
/// file is left alone), points the standard handles at that terminal. Does nothing when there's no terminal.
/// The console devices are opened for reading and writing, so that std can tell they are consoles and print non-ASCII text properly.
/// Not run or compiled here: Windows only.
#[cfg(windows)]
fn attach_parent_console() {
    use std::os::windows::io::IntoRawHandle;
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle,
    };
    // SAFETY: plain calls into the console API; a handle we open is handed over to the process and never closed.
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            return;
        }
        for (which, device) in [(STD_INPUT_HANDLE, "CONIN$"), (STD_OUTPUT_HANDLE, "CONOUT$"), (STD_ERROR_HANDLE, "CONOUT$")] {
            let current = GetStdHandle(which);
            if !current.is_null() && current != INVALID_HANDLE_VALUE {
                continue;
            }
            if let Ok(file) = std::fs::OpenOptions::new().read(true).write(true).open(device) {
                SetStdHandle(which, file.into_raw_handle());
            }
        }
    }
}

#[cfg(not(windows))]
fn attach_parent_console() {}

/// Runs the command `args[1]` names, if it names one, and returns its exit status after it has
/// printed. `None` when it doesn't, so the program opens a window as it always has.
pub fn run_command(args: &[OsString]) -> Option<i32> {
    let name = args.get(1)?.to_str()?;
    let known = COMMANDS.iter().any(|(command, _)| *command == name) || matches!(name, "help" | "--help" | "-h");
    if !known {
        return None;
    }
    attach_parent_console();
    let rest = &args[2..];
    let done = match name {
        "help" | "--help" | "-h" => Ok(help()),
        "describe" => run_describe(rest),
        "check" => run_check(rest),
        "outline" => run_outline(rest),
        "picture" => run_picture(rest),
        "mcp" => run_mcp(rest),
        "guide" => Ok(run_guide(rest)),
        other => Err(Failed(format!("The {other} command isn't ready yet."))),
    };
    Some(match done {
        Ok(status) => status,
        Err(Failed(sentence)) => {
            let _ = writeln!(std::io::stderr(), "{sentence}");
            2
        }
    })
}

/// A command that couldn't go ahead: one plain sentence, for standard error, and status 2.
struct Failed(String);

/// What a command was given: its words, and the options it knows.
struct Given {
    words: Vec<PathBuf>,
    json: bool,
}

/// Reads `args`: words, and `--json` where `json` says it's known. Anything else that starts
/// with `-` is refused by name.
fn read_args(command: &str, args: &[OsString], json: bool) -> Result<Given, Failed> {
    let mut given = Given { words: Vec::new(), json: false };
    for arg in args {
        match arg.to_str() {
            Some("--json") if json => given.json = true,
            Some(option) if option.starts_with('-') && option.len() > 1 => {
                return Err(Failed(format!("The {command} command has no option {option}. Try `help`.")));
            }
            _ => given.words.push(PathBuf::from(arg)),
        }
    }
    Ok(given)
}

/// The one talk a command is pointed at.
fn the_talk(command: &str, given: &Given) -> Result<OpenTalk, Failed> {
    match given.words.as_slice() {
        [path] => open_talk(path).map_err(Failed),
        [] => Err(Failed(format!("The {command} command needs a talk: a folder or a .sptx file."))),
        _ => Err(Failed(format!("The {command} command takes one talk, and was given more."))),
    }
}

fn say(text: &str) {
    let _ = std::io::stdout().write_all(text.as_bytes());
}

fn help() -> i32 {
    let mut text = String::from("stagepack-studio opens a talk in a window; these commands use one without.\n\n");
    for (_, line) in COMMANDS {
        text.push_str(&format!("  stagepack-studio {line}\n"));
    }
    text.push_str("  stagepack-studio help   this list\n\nA TALK is a folder with a slides.md, or a .sptx file. To open a folder called after a command in the window, write ./check.\n");
    say(&text);
    0
}

fn run_outline(args: &[OsString]) -> Result<i32, Failed> {
    let given = read_args("outline", args, true)?;
    let mut talk = the_talk("outline", &given)?;
    let slides = outline(&mut talk);
    if given.json {
        say(&(serde_json::to_string_pretty(&slides).unwrap_or_default() + "\n"));
    } else {
        say(&outline_text(&slides));
    }
    Ok(0)
}

fn run_describe(args: &[OsString]) -> Result<i32, Failed> {
    let given = read_args("describe", args, true)?;
    let mut talk = match given.words.as_slice() {
        [] => None,
        _ => Some(the_talk("describe", &given)?),
    };
    let described = describe(talk.as_mut());
    if given.json {
        say(&(serde_json::to_string_pretty(&described).unwrap_or_default() + "\n"));
    } else {
        say(&describe_text(&described));
    }
    Ok(0)
}

fn run_check(args: &[OsString]) -> Result<i32, Failed> {
    let given = read_args("check", args, true)?;
    let mut talk = the_talk("check", &given)?;
    // The owner's own permissions, read and never written.
    let permissions = BuildPermissions::load(BuildPermissions::default_file().unwrap_or_default());
    let checked = check(&mut talk, &permissions);
    if given.json {
        say(&(serde_json::to_string_pretty(&checked).unwrap_or_default() + "\n"));
    } else {
        say(&check_text(&checked));
    }
    Ok(if checked.count() == 0 { 0 } else { 1 })
}

/// Prints the guide.
fn run_guide(_args: &[OsString]) -> i32 {
    say(GUIDE);
    0
}

fn run_mcp(args: &[OsString]) -> Result<i32, Failed> {
    let given = read_args("mcp", args, false)?;
    // Only a path with something at it is needed now: the talk may be written after the server
    // starts, and each call opens it afresh and says plainly if it isn't a talk yet.
    let [path] = given.words.as_slice() else {
        return Err(Failed("The mcp command needs one talk: a folder or a .sptx file.".to_string()));
    };
    if !path.exists() {
        return Err(Failed(format!("There's no talk at {}.", path.display())));
    }
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    serve(&given.words[0], stdin.lock(), stdout.lock()).map_err(|error| Failed(format!("The server stopped: {error}.")))?;
    Ok(0)
}

/// What `picture` was given besides the talk and the slide.
struct PictureArgs {
    words: Vec<PathBuf>,
    step: Option<u32>,
    width: u32,
    out: Option<PathBuf>,
}

fn read_picture_args(args: &[OsString]) -> Result<PictureArgs, Failed> {
    let mut given = PictureArgs { words: Vec::new(), step: None, width: DEFAULT_WIDTH, out: None };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some(name @ ("--step" | "--width" | "--out")) => {
                let value = args.next().ok_or_else(|| Failed(format!("{name} needs a value after it.")))?;
                match name {
                    "--out" => given.out = Some(PathBuf::from(value)),
                    _ => {
                        let shown = value.to_string_lossy();
                        let number = shown
                            .parse::<u32>()
                            .map_err(|_| Failed(format!("{name} needs a whole number, and {shown} isn't one.")))?;
                        if name == "--step" {
                            given.step = Some(number);
                        } else {
                            given.width = number;
                        }
                    }
                }
            }
            Some(option) if option.starts_with('-') && option.len() > 1 => {
                return Err(Failed(format!("The picture command has no option {option}. Try `help`.")));
            }
            _ => given.words.push(PathBuf::from(arg)),
        }
    }
    Ok(given)
}

fn run_picture(args: &[OsString]) -> Result<i32, Failed> {
    let given = read_picture_args(args)?;
    let out = given.out.ok_or_else(|| Failed("The picture command needs --out FILE.png to write the picture to.".to_string()))?;
    check_out(&out)?;
    let [talk_path, slide] = given.words.as_slice() else {
        return Err(Failed("The picture command needs a talk and a slide number.".to_string()));
    };
    let slide = slide.to_string_lossy();
    let slide: usize = slide.parse().map_err(|_| Failed(format!("The slide must be a whole number, and {slide} isn't one.")))?;
    let mut talk = open_talk(talk_path).map_err(Failed)?;
    let made = picture(&mut talk, slide, given.step, given.width).map_err(Failed)?;
    write_atomically(&out, |file| file.write_all(&made.png))
        .map_err(|error| Failed(format!("Could not write {}: {error}.", out.display())))?;
    say(&picture_text(&made, &out.display().to_string()));
    report_peak();
    Ok(if made.problems.is_empty() { 0 } else { 1 })
}

/// Whether `out` may be written as a picture: its name ends in `.png`, and a file already there
/// is a PNG, so a mistyped name can't replace a talk's own file.
fn check_out(out: &std::path::Path) -> Result<(), Failed> {
    let png = out.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("png"));
    if !png {
        return Err(Failed(format!("The picture goes to a file that ends in .png, and {} doesn't.", out.display())));
    }
    if out.exists() {
        let mut start = [0u8; 8];
        let is_png = std::fs::File::open(out).and_then(|mut file| std::io::Read::read_exact(&mut file, &mut start)).is_ok()
            && start == [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        if !is_png {
            return Err(Failed(format!("{} is already a file and isn't a picture, so it won't be written over.", out.display())));
        }
    }
    Ok(())
}

/// For tests: with `STUDIO_REPORT_PEAK` set, says on standard error the most memory this run
/// held at once, as Linux counts it. Otherwise it does nothing.
fn report_peak() {
    if std::env::var_os("STUDIO_REPORT_PEAK").is_none() {
        return;
    }
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    if let Some(line) = status.lines().find(|line| line.starts_with("VmHWM:")) {
        let _ = writeln!(std::io::stderr(), "peak memory {}", line.trim_start_matches("VmHWM:").trim());
    }
}
