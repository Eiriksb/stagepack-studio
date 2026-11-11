use slides_components::{check_source, compile, component_tag, Bundle, SourceInfo};

const FILE: &str = "components/a.rs";

/// `body` as the inside of a valid component.
fn in_a_component(body: &str) -> String {
    format!("use toolkit::prelude::*;\n#[component]\nfn A() -> Element {{\n    {body}\n    text(\"a\")\n}}\n")
}

/// `items` above a valid component.
fn above_a_component(items: &str) -> String {
    format!("{items}\nuse toolkit::prelude::*;\n#[component]\nfn A() -> Element {{ text(\"a\") }}\n")
}

#[track_caller]
fn assert_refused(source: &str) {
    let refusals = check_source(FILE, source).expect_err(source);
    assert!(!refusals.is_empty(), "no reason given for refusing {source}");
    for refusal in &refusals {
        assert_eq!(refusal.file, FILE);
        assert!(!refusal.message.is_empty());
    }
}

#[track_caller]
fn assert_allowed(source: &str) {
    if let Err(refusals) = check_source(FILE, source) {
        panic!("{source}\nwas refused: {refusals:?}");
    }
}

#[test]
fn a_read_of_the_computer_is_refused() {
    for body in [
        r#"include!("x");"#,
        r#"let a = include_str!("x");"#,
        r#"let a = include_bytes!("x");"#,
        r#"let a = env!("HOME");"#,
        r#"let a = option_env!("HOME");"#,
    ] {
        assert_refused(&in_a_component(body));
    }
    for items in [r#"#[path = "x"] mod a;"#, "mod a;", "#![feature(x)]", "#![feature]"] {
        assert_refused(&above_a_component(items));
    }
}

#[test]
fn a_file_read_hidden_in_a_macro_is_refused() {
    for body in [
        r#"let a = std::include_str!("x");"#,
        r#"let a = core::env!("HOME");"#,
        "use std::include_bytes as b;",
        r#"let a = r#include_str!("x");"#,
        r#"macro_rules! m { ($x:ident) => { $x!("HOME") } } m!(env);"#,
        "macro_rules! m { ($a:tt $b:ident) => { $a $b; } } m!(mod foo);",
        r#"println!("{}", include_str!("x"));"#,
    ] {
        assert_refused(&in_a_component(body));
    }
    for items in [
        r#"#[cfg_attr(all(), path = "x")] mod a {}"#,
        r#"#[cfg_attr(all(), cfg_attr(all(), path = "x"))] mod a {}"#,
        r#"#[debugger_visualizer(gdb_script_file = "x")] mod a {}"#,
        r#"#[doc = include_str!("x")] mod a {}"#,
        "mod a { mod b; }",
    ] {
        assert_refused(&above_a_component(items));
    }
}

// None of these has a word the two tests above look for, in a place they look for it.
#[test]
fn a_way_around_the_scan_is_refused() {
    // rustc skips a first line starting `#!` unless `[` comes next, so the lexer here
    // would take this include for a comment, and rustc 1.98 reads the file.
    let hidden_by_a_first_line = above_a_component("#!/*\nconst X: &str = include_str!(\"x\"); // */");
    assert_refused(&hidden_by_a_first_line);
    assert_refused(&format!("\u{feff}{hidden_by_a_first_line}"));

    // Macros can rearrange what the scan saw: these expand to `#[path = "x"] mod a;` and
    // `mod a;`, and rustc 1.98 reads the file for both.
    assert_refused(&above_a_component(
        r#"macro_rules! m { ($p:ident $k:tt $n:ident { $($t:tt)* }) => { #[$p = "x"] $k $n $($t)* } } m!(path mod a { ; });"#,
    ));
    assert_refused(&above_a_component("macro_rules! m { ($k:tt $n:ident $b:tt) => { $k $n; } } m!(mod a {});"));

    // These give the linker a name of the source's choosing to go looking for.
    assert_refused(&above_a_component(
        r#"#[link(name = "/x", kind = "static", modifiers = "+verbatim")] unsafe extern "C" {}"#,
    ));
    assert_refused(&above_a_component(r#"#[cfg_attr(all(), link(name = "/x"))] unsafe extern "C" {}"#));

    // Each of these fails on rustc 1.98 only because the feature isn't stable yet there,
    // and that can change with the compiler in the bundle.
    assert_refused(&above_a_component(r#"core::arch::global_asm!(".incbin \"x\"");"#));
    assert_refused(&in_a_component(r#"unsafe { core::arch::asm!(".incbin \"x\"") };"#));
    assert_refused(&above_a_component("#![cfg_attr(all(), feature(x))]"));
    assert_refused(&format!(
        "---\n/*\n---\nconst X: &str = include_str!(\"x\");\n// */\n{}",
        above_a_component("")
    ));

    // U+0558 is part of a name for the lexer here and unknown to rustc 1.98, which drops
    // it, carries on and reads the file. It's the same for a literal's suffix.
    let unknown = '\u{558}';
    assert_refused(&in_a_component(&format!(r#"let a = include_str{unknown}!("x");"#)));
    assert_refused(&in_a_component(&format!(r#"let a = {unknown}include_str!("x");"#)));
    assert_refused(&above_a_component(&format!("mod{unknown} a;")));
    assert_refused(&in_a_component(&format!(r#"let a = 1{unknown}include_str!("x");"#)));
    assert_refused(&in_a_component(&format!(r#"let a = "b"{unknown}include_str!("x");"#)));
    assert_refused(&in_a_component(&format!(r##"let a = r#"b"#{unknown}include_str!("x");"##)));
    assert_refused(&in_a_component(&format!(r#"let a = 'b'{unknown}include_str!("x");"#)));
}

#[test]
fn a_name_can_only_use_ascii() {
    let refusals = check_source(FILE, "#[component]\nfn A() -> Element {\n    let æ = 1;\n    text(\"a\")\n}\n").unwrap_err();
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert_eq!(refusals[0].line, 3);
    assert_eq!(refusals[0].message, "Names in a component can only use ASCII letters, digits and underscores");
    // A lifetime is a name as well.
    assert_refused(&in_a_component("let a: &'\u{558}b str = \"\";"));
}

#[test]
fn text_in_any_language_is_fine_in_strings_and_comments() {
    for body in [
        "let a = \"æøå \u{558}\";",
        "// æøå \u{558}",
        "/* æøå \u{558} */",
        "/// æøå \u{558}\nlet a = 1;",
        "let a = 'æ';",
        "let a = r#\"æ\u{558}\"#;",
    ] {
        assert_allowed(&in_a_component(body));
    }
}

#[test]
fn a_nearly_identical_file_is_fine() {
    assert_allowed(&above_a_component("#![allow(unused)]"));
    assert_allowed(&format!("\u{feff}{}", above_a_component("#![allow(unused)]")));
    assert_allowed(&above_a_component(r#"#[cfg(feature = "x")] const X: u32 = 1;"#));
    assert_allowed(&above_a_component(r#"#[unsafe(link_section = "x")] static X: u32 = 1;"#));
}

#[test]
fn words_that_only_matter_in_an_attribute_are_fine_elsewhere() {
    assert_allowed(&in_a_component(r#"let shape = path("M0 0 L1 1"); let link = 1; let feature = 2;"#));
}

#[test]
fn words_in_strings_and_comments_are_fine() {
    for body in [r#"let a = "env!";"#, "// include_str!", "/* mod a; */", r##"let a = r#"macro_rules!"#;"##] {
        assert_allowed(&in_a_component(body));
    }
}

#[test]
fn a_module_with_a_body_is_fine() {
    assert_allowed(&above_a_component("mod helpers { pub fn one() -> u32 { 1 } }"));
}

#[test]
fn a_refusal_names_its_line() {
    let source = "use toolkit::prelude::*;\n#[component] fn A() -> Element {\n    let home = env!(\"HOME\");\n    text(home)\n}\n";
    let refusals = check_source(FILE, source).unwrap_err();
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert_eq!(refusals[0].file, FILE);
    assert_eq!(refusals[0].line, 3);
    assert_eq!(refusals[0].message, "`env` can't be used in a component, as it could read settings from this computer");
}

#[test]
fn every_refusal_is_listed_in_order() {
    let source = "mod a;\n#[component] fn A() -> Element {\n    { env!(\"HOME\"); }\n    include!(\"x\")\n}\n";
    let lines: Vec<u32> = check_source(FILE, source).unwrap_err().iter().map(|refusal| refusal.line).collect();
    assert_eq!(lines, [1, 3, 4]);
}

#[test]
fn a_file_with_no_component_is_refused() {
    assert_refused("fn a() -> u32 { 1 }\n");
    assert_refused("#[component] struct A;\nfn b() {}\n");
    assert_refused("");
}

#[test]
fn a_file_with_two_components_is_refused() {
    let source = "#[component]\nfn A() -> Element { text(\"a\") }\n\n#[component]\nfn B() -> Element { text(\"b\") }\n";
    let refusals = check_source(FILE, source).unwrap_err();
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert_eq!(refusals[0].line, 4);
}

#[test]
fn the_tag_is_the_function_name() {
    let tag = |source: &str| check_source(FILE, source);
    let score = || Ok(SourceInfo { tag: "Score".into() });
    assert_eq!(tag("#[component]\nfn Score(team: String) -> Element { text(team) }\n"), score());
    assert_eq!(tag("#[component]\npub fn Score() -> Element { text(\"a\") }\n"), score());
    assert_eq!(tag("#[component]\n#[allow(unused)]\npub(crate) fn Score() -> Element { text(\"a\") }\n"), score());
    assert_eq!(tag("#[toolkit::component]\nfn Score() -> Element { text(\"a\") }\n"), score());
}

#[test]
fn component_tag_does_not_judge_the_file() {
    let refused = "mod a;\n#[component] fn First() -> Element { text(env!(\"HOME\")) }\n#[component] fn Second() -> Element { text(\"a\") }\n";
    assert_eq!(component_tag(refused), Some("First".to_string()));
    assert_eq!(component_tag("fn a() {}\n"), None);
    assert_eq!(component_tag("#[component] fn A() { \"unterminated }\n"), None);
}

#[test]
fn a_file_that_will_not_lex_is_refused() {
    assert_refused(&above_a_component("const X: &str = \"unterminated;"));
}

// A talk can nest as deep as it has room for, and this must come back rather than overflow the stack.
#[test]
fn a_very_deeply_nested_file_is_answered() {
    let depth = 400_000;
    let source = format!("{}{}", "(".repeat(depth), ")".repeat(depth));
    assert_refused(&source);
    assert_refused(&format!("{}{}", "(".repeat(depth), ""));
}

#[test]
fn a_refused_source_never_reaches_the_compiler() {
    let folder = tempfile::tempdir().unwrap();
    let bundle = Bundle { root: folder.path().join("no such bundle") };
    let refusals = compile(&bundle, FILE, &in_a_component(r#"let a = env!("HOME");"#)).unwrap_err();
    assert_eq!(refusals.len(), 1, "{refusals:?}");
    assert!(refusals[0].message.contains("`env`"), "{refusals:?}");
    assert_eq!(refusals[0].line, 4);
}
