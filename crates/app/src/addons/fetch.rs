//! Getting the Slidev add-on over the network, only when the owner asks: a small `index.json`
//! from a fixed https address, naming the package, then the package itself. Both are read
//! within hard caps of size and time, and the download is checked against the index, then by
//! its signature, then against the index again by its signed `addon.json`. The index proves
//! nothing; only the signature decides what runs.
//!
//! Nothing personal is sent: each request carries only `User-Agent` and `Accept`, with no
//! cookies and no query string of the app's own.

use std::io::Read as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub use addon_abi::package::Index;
use addon_abi::package::{read_package, ADDON_NAME};
use sha2::{Digest, Sha256};

use super::{key, SLIDEV_VERSION};

/// Where the latest release's index is.
pub const INDEX_URL: &str = "https://github.com/Eiriksb/stagepack-studio-addons/releases/latest/download/index.json";
/// What every package's address starts with.
pub const PACKAGE_PREFIX: &str = "https://github.com/Eiriksb/stagepack-studio-addons/releases/download/";

/// The most an index may be.
pub const MAX_INDEX_BYTES: u64 = 64 * 1024;
/// The most a download may be.
pub const MAX_DOWNLOAD_BYTES: u64 = 48 * 1024 * 1024;
/// How long connecting may take.
const CONNECT_TIME: Duration = Duration::from_secs(10);
/// How long the index may take, from asking to the last byte, redirects and all.
const INDEX_TIME: Duration = Duration::from_secs(20);
/// How long the package may take, in the same way.
const PACKAGE_TIME: Duration = Duration::from_secs(180);
/// The most redirects followed.
const MAX_REDIRECTS: usize = 5;
/// How often a cancel is looked for.
const CANCEL_TICK: Duration = Duration::from_millis(50);
/// How much is read between reports of progress.
const CHUNK: usize = 64 * 1024;

/// What the app says when the download page can't be reached at all.
pub const UNREACHABLE: &str = "Couldn't reach the add-on's download page. Check the internet connection, or install it from a file.";
/// What the app says of a download the owner cancelled.
pub const CANCELLED: &str = "The download was cancelled.";

/// Where the add-on is got from: its index, and what the index's package address must start with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub index_url: String,
    pub package_prefix: String,
}

impl Source {
    /// The add-on's releases.
    pub fn official() -> Source {
        Source { index_url: INDEX_URL.to_string(), package_prefix: PACKAGE_PREFIX.to_string() }
    }
}

/// The source the app gets the add-on from: [`Source::official`], unless a test put another in
/// its place.
pub fn source() -> Source {
    #[cfg(feature = "test-hooks")]
    if let Some(source) = test_hook().0.clone() {
        return source;
    }
    Source::official()
}

/// What a test allowed: the source in place of the official one, and the loopback origins
/// plain http may go to.
#[cfg(feature = "test-hooks")]
static TEST_SOURCE: std::sync::Mutex<(Option<Source>, Vec<String>)> = std::sync::Mutex::new((None, Vec::new()));

#[cfg(feature = "test-hooks")]
fn test_hook() -> std::sync::MutexGuard<'static, (Option<Source>, Vec<String>)> {
    TEST_SOURCE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Puts `source` in place of the official one for the whole process, and lets plain http go to
/// its addresses if they are on `http://127.0.0.1:<port>/`, and to no other.
#[cfg(feature = "test-hooks")]
pub fn set_source_for_test(source: Source) {
    let mut hook = test_hook();
    for url in [&source.index_url, &source.package_prefix] {
        if let Some(rest) = url.strip_prefix("http://127.0.0.1:") {
            let port = rest.split('/').next().unwrap_or("");
            if !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()) && rest.len() > port.len() {
                let origin = format!("http://127.0.0.1:{port}");
                if !hook.1.contains(&origin) {
                    hook.1.push(origin);
                }
            }
        }
    }
    hook.0 = Some(source);
}

/// Whether `url` is one the app may ask: https, or a loopback address a test allowed.
fn allowed(url: &str) -> bool {
    if url.bytes().any(|byte| !byte.is_ascii_graphic()) || url.contains('\\') {
        return false;
    }
    if url.starts_with("https://") && url.len() > "https://".len() {
        return true;
    }
    #[cfg(feature = "test-hooks")]
    if test_hook().1.iter().any(|origin| url.strip_prefix(origin.as_str()).is_some_and(|rest| rest.starts_with('/'))) {
        return true;
    }
    false
}

/// Whether the requests to `url` skip a proxy the environment names: only a test's own server.
fn is_loopback(url: &str) -> bool {
    url.starts_with("http://127.0.0.1:")
}

/// The latest release's index from `source`, if it is one the app takes. `cancel` stops it.
pub fn check(source: &Source, cancel: &AtomicBool) -> Result<Index, String> {
    if !allowed(&source.index_url) {
        return Err("The add-on's index isn't at an https address, so the app won't ask for it.".into());
    }
    let bytes = fetch(&source.index_url, What::Index, cancel, &|_, _| {})?;
    let index = parse_index(&bytes)?;
    validate(source, &index)?;
    Ok(index)
}

/// The index in `bytes`, or why it isn't one.
fn parse_index(bytes: &[u8]) -> Result<Index, String> {
    let looks_like_json = bytes.iter().find(|byte| !byte.is_ascii_whitespace()) == Some(&b'{');
    if !looks_like_json {
        return Err("The page the app got isn't the add-on's index; the network may want a sign-in first. Check the internet connection, or install it from a file.".into());
    }
    serde_json::from_slice(bytes).map_err(|error| {
        let said = error.to_string();
        format!("The add-on's index can't be read ({}).", slides_components::shortened(said.lines().next().unwrap_or(""), 120))
    })
}

/// Why `index` isn't one the app takes from `source`, if it isn't.
fn validate(source: &Source, index: &Index) -> Result<(), String> {
    if index.name != ADDON_NAME {
        return Err(format!("The add-on's index offers {:?}, not the Slidev add-on.", slides_components::shortened(&index.name, 40)));
    }
    if numbers(&index.version).is_none() {
        return Err(format!("The add-on's index gives the version {:?}, which isn't three numbers.", slides_components::shortened(&index.version, 40)));
    }
    if index.abi != addon_abi::ABI_VERSION {
        return Err(format!("The add-on on offer is made for add-on interface {}, where the app takes {}.", index.abi, addon_abi::ABI_VERSION));
    }
    if index.slidev != SLIDEV_VERSION {
        return Err(format!(
            "The add-on on offer writes for Slidev {}, where Stagepack's talks are for Slidev {SLIDEV_VERSION}.",
            slides_components::shortened(&index.slidev, 40)
        ));
    }
    if index.size == 0 || index.size > MAX_DOWNLOAD_BYTES {
        return Err(format!("The add-on's index gives a size of {} bytes, where a download may be 1 byte to {} MiB.", index.size, MAX_DOWNLOAD_BYTES >> 20));
    }
    if index.sha256.len() != 64 || !index.sha256.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f')) {
        return Err("The add-on's index gives a sha256 that isn't 64 lowercase hex digits.".into());
    }
    let in_releases = index.url.strip_prefix(source.package_prefix.as_str()).is_some_and(|rest| {
        !rest.is_empty()
            && rest.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'/'))
            && rest.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
    });
    if !in_releases || !allowed(&index.url) {
        return Err("The add-on's index points somewhere other than the add-on's releases, so it was refused.".into());
    }
    Ok(())
}

/// The package `index` names, from `source`, if it is what the index says, its signature holds,
/// and its signed `addon.json` agrees with the index. `progress` is told the bytes had so far
/// and the bytes in all; `cancel` stops it. Nothing is written anywhere.
pub fn download(source: &Source, index: &Index, cancel: &AtomicBool, progress: &dyn Fn(u64, u64)) -> Result<Vec<u8>, String> {
    validate(source, index)?;
    let bytes = fetch(&index.url, What::Package(index.size), cancel, progress)?;
    if bytes.len() as u64 != index.size {
        return Err(format!("The download is {} bytes, where the add-on's index gives a size of {}, so it was refused.", bytes.len(), index.size));
    }
    let digest: String = Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect();
    if digest != index.sha256 {
        return Err("The download's sha256 doesn't match the add-on's index, so it was refused.".into());
    }
    let package = read_package(&bytes, &key::addon_key())?;
    let signed = &package.manifest;
    for (field, signed, offered) in [
        ("name", signed.name.clone(), index.name.clone()),
        ("version", signed.version.clone(), index.version.clone()),
        ("interface", signed.abi.to_string(), index.abi.to_string()),
        ("Slidev version", signed.slidev.clone(), index.slidev.clone()),
    ] {
        if signed != offered {
            return Err(format!(
                "The add-on's signed addon.json gives the {field} {}, where its index gives {}, so it was refused.",
                slides_components::shortened(&signed, 40),
                slides_components::shortened(&offered, 40)
            ));
        }
    }
    Ok(bytes)
}

/// Whether `offered` is a later version than `installed`, both three numbers. A version that
/// isn't is never later, nor is one before it.
pub fn newer(installed: &str, offered: &str) -> bool {
    match (numbers(installed), numbers(offered)) {
        (Some(installed), Some(offered)) => offered > installed,
        _ => false,
    }
}

/// The three numbers of `version`, `X.Y.Z`, with no leading zeros, signs or anything else.
fn numbers(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.').map(|part| {
        let digits = !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()) && (part == "0" || !part.starts_with('0'));
        if digits { part.parse::<u64>().ok() } else { None }
    });
    let numbers = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(numbers)
}

/// What is being fetched, for its caps and sentences.
#[derive(Clone, Copy)]
enum What {
    Index,
    /// The package, of the size its index gives.
    Package(u64),
}

impl What {
    fn cap(self) -> u64 {
        match self {
            What::Index => MAX_INDEX_BYTES,
            What::Package(_) => MAX_DOWNLOAD_BYTES,
        }
    }

    fn time(self) -> Duration {
        match self {
            What::Index => INDEX_TIME,
            What::Package(_) => PACKAGE_TIME,
        }
    }

    fn too_large(self) -> String {
        match self {
            What::Index => "The add-on's index is larger than 64 KiB, so it was refused.".into(),
            What::Package(_) => format!("The download is larger than {} MiB, so it was refused.", MAX_DOWNLOAD_BYTES >> 20),
        }
    }

    fn too_slow(self) -> String {
        match self {
            What::Index => "The add-on's download page took longer than 20 seconds to answer. Check the internet connection, or install it from a file.".into(),
            What::Package(_) => "The download took longer than 3 minutes, so it was stopped. Check the internet connection, or install it from a file.".into(),
        }
    }
}

/// What the fetching thread tells the caller.
enum Said {
    Progress(u64, u64),
    Done(Result<Vec<u8>, String>),
}

/// The body at `url`, read on a thread of its own so that `cancel` is seen within a tick
/// whatever the network does; the thread, left behind, stops at its next read or its time.
fn fetch(url: &str, what: What, cancel: &AtomicBool, progress: &dyn Fn(u64, u64)) -> Result<Vec<u8>, String> {
    if cancel.load(Ordering::SeqCst) {
        return Err(CANCELLED.into());
    }
    let stop = Arc::new(AtomicBool::new(false));
    let (tell, told) = mpsc::channel();
    let url = url.to_string();
    let stopped = stop.clone();
    std::thread::Builder::new()
        .name("addon-fetch".into())
        .spawn(move || {
            let result = fetch_here(&url, what, &stopped, &tell);
            let _ = tell.send(Said::Done(result));
        })
        .map_err(|_| "The download couldn't be started.".to_string())?;
    loop {
        match told.recv_timeout(CANCEL_TICK) {
            Ok(Said::Progress(done, total)) => progress(done, total),
            Ok(Said::Done(result)) => return result,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return Err("The download stopped unexpectedly.".into()),
        }
        if cancel.load(Ordering::SeqCst) {
            stop.store(true, Ordering::SeqCst);
            return Err(CANCELLED.into());
        }
    }
}

/// [`fetch`], on the fetching thread: follows up to [`MAX_REDIRECTS`] redirects, each to an
/// allowed address only, then reads the body within the cap.
fn fetch_here(url: &str, what: What, stop: &AtomicBool, tell: &mpsc::Sender<Said>) -> Result<Vec<u8>, String> {
    let deadline = Instant::now() + what.time();
    let mut url = url.to_string();
    let mut redirects = 0;
    let response = loop {
        if !allowed(&url) {
            return Err(if redirects == 0 {
                "The add-on's download address isn't an https one, so the app won't go there.".into()
            } else {
                "The add-on's download page sent the app on to an address that isn't https, so it stopped there.".into()
            });
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(what.too_slow());
        }
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .user_agent(concat!("stagepack-studio/", env!("CARGO_PKG_VERSION")))
            .accept("application/octet-stream")
            .accept_encoding("")
            .timeout_connect(Some(CONNECT_TIME))
            .timeout_global(Some(left));
        let config = if is_loopback(&url) { config.proxy(None) } else { config };
        let agent = ureq::Agent::new_with_config(config.build());
        let response = agent.get(&url).call().map_err(|error| before_response(error, what))?;
        if stop.load(Ordering::SeqCst) {
            return Err(CANCELLED.into());
        }
        let status = response.status().as_u16();
        if (300..400).contains(&status) && status != 304 {
            let location = response.headers().get("location").and_then(|value| value.to_str().ok()).map(str::to_string);
            let Some(location) = location else {
                return Err(format!("The add-on's download page answered {status} with nowhere to go on to."));
            };
            redirects += 1;
            if redirects > MAX_REDIRECTS {
                return Err(format!("The add-on's download page sent the app on more than {MAX_REDIRECTS} times, so it stopped (too many redirects)."));
            }
            url = resolved(&url, &location);
            continue;
        }
        if status != 200 {
            return Err(format!("The add-on's download page answered {status} instead of the file, so nothing was downloaded."));
        }
        break response;
    };
    let total = match what {
        What::Package(size) => size,
        What::Index => 0,
    };
    let body = response.into_body();
    if body.content_length().is_some_and(|length| length > what.cap()) {
        return Err(what.too_large());
    }
    let mut reader = body.into_reader().take(what.cap() + 1);
    let mut bytes = Vec::new();
    let mut chunk = vec![0; CHUNK];
    loop {
        if stop.load(Ordering::SeqCst) {
            return Err(CANCELLED.into());
        }
        let read = match reader.read(&mut chunk) {
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(while_reading(ureq::Error::from(error), what)),
        };
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.len() as u64 > what.cap() {
            return Err(what.too_large());
        }
        let _ = tell.send(Said::Progress(bytes.len() as u64, total));
    }
    Ok(bytes)
}

/// The address `location` names, from a response to `url`: as it is if it has a scheme, or on
/// `url`'s origin if it is a path. Anything else is left as it is, to be refused.
fn resolved(url: &str, location: &str) -> String {
    let scheme_end = url.find("://").map_or(0, |at| at + 3);
    if location.starts_with("//") {
        return format!("{}{location}", &url[..scheme_end.saturating_sub(2)]);
    }
    if location.starts_with('/') {
        let origin_end = url[scheme_end..].find(['/', '?', '#']).map_or(url.len(), |at| scheme_end + at);
        return format!("{}{location}", &url[..origin_end]);
    }
    location.to_string()
}

/// What the app says of `error`, met before any answer came.
fn before_response(error: ureq::Error, what: What) -> String {
    match error {
        ureq::Error::Timeout(ureq::Timeout::Connect | ureq::Timeout::Resolve) => UNREACHABLE.into(),
        ureq::Error::Timeout(_) => what.too_slow(),
        ureq::Error::Tls(_) => "The add-on's download page couldn't be reached securely (its certificate or connection didn't check out). Check the internet connection, or install it from a file.".into(),
        _ => UNREACHABLE.into(),
    }
}

/// What the app says of `error`, met while the body was read.
fn while_reading(error: ureq::Error, what: What) -> String {
    match error {
        ureq::Error::Timeout(_) => what.too_slow(),
        _ => "The download broke off part way. Check the internet connection, and try again or install it from a file.".into(),
    }
}
