//! The example components in `fixtures/components`, which the guide quotes: each must pass the
//! source check, build, load and render, and the guide may show nothing that isn't in one.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use slides_components::{check_source, compile, Bundle, WasmComponent};
use toolkit::{asset_path_allowed, CtxData, Element, RenderInput, RenderOutput, Theme};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn talk() -> PathBuf {
    repo().join("fixtures/components")
}

/// Each example's file name and its text, in order of name.
fn examples() -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = std::fs::read_dir(talk().join("components"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .map(|path| (path.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read_to_string(&path).unwrap()))
        .collect();
    found.sort();
    found
}

/// An example's file, its tag, and the attributes it's drawn with.
struct Case {
    file: &'static str,
    tag: &'static str,
    attrs: &'static [(&'static str, &'static str)],
}

const CASES: [Case; 5] = [
    Case { file: "bars.rs", tag: "Bars", attrs: &[("data", "assets/sales.csv")] },
    Case { file: "callout.rs", tag: "Callout", attrs: &[("title", "Remember")] },
    Case { file: "clock.rs", tag: "Clock", attrs: &[("size", "240")] },
    Case { file: "reveal.rs", tag: "Reveal", attrs: &[("first", "One"), ("second", "Two"), ("third", "Three")] },
    Case { file: "score.rs", tag: "Score", attrs: &[("team", "Oslo"), ("points", "3"), ("colour", "#2563eb")] },
];

#[test]
fn every_example_passes_the_source_check() {
    let examples = examples();
    let files: Vec<&str> = examples.iter().map(|(file, _)| file.as_str()).collect();
    assert_eq!(files, CASES.map(|case| case.file));
    for (case, (file, source)) in CASES.iter().zip(&examples) {
        let info = check_source(&format!("components/{file}"), source).unwrap_or_else(|errors| panic!("{file}: {errors:?}"));
        assert_eq!(info.tag, case.tag, "{file}");
    }
}

/// The language and text of each fenced block in `markdown`.
fn code_blocks(markdown: &str) -> Vec<(String, String)> {
    let mut blocks = Vec::new();
    let mut current: Option<(String, Vec<&str>)> = None;
    for line in markdown.lines() {
        let fence = line.trim_start().strip_prefix("```");
        match (&mut current, fence) {
            (None, Some(language)) => current = Some((language.trim().to_string(), Vec::new())),
            (None, None) => {}
            (Some(_), Some(_)) => {
                let (language, lines) = current.take().unwrap();
                blocks.push((language, lines.join("\n")));
            }
            (Some((_, lines)), None) => lines.push(line),
        }
    }
    assert!(current.is_none(), "a block in the guide is never closed");
    blocks
}

/// The text of every file in the example talk, other than its components.
fn other_files(folder: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if !path.ends_with("components") {
                found.extend(other_files(&path));
            }
        } else {
            found.push(std::fs::read_to_string(&path).unwrap());
        }
    }
    found
}

#[test]
fn the_guide_quotes_only_the_examples() {
    let guide = std::fs::read_to_string(repo().join("docs/guide/components.md")).unwrap();
    let blocks = code_blocks(&guide);
    let examples: Vec<String> = examples().into_iter().map(|(_, source)| source).collect();
    let others = other_files(&talk());
    let rust = blocks.iter().filter(|(language, _)| language == "rust").count();
    assert!(rust > 0, "the guide shows no code");
    for (language, block) in &blocks {
        let quoted_from = if language == "rust" { &examples } else { &others };
        assert!(
            quoted_from.iter().any(|text| text.contains(block.as_str())),
            "this {language} block in the guide isn't in the example talk, word for word:\n{block}"
        );
    }
}

fn bundle() -> Bundle {
    Bundle::find().expect("STUDIO_BUNDLE should name a bundle made by scripts/make-bundle.sh")
}

fn render(component: &WasmComponent, attrs: &[(&str, &str)], click_step: u32, time: f32) -> RenderOutput {
    let props: BTreeMap<String, String> = attrs.iter().map(|(name, value)| (name.to_string(), value.to_string())).collect();
    let ctx = CtxData::new(click_step, time, 1920.0, 1080.0, 1600.0, 0.0, Theme::default());
    let assets = talk();
    let read = Rc::new(move |path: &str| asset_path_allowed(path).then(|| std::fs::read(assets.join(path)).ok()).flatten());
    component.render(&RenderInput::new(props, ctx), read).unwrap()
}

/// Whether anything in `element` is a labelled box saying what's wrong.
fn has_problem(element: &Element) -> bool {
    match element {
        Element::Problem { .. } => true,
        Element::Stack { children, .. } => children.iter().any(has_problem),
        Element::Shape { child, .. } => child.as_deref().is_some_and(has_problem),
        Element::Canvas { items, .. } => items.iter().any(|item| has_problem(&item.element)),
        _ => false,
    }
}

#[test]
#[ignore = "needs the compiler bundle"]
fn every_example_builds_and_runs() {
    let bundle = bundle();
    for (case, (file, source)) in CASES.iter().zip(examples()) {
        let wasm = compile(&bundle, &format!("components/{file}"), &source).unwrap_or_else(|errors| panic!("{file}: {errors:?}"));
        let component = WasmComponent::load(&wasm).unwrap_or_else(|error| panic!("{file}: {error}"));
        assert_eq!(component.schema().name, case.tag);

        let output = render(&component, case.attrs, 1, 15.0);
        assert!(!has_problem(&output.element), "{file}: {:?}", output.element);
        assert_ne!(output.element, Element::Empty, "{file}");
        match case.tag {
            "Reveal" => {
                assert_eq!(output.steps, 3);
                assert_ne!(render(&component, case.attrs, 0, 0.0).element, render(&component, case.attrs, 3, 0.0).element);
            }
            "Clock" => {
                assert!(output.animated);
                assert_ne!(render(&component, case.attrs, 0, 0.0).element, output.element);
            }
            "Callout" => assert!(component.schema().children),
            _ => assert!(!output.stepped && !output.animated, "{file}"),
        }
    }
}
