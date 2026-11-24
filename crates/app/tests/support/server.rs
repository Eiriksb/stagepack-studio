//! A small HTTP/1.1 server of a test's own on 127.0.0.1, which counts what it is asked, for the
//! tests that get the Slidev add-on. No test reaches the internet.

#![allow(dead_code)]

use std::io::{BufRead as _, BufReader, Write as _};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use stagepack_studio::addons::fetch;

/// What the server answers one request with.
pub enum Reply {
    /// A status, its extra headers and a body, sent with its length.
    Fixed(u16, Vec<(String, String)>, Vec<u8>),
    /// A body that never ends, sent with no length.
    Endless,
    /// The head, saying the body is `.0` bytes, then `.1` bytes of it, then nothing for a long while.
    Stall(u64, usize),
    /// Nothing at all, for a long while.
    Silent,
    /// The head and the first half of `.0`, then the rest once `.1` is set (or after a long while).
    Gated(Vec<u8>, Arc<AtomicBool>),
}

pub fn ok(body: impl Into<Vec<u8>>) -> Reply {
    Reply::Fixed(200, vec![("Content-Type".into(), "application/octet-stream".into())], body.into())
}

pub fn redirect(to: &str) -> Reply {
    Reply::Fixed(302, vec![("Location".into(), to.into())], Vec::new())
}

/// A server of the test's own.
pub struct Server {
    pub base: String,
    /// How many requests it was sent.
    pub hits: Arc<AtomicUsize>,
    /// The head of each request, as it was sent.
    pub heads: Arc<Mutex<Vec<Vec<String>>>>,
}

impl Server {
    /// A server answering each request's path with what `reply` gives.
    pub fn start(reply: impl Fn(&str) -> Reply + Send + Sync + 'static) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let hits = Arc::new(AtomicUsize::new(0));
        let heads = Arc::new(Mutex::new(Vec::new()));
        let reply = Arc::new(reply);
        let (counted, kept) = (hits.clone(), heads.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let (reply, counted, kept) = (reply.clone(), counted.clone(), kept.clone());
                std::thread::spawn(move || answer(stream, &*reply, &counted, &kept));
            }
        });
        Server { base, hits, heads }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// The source whose index is at `index` and whose packages are under `/download/`.
    pub fn source(&self, index: &str) -> fetch::Source {
        fetch::Source { index_url: self.url(index), package_prefix: self.url("/download/") }
    }

    /// [`Server::source`], allowed for plain http by the test hook.
    pub fn allowed(&self, index: &str) -> fetch::Source {
        let source = self.source(index);
        fetch::set_source_for_test(source.clone());
        source
    }
}

fn answer(stream: TcpStream, reply: &dyn Fn(&str) -> Reply, hits: &AtomicUsize, heads: &Mutex<Vec<Vec<String>>>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut head = Vec::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let line = line.trim_end().to_string();
        if line.is_empty() {
            break;
        }
        head.push(line);
    }
    hits.fetch_add(1, Ordering::SeqCst);
    let path = head.first().and_then(|line| line.split(' ').nth(1)).unwrap_or("/").to_string();
    heads.lock().unwrap().push(head);
    let mut out = stream;
    match reply(&path) {
        Reply::Fixed(status, headers, body) => {
            let mut text = format!("HTTP/1.1 {status} Whatever\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
            for (name, value) in headers {
                text.push_str(&format!("{name}: {value}\r\n"));
            }
            text.push_str("\r\n");
            let _ = out.write_all(text.as_bytes());
            let _ = out.write_all(&body);
        }
        Reply::Endless => {
            let _ = out.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n");
            let chunk = vec![0u8; 64 * 1024];
            while out.write_all(&chunk).is_ok() {}
        }
        Reply::Stall(length, sent) => {
            let _ = out.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n").as_bytes());
            let _ = out.write_all(&vec![0u8; sent]);
            let _ = out.flush();
            std::thread::sleep(Duration::from_secs(30));
        }
        Reply::Silent => std::thread::sleep(Duration::from_secs(30)),
        Reply::Gated(body, gate) => {
            let _ = out.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes());
            let half = body.len() / 2;
            let _ = out.write_all(&body[..half]);
            let _ = out.flush();
            let start = Instant::now();
            while !gate.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(30) {
                std::thread::sleep(Duration::from_millis(5));
            }
            let _ = out.write_all(&body[half..]);
        }
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The index of `package` at `version`, under `server`'s `/download/`.
pub fn index_of(server: &Server, package: &[u8], version: &str) -> fetch::Index {
    fetch::Index {
        name: "slidev".into(),
        version: version.into(),
        slidev: "53.0.0".into(),
        abi: 1,
        size: package.len() as u64,
        sha256: sha256_hex(package),
        url: server.url(&format!("/download/slidev-v{version}/slidev-{version}.spaddon")),
    }
}

pub fn json(index: &fetch::Index) -> Vec<u8> {
    serde_json::to_vec(index).unwrap()
}
