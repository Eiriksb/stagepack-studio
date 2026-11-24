//! Runs the built `stagepack-terminal` and speaks to it in frames, as the app does.
//! Every program a test starts is one the test chose, and is gone when the test ends, even a failing one.
use slides_terminal_helper::wire::{self, FromHelper, StartSpec, ToHelper};
use std::io::Write;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

struct Helper {
    child: Child,
    stdin: Option<ChildStdin>,
    frames: Receiver<Result<FromHelper, String>>,
    output: Vec<u8>,
}

impl Helper {
    fn new() -> Helper {
        let mut child = Command::new(env!("CARGO_BIN_EXE_stagepack-terminal"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the helper starts");
        let stdin = child.stdin.take();
        let mut stdout = child.stdout.take().unwrap();
        let (tx, frames) = mpsc::channel();
        thread::spawn(move || {
            while let Ok(Some((k, b))) = wire::read_frame(&mut stdout) {
                let _ = tx.send(FromHelper::decode(k, &b).map_err(|e| e.to_string()));
            }
        });
        Helper { child, stdin, frames, output: Vec::new() }
    }

    fn start(sh: &str) -> Helper {
        let mut h = Helper::new();
        h.say(&ToHelper::Start(spec("sh", &["-c", sh])));
        h
    }

    fn say(&mut self, what: &ToHelper) {
        let stdin = self.stdin.as_mut().expect("stdin is open");
        what.write(stdin).expect("the helper listens");
    }

    /// The next frame, collecting output on the way; `None` once the helper has closed its output.
    fn next(&mut self, within: Duration) -> Option<FromHelper> {
        match self.frames.recv_timeout(within) {
            Ok(Ok(FromHelper::Output(b))) => {
                self.output.extend_from_slice(&b);
                Some(FromHelper::Output(b))
            }
            Ok(Ok(other)) => Some(other),
            Ok(Err(e)) => panic!("the helper sent a bad frame: {e}"),
            Err(mpsc::RecvTimeoutError::Disconnected) => None,
            Err(mpsc::RecvTimeoutError::Timeout) => panic!("nothing came within {within:?}"),
        }
    }

    fn output_text(&self) -> String {
        String::from_utf8_lossy(&self.output).into_owned()
    }

    /// Reads until the output has `needle` in it.
    fn until_output(&mut self, needle: &str) {
        let end = Instant::now() + Duration::from_secs(10);
        while !self.output_text().contains(needle) {
            assert!(Instant::now() < end, "never saw {needle:?}, got {:?}", self.output_text());
            assert!(self.next(Duration::from_secs(10)).is_some(), "the helper ended before {needle:?}");
        }
    }

    /// Reads until `Exited`, and checks the helper then ends with status 0.
    fn until_exited(&mut self, within: Duration) -> wire::Exit {
        let end = Instant::now() + within;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            match self.next(left.max(Duration::from_millis(1))) {
                Some(FromHelper::Exited(e)) => {
                    self.finish();
                    return e;
                }
                Some(_) => {}
                None => panic!("the helper ended without saying Exited"),
            }
        }
    }

    fn finish(&mut self) {
        self.stdin = None;
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "the helper's status was {status}");
                return;
            }
            assert!(Instant::now() < end, "the helper never ended");
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Helper {
    fn drop(&mut self) {
        // Closing the input ends the program; if the helper itself is stuck it is killed.
        self.stdin = None;
        let end = Instant::now() + Duration::from_secs(6);
        while Instant::now() < end {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spec(program: &str, args: &[&str]) -> StartSpec {
    StartSpec {
        program: program.into(),
        args: args.iter().map(|s| s.to_string()).collect(),
        cwd: std::env::temp_dir(),
        cols: 100,
        rows: 30,
        env: vec![("TERM".into(), "xterm-256color".into())],
    }
}

fn expect_started(h: &mut Helper) {
    assert_eq!(h.next(Duration::from_secs(10)), Some(FromHelper::Started));
}

#[test]
fn a_program_runs_and_its_output_comes_back() {
    let mut h = Helper::start("printf hello; exit 3");
    expect_started(&mut h);
    let exit = h.until_exited(Duration::from_secs(10));
    assert!(h.output_text().contains("hello"), "{:?}", h.output_text());
    assert_eq!(exit.code, Some(3));
}

#[test]
fn input_reaches_the_program() {
    let mut h = Helper::new();
    h.say(&ToHelper::Start(spec("cat", &[])));
    expect_started(&mut h);
    h.say(&ToHelper::Input(b"abc\r".to_vec()));
    h.until_output("abc");
    h.say(&ToHelper::End);
    h.until_exited(Duration::from_secs(4));
}

#[test]
fn resize_is_seen() {
    let mut h = Helper::start("stty size; read x; stty size");
    expect_started(&mut h);
    h.until_output("30 100");
    h.say(&ToHelper::Resize(120, 40));
    // Give the size time to be set before the program reads on.
    thread::sleep(Duration::from_millis(200));
    h.say(&ToHelper::Input(b"\r".to_vec()));
    h.until_output("40 120");
    h.until_exited(Duration::from_secs(10));
}

#[test]
fn a_missing_program_says_so() {
    let mut h = Helper::new();
    h.say(&ToHelper::Start(spec("no-such-program-xyz", &[])));
    match h.next(Duration::from_secs(10)) {
        Some(FromHelper::Failed(why)) => assert!(why.contains("no-such-program-xyz"), "{why}"),
        other => panic!("expected Failed, got {other:?}"),
    }
    assert!(h.next(Duration::from_secs(5)).is_none());
    h.finish();
}

#[test]
fn a_missing_folder_says_so() {
    let mut h = Helper::new();
    let mut s = spec("sh", &["-c", "true"]);
    s.cwd = std::env::temp_dir().join("no-such-folder-xyz");
    h.say(&ToHelper::Start(s));
    assert!(matches!(h.next(Duration::from_secs(10)), Some(FromHelper::Failed(_))));
    h.finish();
}

/// Whether a process is still there and not just waiting to be collected.
#[cfg(target_os = "linux")]
fn alive(pid: u32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => !stat.rsplit(')').next().unwrap_or("").trim_start().starts_with('Z'),
        Err(_) => false,
    }
}

/// Reads the pids the program printed (its own, then the background one's), whatever else is on screen.
#[cfg(target_os = "linux")]
fn pids(text: &str) -> Vec<u32> {
    let line = text.lines().find(|l| l.starts_with("PIDS ")).expect("the pids line");
    line.split_whitespace().skip(1).filter_map(|w| w.parse().ok()).collect()
}

/// Kills what a failing test left behind.
#[cfg(target_os = "linux")]
struct Reap(Vec<u32>);
#[cfg(target_os = "linux")]
impl Drop for Reap {
    fn drop(&mut self) {
        for p in &self.0 {
            let _ = Command::new("kill").args(["-9", &p.to_string()]).stderr(Stdio::null()).status();
        }
    }
}
#[cfg(target_os = "linux")]
fn run_stubborn(how_it_ends: impl FnOnce(&mut Helper)) {
    let mut h = Helper::start(r#"trap "" TERM HUP; sleep 1000 & echo PIDS $$ $!; sleep 1000"#);
    expect_started(&mut h);
    h.until_output("PIDS ");
    // The line may still be arriving.
    let end = Instant::now() + Duration::from_secs(5);
    while !h.output_text().contains('\n') || pids(&h.output_text()).len() < 2 {
        assert!(Instant::now() < end);
        h.next(Duration::from_secs(5));
    }
    let all = pids(&h.output_text());
    let _reap = Reap(all.clone());
    assert!(all.iter().all(|p| alive(*p)));
    let told = Instant::now();
    how_it_ends(&mut h);
    h.until_exited(Duration::from_secs(4));
    assert!(told.elapsed() < Duration::from_secs(4));
    let end = Instant::now() + Duration::from_secs(2);
    while all.iter().any(|p| alive(*p)) {
        assert!(Instant::now() < end, "something of the group is still running");
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn every_program_ends_when_told() {
    run_stubborn(|h| h.say(&ToHelper::End));
}

#[cfg(target_os = "linux")]
#[test]
fn a_closed_input_ends_the_program() {
    run_stubborn(|h| h.stdin = None);
}

#[cfg(target_os = "linux")]
#[test]
fn a_program_that_leaves_children_behind_has_them_ended() {
    let mut h = Helper::start(r#"sleep 1000 & echo PIDS $$ $!"#);
    expect_started(&mut h);
    h.until_output("PIDS ");
    let end = Instant::now() + Duration::from_secs(5);
    while pids(&h.output_text()).len() < 2 {
        assert!(Instant::now() < end);
        h.next(Duration::from_secs(5));
    }
    let all = pids(&h.output_text());
    let _reap = Reap(all.clone());
    h.until_exited(Duration::from_secs(5));
    let end = Instant::now() + Duration::from_secs(2);
    while all.iter().any(|p| alive(*p)) {
        assert!(Instant::now() < end, "the child outlived its program");
        thread::sleep(Duration::from_millis(20));
    }
}

fn raw(bytes: &[u8]) {
    let mut h = Helper::new();
    h.say(&ToHelper::Start(spec("sleep", &["1000"])));
    expect_started(&mut h);
    h.stdin.as_mut().unwrap().write_all(bytes).unwrap();
    h.stdin.as_mut().unwrap().flush().unwrap();
    let exit = h.until_exited(Duration::from_secs(4));
    assert!(exit.signal.is_some() || exit.code.is_some());
}

#[test]
fn a_bad_frame_ends_the_session() {
    let mut unknown = vec![99u8];
    unknown.extend_from_slice(&0u32.to_le_bytes());
    raw(&unknown);
}

#[test]
fn a_frame_claiming_four_gigabytes_ends_the_session() {
    let mut huge = vec![wire::TO_INPUT];
    huge.extend_from_slice(&u32::MAX.to_le_bytes());
    raw(&huge);
}

#[test]
fn a_bad_frame_before_the_start_ends_the_helper() {
    let mut h = Helper::new();
    h.stdin.as_mut().unwrap().write_all(&[99, 0, 0, 0, 0]).unwrap();
    assert!(h.next(Duration::from_secs(5)).is_none());
    h.finish();
}

#[test]
fn the_helper_says_nothing_on_stderr() {
    let mut h = Helper::start("printf hi");
    expect_started(&mut h);
    h.until_exited(Duration::from_secs(10));
    let mut err = String::new();
    use std::io::Read;
    h.child.stderr.take().unwrap().read_to_string(&mut err).unwrap();
    assert_eq!(err, "");
}

#[test]
fn a_terminal_too_big_is_refused() {
    let mut h = Helper::new();
    let mut s = spec("sh", &["-c", "true"]);
    s.cols = 501;
    h.say(&ToHelper::Start(s));
    match h.next(Duration::from_secs(10)) {
        Some(FromHelper::Failed(why)) => assert!(why.contains("1 to 500 columns"), "{why}"),
        other => panic!("expected Failed, got {other:?}"),
    }
    h.finish();
    let mut h = Helper::new();
    let mut s = spec("sh", &["-c", "true"]);
    s.rows = 201;
    h.say(&ToHelper::Start(s));
    assert!(matches!(h.next(Duration::from_secs(10)), Some(FromHelper::Failed(_))));
    h.finish();
}

#[test]
fn a_resize_too_big_keeps_the_size() {
    let mut h = Helper::start("read x; stty size");
    expect_started(&mut h);
    h.say(&ToHelper::Resize(501, 40));
    h.say(&ToHelper::Resize(0, 40));
    thread::sleep(Duration::from_millis(200));
    h.say(&ToHelper::Input(b"\r".to_vec()));
    h.until_output("30 100");
    h.until_exited(Duration::from_secs(10));
}

#[test]
fn the_terminals_own_words_are_set() {
    let mut h = Helper::new();
    let mut s = spec("sh", &["-c", "echo T=$TERM C=$COLUMNS L=$LINES"]);
    s.env.clear();
    s.cols = 90;
    s.rows = 25;
    h.say(&ToHelper::Start(s));
    expect_started(&mut h);
    h.until_exited(Duration::from_secs(10));
    assert!(h.output_text().contains("T=xterm-256color C=90 L=25"), "{:?}", h.output_text());
}

#[test]
fn the_apps_environment_wins() {
    let mut h = Helper::new();
    let mut s = spec("sh", &["-c", "echo T=$TERM"]);
    s.env = vec![("TERM".into(), "dumb".into())];
    h.say(&ToHelper::Start(s));
    expect_started(&mut h);
    h.until_exited(Duration::from_secs(10));
    assert!(h.output_text().contains("T=dumb"), "{:?}", h.output_text());
}

/// The most the helper's memory has been, in kilobytes.
#[cfg(target_os = "linux")]
fn peak_kb(pid: u32) -> u64 {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        })
        .unwrap_or(0)
}

#[cfg(target_os = "linux")]
#[test]
fn a_flood_does_not_swell_the_helper() {
    let mut h = Helper::start("yes | head -c 50000000");
    expect_started(&mut h);
    let pid = h.child.id();
    let mut peak = 0;
    let end = Instant::now() + Duration::from_secs(60);
    let mut total = 0usize;
    loop {
        assert!(Instant::now() < end, "the flood never ended");
        match h.next(Duration::from_secs(30)) {
            Some(FromHelper::Output(b)) => {
                total += b.len();
                assert!(b.len() <= wire::MOST_OUTPUT_CHUNK);
                h.output.clear();
                peak = peak.max(peak_kb(pid));
            }
            Some(FromHelper::Exited(_)) => break,
            Some(_) => {}
            None => panic!("ended without Exited"),
        }
    }
    peak = peak.max(peak_kb(pid));
    h.finish();
    assert!(total > 1_000_000, "only {total} bytes came");
    assert!(peak < 64 * 1024, "the helper reached {peak} kB");
}

#[cfg(windows)]
mod windows {
    use super::*;

    fn start_cmd(line: &str) -> Helper {
        let mut h = Helper::new();
        h.say(&ToHelper::Start(spec("cmd", &["/c", line])));
        h
    }

    #[test]
    fn a_program_runs_and_its_output_comes_back_on_windows() {
        let mut h = start_cmd("echo hello & exit 3");
        expect_started(&mut h);
        let exit = h.until_exited(Duration::from_secs(10));
        assert!(h.output_text().contains("hello"));
        assert_eq!(exit.code, Some(3));
    }

    #[test]
    fn every_program_ends_when_told_on_windows() {
        // A child that outlasts the program's own end, started with `start /b`.
        let mut h = start_cmd("start /b ping -n 1000 127.0.0.1 >nul & ping -n 1000 127.0.0.1 >nul");
        expect_started(&mut h);
        thread::sleep(Duration::from_millis(500));
        let told = Instant::now();
        h.say(&ToHelper::End);
        h.until_exited(Duration::from_secs(5));
        assert!(told.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_closed_input_ends_the_program_on_windows() {
        let mut h = start_cmd("ping -n 1000 127.0.0.1 >nul");
        expect_started(&mut h);
        h.stdin = None;
        h.until_exited(Duration::from_secs(5));
    }
}

/// Starts the program that ignores hang-up and terminate, and gives back its pids.
#[cfg(target_os = "linux")]
fn stubborn_running() -> (Helper, Vec<u32>) {
    let mut h = Helper::start(r#"trap "" TERM HUP INT; sleep 1000 & echo PIDS $$ $!; sleep 1000"#);
    expect_started(&mut h);
    h.until_output("PIDS ");
    let end = Instant::now() + Duration::from_secs(5);
    while pids(&h.output_text()).len() < 2 {
        assert!(Instant::now() < end);
        h.next(Duration::from_secs(5));
    }
    let all = pids(&h.output_text());
    (h, all)
}

#[cfg(target_os = "linux")]
fn send_signal(pid: u32, sig: &str) {
    let _ = Command::new("kill").args([&format!("-{sig}"), &pid.to_string()]).status();
}

#[cfg(target_os = "linux")]
fn wait_gone(all: &[u32], within: Duration) {
    let end = Instant::now() + within;
    while all.iter().any(|p| alive(*p)) {
        assert!(Instant::now() < end, "something of the group is still running");
        thread::sleep(Duration::from_millis(20));
    }
}

/// A terminal's hang-up, interrupt and quit don't end the helper, so they can't leave the program
/// behind either: the helper stays, and the program goes when the app lets go.
#[cfg(target_os = "linux")]
#[test]
fn a_terminals_signals_leave_the_helper_standing_and_the_program_goes_with_the_app() {
    for sig in ["HUP", "INT", "QUIT"] {
        let (mut h, all) = stubborn_running();
        let _reap = Reap(all.clone());
        send_signal(h.child.id(), sig);
        thread::sleep(Duration::from_millis(400));
        assert!(h.child.try_wait().unwrap().is_none(), "SIG{sig} ended the helper");
        assert!(all.iter().all(|p| alive(*p)), "SIG{sig} ended the program early");
        h.stdin = None;
        h.until_exited(Duration::from_secs(5));
        wait_gone(&all, Duration::from_secs(5));
        h.finish();
    }
}

/// `pkill stagepack` sends terminate to the helper: the program's whole group is ended, and said so.
#[cfg(target_os = "linux")]
#[test]
fn a_terminate_signal_ends_the_program() {
    let (mut h, all) = stubborn_running();
    let _reap = Reap(all.clone());
    send_signal(h.child.id(), "TERM");
    h.until_exited(Duration::from_secs(5));
    wait_gone(&all, Duration::from_secs(5));
    h.finish();
}

/// The helper is no longer in the group of whoever started it, so a terminal's signal to that group misses it.
#[cfg(target_os = "linux")]
#[test]
fn the_helper_leaves_the_starters_group() {
    let mut h = Helper::new();
    h.say(&ToHelper::Start(spec("sh", &["-c", "sleep 1"])));
    expect_started(&mut h);
    let mine = unsafe { libc::getpgid(0) };
    let theirs = unsafe { libc::getpgid(h.child.id() as i32) };
    assert_ne!(mine, theirs);
    h.until_exited(Duration::from_secs(5));
    h.finish();
}
