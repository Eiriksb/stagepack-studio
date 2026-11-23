//! Commands that use a talk without opening a window, for an AI or a script to run: what a
//! talk holds, said in text or JSON. Nothing here starts the window toolkit.

mod outline;
mod talk;

use std::ffi::OsString;
use std::io::Write as _;
use std::path::PathBuf;

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

/// Runs the command `args[1]` names, if it names one, and returns its exit status after it has
/// printed. `None` when it doesn't, so the program opens a window as it always has.
pub fn run_command(args: &[OsString]) -> Option<i32> {
    let name = args.get(1)?.to_str()?;
    let known = COMMANDS.iter().any(|(command, _)| *command == name) || matches!(name, "help" | "--help" | "-h");
    if !known {
        return None;
    }
    let rest = &args[2..];
    let done = match name {
        "help" | "--help" | "-h" => Ok(help()),
        "outline" => run_outline(rest),
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
