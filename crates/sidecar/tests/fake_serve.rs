use std::fs;
use std::path::{Path, PathBuf};

use studio_sidecar::{Sidecar, SidecarEvent, SpawnOptions};

fn fake() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fake-serve.sh")
}

fn opts() -> SpawnOptions {
    SpawnOptions {
        stagepack: fake(),
        home: PathBuf::from("/h"),
        deck: PathBuf::from("/d/talk.zip"),
        addons: vec![],
    }
}

// Env vars are process-global, so every test uses its own temp dir and
// a mutex-free approach: spawn with env set under a lock.
static ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Run {
    sc: Sidecar,
    rx: async_channel::Receiver<SidecarEvent>,
    dir: PathBuf,
}

fn start(name: &str, script: &str, exit: Option<&str>, o: SpawnOptions) -> Run {
    let dir = std::env::temp_dir().join(format!("studio-sidecar-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("script"), script).unwrap();
    let _g = ENV.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        std::env::set_var("FAKE_SERVE_SCRIPT", dir.join("script"));
        std::env::set_var("FAKE_SERVE_STDIN", dir.join("stdin"));
        std::env::set_var("FAKE_SERVE_ARGS", dir.join("args"));
        match exit {
            Some(c) => std::env::set_var("FAKE_SERVE_EXIT", c),
            None => std::env::remove_var("FAKE_SERVE_EXIT"),
        }
    }
    let (sc, rx) = Sidecar::spawn(o).unwrap();
    Run { sc, rx, dir }
}

const UNPACKED: &str = r#"{"event":"unpacked","id":"a1","title":"T","entry":"slides.md","workDir":"/w","code":["x.ts"],"trusted":false}"#;
const READY: &str = r#"{"event":"ready","port":3030,"baseUrl":"http://localhost:3030","workDir":"/w"}"#;

fn recv(rx: &async_channel::Receiver<SidecarEvent>) -> SidecarEvent {
    rx.recv_blocking().expect("channel closed early")
}

#[test]
fn events_arrive_in_order() {
    let mut r = start("order", &format!("{UNPACKED}\n{READY}\n"), None, opts());
    assert_eq!(
        recv(&r.rx),
        SidecarEvent::Unpacked {
            id: "a1".into(),
            title: "T".into(),
            entry: "slides.md".into(),
            work_dir: "/w".into(),
            code: vec!["x.ts".into()],
            trusted: false
        }
    );
    assert_eq!(
        recv(&r.rx),
        SidecarEvent::Ready { port: 3030, base_url: "http://localhost:3030".into(), work_dir: "/w".into() }
    );
    r.sc.stop().unwrap();
    assert_eq!(recv(&r.rx), SidecarEvent::Stopped);
    assert_eq!(recv(&r.rx), SidecarEvent::Exited { code: Some(0) });
    assert!(r.rx.recv_blocking().is_err(), "channel closes after Exited");
    let _ = fs::remove_dir_all(&r.dir);
}

#[test]
fn answer_and_save_write_json_lines() {
    let mut r = start("stdin", "", None, opts());
    r.sc.answer(true).unwrap();
    r.sc.answer(false).unwrap();
    r.sc.save().unwrap();
    r.sc.stop().unwrap();
    while !matches!(recv(&r.rx), SidecarEvent::Exited { .. }) {}
    assert_eq!(
        fs::read_to_string(r.dir.join("stdin")).unwrap(),
        "{\"answer\":\"yes\"}\n{\"answer\":\"no\"}\n{\"cmd\":\"save\"}\n{\"cmd\":\"stop\"}\n"
    );
    let _ = fs::remove_dir_all(&r.dir);
}

#[test]
fn drop_closes_stdin() {
    let r = start("drop", "", None, opts());
    let rx = r.rx.clone();
    drop(r.sc);
    assert_eq!(recv(&rx), SidecarEvent::Stopped);
    assert_eq!(recv(&rx), SidecarEvent::Exited { code: Some(0) });
    let _ = fs::remove_dir_all(&r.dir);
}

#[test]
fn exit_code_three_is_reported() {
    let mut r = start("three", "", Some("3"), opts());
    r.sc.stop().unwrap();
    assert_eq!(recv(&r.rx), SidecarEvent::Stopped);
    assert_eq!(recv(&r.rx), SidecarEvent::Exited { code: Some(3) });
    let _ = fs::remove_dir_all(&r.dir);
}

#[test]
fn garbage_line_is_an_error_event() {
    let mut r = start("garbage", "not json\n", None, opts());
    assert_eq!(
        recv(&r.rx),
        SidecarEvent::Error { message: "Unreadable message from Stagepack: not json".into(), log: "".into() }
    );
    r.sc.stop().unwrap();
    while !matches!(recv(&r.rx), SidecarEvent::Exited { .. }) {}
    let _ = fs::remove_dir_all(&r.dir);
}

#[test]
fn optional_fields_default() {
    let script = "{\"event\":\"save-failed\",\"message\":\"m\"}\n{\"event\":\"busy\",\"baseUrl\":\"\"}\n{\"event\":\"error\",\"message\":\"e\"}\n";
    let mut r = start("defaults", script, None, opts());
    assert_eq!(recv(&r.rx), SidecarEvent::SaveFailed { message: "m".into(), names: vec![] });
    assert_eq!(recv(&r.rx), SidecarEvent::Busy { base_url: "".into() });
    assert_eq!(recv(&r.rx), SidecarEvent::Error { message: "e".into(), log: "".into() });
    r.sc.stop().unwrap();
    while !matches!(recv(&r.rx), SidecarEvent::Exited { .. }) {}
    let _ = fs::remove_dir_all(&r.dir);
}

#[test]
fn args_are_in_the_documented_order() {
    let mut o = opts();
    o.addons = vec![PathBuf::from("/a1"), PathBuf::from("/a2")];
    let mut r = start("args", "", None, o);
    r.sc.stop().unwrap();
    while !matches!(recv(&r.rx), SidecarEvent::Exited { .. }) {}
    assert_eq!(
        fs::read_to_string(r.dir.join("args")).unwrap(),
        "serve\n--json\n--home\n/h\n--addon\n/a1\n--addon\n/a2\n/d/talk.zip\n"
    );
    let _ = fs::remove_dir_all(&r.dir);
}

#[cfg(unix)]
#[test]
fn child_runs_in_its_own_process_group() {
    fn pgrp(stat: &str) -> i64 {
        // Field 5; comm (field 2) may contain spaces, so split after the last ')'.
        let rest = &stat[stat.rfind(')').unwrap() + 2..];
        rest.split(' ').nth(2).unwrap().parse().unwrap()
    }
    let mut r = start("pgrp", "", None, opts());
    let child = fs::read_to_string(format!("/proc/{}/stat", r.sc.id())).unwrap();
    let mine = fs::read_to_string("/proc/self/stat").unwrap();
    assert_ne!(pgrp(&child), pgrp(&mine));
    assert_eq!(pgrp(&child), r.sc.id() as i64, "child leads its own group");
    r.sc.stop().unwrap();
    while !matches!(recv(&r.rx), SidecarEvent::Exited { .. }) {}
    let _ = fs::remove_dir_all(&r.dir);
}
