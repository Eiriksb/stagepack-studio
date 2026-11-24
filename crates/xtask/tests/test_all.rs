//! The two scripts that run every test name the same steps in the same order.

use std::path::PathBuf;
use std::process::Command;

fn repo() -> PathBuf {
    xtask::stage::repo_root()
}

const STEPS: [&str; 5] = ["clippy", "workspace", "components", "release", "packaging"];

#[test]
fn test_all_lists_its_steps() {
    let script = repo().join("scripts/test-all.sh");
    // On Windows `bash` may be WSL's launcher, which can't read a Windows path, or not there at all.
    let can_run = Command::new("bash").arg("-c").arg("[ -r \"$0\" ]").arg(&script).status().is_ok_and(|s| s.success());
    if !can_run {
        eprintln!("skipped: bash can't run {}", script.display());
        return;
    }
    let out = Command::new("bash").arg(&script).arg("--list").output().unwrap();
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
    // The step function must return the one variable its scriptblock writes (it once didn't, so every
    // step but one looked ok), and a rerun counts only when a test passed.
    assert!(text.contains("$script:worst = 0") && text.contains("return $script:worst"));
    assert!(text.contains("{ $script:worst = $LASTEXITCODE }"));
    assert!(!text.contains("$worst") || text.matches("$worst").count() == text.matches("$script:worst").count());
    assert!(text.contains("-Pattern 'test result: ok\\. [1-9][0-9]* passed'"));
    // Clippy is strict unless -Lax.
    assert!(text.contains("if ($Lax)") && text.contains("'-D', 'warnings'"));
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

#[test]
fn test_all_ps1_runs_the_same_everywhere() {
    let text = std::fs::read_to_string(repo().join("scripts/test-all.ps1")).unwrap().replace("\r\n", "\n");
    // test_all skips its bash test itself, so it is always run, whatever `bash` is.
    assert!(text.contains("'--test', 'test_all'") && !text.contains("Get-Command bash"));
    // The components run under the PowerShell that runs this script.
    assert!(text.contains("(Get-Process -Id $PID).Path"));
    assert!(!text.contains("& powershell "));
    // $LASTEXITCODE is set before anything reads it (StrictMode stops on an unset variable).
    let set = text.find("$global:LASTEXITCODE = 0").expect("$LASTEXITCODE is set first");
    assert!(text.lines().position(|l| l.contains("$global:LASTEXITCODE = 0")) < text.lines().position(|l| l.contains("function Invoke-Step")) && set > 0);
    // A rerun of the components doesn't leave STUDIO_BUNDLE set for the later steps.
    let at = text.find("$env:STUDIO_BUNDLE = ").expect("the rerun sets STUDIO_BUNDLE");
    let rest = &text[at..];
    assert!(text[..at].contains("$saved = $env:STUDIO_BUNDLE") && rest.contains("$env:STUDIO_BUNDLE = $saved"), "STUDIO_BUNDLE must be put back");
}
