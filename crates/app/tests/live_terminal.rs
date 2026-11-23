//! A live terminal session in the app: the helper started with a program of the test's own choosing,
//! what it prints shown, keys sent to it, a broken helper survived, and everything ended.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use slides_terminal::{Colour, Screen};
use slides_terminal_helper::wire::StartSpec;
use stagepack_studio::live_terminal::{Session, SessionState, find_helper, split_command, threads_running};

/// The tests share the count of threads and the process's memory, so they take turns.
fn one_at_a_time() -> MutexGuard<'static, ()> {
    static TURN: Mutex<()> = Mutex::new(());
    TURN.lock().unwrap_or_else(|e| e.into_inner())
}

/// The helper, found in the target folder beside the test's own program, or built there once.
fn helper_program() -> PathBuf {
    static HELPER: OnceLock<PathBuf> = OnceLock::new();
    HELPER
        .get_or_init(|| {
            let exe = std::env::current_exe().unwrap();
            let target = exe.parent().and_then(Path::parent).unwrap().to_path_buf();
            let helper = target.join("stagepack-terminal");
            let release = target.file_name().is_some_and(|name| name == "release");
            let mut build = std::process::Command::new(std::env!("CARGO"));
            build.args(["build", "-p", "slides-terminal-helper"]);
            if release {
                build.arg("--release");
            }
            let status = build.status().expect("cargo can be run");
            assert!(status.success(), "the helper couldn't be built");
            assert!(helper.is_file(), "no helper at {}", helper.display());
            helper
        })
        .clone()
}

fn spec(program: &str, args: &[&str], cwd: &Path) -> StartSpec {
    StartSpec { program: program.into(), args: args.iter().map(|a| a.to_string()).collect(), cwd: cwd.into(), cols: 40, rows: 6, env: Vec::new() }
}

fn sh(script: &str, cwd: &Path) -> StartSpec {
    spec("sh", &["-c", script], cwd)
}

fn screen_text(screen: &Screen) -> String {
    let mut text = String::new();
    for row in screen.rows() {
        let line: String = row.iter().map(|cell| cell.text).collect();
        text.push_str(line.trim_end());
        text.push('\n');
    }
    text
}

/// Waits up to `most` for `done`.
fn within(most: Duration, mut done: impl FnMut() -> bool) -> bool {
    let until = Instant::now() + most;
    while Instant::now() < until {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    done()
}

fn no_threads_left() -> bool {
    within(Duration::from_secs(10), || threads_running() == 0)
}

#[test]
fn a_session_shows_what_the_program_prints() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let session = Session::start(&helper_program(), sh("printf '\\033[31mred\\033[0m'; sleep 5", dir.path()));
    assert!(within(Duration::from_secs(3), || screen_text(&session.screen()).starts_with("red")), "{:?}", session.screen());
    let screen = session.screen();
    let r = screen.cell(0, 0).unwrap();
    assert_eq!((r.text, r.style.fg), ('r', Colour::Indexed(1)));
    assert_eq!(screen.cell(3, 0).unwrap().style.fg, Colour::Default);
    assert_eq!(session.state(), SessionState::Running);
    drop(session);
    assert!(no_threads_left());
}

#[test]
fn keys_reach_the_program() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let session = Session::start(&helper_program(), spec("cat", &[], dir.path()));
    assert!(within(Duration::from_secs(3), || session.state() == SessionState::Running));
    session.send_keys(b"hi\r");
    // The terminal echoes the typing, and cat prints the line again.
    assert!(within(Duration::from_secs(3), || screen_text(&session.screen()).matches("hi").count() == 2), "{:?}", session.screen());
    session.end();
    assert!(within(Duration::from_secs(8), || session.state().is_over()), "{:?}", session.state());
    drop(session);
    assert!(no_threads_left());
}

#[test]
fn split_command_reads_words_and_quotes() {
    let words = |run: &str| split_command(run);
    let strings = |list: &[&str]| -> Result<Vec<String>, String> { Ok(list.iter().map(|s| s.to_string()).collect()) };
    assert_eq!(words("bash"), strings(&["bash"]));
    assert_eq!(words("  a   b  "), strings(&["a", "b"]));
    assert_eq!(words(r#"a "b c" "d\"e""#), strings(&["a", "b c", "d\"e"]));
    assert_eq!(words(r#"echo "a\\b" "c\d" "" ls*"#), strings(&["echo", "a\\b", "c\\d", "", "ls*"]));
    // Nothing else is special: no shell is added.
    assert_eq!(words("ls * | wc > out $HOME 'x y'"), strings(&["ls", "*", "|", "wc", ">", "out", "$HOME", "'x", "y'"]));
    assert_eq!(words("   "), Err("The command to run is empty.".to_string()));
    assert_eq!(words(r#"a "b c"#), Err("The command to run has a quote that isn't closed.".to_string()));
    assert_eq!(words(&"a ".repeat(64)), Ok(vec!["a".to_string(); 64]));
    assert_eq!(words(&"a ".repeat(65)), Err("The command to run is too long.".to_string()));
    assert_eq!(words(&"a".repeat(4096)).map(|w| w.len()), Ok(1));
    assert_eq!(words(&"a".repeat(4097)), Err("The command to run is too long.".to_string()));
    // The two ends of a word.
    assert_eq!(words(r#""a b"c"d e""#), strings(&["a bcd e"]));
}

fn resident_peak_kb() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let line = status.lines().find(|l| l.starts_with("VmHWM:")).unwrap();
    line.split_whitespace().nth(1).unwrap().parse().unwrap()
}

#[test]
fn a_flood_is_dropped_not_held() {
    // Forty million bytes, ten times what's taken in a second: the computer's own pseudo-terminal
    // passes only a few million a second (a hundred and fifty million bytes would take it a minute).
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let before = resident_peak_kb();
    let session = Session::start(&helper_program(), sh("yes | head -c 40000000", dir.path()));
    assert!(within(Duration::from_secs(30), || session.state().is_over()), "{:?}", session.state());
    assert!(matches!(session.state(), SessionState::Ended(_)), "{:?}", session.state());
    let grown = resident_peak_kb().saturating_sub(before);
    assert!(grown < 100 * 1024, "memory grew by {} MB", grown / 1024);
    // It said so, once.
    let notices = session.notices();
    assert_eq!(notices.iter().filter(|n| n.contains("dropped")).count(), 1, "{notices:?}");
    // The recording is bounded too, and the window stayed answerable all along.
    assert!(session.recorded().is_none_or(|cast| cast.len() <= slides_terminal::MOST_BYTES));
    drop(session);
    assert!(no_threads_left());
}

#[test]
fn a_broken_helper_is_survived() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let noisy = dir.path().join("noisy");
    std::fs::write(&noisy, "#!/bin/sh\nhead -c 3000 /dev/urandom\n").unwrap();
    let silent = dir.path().join("silent");
    std::fs::write(&silent, "#!/bin/sh\nexec sleep 1000\n").unwrap();
    let huge = dir.path().join("huge");
    // A frame that claims four gigabytes.
    std::fs::write(&huge, "#!/bin/sh\nprintf '\\002\\377\\377\\377\\377'\nexec sleep 1000\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    for script in [&noisy, &silent, &huge] {
        std::fs::set_permissions(script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let not_executable = dir.path().join("plain");
    std::fs::write(&not_executable, "just words").unwrap();
    for helper in [Path::new("/bin/true"), &noisy, &huge, &dir.path().join("missing"), &not_executable] {
        let session = Session::start(helper, sh("echo hi", dir.path()));
        assert!(within(Duration::from_secs(3), || session.state().is_over()), "{helper:?}: {:?}", session.state());
        let SessionState::Failed(why) = session.state() else { panic!("{helper:?}: {:?}", session.state()) };
        assert!(why.ends_with('.') && why.len() > 10, "{why:?}");
        assert!(no_threads_left(), "{helper:?} left a thread");
    }
    // One that says nothing and never leaves is ended with the session.
    let session = Session::start(&silent, sh("echo hi", dir.path()));
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(session.state(), SessionState::Starting);
    session.end();
    assert!(within(Duration::from_secs(8), || matches!(session.state(), SessionState::Failed(why) if why.contains("didn't end when asked"))), "{:?}", session.state());
    drop(session);
    assert!(no_threads_left());
    assert!(!process_is_running("silent"), "the silent helper was left running");
}

/// Whether a process whose command line has `word` in it is running.
fn process_is_running(word: &str) -> bool {
    std::process::Command::new("pgrep").args(["-f", word]).output().is_ok_and(|out| out.status.success())
}

#[test]
fn ending_a_session_ends_its_program() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let session = Session::start(&helper_program(), sh("exec sleep 7351", dir.path()));
    assert!(within(Duration::from_secs(5), || process_is_running("sleep 7351")));
    drop(session);
    assert!(within(Duration::from_secs(5), || !process_is_running("sleep 7351")), "the program was left running");
    assert!(no_threads_left());
}

#[test]
fn a_session_is_recorded() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let session = Session::start(&helper_program(), sh("printf one; sleep 0.3; printf two", dir.path()));
    assert!(within(Duration::from_secs(8), || session.state().is_over()), "{:?}", session.state());
    let cast = session.recorded().expect("it fits");
    let recording = slides_terminal::read(&cast).expect("a recording that can be read");
    assert_eq!((recording.width, recording.height), (40, 6));
    let outputs: Vec<&str> = recording
        .events
        .iter()
        .filter_map(|event| match &event.what {
            slides_terminal::What::Output(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(outputs.concat(), "onetwo");
    assert!(outputs.len() >= 2, "{outputs:?}");
    assert!(recording.events.windows(2).all(|pair| pair[0].at <= pair[1].at));
    drop(session);
    assert!(no_threads_left());
}

#[test]
fn the_helper_is_found_where_it_is_said_to_be() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("here");
    std::fs::write(&file, "").unwrap();
    // SAFETY: no other test reads or writes the environment while this one holds its turn.
    unsafe { std::env::set_var("STUDIO_TERMINAL_HELPER", &file) };
    assert_eq!(find_helper(), Some(file));
    unsafe { std::env::set_var("STUDIO_TERMINAL_HELPER", dir.path().join("not here")) };
    assert_eq!(find_helper(), None);
    unsafe { std::env::remove_var("STUDIO_TERMINAL_HELPER") };
}

#[test]
fn the_screen_answers_a_program_that_asks_and_nothing_else() {
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    // The program asks where the cursor is and prints what the terminal says, as a shell does.
    let script = "stty raw -echo; printf '\\033[3;5H\\033[6n'; dd bs=1 count=6 2>/dev/null | od -c | head -1; sleep 1";
    let session = Session::start(&helper_program(), sh(script, dir.path()));
    assert!(within(Duration::from_secs(5), || screen_text(&session.screen()).contains("033")), "{:?}", session.screen());
    assert!(screen_text(&session.screen()).contains("3   ;   5   R"), "{:?}", session.screen());
    drop(session);
    assert!(no_threads_left());
}

#[test]
fn pictures_are_drawn_off_the_windows_thread_and_stop_with_the_session() {
    use slides_engine::{Fonts, Rect};
    use stagepack_studio::live_terminal::Look;
    let _turn = one_at_a_time();
    let dir = tempfile::tempdir().unwrap();
    let session = Session::start(&helper_program(), sh("printf hello; sleep 0.3; printf ' again'; sleep 5", dir.path()));
    let look = Look { rect: Rect { x: 10.0, y: 10.0, w: 400.0, h: 200.0 }, rotate: 0.0, scale: 1.0, fonts: Fonts::bundled_only().seed() };
    session.set_look(look);
    assert!(within(Duration::from_secs(10), || session.latest_picture().is_some()));
    // Another picture comes when the screen changes.
    assert!(within(Duration::from_secs(5), || session.pictures_drawn() >= 2 && screen_text(&session.screen()).contains("again")));
    session.end();
    assert!(within(Duration::from_secs(8), || session.state().is_over()));
    // The last screen is drawn, and then the drawing thread is gone with the others.
    assert!(no_threads_left());
    assert!(session.latest_picture().is_some());
    drop(session);
}
