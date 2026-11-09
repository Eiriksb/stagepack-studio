use std::fs;
use std::path::PathBuf;

use studio_protocol::{
    Command, CommandBody, PageMessage, ProtocolError, command_script, parse_page_message,
};

fn studio_src() -> String {
    if let Ok(v) = std::env::var("STUDIO_SRC")
        && !v.is_empty()
    {
        return v;
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    if let Ok(text) = fs::read_to_string(root.join("dev.env")) {
        for line in text.lines() {
            if let Some(v) = line.trim().strip_prefix("STUDIO_SRC=")
                && !v.trim().is_empty()
            {
                return v.trim().to_string();
            }
        }
    }
    panic!(
        "STUDIO_SRC is not set: export it, or put STUDIO_SRC=<path> in dev.env at the repo root"
    );
}

fn fixtures(prefix_cmd: bool) -> Vec<(String, String)> {
    let dir = PathBuf::from(studio_src()).join("test/fixtures/bridge-v1");
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
    {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if name.starts_with("cmd-") == prefix_cmd {
            out.push((name, fs::read_to_string(&path).unwrap()));
        }
    }
    assert!(!out.is_empty(), "no fixtures found in {}", dir.display());
    out.sort();
    out
}

#[test]
fn every_page_fixture_parses() {
    for (name, text) in fixtures(false) {
        let msg = parse_page_message(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        let again = msg.to_json();
        let value: serde_json::Value = serde_json::from_str(&again).unwrap();
        assert_eq!(value["v"], 1, "{name}: serialised form carries v");
        if matches!(msg, PageMessage::Result { error: None, .. }) {
            assert!(value.get("error").is_none(), "{name}: error key skipped");
        }
        assert_eq!(parse_page_message(&again).unwrap(), msg, "{name}");
    }
}

#[test]
fn every_command_fixture_round_trips() {
    for (name, text) in fixtures(true) {
        let cmd: Command = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        let again = serde_json::to_string(&cmd).unwrap();
        let back: Command = serde_json::from_str(&again).unwrap();
        assert_eq!(back, cmd, "{name}");
        let want: serde_json::Value = serde_json::from_str(&text).unwrap();
        let got: serde_json::Value = serde_json::from_str(&again).unwrap();
        assert_eq!(got["v"], 1);
        let keys = |v: &serde_json::Value| {
            let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
            k.sort();
            k
        };
        assert_eq!(keys(&got), keys(&want), "{name}");
    }
}

#[test]
fn a_selection_says_whether_the_click_missed() {
    let missed = parse_page_message(r#"{"v":1,"type":"selection","selection":null,"missed":true}"#)
        .expect("missed parses");
    assert!(matches!(
        missed,
        PageMessage::Selection {
            selection: None,
            missed: true
        }
    ));
    // It is a required field of v1.
    assert!(matches!(
        parse_page_message(r#"{"v":1,"type":"selection","selection":null}"#),
        Err(ProtocolError::Bad(_))
    ));
}

#[test]
fn finish_edit_is_a_command() {
    let cmd = Command {
        id: 21,
        body: CommandBody::FinishEdit,
    };
    assert_eq!(
        serde_json::to_string(&cmd).unwrap(),
        r#"{"v":1,"id":21,"cmd":"finish-edit"}"#
    );
}

#[test]
fn version_two_is_refused() {
    let err = parse_page_message(r#"{"v":2,"type":"slide","slide":1,"total":1}"#).unwrap_err();
    assert!(matches!(err, ProtocolError::Version(2)), "{err:?}");
}

#[test]
fn missing_version_and_garbage_are_bad() {
    for json in [
        r#"{"type":"slide","slide":1,"total":1}"#,
        "[1]",
        "nope",
        "3",
    ] {
        assert!(
            matches!(parse_page_message(json), Err(ProtocolError::Bad(_))),
            "{json}"
        );
    }
    assert!(serde_json::from_str::<Command>(r#"{"v":2,"id":1,"cmd":"undo"}"#).is_err());
}

#[test]
fn command_script_escapes() {
    let cmd = Command {
        id: 1,
        body: CommandBody::Go { slide: 3 },
    };
    assert_eq!(
        command_script(&cmd),
        r#"window.__studioBridge.dispatch("{\"v\":1,\"id\":1,\"cmd\":\"go\",\"slide\":3}")"#
    );
}
