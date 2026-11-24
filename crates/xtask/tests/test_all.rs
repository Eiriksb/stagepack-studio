//! The two scripts that run every test name the same steps in the same order.

use std::path::PathBuf;
use std::process::Command;

fn repo() -> PathBuf {
    xtask::stage::repo_root()
}

const STEPS: [&str; 5] = ["clippy", "workspace", "components", "release", "packaging"];

#[test]
fn test_all_lists_its_steps() {
    let out = Command::new("bash").arg(repo().join("scripts/test-all.sh")).arg("--list").output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    let names: Vec<&str> = text.lines().map(|l| l.split('\t').next().unwrap()).collect();
    assert_eq!(names, STEPS);
}

#[test]
fn test_all_ps1_names_the_same_steps() {
    let text = std::fs::read_to_string(repo().join("scripts/test-all.ps1")).unwrap().replace("\r\n", "\n");
    // The steps table: lines of the form `@{ Name = 'clippy'; Text = '...' }`.
    let names: Vec<&str> = text
        .lines()
        .filter_map(|l| l.trim_start().strip_prefix("@{ Name = '"))
        .map(|rest| rest.split('\'').next().unwrap())
        .collect();
    assert_eq!(names, STEPS);
    // Quick and List are switches, a step's failure doesn't stop the next, and the end is explicit.
    assert!(text.contains("[switch]$Quick") && text.contains("[switch]$List"));
    assert!(text.contains("$ErrorActionPreference = 'Continue'") && text.contains("$LASTEXITCODE"));
    assert!(text.trim_end().ends_with("exit 0") && text.contains("exit 1"));
    // The same release tests and the twin of test-components.sh exist.
    let sh = std::fs::read_to_string(repo().join("scripts/test-all.sh")).unwrap();
    for test in sh.lines().filter(|l| l.contains("slides-") || l.contains("stagepack-studio ui_")).flat_map(|l| l.split_whitespace().skip(2)) {
        let test = test.trim_end_matches('"');
        if test.contains('_') {
            assert!(text.contains(test), "test-all.ps1 lacks the release test {test}");
        }
    }
    assert!(repo().join("scripts/test-components.ps1").is_file());
}
