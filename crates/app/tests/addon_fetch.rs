//! Getting the Slidev add-on over the network: the index, the package, and every way a server
//! can be hostile or broken. Every server here is a small HTTP/1.1 server of the test's own on
//! 127.0.0.1; no test reaches the internet.

#[path = "support/addons.rs"]
mod support;
#[path = "support/server.rs"]
mod server;

use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use server::*;
use stagepack_studio::addons::{self, InstalledState, fetch};
use support::*;

/// The signed package of a made-up add-on at `version`.
fn package(version: &str) -> Vec<u8> {
    package_signed(&answering_guest_at(&export_answer(), version), version, test_key())
}

fn no_cancel() -> AtomicBool {
    AtomicBool::new(false)
}

fn one_sentence(sentence: &str) {
    assert!(!sentence.is_empty() && !sentence.contains('\n'), "one sentence: {sentence:?}");
}

/// A server serving `index` at `/index.json` and `package` at the index's path.
fn serving(index: Vec<u8>, path: String, package: Vec<u8>) -> impl Fn(&str) -> Reply + Send + Sync + 'static {
    move |asked| {
        if asked == "/index.json" {
            ok(index.clone())
        } else if asked == path {
            ok(package.clone())
        } else {
            Reply::Fixed(404, Vec::new(), b"not here".to_vec())
        }
    }
}

/// What a late server serves: its index, its package's path and its package, once they are set.
type Served = Arc<Mutex<Option<(Vec<u8>, String, Vec<u8>)>>>;

/// A server whose index and package are set once it has started, since they name its address.
fn late_server() -> (Server, Served) {
    let served: Served = Arc::new(Mutex::new(None));
    let held = served.clone();
    let server = Server::start(move |asked| {
        let Some((index, path, package)) = held.lock().unwrap().clone() else { return Reply::Silent };
        serving(index, path, package)(asked)
    });
    (server, served)
}

fn path_of(server: &Server, url: &str) -> String {
    url.strip_prefix(&server.base).unwrap().to_string()
}

#[test]
fn a_good_index_and_package_install() {
    test_key();
    let app = tempfile::tempdir().unwrap();
    let package = package("0.2.0");
    let (server, served) = late_server();
    let index = index_of(&server, &package, "0.2.0");
    *served.lock().unwrap() = Some((json(&index), path_of(&server, &index.url), package.clone()));
    // The index is reached by a redirect, as GitHub sends a release file on to its storage.
    let hops = Server::start({
        let to = server.url("/index.json");
        move |_| redirect(&to)
    });
    fetch::set_source_for_test(hops.source("/latest/index.json"));
    let source = fetch::Source { index_url: hops.url("/latest/index.json"), package_prefix: server.url("/download/") };
    fetch::set_source_for_test(server.source("/index.json"));

    let checked = fetch::check(&source, &no_cancel()).unwrap();
    assert_eq!(checked, index);
    let seen = Mutex::new(Vec::new());
    let bytes = fetch::download(&source, &checked, &no_cancel(), &|done, total| seen.lock().unwrap().push((done, total))).unwrap();
    assert_eq!(bytes, package);
    let seen = seen.into_inner().unwrap();
    assert_eq!(seen.last(), Some(&(package.len() as u64, package.len() as u64)), "{seen:?}");

    let installed = addons::install_from_bytes(app.path(), &bytes).unwrap();
    assert_eq!(installed.version, "0.2.0");
    assert!(matches!(installed.state, InstalledState::Ready(_)));
    assert_eq!(addons::status(app.path()), addons::Status::Installed("0.2.0".into()));

    // Nothing personal was sent: the host, who asks, and what it takes; no query string.
    for head in server.heads.lock().unwrap().iter().chain(hops.heads.lock().unwrap().iter()) {
        assert!(!head[0].contains('?'), "{head:?}");
        let mut names: Vec<String> = head[1..].iter().map(|line| line.split(':').next().unwrap().to_ascii_lowercase()).collect();
        names.sort();
        assert_eq!(names, ["accept", "host", "user-agent"], "{head:?}");
        let agent = head.iter().find(|line| line.to_ascii_lowercase().starts_with("user-agent:")).unwrap();
        assert_eq!(agent.split_once(':').unwrap().1.trim(), format!("stagepack-studio/{}", env!("CARGO_PKG_VERSION")));
    }
    assert_eq!(server.hits.load(Ordering::SeqCst), 2);
}

#[test]
fn an_index_over_64_kib_is_refused() {
    let package = package("0.2.0");
    let (server, served) = late_server();
    let index = index_of(&server, &package, "0.2.0");
    let source = server.allowed("/index.json");
    let padded = |len: usize| {
        let mut bytes = json(&index);
        bytes.resize(len, b' ');
        bytes
    };

    // 64 KiB exactly is taken.
    *served.lock().unwrap() = Some((padded(64 * 1024), path_of(&server, &index.url), package.clone()));
    assert_eq!(fetch::check(&source, &no_cancel()).unwrap(), index);

    // A byte more isn't.
    *served.lock().unwrap() = Some((padded(64 * 1024 + 1), path_of(&server, &index.url), package.clone()));
    let refused = fetch::check(&source, &no_cancel()).unwrap_err();
    one_sentence(&refused);
    assert!(refused.contains("64 KiB"), "{refused}");
}

#[test]
fn an_endless_package_is_cut_at_48_mib() {
    let server = Server::start(|asked| if asked.starts_with("/download/") { Reply::Endless } else { Reply::Silent });
    let source = server.allowed("/index.json");
    let mut index = index_of(&server, b"", "0.2.0");
    index.size = 48 * 1024 * 1024;
    let started = Instant::now();
    let refused = fetch::download(&source, &index, &no_cancel(), &|_, _| {}).unwrap_err();
    one_sentence(&refused);
    assert!(refused.contains("48 MiB"), "{refused}");
    assert!(started.elapsed() < Duration::from_secs(60));

    // With a smaller size in its index, it is cut just past that size.
    index.size = 1000;
    let refused = fetch::download(&source, &index, &no_cancel(), &|done, _| assert!(done <= 1000, "{done}")).unwrap_err();
    one_sentence(&refused);
    assert!(refused.contains("1000 bytes"), "{refused}");

    // An index that says the package is larger is refused before anything is fetched.
    let hits = server.hits.load(Ordering::SeqCst);
    index.size = 48 * 1024 * 1024 + 1;
    let refused = fetch::download(&source, &index, &no_cancel(), &|_, _| {}).unwrap_err();
    one_sentence(&refused);
    assert_eq!(server.hits.load(Ordering::SeqCst), hits);
}

#[test]
fn a_url_outside_the_prefix_is_refused_before_connecting() {
    let package = package("0.2.0");
    let (server, served) = late_server();
    let source = server.allowed("/index.json");
    let elsewhere = Server::start(|_| ok(b"nothing".to_vec()));
    fetch::set_source_for_test(elsewhere.source("/index.json"));

    for url in [
        elsewhere.url("/download/slidev-v0.2.0/slidev-0.2.0.spaddon"),
        server.url("/other/slidev-0.2.0.spaddon"),
        server.url("/download/../other/slidev-0.2.0.spaddon"),
        server.url("/download/slidev-0.2.0.spaddon?from=index"),
        server.url("/download/%2e%2e/other/slidev-0.2.0.spaddon"),
        server.url("/download/a@b/slidev-0.2.0.spaddon"),
        server.url("/DOWNLOAD/slidev-0.2.0.spaddon"),
        server.url("/download/slidev-v0.2.0//slidev-0.2.0.spaddon"),
        server.url("/download/slidev-v0.2.0/"),
        server.url("/download/a\\b/slidev-0.2.0.spaddon"),
        "https://example.invalid/slidev-0.2.0.spaddon".to_string(),
    ] {
        let mut index = index_of(&server, &package, "0.2.0");
        index.url = url.clone();
        *served.lock().unwrap() = Some((json(&index), "/never".into(), Vec::new()));
        let hits = server.hits.load(Ordering::SeqCst);
        let refused = fetch::check(&source, &no_cancel()).unwrap_err();
        one_sentence(&refused);
        assert_eq!(server.hits.load(Ordering::SeqCst), hits + 1, "only the index was asked for: {url}");

        let refused = fetch::download(&source, &index, &no_cancel(), &|_, _| {}).unwrap_err();
        one_sentence(&refused);
        assert_eq!(server.hits.load(Ordering::SeqCst), hits + 1, "{url}");
        assert_eq!(elsewhere.hits.load(Ordering::SeqCst), 0, "{url}");
    }
}

#[test]
fn a_redirect_to_http_is_refused() {
    // A server no hook allowed, which must never be reached.
    let unallowed = Server::start(|_| ok(b"{}".to_vec()));
    let targets = ["http://example.invalid/index.json".to_string(), "file:///etc/passwd".to_string(), unallowed.url("/index.json")];
    for to in targets {
        let server = Server::start({
            let to = to.clone();
            move |_| redirect(&to)
        });
        let source = server.allowed("/index.json");
        let refused = fetch::check(&source, &no_cancel()).unwrap_err();
        one_sentence(&refused);
        assert!(refused.contains("https"), "{to}: {refused}");
    }
    assert_eq!(unallowed.hits.load(Ordering::SeqCst), 0);

    // Nor is an https one on this computer or a private network, whichever way it is written.
    for to in [
        "https://127.0.0.1/index.json",
        "https://localhost/index.json",
        "https://LocalHost./index.json",
        "https://app.localhost:8443/index.json",
        "https://192.168.1.1/index.json",
        "https://10.0.0.1/index.json",
        "https://169.254.169.254/latest",
        "https://0.0.0.0/index.json",
        "https://[::1]/index.json",
        "https://[fd00::1]/index.json",
        "https://[::ffff:127.0.0.1]/index.json",
        "https://2130706433/index.json",
        "https://0x7f.1/index.json",
        "https://127.1/index.json",
        "https://100.64.0.1/index.json",
        "https://198.18.0.1/index.json",
        "https://224.0.0.1/index.json",
        "https://240.0.0.1/index.json",
        "https://[::127.0.0.1]/index.json",
        "https://[64:ff9b::7f00:1]/index.json",
        "https://[2002:c0a8:101::1]/index.json",
        "https://[fec0::1]/index.json",
        "https://[ff02::1]/index.json",
    ] {
        let server = Server::start(move |_| redirect(to));
        let source = server.allowed("/index.json");
        let refused = fetch::check(&source, &no_cancel()).unwrap_err();
        // Refused by the check of local and private addresses, and by no other.
        assert_eq!(refused, "The add-on's download page sent the app on to a local or private address, so it stopped there.", "{to}");
    }

    // A user name before the host is refused with a sentence of its own.
    let server = Server::start(move |_| redirect("https://user@example.invalid/index.json"));
    let refused = fetch::check(&server.allowed("/index.json"), &no_cancel()).unwrap_err();
    assert_eq!(refused, "The add-on's download page sent the app on to an address with a user name in it, so it stopped there.");

    // Five redirects are followed, a sixth isn't.
    let package = package("0.2.0");
    let (server, served) = late_server();
    let index = index_of(&server, &package, "0.2.0");
    *served.lock().unwrap() = Some((json(&index), path_of(&server, &index.url), package));
    let base: Arc<std::sync::OnceLock<String>> = Arc::new(std::sync::OnceLock::new());
    let hops = Server::start({
        let end = server.url("/index.json");
        let base = base.clone();
        // /hop/N redirects N more times among the hops, then once to the index: N + 1 in all.
        move |asked| {
            let left: usize = asked.trim_start_matches("/hop/").parse().unwrap();
            if left == 0 { redirect(&end) } else { redirect(&format!("{}/hop/{}", base.get().unwrap(), left - 1)) }
        }
    });
    base.set(hops.base.clone()).unwrap();
    fetch::set_source_for_test(hops.source("/hop/0"));
    fetch::set_source_for_test(server.source("/index.json"));
    let hops_source = |left: usize| fetch::Source { index_url: hops.url(&format!("/hop/{left}")), package_prefix: server.url("/download/") };
    assert_eq!(fetch::check(&hops_source(4), &no_cancel()).unwrap(), index);
    let refused = fetch::check(&hops_source(5), &no_cancel()).unwrap_err();
    one_sentence(&refused);
    assert!(refused.contains("redirect"), "{refused}");
}

#[test]
fn a_wrong_sha256_or_size_is_refused() {
    test_key();
    let package = package("0.2.0");
    let (server, served) = late_server();
    let index = index_of(&server, &package, "0.2.0");
    *served.lock().unwrap() = Some((json(&index), path_of(&server, &index.url), package.clone()));
    let source = server.allowed("/index.json");

    let mut wrong = index.clone();
    wrong.sha256 = sha256_hex(b"something else");
    let refused = fetch::download(&source, &wrong, &no_cancel(), &|_, _| {}).unwrap_err();
    one_sentence(&refused);
    assert!(refused.contains("sha256"), "{refused}");

    for size in [index.size - 1, index.size + 1] {
        let mut wrong = index.clone();
        wrong.size = size;
        let refused = fetch::download(&source, &wrong, &no_cancel(), &|_, _| {}).unwrap_err();
        one_sentence(&refused);
        assert!(refused.contains("size") || refused.contains("bytes"), "{refused}");
    }

    // An index whose sha256 isn't one, or whose size is past the cap, isn't taken at all.
    for (sha256, size) in [("ABC".to_string(), index.size), (index.sha256.to_uppercase(), index.size), (index.sha256.clone(), 0), (index.sha256.clone(), 48 * 1024 * 1024 + 1)] {
        let mut wrong = index.clone();
        wrong.sha256 = sha256;
        wrong.size = size;
        *served.lock().unwrap() = Some((json(&wrong), path_of(&server, &index.url), package.clone()));
        one_sentence(&fetch::check(&source, &no_cancel()).unwrap_err());
    }
}

#[test]
fn an_index_that_disagrees_with_the_signed_manifest_is_refused() {
    test_key();
    let app = tempfile::tempdir().unwrap();
    let package = package("0.2.0");
    let (server, served) = late_server();
    let source = server.allowed("/index.json");

    // It names 0.3.0, with the right size and sha256 of a package signed as 0.2.0.
    let mut index = index_of(&server, &package, "0.3.0");
    *served.lock().unwrap() = Some((json(&index), path_of(&server, &index.url), package.clone()));
    let checked = fetch::check(&source, &no_cancel()).unwrap();
    let refused = fetch::download(&source, &checked, &no_cancel(), &|_, _| {}).unwrap_err();
    one_sentence(&refused);
    assert!(refused.contains("0.3.0") && refused.contains("0.2.0"), "{refused}");

    // One signed with another key is refused by its signature.
    let forged = package_signed(&answering_guest_at(&export_answer(), "0.3.0"), "0.3.0", &other_key());
    index = index_of(&server, &forged, "0.3.0");
    *served.lock().unwrap() = Some((json(&index), path_of(&server, &index.url), forged));
    let refused = fetch::download(&source, &index, &no_cancel(), &|_, _| {}).unwrap_err();
    assert!(refused.contains("signature"), "{refused}");

    // An index for another name, interface or Slidev, or with a field more or less, isn't taken.
    let good = serde_json::to_value(index_of(&server, &package, "0.2.0")).unwrap();
    let mut wrongs = Vec::new();
    for (field, value) in [("name", serde_json::json!("other")), ("abi", serde_json::json!(2)), ("slidev", serde_json::json!("52.0.0")), ("version", serde_json::json!("0.2"))] {
        let mut wrong = good.clone();
        wrong[field] = value;
        wrongs.push(wrong);
    }
    let mut extra = good.clone();
    extra["telemetry"] = serde_json::json!(true);
    wrongs.push(extra);
    let mut missing = good.clone();
    missing.as_object_mut().unwrap().remove("sha256");
    wrongs.push(missing);
    for wrong in wrongs {
        *served.lock().unwrap() = Some((serde_json::to_vec(&wrong).unwrap(), "/never".into(), Vec::new()));
        one_sentence(&fetch::check(&source, &no_cancel()).unwrap_err());
    }
    assert_eq!(addons::status(app.path()), addons::Status::NotInstalled);
}

#[test]
fn an_unreadable_index_says_nothing_the_server_wrote() {
    let phish = "Your copy is out of date, get it at evil.example";
    let package = package("0.2.0");
    let (server, served) = late_server();
    let source = server.allowed("/index.json");
    let good = serde_json::to_value(index_of(&server, &package, "0.2.0")).unwrap();
    let mut wrongs = Vec::new();
    let mut unknown = good.clone();
    unknown[phish] = serde_json::json!(1);
    wrongs.push(serde_json::to_vec(&unknown).unwrap());
    for field in ["name", "version", "slidev", "sha256", "url"] {
        let mut wrong = good.clone();
        wrong[field] = serde_json::json!(phish);
        wrongs.push(serde_json::to_vec(&wrong).unwrap());
    }
    let mut wrong_type = good.clone();
    wrong_type["size"] = serde_json::json!(phish);
    wrongs.push(serde_json::to_vec(&wrong_type).unwrap());
    wrongs.push(format!("{{\"{phish}\"").into_bytes());
    wrongs.push(format!("<html>{phish}</html>").into_bytes());
    for wrong in wrongs {
        *served.lock().unwrap() = Some((wrong.clone(), "/never".into(), Vec::new()));
        let refused = fetch::check(&source, &no_cancel()).unwrap_err();
        one_sentence(&refused);
        for words in ["evil", "out of date", "Your copy"] {
            assert!(!refused.contains(words), "{refused} from {}", String::from_utf8_lossy(&wrong));
        }
    }
    let mut unknown = good.clone();
    unknown[phish] = serde_json::json!(1);
    *served.lock().unwrap() = Some((serde_json::to_vec(&unknown).unwrap(), "/never".into(), Vec::new()));
    let refused = fetch::check(&source, &no_cancel()).unwrap_err();
    assert!(refused.starts_with("The add-on's index can't be read (line 1, column "), "{refused}");
}

#[test]
fn html_instead_of_an_index_says_so() {
    let server = Server::start(|_| {
        Reply::Fixed(200, vec![("Content-Type".into(), "text/html".into())], b"<!doctype html><html><body>Sign in to the network</body></html>".to_vec())
    });
    let source = server.allowed("/index.json");
    let refused = fetch::check(&source, &no_cancel()).unwrap_err();
    one_sentence(&refused);
    assert!(refused.contains("isn't the add-on's index"), "{refused}");
    assert!(refused.contains("Check the internet connection"), "{refused}");

    // A page that isn't there says so too.
    let missing = Server::start(|_| Reply::Fixed(404, Vec::new(), b"Not Found".to_vec()));
    let refused = fetch::check(&missing.allowed("/index.json"), &no_cancel()).unwrap_err();
    one_sentence(&refused);
    assert!(refused.contains("404"), "{refused}");
}

#[test]
fn a_refused_connection_says_check_the_internet() {
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let source = fetch::Source { index_url: format!("http://127.0.0.1:{port}/index.json"), package_prefix: format!("http://127.0.0.1:{port}/download/") };
    fetch::set_source_for_test(source.clone());
    assert_eq!(fetch::check(&source, &no_cancel()).unwrap_err(), fetch::UNREACHABLE);
    assert_eq!(
        fetch::UNREACHABLE,
        "Couldn't reach the add-on's download page. Check the internet connection, or install it from a file."
    );
}

#[test]
fn cancel_stops_within_a_second() {
    // While the index is awaited.
    let silent = Server::start(|_| Reply::Silent);
    let source = silent.allowed("/index.json");
    let cancel = AtomicBool::new(false);
    let (refused, late) = cancelled_after(&cancel, Duration::from_millis(300), || fetch::check(&source, &cancel).map(|_| ()));
    one_sentence(&refused);
    assert!(late < Duration::from_secs(1), "{late:?}");

    // And part way through the package.
    let stalling = Server::start(|asked| if asked.starts_with("/download/") { Reply::Stall(10_000_000, 100_000) } else { Reply::Silent });
    let source = stalling.allowed("/index.json");
    let mut index = index_of(&stalling, b"", "0.2.0");
    index.size = 10_000_000;
    let cancel = AtomicBool::new(false);
    let progressed = AtomicBool::new(false);
    let (refused, late) = cancelled_after(&cancel, Duration::from_millis(500), || {
        fetch::download(&source, &index, &cancel, &|done, _| progressed.store(done > 0, Ordering::SeqCst)).map(|_| ())
    });
    one_sentence(&refused);
    assert!(late < Duration::from_secs(1), "{late:?}");
    assert!(progressed.load(Ordering::SeqCst), "the bytes sent were reported");

    // Asked to cancel before it starts, it connects to nothing.
    let cancel = AtomicBool::new(true);
    let hits = stalling.hits.load(Ordering::SeqCst);
    assert!(fetch::download(&source, &index, &cancel, &|_, _| {}).is_err());
    assert_eq!(stalling.hits.load(Ordering::SeqCst), hits);
}

/// What `run` failed with, once `cancel` was set `after` its start, and how long after the
/// cancel it came back.
fn cancelled_after(cancel: &AtomicBool, after: Duration, run: impl FnOnce() -> Result<(), String>) -> (String, Duration) {
    std::thread::scope(|scope| {
        let cancelled_at = scope.spawn(move || {
            std::thread::sleep(after);
            cancel.store(true, Ordering::SeqCst);
            Instant::now()
        });
        let result = run();
        let ended = Instant::now();
        let cancelled_at = cancelled_at.join().unwrap();
        (result.unwrap_err(), ended.saturating_duration_since(cancelled_at))
    })
}

#[test]
fn newer_compares_numbers() {
    assert!(fetch::newer("0.1.9", "0.1.10"));
    assert!(fetch::newer("0.1.0", "0.2.0"));
    assert!(fetch::newer("0.9.9", "1.0.0"));
    assert!(!fetch::newer("0.1.10", "0.1.9"));
    assert!(!fetch::newer("0.2.0", "0.2.0"));
    assert!(!fetch::newer("1.0.0", "0.9.9"));
    for malformed in ["0.2", "0.2.0.1", "v0.2.0", "0.2.x", "", "0.2.0-beta", "+1.0.0", " 1.0.0", "0..1", "01.2.3"] {
        // One on offer that isn't three numbers is never later.
        assert!(!fetch::newer("0.1.0", malformed), "{malformed:?}");
        assert!(!fetch::newer(malformed, malformed), "{malformed:?}");
        // One installed that isn't, as from a file, is older than any that is, so that it can
        // always be updated.
        assert!(fetch::newer(malformed, "9.9.9"), "{malformed:?}");
        assert!(fetch::newer(malformed, "0.0.0"), "{malformed:?}");
    }
}

#[test]
fn http_is_refused_without_the_test_hook() {
    // A server no hook allowed.
    let server = Server::start(|_| ok(b"{}".to_vec()));
    let source = server.source("/index.json");
    let refused = fetch::check(&source, &no_cancel()).unwrap_err();
    one_sentence(&refused);
    assert!(refused.contains("https"), "{refused}");
    let index = index_of(&server, b"x", "0.2.0");
    one_sentence(&fetch::download(&source, &index, &no_cancel(), &|_, _| {}).unwrap_err());
    assert_eq!(server.hits.load(Ordering::SeqCst), 0);

    // Nor is plain http anywhere else, nor an address the official one isn't.
    let official = fetch::Source::official();
    assert_eq!(official.index_url, *fetch::INDEX_URL);
    assert_eq!(official.package_prefix, fetch::PACKAGE_PREFIX);
    // The index of the app's own line, add-on interface 1 and Slidev 53, which never moves.
    assert_eq!(*fetch::INDEX_URL, "https://github.com/Eiriksb/stagepack-studio-addons/releases/download/index/index-abi1-slidev53.json");
    assert_eq!(*fetch::INDEX_URL, addon_abi::package::index_url(addon_abi::ABI_VERSION, addons::SLIDEV_VERSION));
    // It is reached as a package is, by a release file's address that GitHub sends on to its
    // storage, so it passes the same checks.
    assert!(fetch::INDEX_URL.starts_with(fetch::PACKAGE_PREFIX));
    assert_eq!(fetch::PACKAGE_PREFIX, "https://github.com/Eiriksb/stagepack-studio-addons/releases/download/");
    for url in ["http://example.invalid/index.json", "http://localhost:1/index.json", "ftp://example.invalid/index.json"] {
        let source = fetch::Source { index_url: url.into(), package_prefix: "http://example.invalid/".into() };
        one_sentence(&fetch::check(&source, &no_cancel()).unwrap_err());
    }
}
