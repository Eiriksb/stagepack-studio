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
    /// Pages of memory, if not the usual 2.
    pages: Option<usize>,
}

fn guest(parts: Guest) -> Vec<u8> {
    let Guest { abi, imports, extra, run, pages } = parts;
    let echo_report = data(2048, &report_entry());
    let extra = if run.is_none() { format!("{extra}\n{echo_report}") } else { extra.to_string() };
    let text = TEMPLATE
        .replace("{abi}", &abi.unwrap_or(1).to_string())
        .replace("{imports}", imports)
        .replace("{pages}", &pages.unwrap_or(2).to_string())
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

/// Where an answering guest's answer lies: past the room `alloc` gives its input from.
const ANSWER_AT: usize = 1024 * 1024;

/// The parts of a guest whose `run` gives back `bundle`, whatever it is given, and the pages
/// of memory that holds it.
fn answering(bundle: &Bundle) -> (String, String, usize) {
    let bytes = encode(bundle);
    let run = format!("(i64.or (i64.shl (i64.const {ANSWER_AT}) (i64.const 32)) (i64.const {}))", bytes.len());
    (data(ANSWER_AT as u32, &bytes), run, (ANSWER_AT + bytes.len()) / 65536 + 1)
}

fn load_answering(bundle: &Bundle) -> Addon {
    let (extra, run, pages) = answering(bundle);
    Addon::load(&guest(Guest { extra: &extra, run: Some(&run), pages: Some(pages), ..Guest::default() })).expect("the guest loads")
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
            .replace("{pages}", "2")
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
    let limits = AddonLimits { load_time: Duration::from_millis(200), ..AddonLimits::default() };
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
fn an_error_without_a_full_stop_is_given_one() {
    let addon = load_answering(&vec![file(".error", b"The talk has no slides")]);
    let error = addon.run("export", "{}", vec![]).unwrap_err();
    assert_eq!(error.to_string(), "The add-on couldn't finish: The talk has no slides.");
    let addon = load_answering(&vec![file(".error", b"")]);
    assert_eq!(addon.run("export", "{}", vec![]).unwrap_err().to_string(), "The add-on couldn't finish: it gave no reason.");
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
    let refused = |name: &str| match addon.run("export", "{}", vec![file(name, b"")]) {
        Err(AddonError::InvalidInput(sentence)) => sentence,
        other => panic!("expected InvalidInput for {name:?}, got {other:?}"),
    };
    assert_eq!(refused(".gitignore"), r#"A file named ".gitignore" can't be given to an add-on."#);
    assert!(refused(".op").contains(".op"));
    let long = refused(&format!("a\n{}", "b".repeat(5000)));
    assert!(long.len() < 200 && !long.contains('\n'), "{long:?}");
}

#[test]
fn an_operation_the_addon_does_not_name_is_refused_before_running() {
    let addon = Addon::load(&guest(Guest { run: Some("(loop $forever (br $forever)) (i64.const 0)"), ..Guest::default() })).unwrap();
    assert_eq!(
        addon.run("delete", "{}", files()),
        Err(AddonError::InvalidInput(r#"The add-on has no operation called "delete"."#.into()))
    );
}

#[test]
fn a_guest_that_recurses_deeply_is_given_room() {
    // 200 000 calls deep: megabytes of stack, far past a component's 256 KiB, well within 64 MiB.
    let extra = "(func $down (param $n i32) (if (local.get $n) (then (call $down (i32.sub (local.get $n) (i32.const 1))))))\n(func $deep (call $down (i32.const 200000))) (start $deep)";
    let addon = Addon::load(&guest(Guest { extra, ..Guest::default() })).expect("a deep guest loads");
    assert_eq!(addon.run("export", "{}", files()).unwrap().0, files());
}

#[test]
fn a_guest_that_recurses_forever_has_stopped_working() {
    let extra = "(func $down (call $down))";
    let addon = Addon::load(&guest(Guest { extra, run: Some("(call $down) (i64.const 0)"), ..Guest::default() })).unwrap();
    let error = addon.run("export", "{}", files()).unwrap_err();
    assert_eq!(error, AddonError::Trapped);
    assert_eq!(error.to_string(), "The add-on stopped working.");
    // And the app is still here to run it again.
    assert_eq!(addon.run("export", "{}", files()), Err(AddonError::Trapped));
}

#[test]
fn a_guest_using_a_proposal_add_ons_do_without_is_refused() {
    let modules = [
        // Memory past 4 GiB.
        r#"(module (memory (export "memory") i64 1))"#,
        // Memory shared between threads.
        r#"(module (memory (export "memory") 1 1 shared))"#,
        // A second memory.
        r#"(module (memory (export "memory") 1) (memory 1))"#,
        // Exceptions.
        r#"(module (memory (export "memory") 1) (tag $oops) (func (throw $oops)))"#,
        // Garbage collection.
        r#"(module (memory (export "memory") 1) (type $pair (struct (field i32) (field i32))))"#,
        // Typed function references.
        r#"(module (memory (export "memory") 1) (type $f (func)) (func (param (ref $f))))"#,
    ];
    for text in modules {
        let wasm = wat::parse_str(text).unwrap_or_else(|error| panic!("{error}\n{text}"));
        malformed(Addon::load(&wasm), "WebAssembly this app can run");
    }
}

#[test]
fn an_import_name_is_kept_to_one_short_line() {
    let module = format!("env\n{}", "x".repeat(5000));
    let imports = format!(r#"(import "{}" "now" (func (result i64)))"#, module.replace('\n', "\\0a"));
    let Err(AddonError::Malformed(message)) = Addon::load(&guest(Guest { imports: &imports, ..Guest::default() })) else {
        panic!("expected Malformed")
    };
    assert!(message.len() < 300 && !message.contains('\n'), "{message:?}");
}

#[test]
fn a_report_is_held_to_its_limits() {
    let report = |json: String| load_answering(&vec![file(REPORT, json.as_bytes())]).run("export", "{}", vec![]);
    malformed(report(r#"{"notices":[{"slide":0,"kind":"frozen","text":"A chart."}]}"#.into()), "slide 0");
    let notice = r#"{"slide":null,"kind":"frozen","text":"A."}"#;
    let many = format!(r#"{{"notices":[{}]}}"#, vec![notice; 10_001].join(","));
    malformed(report(many), "10000 notices");
    let long = format!(r#"{{"notices":[{{"slide":1,"kind":"left_out","text":"Two\nlines {}"}}]}}"#, "x".repeat(5000));
    let (_, report) = report(long).unwrap();
    let text = &report.notices[0].text;
    assert!(text.starts_with("Two lines ") && text.chars().count() <= 500, "{} characters", text.chars().count());
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
        AddonError::InvalidInput("A file named \".git\" can't be given to an add-on.".into()),
        AddonError::Trapped,
    ];
    for error in errors {
        let text = error.to_string();
        assert!(text.ends_with('.') && text.matches(". ").count() == 0, "{text:?}");
        assert!(text.chars().next().unwrap().is_uppercase(), "{text:?}");
    }
}
