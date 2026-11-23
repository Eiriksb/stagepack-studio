//! The MCP server, run as the built program with piped standard input and output.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// A running server and the way to talk to it.
struct Server {
    child: Child,
    input: Option<ChildStdin>,
    lines: Receiver<String>,
}

impl Server {
    fn start(talk: &Path, home: &Path) -> Server {
        let mut child = Command::new(env!("CARGO_BIN_EXE_stagepack-studio"))
            .args(["mcp", talk.to_str().unwrap()])
            .env("HOME", home)
            .env("XDG_DATA_HOME", home.join("data"))
            .env("XDG_CACHE_HOME", home.join("cache"))
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the program runs");
        let input = child.stdin.take();
        let output = child.stdout.take().unwrap();
        let (sender, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else { break };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Server { child, input, lines }
    }

    fn send_raw(&mut self, bytes: &[u8]) {
        let input = self.input.as_mut().unwrap();
        input.write_all(bytes).unwrap();
        input.flush().unwrap();
    }

    fn read(&mut self) -> Value {
        let line = self.lines.recv_timeout(Duration::from_secs(30)).expect("a reply within 30 seconds");
        serde_json::from_str(&line).unwrap_or_else(|_| panic!("a JSON line, got {line}"))
    }

    /// Sends one line and reads the one line it is answered with.
    fn ask_raw(&mut self, line: &str) -> Value {
        self.send_raw(format!("{line}\n").as_bytes());
        self.read()
    }

    fn ask(&mut self, message: Value) -> Value {
        self.ask_raw(&message.to_string())
    }

    fn call(&mut self, name: &str, arguments: Value) -> Value {
        self.ask(json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": { "name": name, "arguments": arguments } }))
    }

    /// Closes standard input and waits for the program to end.
    fn finish(mut self) -> i32 {
        self.input = None;
        let started = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status.code().unwrap_or(-1);
            }
            assert!(started.elapsed() < Duration::from_secs(5), "the server ends when its input does");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn scratch_talk(dir: &Path) -> PathBuf {
    let talk = dir.join("talk");
    std::fs::create_dir_all(&talk).unwrap();
    let slides = "---\nlayout: title\n---\n\n# Hello there\n\n---\nlayout: two-columns\n---\n\n# Two sides\n\nleft words\n\n<Box x=\"100\" y=\"100\" w=\"300\" h=\"100\" click=\"1\" fill=\"#ff0000\" />\n";
    std::fs::write(talk.join("slides.md"), slides).unwrap();
    talk
}

fn initialize(server: &mut Server) {
    let reply = server.ask(json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "1" } } }));
    assert_eq!(reply["result"]["protocolVersion"], "2025-06-18");
}

fn text_of(reply: &Value) -> String {
    reply["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string()
}

#[test]
fn an_assistant_can_list_and_call_the_tools() {
    let dir = tempfile::tempdir().unwrap();
    let talk = scratch_talk(dir.path());
    let mut server = Server::start(&talk, dir.path());

    let reply = server.ask(json!({ "jsonrpc": "2.0", "id": "start", "method": "initialize", "params": { "protocolVersion": "2025-06-18" } }));
    assert_eq!(reply["jsonrpc"], "2.0");
    assert_eq!(reply["id"], "start");
    assert_eq!(reply["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(reply["result"]["serverInfo"]["name"], "stagepack-studio");
    assert!(reply["result"]["capabilities"]["tools"].is_object());
    assert!(reply["result"]["instructions"].as_str().unwrap().contains("guide"));
    let reply = server.ask(json!({ "jsonrpc": "2.0", "id": 2, "method": "initialize", "params": { "protocolVersion": "1999-01-01" } }));
    assert_eq!(reply["result"]["protocolVersion"], "2025-06-18");
    let reply = server.ask(json!({ "jsonrpc": "2.0", "id": 3, "method": "initialize", "params": { "protocolVersion": "2024-11-05" } }));
    assert_eq!(reply["result"]["protocolVersion"], "2024-11-05");

    let reply = server.ask(json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/list" }));
    let tools = reply["result"]["tools"].as_array().unwrap();
    let mut names: Vec<&str> = tools.iter().map(|tool| tool["name"].as_str().unwrap()).collect();
    names.sort();
    assert_eq!(names, ["check", "describe", "guide", "outline", "picture"]);
    for tool in tools {
        assert!(!tool["description"].as_str().unwrap().is_empty());
        assert_eq!(tool["inputSchema"]["type"], "object");
    }

    let reply = server.call("outline", json!({}));
    assert_eq!(reply["id"], 7);
    assert!(text_of(&reply).contains("1. "), "{reply}");
    assert!(reply["result"]["isError"].is_null());
    assert!(text_of(&server.call("describe", json!({}))).contains("two-columns"));
    assert!(!text_of(&server.call("guide", json!({}))).is_empty());
    assert!(!text_of(&server.call("check", json!({}))).is_empty());

    let reply = server.call("picture", json!({ "slide": 1, "width": 320 }));
    let content = reply["result"]["content"].as_array().unwrap();
    assert_eq!(content[0]["type"], "image");
    assert_eq!(content[0]["mimeType"], "image/png");
    assert_eq!(content[1]["type"], "text");
    assert!(content[1]["text"].as_str().unwrap().contains("Slide 1"));
    let png = base64::engine::general_purpose::STANDARD.decode(content[0]["data"].as_str().unwrap()).unwrap();
    let decoded = image::load_from_memory(&png).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (320, 180));
    assert_eq!(server.finish(), 0);
}

fn code(reply: &Value) -> i64 {
    reply["error"]["code"].as_i64().unwrap_or_else(|| panic!("an error, got {reply}"))
}

#[test]
fn any_line_gets_an_answer_or_none() {
    let dir = tempfile::tempdir().unwrap();
    let talk = scratch_talk(dir.path());
    let mut server = Server::start(&talk, dir.path());
    initialize(&mut server);

    // Not JSON.
    let reply = server.ask_raw("this is not json");
    assert_eq!((code(&reply), reply["id"].clone()), (-32700, Value::Null));
    // A batch.
    assert_eq!(code(&server.ask_raw(r#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#)), -32600);
    // An id that is null or of another type.
    let reply = server.ask_raw(r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#);
    assert_eq!((code(&reply), reply["id"].clone()), (-32600, Value::Null));
    assert_eq!(code(&server.ask_raw(r#"{"jsonrpc":"2.0","id":[1],"method":"ping"}"#)), -32600);
    // A notification: no answer. A blank line: none either.
    server.send_raw(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n\n   \n");
    server.send_raw(b"{\"jsonrpc\":\"2.0\",\"method\":\"nothing/known\"}\n");
    // An unknown method.
    let reply = server.ask(json!({ "jsonrpc": "2.0", "id": 9, "method": "resources/list" }));
    assert_eq!((code(&reply), reply["id"].clone()), (-32601, json!(9)));
    // An unknown tool, and the arguments the schema refuses.
    assert_eq!(code(&server.call("paint", json!({}))), -32602);
    for slide in [json!(0), json!(-1), json!("3"), json!(1.5), json!(null)] {
        assert_eq!(code(&server.call("picture", json!({ "slide": slide }))), -32602, "{slide}");
    }
    assert_eq!(code(&server.call("picture", json!({}))), -32602);
    assert_eq!(code(&server.call("picture", json!({ "slide": 1, "step": -1 }))), -32602);
    assert_eq!(code(&server.call("picture", json!({ "slide": 1, "width": 0 }))), -32602);
    assert_eq!(code(&server.call("picture", json!({ "slide": 1, "width": 100000 }))), -32602);
    assert_eq!(code(&server.ask(json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": { "arguments": {} } }))), -32602);
    // What the schema allows but the talk doesn't have: a tool that says so.
    for arguments in [json!({ "slide": 1000000000000u64 }), json!({ "slide": 1, "step": 50 })] {
        let reply = server.call("picture", arguments);
        assert_eq!(reply["result"]["isError"], true, "{reply}");
        assert!(text_of(&reply).ends_with('.'), "{reply}");
    }
    // A line of 5 MB is refused and skipped; the next one is read.
    let mut long = vec![b'x'; 5 * 1024 * 1024];
    long.push(b'\n');
    server.send_raw(&long);
    let reply = server.read();
    assert_eq!((code(&reply), reply["id"].clone()), (-32700, Value::Null));
    // Still serving.
    assert_eq!(server.ask(json!({ "jsonrpc": "2.0", "id": 99, "method": "ping" }))["result"], json!({}));
    // Standard input closed in the middle of a line: that line is answered, and the server ends.
    server.send_raw(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"met");
    server.input = None;
    assert_eq!(code(&server.read()), -32700);
    assert_eq!(server.finish(), 0);
}

#[test]
fn each_call_reads_the_talk_afresh() {
    let dir = tempfile::tempdir().unwrap();
    let talk = scratch_talk(dir.path());
    let mut server = Server::start(&talk, dir.path());
    initialize(&mut server);
    let clean = text_of(&server.call("check", json!({})));
    assert!(!clean.contains("missing.png"), "{clean}");

    let slides = std::fs::read_to_string(talk.join("slides.md")).unwrap();
    std::fs::write(talk.join("slides.md"), format!("{slides}\n<Picture src=\"assets/missing.png\" />\n")).unwrap();
    let reply = server.call("check", json!({}));
    assert!(text_of(&reply).contains("missing.png"), "{reply}");

    std::fs::write(talk.join("slides.md"), "").unwrap();
    let reply = server.call("outline", json!({}));
    assert!(reply["error"].is_null(), "{reply}");
    assert!(reply["result"]["content"][0]["text"].is_string(), "{reply}");
    assert_eq!(server.finish(), 0);
}

/// Every file under `root`, with its size and hash.
fn listing(root: &Path) -> Vec<String> {
    fn walk(path: &Path, out: &mut Vec<String>) {
        let mut entries: Vec<_> = std::fs::read_dir(path).unwrap().map(|entry| entry.unwrap().path()).collect();
        entries.sort();
        for entry in entries {
            if entry.is_dir() {
                out.push(format!("{} /", entry.display()));
                walk(&entry, out);
            } else {
                let bytes = std::fs::read(&entry).unwrap();
                out.push(format!("{} {} {:x}", entry.display(), bytes.len(), Sha256::digest(&bytes)));
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out
}

#[test]
fn the_server_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let talk = scratch_talk(dir.path());
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let before = (listing(&talk), listing(&home));
    let mut server = Server::start(&talk, &home);
    initialize(&mut server);
    for tool in ["describe", "check", "outline", "guide"] {
        assert!(server.call(tool, json!({}))["result"].is_object());
    }
    assert!(server.call("picture", json!({ "slide": 2, "step": 1, "width": 256 }))["result"].is_object());
    assert_eq!(server.finish(), 0);
    assert_eq!((listing(&talk), listing(&home)), before);
}

#[test]
fn nothing_at_the_path_ends_the_command() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_stagepack-studio"))
        .args(["mcp", dir.path().join("nothing").to_str().unwrap()])
        .env("HOME", dir.path())
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
}

#[test]
fn an_empty_folder_serves_until_a_talk_is_written() {
    let dir = tempfile::tempdir().unwrap();
    let talk = dir.path().join("new");
    std::fs::create_dir_all(&talk).unwrap();
    let mut server = Server::start(&talk, dir.path());
    initialize(&mut server);
    let reply = server.call("outline", json!({}));
    assert_eq!(reply["result"]["isError"], true, "{reply}");
    assert!(text_of(&reply).contains("slides.md"), "{reply}");
    std::fs::write(talk.join("slides.md"), "# Hello there\n").unwrap();
    let reply = server.call("outline", json!({}));
    assert!(reply["result"]["isError"].is_null(), "{reply}");
    assert!(text_of(&reply).contains("1. "), "{reply}");
    assert_eq!(server.finish(), 0);
}

#[test]
fn a_message_must_say_jsonrpc_2() {
    let dir = tempfile::tempdir().unwrap();
    let talk = scratch_talk(dir.path());
    let mut server = Server::start(&talk, dir.path());
    let reply = server.ask_raw(r#"{"jsonrpc":"1.0","id":4,"method":"ping"}"#);
    assert_eq!((code(&reply), reply["id"].clone()), (-32600, json!(4)));
    assert_eq!(code(&server.ask_raw(r#"{"id":"a","method":"ping"}"#)), -32600);
    // A client's response to nothing we asked: no answer.
    server.send_raw(b"{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{}}\n");
    assert_eq!(server.ask(json!({ "jsonrpc": "2.0", "id": 5, "method": "ping" }))["id"], 5);
    assert_eq!(server.finish(), 0);
}
