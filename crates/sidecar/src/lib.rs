//! Client for `stagepack serve --json`: spawns it, turns its stdout into
//! typed events, and writes commands to its stdin.

use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum SidecarEvent {
    Unpacked {
        id: String,
        title: String,
        entry: String,
        #[serde(rename = "workDir")]
        work_dir: PathBuf,
        code: Vec<String>,
        trusted: bool,
    },
    Confirm {
        question: String,
    },
    Ready {
        port: u16,
        #[serde(rename = "baseUrl")]
        base_url: String,
        #[serde(rename = "workDir")]
        work_dir: PathBuf,
    },
    Saved {
        updated: String,
    },
    Conflict {
        message: String,
    },
    SaveFailed {
        message: String,
        #[serde(default)]
        names: Vec<String>,
    },
    Busy {
        #[serde(rename = "baseUrl")]
        base_url: String,
    },
    Error {
        message: String,
        #[serde(default)]
        log: String,
    },
    Stopped,
    /// Synthesised by the client once the process has ended; always the last item.
    Exited { code: Option<i32> },
}

pub struct SpawnOptions {
    pub stagepack: PathBuf,
    pub home: PathBuf,
    pub deck: PathBuf,
    pub addons: Vec<PathBuf>,
}

pub struct Sidecar {
    child: Arc<Mutex<Child>>,
    stdin: Option<ChildStdin>,
}

const POLL: Duration = Duration::from_millis(20);
const DROP_GRACE: Duration = Duration::from_secs(10);

fn lock(c: &Mutex<Child>) -> MutexGuard<'_, Child> {
    c.lock().unwrap_or_else(|e| e.into_inner())
}

impl Sidecar {
    pub fn spawn(o: SpawnOptions) -> io::Result<(Sidecar, async_channel::Receiver<SidecarEvent>)> {
        let mut cmd = Command::new(&o.stagepack);
        cmd.arg("serve").arg("--json").arg("--home").arg(&o.home);
        for a in &o.addons {
            cmd.arg("--addon").arg(a);
        }
        cmd.arg(&o.deck)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        // Own process group: serve deletes its work folder on SIGINT, so a
        // Ctrl-C aimed at the app's terminal must not reach it.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0000_0200); // CREATE_NEW_PROCESS_GROUP
        }
        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().expect("piped stdout");
        let child = Arc::new(Mutex::new(child));
        let (tx, rx) = async_channel::unbounded();
        let waiter = Arc::clone(&child);
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if line.trim().is_empty() {
                    continue;
                }
                let ev = serde_json::from_str::<SidecarEvent>(&line).unwrap_or_else(|_| {
                    SidecarEvent::Error {
                        message: format!("Unreadable message from Stagepack: {line}"),
                        log: String::new(),
                    }
                });
                if tx.send_blocking(ev).is_err() {
                    break;
                }
            }
            // Poll rather than wait() so Drop can still take the lock to kill.
            let code = loop {
                match lock(&waiter).try_wait() {
                    Ok(Some(status)) => break status.code(),
                    Ok(None) => thread::sleep(POLL),
                    Err(_) => break None,
                }
            };
            let _ = tx.send_blocking(SidecarEvent::Exited { code });
        });
        Ok((Sidecar { child, stdin }, rx))
    }

    /// The child's process id.
    pub fn id(&self) -> u32 {
        lock(&self.child).id()
    }

    fn send(&mut self, line: &str) -> io::Result<()> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "stdin is closed"))?;
        stdin.write_all(line.as_bytes())?;
        stdin.write_all(b"\n")?;
        stdin.flush()
    }

    pub fn answer(&mut self, yes: bool) -> io::Result<()> {
        self.send(if yes { r#"{"answer":"yes"}"# } else { r#"{"answer":"no"}"# })
    }

    pub fn save(&mut self) -> io::Result<()> {
        self.send(r#"{"cmd":"save"}"#)
    }

    /// Writes `{"cmd":"stop"}` then closes stdin.
    pub fn stop(&mut self) -> io::Result<()> {
        let r = self.send(r#"{"cmd":"stop"}"#);
        self.stdin = None;
        r
    }
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        self.stdin = None;
        let deadline = Instant::now() + DROP_GRACE;
        loop {
            match lock(&self.child).try_wait() {
                Ok(Some(_)) | Err(_) => return,
                Ok(None) => {}
            }
            if Instant::now() >= deadline {
                let _ = lock(&self.child).kill();
                let _ = lock(&self.child).wait();
                return;
            }
            thread::sleep(POLL);
        }
    }
}
