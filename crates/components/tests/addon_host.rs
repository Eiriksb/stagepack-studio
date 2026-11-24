//! Running an add-on: each test guest is the template in `addon-guests/guest.wat` with a
//! part of its own, and the host holds every one of them to the add-on limits.

use std::time::{Duration, Instant};

use addon_abi::{encode, Bundle, NoticeKind, Report, REPORT};
use slides_components::addon::{Addon, AddonError, AddonInfo, AddonLimits, MAX_ADDON_BYTES};

const TEMPLATE: &str = include_str!("addon-guests/guest.wat");
const ECHO: &str = include_str!("addon-guests/echo.wat");

/// The parts of a test guest. Whatever is left out is filled in with a working default.
#[derive(Default)]
struct Guest<'a> {
    abi: Option<u32>,
    imports: &'a str,
    extra: &'a str,
    /// The body of `run(ptr, len) -> i64`, the echo guest's if left out.
    run: Option<&'a str>,
}

fn guest(parts: Guest) -> Vec<u8> {
    let Guest { abi, imports, extra, run } = parts;
    let echo_report = data(2048, &report_entry());
    let extra = if run.is_none() { format!("{extra}\n{echo_report}") } else { extra.to_string() };
    let text = TEMPLATE
        .replace("{abi}", &abi.unwrap_or(1).to_string())
        .replace("{imports}", imports)
        .replace("{extra}", &extra)
        .replace("{run}", run.unwrap_or(ECHO));
    wat::parse_str(&text).unwrap_or_else(|error| panic!("{error}\n{text}"))
}

/// The `.report.json` entry the echo guest adds: its name and data, each after its length.
fn report_entry() -> Vec<u8> {
    let entry = encode(&vec![(REPORT.to_string(), br#"{"notices":[]}"#.to_vec())]);
    assert_eq!(entry.len() - 8, 34, "the echo guest copies 34 bytes");
    entry[8..].to_vec()
}

/// A data segment holding `bytes` at `at`.
fn data(at: u32, bytes: &[u8]) -> String {
    let escaped: String = bytes.iter().map(|byte| format!("\\{byte:02x}")).collect();
    format!(r#"(data (i32.const {at}) "{escaped}")"#)
}

/// The parts of a guest whose `run` gives back `bundle`, whatever it is given.
fn answering(bundle: &Bundle) -> (String, String) {
    let bytes = encode(bundle);
    (data(4096, &bytes), format!("(i64.or (i64.shl (i64.const 4096) (i64.const 32)) (i64.const {}))", bytes.len()))
}

fn load_answering(bundle: &Bundle) -> Addon {
    let (extra, run) = answering(bundle);
    Addon::load(&guest(Guest { extra: &extra, run: Some(&run), ..Guest::default() })).expect("the guest loads")
}

fn file(name: &str, data: &[u8]) -> (String, Vec<u8>) {
    (name.to_string(), data.to_vec())
}

fn files() -> Bundle {
    vec![file("slides.md", b"# One\n\n---\n\n# Two\n"), file("public/empty.png", b""), file("public/smörgås.png", &[1, 2, 3])]
}

#[track_caller]
fn malformed(result: Result<impl std::fmt::Debug, AddonError>, naming: &str) {
    match result {
        Err(AddonError::Malformed(message)) => assert!(message.contains(naming), "{message:?} doesn't name {naming:?}"),
        other => panic!("expected Malformed naming {naming:?}, got {other:?}"),
    }
}

#[test]
fn a_good_guest_echoes_its_files() {
    let addon = Addon::load(&guest(Guest::default())).expect("the echo guest loads");
    assert_eq!(
        addon.info(),
        &AddonInfo { name: "test".into(), version: "1.0.0".into(), operations: vec!["export".into(), "import".into()] }
    );
    let (out, report) = addon.run("export", "{}", files()).expect("the echo guest runs");
    assert_eq!(out, files());
    assert_eq!(report, Report { notices: vec![] });
    // Each run starts afresh: the second sees nothing of the first.
    assert_eq!(addon.run("import", r#"{"a":1}"#, files()).unwrap().0, files());
    assert_eq!(addon.run("export", "{}", vec![]).unwrap().0, vec![]);
}

#[test]
fn a_report_comes_back_with_its_notices() {
    let report = br#"{"notices":[{"slide":2,"kind":"placeholder","text":"A Vue component became a placeholder."}]}"#;
    let addon = load_answering(&vec![file("slides.md", b"# A"), file(REPORT, report)]);
    let (out, report) = addon.run("import", "{}", vec![]).unwrap();
    assert_eq!(out, vec![file("slides.md", b"# A")]);
    assert_eq!(report.notices.len(), 1);
    assert_eq!((report.notices[0].slide, report.notices[0].kind), (Some(2), NoticeKind::Placeholder));
}

#[test]
fn a_guest_with_an_import_is_refused() {
    let imports = r#"(import "env" "now" (func (result i64)))"#;
    malformed(Addon::load(&guest(Guest { imports, ..Guest::default() })), "env.now");
    let imports = r#"(import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32)))"#;
    malformed(Addon::load(&guest(Guest { imports, ..Guest::default() })), "fd_write");
}

#[test]
fn a_wrong_abi_is_refused() {
    malformed(Addon::load(&guest(Guest { abi: Some(2), ..Guest::default() })), "interface 2");
    malformed(Addon::load(&guest(Guest { abi: Some(0), ..Guest::default() })), "interface 0");
}

#[test]
fn a_module_missing_an_export_is_refused() {
    let text = r#"(module (memory (export "memory") 1) (func (export "addon_abi") (result i32) (i32.const 1)))"#;
    malformed(Addon::load(&wat::parse_str(text).unwrap()), "alloc");
    malformed(Addon::load(b"not wasm"), "");
    let wrong_type = r#"(module (memory (export "memory") 1)
      (func (export "addon_abi") (result i32) (i32.const 1))
      (func (export "alloc") (param i32) (result i32) (i32.const 0))
      (func (export "info") (result i32) (i32.const 0))
      (func (export "run") (param i32 i32) (result i64) (i64.const 0)))"#;
    malformed(Addon::load(&wat::parse_str(wrong_type).unwrap()), "info");
}

#[test]
fn a_bad_info_is_refused() {
    let wrong = |json: &str| {
        let extra = data(8192, json.as_bytes());
        let text = TEMPLATE
            .replace("(i64.const 1024) (i64.const 32)) (i64.const 66)", &format!("(i64.const 8192) (i64.const 32)) (i64.const {})", json.len()))
            .replace("{abi}", "1")
            .replace("{imports}", "")
            .replace("{extra}", &extra)
            .replace("{run}", "(i64.const 0)");
        Addon::load(&wat::parse_str(&text).unwrap())
    };
    assert!(wrong(r#"{"name":"test","version":"1.0.0","operations":["export"]}"#).is_ok());
    malformed(wrong("not json"), "info");
    malformed(wrong(r#"{"name":"Not A Name","version":"1.0.0","operations":[]}"#), "name");
    malformed(wrong(r#"{"name":"test","version":"","operations":[]}"#), "version");
    malformed(wrong(r#"{"name":"test","version":"1.0.0","operations":["ex port"]}"#), "operation");
}

#[test]
fn a_guest_that_loops_is_stopped() {
    let addon = Addon::load(&guest(Guest { run: Some("(loop $forever (br $forever)) (i64.const 0)"), ..Guest::default() })).unwrap();
    let limits = AddonLimits { time: Duration::from_millis(200), ..AddonLimits::default() };
    let started = Instant::now();
    assert_eq!(addon.run_with_limits("export", "{}", files(), &limits), Err(AddonError::TookTooLong));
    assert!(started.elapsed() < Duration::from_secs(5), "stopped after {:?}", started.elapsed());
}

#[test]
fn a_guest_that_loops_while_loading_is_stopped() {
    let extra = "(func $start (loop $forever (br $forever))) (start $start)";
    let limits = AddonLimits { time: Duration::from_millis(200), ..AddonLimits::default() };
    let started = Instant::now();
    assert_eq!(Addon::load_with_limits(&guest(Guest { extra, ..Guest::default() }), &limits).err(), Some(AddonError::TookTooLong));
    assert!(started.elapsed() < Duration::from_secs(5), "stopped after {:?}", started.elapsed());
}

#[test]
fn a_guest_that_grows_memory_is_stopped() {
    // 20 000 pages is 1.25 GiB, more than an add-on may have.
    let run = "(if (i32.eq (memory.grow (i32.const 20000)) (i32.const -1)) (then unreachable)) (i64.const 0)";
    let addon = Addon::load(&guest(Guest { run: Some(run), ..Guest::default() })).unwrap();
    assert_eq!(addon.run("export", "{}", files()), Err(AddonError::OutOfMemory));
}

#[test]
fn a_guest_may_use_more_memory_than_a_component() {
    // 2 000 pages is 125 MiB: over a component's 64 MiB, well under an add-on's 1 GiB.
    let extra = "(func $grow (if (i32.eq (memory.grow (i32.const 2000)) (i32.const -1)) (then unreachable))) (start $grow)";
    let addon = Addon::load(&guest(Guest { extra, ..Guest::default() })).unwrap();
    assert_eq!(addon.run("export", "{}", files()).unwrap().0, files());
}

#[test]
fn a_guest_that_traps_has_stopped_working() {
    let addon = Addon::load(&guest(Guest { run: Some("unreachable"), ..Guest::default() })).unwrap();
    assert_eq!(addon.run("export", "{}", files()), Err(AddonError::Trapped));
}

#[test]
fn an_error_file_is_a_failure() {
    let addon = load_answering(&vec![file(".error", "The talk has no slides.".as_bytes())]);
    assert_eq!(addon.run("export", "{}", files()), Err(AddonError::Failed("The talk has no slides.".into())));
    let error = AddonError::Failed("The talk has no slides.".into());
    assert_eq!(error.to_string(), "The add-on couldn't finish: The talk has no slides.");
}

#[test]
fn an_error_is_kept_to_one_short_line() {
    let long = format!("Bad\nthing\u{7}{}", "x".repeat(2000));
    let addon = load_answering(&vec![file(".error", long.as_bytes())]);
    let Err(AddonError::Failed(sentence)) = addon.run("export", "{}", vec![]) else { panic!("expected a failure") };
    assert!(sentence.starts_with("Bad thing"), "{sentence:?}");
    assert!(!sentence.chars().any(char::is_control), "{sentence:?}");
    assert!(sentence.chars().count() <= 500, "{} characters", sentence.chars().count());
}

#[test]
fn an_unsafe_name_in_the_output_is_refused() {
    let report = file(REPORT, br#"{"notices":[]}"#);
    for name in ["../evil.md", "/etc/passwd", "C:x", ".git/config"] {
        malformed(load_answering(&vec![file(name, b"x"), report.clone()]).run("export", "{}", vec![]), name);
    }
    let error = load_answering(&vec![file("../evil.md", b"x"), report.clone()]).run("export", "{}", vec![]).unwrap_err();
    assert_eq!(
        error.to_string(),
        r#"The add-on can't be used: what it gave back can't be read: the bundle has a file named "../evil.md", which isn't a safe name."#
    );
    // Only the app gives an add-on its operation.
    malformed(load_answering(&vec![file(".op", b"x"), report.clone()]).run("export", "{}", vec![]), ".op");
}

#[test]
fn an_output_without_a_report_is_refused() {
    malformed(load_answering(&vec![file("slides.md", b"# A")]).run("export", "{}", vec![]), "report");
    let bad_report = file(REPORT, b"{}");
    malformed(load_answering(&vec![bad_report]).run("export", "{}", vec![]), "report");
}

#[test]
fn an_answer_outside_the_memory_is_refused() {
    let addon = Addon::load(&guest(Guest { run: Some("(i64.const 0x7fff000000001000)"), ..Guest::default() })).unwrap();
    malformed(addon.run("export", "{}", vec![]), "memory");
}

#[test]
fn a_too_large_output_is_refused() {
    // The length alone is over the limit, whatever the pointer.
    let addon = Addon::load(&guest(Guest { run: Some("(i64.const 0x0000000020000001)"), ..Guest::default() })).unwrap();
    assert!(matches!(addon.run("export", "{}", vec![]), Err(AddonError::TooLarge(_))));
}

#[test]
fn a_too_large_input_is_refused_before_running() {
    // A guest that would loop if it ran, so a refusal that comes fast came before it.
    let addon = Addon::load(&guest(Guest { run: Some("(loop $forever (br $forever)) (i64.const 0)"), ..Guest::default() })).unwrap();
    let started = Instant::now();
    // Zeroed and never touched, so this costs no memory.
    let huge = vec![file("public/video.mp4", &[]), ("public/huge.bin".to_string(), vec![0u8; 512 * 1024 * 1024])];
    let Err(AddonError::TooLarge(sentence)) = addon.run("export", "{}", huge) else { panic!("expected TooLarge") };
    assert!(sentence.contains("512 MiB"), "{sentence}");

    let many: Bundle = (0..10_000).map(|n| (format!("public/{n}.png"), vec![])).collect();
    let Err(AddonError::TooLarge(sentence)) = addon.run("export", "{}", many) else { panic!("expected TooLarge") };
    assert!(sentence.contains("10 000"), "{sentence}");
    assert!(started.elapsed() < Duration::from_secs(5), "refused after {:?}", started.elapsed());
}

#[test]
fn an_unsafe_name_in_the_input_is_refused_before_running() {
    let addon = Addon::load(&guest(Guest { run: Some("(loop $forever (br $forever)) (i64.const 0)"), ..Guest::default() })).unwrap();
    malformed(addon.run("export", "{}", vec![file(".gitignore", b"")]), ".gitignore");
    malformed(addon.run("export", "{}", vec![file(".op", b"")]), ".op");
}

#[test]
fn a_too_large_addon_is_refused() {
    let Err(AddonError::TooLarge(sentence)) = Addon::load(&vec![0; MAX_ADDON_BYTES + 1]) else { panic!("expected TooLarge") };
    assert!(sentence.contains("MiB"), "{sentence}");
}

#[test]
fn every_error_is_one_sentence() {
    let errors = [
        AddonError::Malformed("it has no `run` function".into()),
        AddonError::TooLarge("The files for the add-on come to more than 512 MiB."),
        AddonError::TookTooLong,
        AddonError::OutOfMemory,
        AddonError::Failed("The talk has no slides.".into()),
        AddonError::Trapped,
    ];
    for error in errors {
        let text = error.to_string();
        assert!(text.ends_with('.') && text.matches(". ").count() == 0, "{text:?}");
        assert!(text.chars().next().unwrap().is_uppercase(), "{text:?}");
    }
}
