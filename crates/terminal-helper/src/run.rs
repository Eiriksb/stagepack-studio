//! Runs one program in a pseudo-terminal for the app, spoken to in the frames of [`crate::wire`].
//!
//! Nothing is printed to standard output but frames, and nothing to standard error. The program is
//! ended when told to, when the app's input closes, or when a frame makes no sense: first politely, then
//! after two seconds by force, and its whole process group (a job on Windows) goes with it.
use crate::wire::{self, Exit, FromHelper, StartSpec, ToHelper};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{self, Read, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// How long the program has to end politely before it is ended by force.
const POLITE: Duration = Duration::from_secs(2);
/// How long the last of the output is waited for once the program has ended.
const DRAIN: Duration = Duration::from_secs(1);

/// What the helper's threads tell its main loop.
enum Event {
    Said(ToHelper),
    /// The app's input closed, a frame made no sense, or the app stopped reading.
    Gone,
    /// Everything the program printed has been passed on.
    PumpDone,
    /// The program ended.
    Ended(Exit),
}

type Out = Arc<Mutex<io::Stdout>>;

fn say(out: &Out, what: &FromHelper) -> io::Result<()> {
    let mut out = out.lock().unwrap_or_else(|e| e.into_inner());
    what.write(&mut *out)
}

/// The helper's whole life. Returns the status to exit with, which is always 0: how the program ended
/// is said in a frame, not by the helper's own status.
pub fn main() -> i32 {
    let out: Out = Arc::new(Mutex::new(io::stdout()));
    let (tx, rx) = mpsc::channel();
    #[cfg(unix)]
    apart_from_the_terminal(&tx);
    {
        let tx = tx.clone();
        thread::spawn(move || read_input(tx));
    }
    session(&out, &tx, &rx);
    0
}

/// Keeps the helper out of reach of what a terminal or `pkill` sends: it leaves the app's process group, so
/// Ctrl+C and a closed terminal don't reach it, ignores the hang-up, interrupt and quit signals that still
/// might, and turns a terminate signal into an end, so the program's group is ended before the helper goes.
/// It is only the app closing the input (or telling it to end) that ends the program otherwise.
#[cfg(unix)]
fn apart_from_the_terminal(tx: &Sender<Event>) {
    // SAFETY: plain calls with plain numbers; nothing else is running yet but this thread, so the mask
    // set here is the one every thread of the helper gets. The program's own start clears both again.
    unsafe {
        // Fails when the helper already leads a group of its own, which is as good.
        libc::setsid();
        for sig in [libc::SIGHUP, libc::SIGINT, libc::SIGQUIT] {
            libc::signal(sig, libc::SIG_IGN);
        }
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, libc::SIGTERM);
        libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
        let tx = tx.clone();
        thread::spawn(move || {
            let mut got = 0;
            if libc::sigwait(&set, &mut got) == 0 {
                let _ = tx.send(Event::Gone);
            }
        });
    }
}

fn read_input(tx: Sender<Event>) {
    let mut input = io::stdin().lock();
    loop {
        let said = match wire::read_frame(&mut input) {
            Ok(Some((kind, bytes))) => ToHelper::decode(kind, &bytes).ok(),
            _ => None,
        };
        match said {
            Some(said) => {
                if tx.send(Event::Said(said)).is_err() {
                    return;
                }
            }
            None => {
                let _ = tx.send(Event::Gone);
                return;
            }
        }
    }
}

fn session(out: &Out, tx: &Sender<Event>, rx: &Receiver<Event>) {
    let spec = match rx.recv() {
        Ok(Event::Said(ToHelper::Start(spec))) => spec,
        _ => return,
    };
    let mut live = match start(&spec) {
        Ok(live) => live,
        Err(why) => {
            let _ = say(out, &FromHelper::Failed(why));
            return;
        }
    };
    if say(out, &FromHelper::Started).is_err() {
        live.end(rx);
        return;
    }
    live.watch(out, tx);

    let exit = loop {
        match rx.recv() {
            Ok(Event::Said(ToHelper::Input(bytes))) => live.type_in(bytes),
            Ok(Event::Said(ToHelper::Resize(cols, rows))) => live.resize(cols, rows),
            Ok(Event::Ended(exit)) => break live.after_end(rx, exit),
            Ok(Event::PumpDone) => live.pumped = true,
            // An end, a second start, a closed input or a bad frame: all end the program.
            Ok(Event::Said(_)) | Ok(Event::Gone) | Err(_) => break live.end(rx),
        }
    };
    live.drain(rx);
    let _ = say(out, &FromHelper::Exited(exit));
}

fn size_ok(cols: u16, rows: u16) -> bool {
    (1..=wire::MOST_COLS).contains(&cols) && (1..=wire::MOST_ROWS).contains(&rows)
}

fn failed_to_start(program: &str, why: &str) -> String {
    let missing = ["No viable candidates", "does not exist", "doesn't exist", "Unable to resolve", "not found", "cannot find"];
    if missing.iter().any(|m| why.contains(m)) {
        format!("There's no program called \"{program}\" here.")
    } else if why.contains("not executable") || why.contains("directory") {
        format!("\"{program}\" can't be run here, it isn't a program.")
    } else {
        format!("The program \"{program}\" couldn't be started.")
    }
}

/// A program that is running, and what it takes to end it.
struct Live {
    master: Option<Box<dyn portable_pty::MasterPty + Send>>,
    reader: Option<Box<dyn Read + Send>>,
    typing: Sender<Vec<u8>>,
    killer: Box<dyn portable_pty::ChildKiller + Send + Sync>,
    child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    group: Group,
    pumped: bool,
}

fn start(spec: &StartSpec) -> Result<Live, String> {
    let program = spec.program.as_str();
    if program.is_empty() {
        return Err("There's no program to run.".into());
    }
    if !size_ok(spec.cols, spec.rows) {
        return Err("A terminal can be 1 to 500 columns and 1 to 200 rows.".into());
    }
    if !spec.cwd.is_dir() {
        return Err(format!("The folder \"{}\" isn't there.", spec.cwd.display()));
    }
    let size = PtySize { rows: spec.rows, cols: spec.cols, pixel_width: 0, pixel_height: 0 };
    let pair = native_pty_system()
        .openpty(size)
        .map_err(|_| "The computer wouldn't make a terminal.".to_string())?;
    let mut cmd = CommandBuilder::new(program);
    cmd.args(&spec.args);
    cmd.cwd(&spec.cwd);
    // A terminal's own words, unless the app says otherwise.
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLUMNS", spec.cols.to_string());
    cmd.env("LINES", spec.rows.to_string());
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| failed_to_start(program, &format!("{e:#}")))?;
    // The program holds the only other end now; ours must close so the output ends when it does.
    drop(pair.slave);
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|_| "The computer wouldn't read the terminal.".to_string())?;
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|_| "The computer wouldn't write to the terminal.".to_string())?;
    let (typing, typed) = mpsc::channel::<Vec<u8>>();
    // Typing goes through its own thread so a program that isn't reading can't stop the helper hearing "end".
    thread::spawn(move || {
        while let Ok(bytes) = typed.recv() {
            if writer.write_all(&bytes).and_then(|_| writer.flush()).is_err() {
                break;
            }
        }
    });
    let group = match Group::of(child.as_ref()) {
        Ok(group) => group,
        Err(why) => {
            let _ = child.kill();
            return Err(why);
        }
    };
    Ok(Live {
        master: Some(pair.master),
        reader: Some(reader),
        typing,
        killer: child.clone_killer(),
        child: Some(child),
        group,
        pumped: false,
    })
}

impl Live {
    /// Starts the threads that pass the output on and notice the program ending.
    fn watch(&mut self, out: &Out, tx: &Sender<Event>) {
        if let Some(mut reader) = self.reader.take() {
            let (out, tx) = (out.clone(), tx.clone());
            thread::spawn(move || {
                let mut buf = vec![0u8; wire::MOST_OUTPUT_CHUNK];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if say(&out, &FromHelper::Output(buf[..n].to_vec())).is_err() {
                                let _ = tx.send(Event::Gone);
                                break;
                            }
                        }
                    }
                }
                let _ = tx.send(Event::PumpDone);
            });
        }
        if let Some(mut child) = self.child.take() {
            let tx = tx.clone();
            thread::spawn(move || {
                let exit = match child.wait() {
                    Ok(status) => match status.signal() {
                        Some(sig) => Exit { code: None, signal: Some(sig.to_string()) },
                        None => Exit { code: Some(status.exit_code() as i32), signal: None },
                    },
                    Err(_) => Exit { code: None, signal: None },
                };
                let _ = tx.send(Event::Ended(exit));
            });
        }
    }

    fn type_in(&self, bytes: Vec<u8>) {
        let _ = self.typing.send(bytes);
    }

    fn resize(&self, cols: u16, rows: u16) {
        if !size_ok(cols, rows) {
            return;
        }
        if let Some(master) = &self.master {
            let _ = master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        }
    }

    /// Ends the program and everything it started, and says how it ended.
    fn end(&mut self, rx: &Receiver<Event>) -> Exit {
        let deadline = Instant::now() + POLITE;
        if self.group.exists() {
            self.group.ask_politely();
        }
        self.master = None;
        let mut exit = None;
        while exit.is_none() && Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(20)) {
                Ok(Event::Ended(e)) => exit = Some(e),
                Ok(Event::PumpDone) => self.pumped = true,
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        self.after_end_until(rx, exit, deadline)
    }

    /// The program itself has ended; whatever it left behind is ended too.
    fn after_end(&mut self, rx: &Receiver<Event>, exit: Exit) -> Exit {
        let deadline = Instant::now() + POLITE;
        if self.group.exists() {
            self.group.ask_politely();
        }
        self.master = None;
        self.after_end_until(rx, Some(exit), deadline)
    }

    /// Waits until the group is gone or the polite time is up, then ends what is left by force.
    fn after_end_until(&mut self, rx: &Receiver<Event>, mut exit: Option<Exit>, deadline: Instant) -> Exit {
        while self.group.exists() && Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(20)) {
                Ok(Event::Ended(e)) => exit = Some(e),
                Ok(Event::PumpDone) => self.pumped = true,
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        self.group.force();
        if exit.is_none() {
            let _ = self.killer.kill();
        }
        // The leader's end is waited for a little longer, as it is only a signal away now.
        let until = Instant::now() + DRAIN;
        while exit.is_none() && Instant::now() < until {
            match rx.recv_timeout(Duration::from_millis(20)) {
                Ok(Event::Ended(e)) => exit = Some(e),
                Ok(Event::PumpDone) => self.pumped = true,
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        exit.unwrap_or(Exit { code: None, signal: Some("Killed".into()) })
    }

    /// Waits a moment for the last of the output to have been passed on.
    fn drain(&mut self, rx: &Receiver<Event>) {
        let until = Instant::now() + DRAIN;
        while !self.pumped && Instant::now() < until {
            match rx.recv_timeout(Duration::from_millis(20)) {
                Ok(Event::PumpDone) => self.pumped = true,
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    }
}

#[cfg(unix)]
struct Group(Option<i32>);

#[cfg(unix)]
impl Group {
    /// The program was started as the leader of a session of its own, so its process group is its pid.
    fn of(child: &(dyn portable_pty::Child + Send + Sync)) -> Result<Group, String> {
        Ok(Group(child.process_id().map(|p| p as i32)))
    }

    fn signal(&self, sig: i32) {
        if let Some(pgid) = self.0 {
            // SAFETY: kill(2) on a group we made, with a plain signal number.
            unsafe { libc::kill(-pgid, sig) };
        }
    }

    /// Whether anything of the program's group is still there.
    fn exists(&self) -> bool {
        match self.0 {
            // SAFETY: signal 0 only asks whether the group is there.
            Some(pgid) => unsafe { libc::kill(-pgid, 0) == 0 },
            None => false,
        }
    }

    fn ask_politely(&self) {
        self.signal(libc::SIGHUP);
        self.signal(libc::SIGTERM);
    }

    fn force(&self) {
        if self.exists() {
            self.signal(libc::SIGKILL);
        }
    }
}

#[cfg(windows)]
use crate::winjob::Group;
