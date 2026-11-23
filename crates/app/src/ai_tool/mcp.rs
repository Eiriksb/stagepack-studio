//! The Model Context Protocol server for one talk: JSON-RPC 2.0, one message per line, on
//! standard input and output. Each tool call opens the talk afresh, since the AI changes its
//! files between calls. Nothing is written but replies; the program prints nothing else to
//! standard output while it serves.

use std::io::{self, BufRead, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use base64::Engine as _;
use serde_json::{Value, json};

use super::{check, check_text, describe, describe_text, open_talk, outline, outline_text, picture};
use super::{DEFAULT_WIDTH, LEAST_WIDTH, MOST_WIDTH};
use crate::permissions::BuildPermissions;

/// The longest line the server reads, in bytes.
const LONGEST_LINE: usize = 4 * 1024 * 1024;
/// The protocol versions this server knows, newest first.
const VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const NO_SUCH_METHOD: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

/// What the `guide` tool says. The guide for writing talks replaces this one function.
fn guide_text() -> String {
    "The guide for writing talks isn't in this copy of the app yet. Call describe to see what a talk may hold, and check after each change.".to_string()
}

/// One line of input.
enum Line {
    Text(Vec<u8>),
    /// Longer than the limit; already skipped to its end.
    TooLong,
}

/// Reads the next line without holding more than `LONGEST_LINE` of it. `None` at the end of
/// the input. A last line with no newline counts as a line.
fn next_line(input: &mut impl BufRead) -> io::Result<Option<Line>> {
    let mut line = Vec::new();
    let mut too_long = false;
    let mut any = false;
    loop {
        let (used, done) = {
            let chunk = match input.fill_buf() {
                Ok(chunk) => chunk,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            };
            if chunk.is_empty() {
                break;
            }
            any = true;
            let (taken, done) = match chunk.iter().position(|byte| *byte == b'\n') {
                Some(at) => (at, true),
                None => (chunk.len(), false),
            };
            if !too_long {
                if line.len() + taken > LONGEST_LINE {
                    too_long = true;
                    line = Vec::new();
                } else {
                    line.extend_from_slice(&chunk[..taken]);
                }
            }
            (taken + usize::from(done), done)
        };
        input.consume(used);
        if done {
            break;
        }
    }
    Ok(match (any, too_long) {
        (false, _) => None,
        (true, true) => Some(Line::TooLong),
        (true, false) => Some(Line::Text(line)),
    })
}

/// Serves the talk at `talk` until `input` ends.
pub fn serve(talk: &Path, mut input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    while let Some(line) = next_line(&mut input)? {
        let reply = match line {
            Line::TooLong => Some(error_reply(&Value::Null, PARSE_ERROR, "That line is longer than 4 MB, so it was skipped.")),
            Line::Text(bytes) => {
                if bytes.iter().all(|byte| byte.is_ascii_whitespace()) {
                    continue;
                }
                answer(talk, &bytes)
            }
        };
        if let Some(reply) = reply {
            let mut text = reply.to_string();
            text.push('\n');
            output.write_all(text.as_bytes())?;
            output.flush()?;
        }
    }
    Ok(())
}

fn error_reply(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn result_reply(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// The reply to one line, or `None` for a notification.
fn answer(talk: &Path, bytes: &[u8]) -> Option<Value> {
    let message: Value = match serde_json::from_slice(bytes) {
        Ok(message) => message,
        Err(_) => return Some(error_reply(&Value::Null, PARSE_ERROR, "That isn't JSON.")),
    };
    let Some(object) = message.as_object() else {
        let what = if message.is_array() { "Batches aren't supported; send one message per line." } else { "A message is a JSON object." };
        return Some(error_reply(&Value::Null, INVALID_REQUEST, what));
    };
    let id = match object.get("id") {
        None => None,
        Some(id) if id.is_number() || id.is_string() => Some(id.clone()),
        Some(_) => return Some(error_reply(&Value::Null, INVALID_REQUEST, "A request's id is a number or a string.")),
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return id.map(|id| error_reply(&id, INVALID_REQUEST, "A message says \"jsonrpc\": \"2.0\"."));
    }
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        // A client's reply to something: this server asks nothing, so there's nothing to answer.
        return None;
    };
    let params = object.get("params").cloned().unwrap_or(Value::Null);
    // A notification gets no answer, whatever it says.
    let id = id?;
    Some(match method {
        "initialize" => result_reply(&id, initialize(talk, &params)),
        "ping" => result_reply(&id, json!({})),
        "tools/list" => result_reply(&id, json!({ "tools": tools() })),
        "tools/call" => match call(talk, &params) {
            Ok(result) => result_reply(&id, result),
            Err(message) => error_reply(&id, INVALID_PARAMS, &message),
        },
        other => error_reply(&id, NO_SUCH_METHOD, &format!("This server has no method {other}.")),
    })
}

fn initialize(talk: &Path, params: &Value) -> Value {
    let asked = params.get("protocolVersion").and_then(Value::as_str);
    let version = VERSIONS.iter().find(|known| Some(**known) == asked).unwrap_or(&VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "stagepack-studio", "version": env!("CARGO_PKG_VERSION") },
        "instructions": format!(
            "This server serves the talk at {}, which it reads afresh at every call. Call the guide tool first, to learn how a talk is written.",
            talk.display()
        ),
    })
}

fn tools() -> Value {
    let none = json!({ "type": "object", "properties": {} });
    json!([
        {
            "name": "describe",
            "description": "What this talk may hold: the layouts and their regions, the settings of the talk and of a slide, and every tag with each setting's name, kind, meaning and choices, including this talk's own components.",
            "inputSchema": none,
        },
        {
            "name": "check",
            "description": "Every complaint about this talk, by slide, as the window would show them. Says so when there are none.",
            "inputSchema": none,
        },
        {
            "name": "outline",
            "description": "Each slide's number, layout, first words, clicks, whether it is hidden and whether it has notes.",
            "inputSchema": none,
        },
        {
            "name": "picture",
            "description": "A slide drawn as a PNG at a click step, with the slide's complaints.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "slide": { "type": "integer", "minimum": 1, "description": "The slide's number, from 1." },
                    "step": { "type": "integer", "minimum": 0, "description": "The click step, from 0. The slide's last when left out." },
                    "width": { "type": "integer", "minimum": LEAST_WIDTH, "maximum": MOST_WIDTH, "description": format!("Pixels wide; {DEFAULT_WIDTH} when left out.") },
                },
                "required": ["slide"],
            },
        },
        {
            "name": "guide",
            "description": "The guide for writing a talk. Read it first.",
            "inputSchema": none,
        },
    ])
}

/// An integer argument that must lie in `least..=most`. `Ok(None)` when it's absent.
fn whole(arguments: &Value, name: &str, least: u64, most: u64) -> Result<Option<u64>, String> {
    let Some(value) = arguments.get(name) else { return Ok(None) };
    let number = value
        .as_u64()
        .ok_or_else(|| format!("The argument {name} must be a whole number, {least} or more."))?;
    if number < least || number > most {
        return Err(format!("The argument {name} must be from {least} to {most}."));
    }
    Ok(Some(number))
}

/// A tool call: `Err` is for a request the schema refuses (error -32602); a tool that runs and
/// fails is an `Ok` with `isError`.
fn call(talk: &Path, params: &Value) -> Result<Value, String> {
    let name = params.get("name").and_then(Value::as_str).ok_or("A tool call needs the name of a tool.")?;
    let arguments = match params.get("arguments") {
        None | Some(Value::Null) => json!({}),
        Some(arguments) if arguments.is_object() => arguments.clone(),
        Some(_) => return Err("The arguments of a tool call are an object.".to_string()),
    };
    let ran = match name {
        "picture" => {
            let slide = whole(&arguments, "slide", 1, u64::MAX)?.ok_or("The picture tool needs a slide number.")?;
            let step = whole(&arguments, "step", 0, u32::MAX as u64)?;
            let width = whole(&arguments, "width", LEAST_WIDTH as u64, MOST_WIDTH as u64)?;
            guarded(|| draw(talk, slide, step, width))
        }
        "describe" | "check" | "outline" | "guide" => guarded(|| text_tool(talk, name)),
        other => return Err(format!("This server has no tool {other}.")),
    };
    Ok(ran)
}

/// Runs a tool; a panic in it is a failed call, not a dead server.
fn guarded(tool: impl FnOnce() -> Value) -> Value {
    catch_unwind(AssertUnwindSafe(tool)).unwrap_or_else(|_| failure("Something went wrong drawing this talk."))
}

fn failure(sentence: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": sentence }], "isError": true })
}

fn text_result(text: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": text }] })
}

fn text_tool(talk: &Path, name: &str) -> Value {
    if name == "guide" {
        return text_result(&guide_text());
    }
    let mut opened = match open_talk(talk) {
        Ok(opened) => opened,
        Err(sentence) => return failure(&sentence),
    };
    match name {
        "describe" => text_result(&describe_text(&describe(Some(&mut opened)))),
        "outline" => text_result(&outline_text(&outline(&mut opened))),
        _ => {
            // The owner's own permissions, read and never written.
            let permissions = BuildPermissions::load(BuildPermissions::default_file().unwrap_or_default());
            text_result(&check_text(&check(&mut opened, &permissions)))
        }
    }
}

fn draw(talk: &Path, slide: u64, step: Option<u64>, width: Option<u64>) -> Value {
    let mut opened = match open_talk(talk) {
        Ok(opened) => opened,
        Err(sentence) => return failure(&sentence),
    };
    let slide = usize::try_from(slide).unwrap_or(usize::MAX);
    let width = width.map_or(DEFAULT_WIDTH, |width| width as u32);
    match picture(&mut opened, slide, step.map(|step| step as u32), width) {
        Ok(made) => {
            let clicks = match made.steps {
                0 => String::new(),
                steps => format!(", click {} of {steps}", made.step),
            };
            let mut text = format!("Slide {}{clicks}.\n", made.slide);
            for problem in &made.problems {
                text.push_str(&format!("Slide {}: {problem}\n", made.slide));
            }
            json!({ "content": [
                { "type": "image", "data": base64::engine::general_purpose::STANDARD.encode(&made.png), "mimeType": "image/png" },
                { "type": "text", "text": text },
            ] })
        }
        Err(sentence) => failure(&sentence),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_panic_in_a_tool_is_a_failed_call() {
        let reply = guarded(|| panic!("a slide that can't be laid out"));
        assert_eq!(reply["isError"], true);
        assert_eq!(reply["content"][0]["text"], "Something went wrong drawing this talk.");
        assert_eq!(guarded(|| text_result("fine"))["content"][0]["text"], "fine");
    }
}
