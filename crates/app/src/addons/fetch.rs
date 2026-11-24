//! Getting the Slidev add-on over the network, only when the owner asks: a small index from a
//! fixed https address, the one of the app's own line of add-on interface and Slidev, naming the
//! package, then the package itself. Both are read
//! within hard caps of size and time, and the download is checked against the index, then by
//! its signature, then against the index again by its signed `addon.json`. The index proves
//! nothing; only the signature decides what runs.
//!
//! Nothing personal is sent: each request carries only `User-Agent` and `Accept`, with no
//! cookies and no query string of the app's own.

use std::io::Read as _;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub use addon_abi::package::Index;
use addon_abi::package::{index_url, read_package, version_numbers as numbers, ADDON_NAME};
use sha2::{Digest, Sha256};
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::NextTimeout;

use super::{key, SLIDEV_VERSION};

// The addresses are made as the packaging tool makes them, which writes the index, so they can't drift.
pub use addon_abi::package::PACKAGE_PREFIX;

/// Where the index of the app's own line is: the one for add-on interface [`addon_abi::ABI_VERSION`]
/// and Slidev [`SLIDEV_VERSION`]'s major version, in the release tagged `index`. It never moves,
/// so an app keeps finding the last add-on made for it, whatever is published for later apps.
pub static INDEX_URL: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| index_url(addon_abi::ABI_VERSION, SLIDEV_VERSION));

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
        Source { index_url: INDEX_URL.clone(), package_prefix: PACKAGE_PREFIX.to_string() }
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

/// How many requests the app has sent for the add-on, for the tests that pin when it does.
#[cfg(feature = "test-hooks")]
static REQUESTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// How many requests the app has sent for the add-on since it started, to any address.
#[cfg(feature = "test-hooks")]
pub fn requests_for_test() -> usize {
    REQUESTS.load(Ordering::SeqCst)
}

/// Whether `url` is one the app may ask: https, or a loopback address a test allowed.
fn allowed(url: &str) -> bool {
    if url.bytes().any(|byte| !byte.is_ascii_graphic()) || url.contains('\\') {
        return false;
    }
    if url.starts_with("https://") && url.len() > "https://".len() && !private_host(url) {
        return true;
    }
    #[cfg(feature = "test-hooks")]
    if test_hook().1.iter().any(|origin| url.strip_prefix(origin.as_str()).is_some_and(|rest| rest.starts_with('/'))) {
        return true;
    }
    false
}

/// Whether `url` names a user before its host, which only hides which host it is.
fn has_user(url: &str) -> bool {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.split(['/', '?', '#']).next().unwrap_or("").contains('@')
}

/// Whether `url`'s host is written as one that isn't on the internet at large: `localhost`, an
/// address [`global`] refuses, or one written as a number the resolver might read as such (as
/// `2130706433` or `0x7f.1`), or a user name before it. A name is looked at once it's resolved,
/// by [`GlobalOnly`], which keeps only the addresses [`global`] takes.
fn private_host(url: &str) -> bool {
    let Some((_, rest)) = url.split_once("://") else { return true };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if has_user(url) {
        return true;
    }
    let host = if let Some(v6) = authority.strip_prefix('[') {
        let Some((v6, _)) = v6.split_once(']') else { return true };
        return match v6.parse::<Ipv6Addr>() {
            Ok(address) => !global(IpAddr::V6(address)),
            Err(_) => true,
        };
    } else {
        authority.rsplit_once(':').map_or(authority, |(host, _)| host)
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() || host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    // No top-level name starts with a digit: such a host is a number, which only a dotted IPv4
    // address of four parts may be, and a global one.
    let last = host.rsplit('.').next().unwrap_or("");
    if last.starts_with(|c: char| c.is_ascii_digit()) {
        return match host.parse::<Ipv4Addr>() {
            Ok(address) => !global(IpAddr::V4(address)),
            Err(_) => true,
        };
    }
    false
}

/// Whether `address` is on the internet at large: not this computer's, a private or local
/// network's, a carrier's shared space, one set aside for benchmarks, documentation, multicast
/// or later use, nor an IPv6 address that only carries an IPv4 one. A NAT64 or 6to4 address is
/// as global as the IPv4 address it carries.
pub(crate) fn global(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => global_v4(v4),
        IpAddr::V6(v6) => global_v6(v6),
    }
}

fn global_v4(address: Ipv4Addr) -> bool {
    let [a, b, c, _] = address.octets();
    let refused = a == 0 // this network, and unspecified
        || a == 10 // private
        || a == 127 // loopback
        || (a == 100 && (b & 0xc0) == 64) // shared carrier-grade NAT, 100.64/10
        || (a == 169 && b == 254) // link-local
        || (a == 172 && (b & 0xf0) == 16) // private
        || (a == 192 && b == 168) // private
        || (a == 192 && b == 0 && c == 0) // IETF protocol assignments
        || (a == 192 && b == 0 && c == 2) // documentation
        || (a == 198 && (b & 0xfe) == 18) // benchmarking, 198.18/15
        || (a == 198 && b == 51 && c == 100) // documentation
        || (a == 203 && b == 0 && c == 113) // documentation
        || a >= 224; // multicast 224/4, reserved 240/4 and broadcast
    !refused
}

fn global_v6(address: Ipv6Addr) -> bool {
    let s = address.segments();
    let carried = |high: u16, low: u16| Ipv4Addr::from((u32::from(high) << 16) | u32::from(low));
    // NAT64's well-known prefix, 64:ff9b::/96, and 6to4, 2002::/16: as global as what they carry.
    if s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        return global_v4(carried(s[6], s[7]));
    }
    if s[0] == 0x2002 {
        return global_v4(carried(s[1], s[2]));
    }
    let refused = s[..5] == [0; 5] && (s[5] == 0 || s[5] == 0xffff) // IPv4-compatible (with :: and ::1) and IPv4-mapped
        || s[..6] == [0, 0, 0, 0, 0xffff, 0] // IPv4-translated
        || (s[0] == 0x64 && s[1] == 0xff9b && s[2] == 1) // NAT64 for local use, 64:ff9b:1::/48
        || s[..4] == [0x100, 0, 0, 0] // discard only, 100::/64
        || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
        || (s[0] == 0x3fff && s[1] < 0x1000) // documentation, 3fff::/20
        || (s[0] & 0xfe00) == 0xfc00 // unique local, fc00::/7
        || (s[0] & 0xffc0) == 0xfe80 // link-local
        || (s[0] & 0xffc0) == 0xfec0 // site-local, fec0::/10
        || (s[0] & 0xff00) == 0xff00; // multicast
    !refused
}

/// A resolver that keeps only the [`global`] addresses `inner` finds for a name, so that a name
/// that resolves to this computer or a private network, or is made to later, is never connected
/// to; and refuses the connection, setting `refused`, when none are left. The proxy the
/// environment names is resolved as it is, since the owner chose it, and the add-on's addresses
/// are then resolved by the proxy. With the test hook, so are the loopback origins a test allowed.
#[derive(Debug)]
struct GlobalOnly<R> {
    inner: R,
    refused: Arc<AtomicBool>,
}

impl<R: Resolver> Resolver for GlobalOnly<R> {
    fn resolve(&self, uri: &ureq::http::Uri, config: &ureq::config::Config, timeout: NextTimeout) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let found = self.inner.resolve(uri, config, timeout)?;
        let is_proxy = config.proxy().is_some_and(|proxy| {
            uri.host().is_some_and(|host| host.eq_ignore_ascii_case(proxy.host())) && uri.port_u16().or_else(|| default_port(uri)) == Some(proxy.port())
        });
        if is_proxy || test_origin(uri) {
            return Ok(found);
        }
        let mut kept = self.inner.empty();
        for address in found.iter().filter(|address| global(address.ip())) {
            kept.push(*address);
        }
        if kept.is_empty() {
            self.refused.store(true, Ordering::SeqCst);
            return Err(ureq::Error::HostNotFound);
        }
        Ok(kept)
    }
}

fn default_port(uri: &ureq::http::Uri) -> Option<u16> {
    match uri.scheme_str() {
        Some("https") => Some(443),
        Some("http") => Some(80),
        _ => None,
    }
}

/// Whether `uri` is on a loopback origin a test allowed.
#[cfg(feature = "test-hooks")]
fn test_origin(uri: &ureq::http::Uri) -> bool {
    let (Some(scheme), Some(authority)) = (uri.scheme_str(), uri.authority()) else { return false };
    let origin = format!("{scheme}://{authority}");
    test_hook().1.contains(&origin)
}

#[cfg(not(feature = "test-hooks"))]
fn test_origin(_: &ureq::http::Uri) -> bool {
    false
}

/// Why the app won't go to `url`, the address it was sent to after `redirects` redirects.
fn refusal(url: &str, redirects: usize) -> String {
    let first = redirects == 0;
    if has_user(url) {
        return if first {
            "The add-on's download address has a user name in it, so the app won't go there.".into()
        } else {
            "The add-on's download page sent the app on to an address with a user name in it, so it stopped there.".into()
        };
    }
    if url.starts_with("https://") && private_host(url) {
        return private_refusal(redirects);
    }
    if first {
        "The add-on's download address isn't an https one, so the app won't go there.".into()
    } else {
        "The add-on's download page sent the app on to an address that isn't https, so it stopped there.".into()
    }
}

/// What the app says of an address on this computer or a private network, after `redirects`.
fn private_refusal(redirects: usize) -> String {
    if redirects == 0 {
        "The add-on's download address leads to a local or private address, so the app won't go there.".into()
    } else {
        "The add-on's download page sent the app on to a local or private address, so it stopped there.".into()
    }
}

/// Whether the requests to `url` skip a proxy the environment names: only a test's own server.
fn is_loopback(url: &str) -> bool {
    url.starts_with("http://127.0.0.1:")
}

/// The index of the app's line from `source`, if it is one the app takes. `cancel` stops it.
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
    // serde's own words would carry the server's (an unknown field is named in them), so only
    // where it stopped is said.
    serde_json::from_slice(bytes).map_err(|error| format!("The add-on's index can't be read (line {}, column {}).", error.line(), error.column()))
}

/// Why `index` isn't one the app takes from `source`, if it isn't.
fn validate(source: &Source, index: &Index) -> Result<(), String> {
    if index.name != ADDON_NAME {
        return Err("The add-on's index offers another add-on than the Slidev add-on.".into());
    }
    if numbers(&index.version).is_none() {
        return Err("The add-on's index gives a version that isn't three numbers.".into());
    }
    if index.abi != addon_abi::ABI_VERSION {
        return Err(format!("The add-on on offer is made for add-on interface {}, where the app takes {}.", index.abi, addon_abi::ABI_VERSION));
    }
    if index.slidev != SLIDEV_VERSION {
        // Said back only if it is a version, so that none of the server's words are.
        return Err(if numbers(&index.slidev).is_some() && index.slidev.len() <= 32 {
            format!("The add-on on offer writes for Slidev {}, where Stagepack's talks are for Slidev {SLIDEV_VERSION}.", index.slidev)
        } else {
            format!("The add-on on offer writes for another Slidev than Stagepack's talks, which are for Slidev {SLIDEV_VERSION}.")
        });
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

/// What is being fetched, for its caps and sentences.
#[derive(Clone, Copy)]
enum What {
    Index,
    /// The package, of the size its index gives.
    Package(u64),
}

impl What {
    /// The most that is read: the package no more than its index's size.
    fn cap(self) -> u64 {
        match self {
            What::Index => MAX_INDEX_BYTES,
            What::Package(size) => size.min(MAX_DOWNLOAD_BYTES),
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
            What::Package(size) => format!(
                "The download is larger than the {} bytes its index gives (a download may be at most {} MiB), so it was refused.",
                size.min(MAX_DOWNLOAD_BYTES),
                MAX_DOWNLOAD_BYTES >> 20
            ),
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
    fetch_via(url, what, stop, tell, &DefaultResolver::default)
}

/// [`fetch_here`], with names resolved by what `resolver` makes, keeping only global addresses.
fn fetch_via<R: Resolver>(url: &str, what: What, stop: &AtomicBool, tell: &mpsc::Sender<Said>, resolver: &dyn Fn() -> R) -> Result<Vec<u8>, String> {
    let deadline = Instant::now() + what.time();
    let mut url = url.to_string();
    let mut redirects = 0;
    let response = loop {
        if !allowed(&url) {
            return Err(refusal(&url, redirects));
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
        // ureq refuses plain http itself too, so that only the test hook can open it.
        #[cfg(not(feature = "test-hooks"))]
        let config = config.https_only(true);
        #[cfg(feature = "test-hooks")]
        let config = config.https_only(!is_loopback(&url));
        let config = if is_loopback(&url) { config.proxy(None) } else { config };
        let refused = Arc::new(AtomicBool::new(false));
        let agent = ureq::Agent::with_parts(config.build(), ureq::unversioned::transport::DefaultConnector::default(), GlobalOnly { inner: resolver(), refused: refused.clone() });
        #[cfg(feature = "test-hooks")]
        REQUESTS.fetch_add(1, Ordering::SeqCst);
        let response = agent.get(&url).call().map_err(|error| if refused.load(Ordering::SeqCst) { private_refusal(redirects) } else { before_response(error, what) })?;
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
    if let (What::Package(size), Some(length)) = (what, body.content_length())
        && length != size
    {
        return Err(format!("The download page gives a length of {length} bytes, where the add-on's index gives a size of {size}, so it was refused."));
    }
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

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use super::*;

    /// A resolver of the test's own, finding `found` for every name.
    #[derive(Debug)]
    struct Stub(Vec<SocketAddr>);

    impl Resolver for Stub {
        fn resolve(&self, _: &ureq::http::Uri, _: &ureq::config::Config, _: NextTimeout) -> Result<ResolvedSocketAddrs, ureq::Error> {
            let mut found = self.empty();
            for address in &self.0 {
                found.push(*address);
            }
            Ok(found)
        }
    }

    fn addresses(list: &[&str]) -> Vec<SocketAddr> {
        list.iter().map(|address| address.parse().unwrap()).collect()
    }

    fn resolved(found: &[&str], uri: &str, config: &ureq::config::Config) -> (Result<Vec<SocketAddr>, ureq::Error>, bool) {
        let refused = Arc::new(AtomicBool::new(false));
        let resolver = GlobalOnly { inner: Stub(addresses(found)), refused: refused.clone() };
        let result = resolver.resolve(&uri.parse().unwrap(), config, NextTimeout::default()).map(|kept| kept.to_vec());
        (result, refused.load(Ordering::SeqCst))
    }

    const REFUSED: &[&str] = &[
        "0.0.0.0", "0.1.2.3", "10.1.2.3", "100.64.0.1", "100.127.255.254", "127.0.0.1", "127.9.9.9", "169.254.169.254", "172.16.0.1",
        "172.31.255.255", "192.0.0.8", "192.0.2.1", "192.168.1.1", "198.18.0.1", "198.19.255.255", "198.51.100.7", "203.0.113.9", "224.0.0.1",
        "239.255.255.250", "240.0.0.1", "255.255.255.255", "::", "::1", "::127.0.0.1", "::8.8.8.8", "::ffff:127.0.0.1", "::ffff:8.8.8.8",
        "::ffff:0:8.8.8.8", "64:ff9b::7f00:1", "64:ff9b::a00:1", "64:ff9b:1::808:808", "2002:7f00:1::", "2002:c0a8:101::1", "100::1",
        "2001:db8::1", "3fff::1", "fc00::1", "fd12:3456::1", "fe80::1", "febf::1", "fec0::1", "feff::1", "ff02::1", "ff0e::1",
    ];

    const KEPT: &[&str] = &[
        "8.8.8.8", "140.82.121.3", "185.199.108.133", "1.1.1.1", "100.63.255.255", "100.128.0.1", "172.15.255.255", "172.32.0.1", "198.17.255.255",
        "198.20.0.1", "223.255.255.255", "2606:50c0:8000::153", "2a00:1450:4001:80b::200e", "64:ff9b::808:808", "2002:808:808::1", "2001:db9::1",
        "3fff:1000::1",
    ];

    #[test]
    fn only_global_addresses_are_global() {
        for address in REFUSED {
            assert!(!global(address.parse().unwrap()), "{address} is not global");
        }
        for address in KEPT {
            assert!(global(address.parse().unwrap()), "{address} is global");
        }
    }

    #[test]
    fn a_name_is_connected_to_only_at_its_global_addresses() {
        let config = ureq::config::Config::default();
        let (kept, refused) = resolved(&["10.0.0.1:443", "140.82.121.3:443", "[::1]:443", "[2606:50c0:8000::153]:443"], "https://github.com/x", &config);
        assert_eq!(kept.unwrap(), addresses(&["140.82.121.3:443", "[2606:50c0:8000::153]:443"]));
        assert!(!refused);

        // A name with none, as one made to resolve to this computer or a private network.
        for address in REFUSED {
            let found = format!("{}:443", if address.contains(':') { format!("[{address}]") } else { address.to_string() });
            let (kept, refused) = resolved(&[found.as_str()], "https://rebound.example/x", &config);
            assert!(matches!(kept, Err(ureq::Error::HostNotFound)), "{address}: {kept:?}");
            assert!(refused, "{address}");
        }
    }

    #[test]
    fn the_proxy_the_environment_names_is_resolved_as_it_is() {
        let proxy = ureq::Proxy::new("http://10.1.2.3:3128").unwrap();
        let config = ureq::config::Config::builder().proxy(Some(proxy)).build();
        let (kept, refused) = resolved(&["10.1.2.3:3128"], "http://10.1.2.3:3128", &config);
        assert_eq!(kept.unwrap(), addresses(&["10.1.2.3:3128"]));
        assert!(!refused);
        // But nothing else on that network, nor the proxy's host on another port.
        for uri in ["https://rebound.example/x", "http://10.1.2.3:80"] {
            let (kept, refused) = resolved(&["10.1.2.3:3128"], uri, &config);
            assert!(kept.is_err() && refused, "{uri}");
        }
    }

    #[cfg(feature = "test-hooks")]
    #[test]
    fn a_loopback_origin_a_test_allowed_is_resolved_as_it_is() {
        set_source_for_test(Source { index_url: "http://127.0.0.1:9/index.json".into(), package_prefix: "http://127.0.0.1:9/download/".into() });
        let config = ureq::config::Config::default();
        let (kept, _) = resolved(&["127.0.0.1:9"], "http://127.0.0.1:9/index.json", &config);
        assert_eq!(kept.unwrap(), addresses(&["127.0.0.1:9"]));
        // Another port isn't.
        let (kept, refused) = resolved(&["127.0.0.1:10"], "http://127.0.0.1:10/index.json", &config);
        assert!(kept.is_err() && refused);
    }

    #[test]
    fn a_name_resolving_to_a_private_address_is_refused_in_one_sentence() {
        let (tell, _told) = mpsc::channel();
        let stub = || Stub(addresses(&["192.168.1.1:443", "[fd00::1]:443"]));
        let refused = fetch_via("https://rebound.example/index.json", What::Index, &AtomicBool::new(false), &tell, &stub).unwrap_err();
        assert_eq!(refused, "The add-on's download address leads to a local or private address, so the app won't go there.");
    }

    #[test]
    fn the_official_addresses_pass_the_checks() {
        for url in [INDEX_URL.as_str(), &format!("{PACKAGE_PREFIX}slidev-v0.1.0/slidev-0.1.0.spaddon")] {
            assert!(allowed(url) && !private_host(url), "{url}");
        }
        // And where GitHub sends a release's file on to, signed query string and all.
        for url in [
            "https://release-assets.githubusercontent.com/github-production-release-asset/1/2?sp=r&sv=2018-11-09&sr=b&spr=https&se=2026-11-24T10%3A00%3A00Z&sig=abc%2Bdef%3D",
            "https://objects.githubusercontent.com/github-production-release-asset-2e65be/1/2?X-Amz-Algorithm=AWS4-HMAC-SHA256&X-Amz-Credential=a%2F20261124%2Fus-east-1",
        ] {
            assert!(allowed(url), "{url}");
        }
    }

    #[test]
    fn a_user_name_before_the_host_has_its_own_sentence() {
        assert_eq!(refusal("https://user@example.com/x", 0), "The add-on's download address has a user name in it, so the app won't go there.");
        assert_eq!(refusal("https://user@example.com/x", 1), "The add-on's download page sent the app on to an address with a user name in it, so it stopped there.");
        // An @ in the query string is no user name.
        assert!(!has_user("https://example.com/x?a=b@c"));
    }
}
