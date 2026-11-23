//! The Windows installer can't be built or run here, so these read its files as text.

use std::path::PathBuf;

fn repo() -> PathBuf {
    xtask::stage::repo_root()
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(repo().join(relative)).unwrap_or_else(|e| panic!("can't read {relative}: {e}")).replace("\r\n", "\n")
}

fn iss() -> String {
    read("packaging/windows/stagepack-studio.iss")
}

/// The lines of one `[Section]`, without comments or blanks.
fn section(text: &str, name: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim_end();
        if line.starts_with('[') && line.ends_with(']') {
            inside = line == format!("[{name}]");
        } else if inside && !line.is_empty() && !line.starts_with(';') {
            lines.push(line.to_string());
        }
    }
    lines
}

/// The script without its comments.
fn code(text: &str) -> String {
    text.lines().filter(|l| !l.trim_start().starts_with(';')).collect::<Vec<_>>().join("\n")
}

#[test]
fn the_installer_needs_no_administrator() {
    let text = iss();
    let setup = section(&text, "Setup");
    for line in ["PrivilegesRequired=lowest", "PrivilegesRequiredOverridesAllowed=", "DefaultDirName={localappdata}\\Programs\\Stagepack Studio", "DisableProgramGroupPage=yes", "ArchitecturesAllowed=x64compatible", "ArchitecturesInstallIn64BitMode=x64compatible", "ChangesAssociations=yes"] {
        assert!(setup.iter().any(|l| l == line), "[Setup] lacks the line {line}");
    }
    let code = code(&text);
    for forbidden in ["HKLM", "HKCR", "{commonpf", "{commonappdata}", "{commondesktop}", "{commonprograms}", "{sys}", "{autopf", "{pf"] {
        assert!(!code.contains(forbidden), "the installer uses {forbidden}");
    }
    // Every registry line is under HKCU\Software\Classes.
    for line in section(&text, "Registry") {
        assert!(line.starts_with("Root: HKCU; Subkey: \"Software\\Classes\\"), "a registry line outside HKCU\\Software\\Classes: {line}");
    }
}

#[test]
fn the_windows_script_never_touches_the_data() {
    let text = iss();
    for name in ["UninstallDelete", "InstallDelete", "Code"] {
        for line in section(&text, name) {
            let lower = line.to_lowercase();
            for data in ["{userappdata}", "%appdata%", "{localappdata}\\stagepack-studio", "{userdocs}", "{app}\\..", "{userprofile}"] {
                assert!(!lower.contains(data), "[{name}] touches {data}: {line}");
            }
        }
    }
    // Nothing is deleted but what Inno's own log holds, and the app folder itself.
    for line in section(&text, "UninstallDelete") {
        assert!(line.contains("{app}"), "[UninstallDelete] reaches outside the app folder: {line}");
    }
    assert!(section(&text, "InstallDelete").is_empty(), "[InstallDelete] must stay empty");
    assert!(section(&text, "Code").is_empty(), "[Code] must stay empty unless something can't be done declaratively");
    // The PowerShell script doesn't delete anything under the user's data either.
    let ps = read("scripts/package-windows.ps1");
    assert!(!ps.contains("$env:APPDATA") && !ps.contains("%APPDATA%") && !ps.contains("Remove-Item"), "package-windows.ps1 deletes or reaches the data");
}

#[test]
fn the_file_type_keeps_stagepack() {
    let text = iss();
    let lines = section(&text, "Registry");
    let find = |subkey: &str| lines.iter().find(|l| l.contains(&format!("Subkey: \"{subkey}\";"))).unwrap_or_else(|| panic!("no registry line for {subkey}")).clone();

    let default = find("Software\\Classes\\.sptx");
    assert!(default.contains("ValueName: \"\"") && default.contains("ValueData: \"StagepackStudio.Talk\"") && default.contains("Flags: uninsdeletevalue"), "{default}");

    let with = find("Software\\Classes\\.sptx\\OpenWithProgids");
    assert!(with.contains("ValueType: string; ValueName: \"StagepackStudio.Talk\"; ValueData: \"\"") && with.contains("Flags: uninsdeletevalue"), "{with}");

    let icon = find("Software\\Classes\\StagepackStudio.Talk\\DefaultIcon");
    assert!(icon.contains("Flags: uninsdeletekey"), "{icon}");
    let command = find("Software\\Classes\\StagepackStudio.Talk\\shell\\open\\command");
    assert!(command.contains(r#"ValueData: """{app}\stagepack-studio.exe"" ""%1""""#) && command.contains("Flags: uninsdeletekey"), "{command}");

    // Nothing deletes the keys that other programs share: .sptx and its OpenWithProgids.
    for line in &lines {
        if line.contains("Software\\Classes\\.sptx") {
            assert!(!line.contains("uninsdeletekey"), "a shared key may be deleted: {line}");
            assert!(!line.contains("dontcreatekey"), "{line}");
        }
    }
    assert!(!code(&text).to_lowercase().contains("deletekey: "), "an explicit key deletion");
}

#[test]
fn the_installer_ships_the_helper_and_the_bundle() {
    let text = iss();
    let files = section(&text, "Files");
    assert_eq!(files.len(), 1, "[Files] should be the one staged folder: {files:?}");
    assert!(files[0].starts_with("Source: \"{#StagedDir}\\*\"; DestDir: \"{app}\"") && files[0].contains("recursesubdirs") && files[0].contains("createallsubdirs"), "{}", files[0]);
    let stage = read("crates/xtask/src/stage.rs");
    assert!(stage.contains("\"stagepack-terminal\"") && stage.contains("\"bundle\"") && stage.contains("const EXE: &str = if cfg!(windows) { \".exe\" }"));
    // The Windows folder is the same staging, run on Windows.
    let windows = read("crates/xtask/src/windows.rs");
    assert!(windows.contains("crate::stage::stage("));
    assert!(stage.contains("make-bundle.ps1"));
}

#[test]
fn the_windows_folder_is_refused_here() {
    if cfg!(windows) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let message = xtask::windows::windows_folder(&dir.path().join("out"), None).unwrap_err();
    assert_eq!(message, "The Windows folder is staged on Windows.");
    assert!(!dir.path().join("out").exists());
}

#[test]
fn the_powershell_script_runs_the_tests_first() {
    let ps = read("scripts/package-windows.ps1");
    assert!(ps.contains("Set-StrictMode -Version Latest") && ps.contains("$ErrorActionPreference = 'Stop'"));
    assert!(ps.contains("[switch]$SkipTests") && ps.contains("test-all.ps1") && ps.contains("-Quick"));
    let at = |needle: &str| ps.find(needle).unwrap_or_else(|| panic!("package-windows.ps1 lacks {needle}"));
    assert!(at("test-all.ps1") < at("windows-folder"), "tests must run before staging");
    assert!(at("windows-folder") < at("& $iscc"), "staging must come before the compiler");
    for place in ["Inno Setup 6\\ISCC.exe", "${env:ProgramFiles(x86)}", "$env:ProgramFiles", "$env:LOCALAPPDATA", "Get-Command ISCC.exe", "jrsoftware.org"] {
        assert!(ps.contains(place), "package-windows.ps1 lacks {place}");
    }
    // A missing test script is an error unless the tests are skipped on request; each step's exit code is checked.
    assert!(ps.contains("-NoProfile -File") && !ps.contains("does not exist yet"), "the tests must run in their own PowerShell, and a missing script is an error");
    assert!(ps.matches("if ($LASTEXITCODE -ne 0)").count() >= 3, "cargo's, the tests' and ISCC's exit codes must each be checked");
    assert!(ps.contains("Push-Location") && ps.contains("finally") && ps.contains("Pop-Location"));
    assert!(ps.contains("/DAppVersionNumeric="));
    assert!(ps.contains("/DAppVersion=") && ps.contains("/DStagedDir="));
}

#[test]
fn the_app_id_is_one_guid_that_never_changes() {
    let text = iss();
    let ids: Vec<&str> = text.lines().filter(|l| l.starts_with("AppId=")).collect();
    assert_eq!(ids.len(), 1);
    // `{{GUID}`: Inno's escaped brace, then a GUID in capitals, then the closing brace.
    let id = ids[0].strip_prefix("AppId={{").and_then(|s| s.strip_suffix('}')).expect("AppId={{GUID}");
    let parts: Vec<&str> = id.split('-').collect();
    assert_eq!(parts.iter().map(|p| p.len()).collect::<Vec<_>>(), [8, 4, 4, 4, 12]);
    assert!(id.chars().all(|c| c == '-' || c.is_ascii_digit() || ('A'..='F').contains(&c)), "{id}");
    assert_ne!(id, "00000000-0000-0000-0000-000000000000");
    // The GUID appears nowhere else in the file (a second copy could drift), and the comment says it must not change.
    assert_eq!(text.matches(id).count(), 1);
    assert!(text.contains("NEVER change"));
}

#[test]
fn the_installer_script_is_plain() {
    let text = iss();
    assert!(!text.contains('\t'), "tabs in the .iss");
    assert!(text.is_ascii());
    // No indented line outside the preprocessor's own `#if` blocks, and every section header stands alone at column 0.
    let mut seen = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.starts_with('[') {
            assert!(line.ends_with(']') && !line.contains(' '), "line {}: {line}", n + 1);
            seen.push(line.to_string());
        } else if !line.is_empty() && !line.starts_with(';') && !line.starts_with('#') {
            assert!(!line.starts_with(' '), "line {}: indented", n + 1);
        }
    }
    assert_eq!(seen, ["[Setup]", "[Languages]", "[Files]", "[Icons]", "[Registry]", "[Run]"]);
    let setup = section(&text, "Setup");
    for line in [
        "WizardStyle=modern",
        "UninstallDisplayIcon={app}\\icons\\stagepack-studio.ico",
        "CloseApplications=yes",
        "SetupIconFile={#StagedDir}\\icons\\stagepack-studio.ico",
        "AppVersion={#AppVersion}",
        "VersionInfoVersion={#AppVersionNumeric}",
    ] {
        assert!(setup.iter().any(|l| l == line), "[Setup] lacks {line}");
    }
    let run = section(&text, "Run");
    assert_eq!(run.len(), 1);
    assert!(run[0].contains("Flags: postinstall nowait skipifsilent") && run[0].contains("Filename: \"{app}\\stagepack-studio.exe\""));
    // The Start-menu entry.
    assert!(section(&text, "Icons").iter().any(|l| l.contains("{autoprograms}\\Stagepack Studio") && l.contains("Filename: \"{app}\\stagepack-studio.exe\"") && l.contains("IconFilename: \"{app}\\icons\\stagepack-studio.ico\"")));
    // The version and the staged folder come from the command line.
    assert!(text.contains("#ifndef AppVersion") && text.contains("#ifndef StagedDir"));
}

#[test]
fn the_shipped_windows_app_has_no_console_but_commands_print_in_the_terminal() {
    // Windows only and never run here: these check that the pieces are in place.
    let main = read("crates/app/src/main.rs");
    assert!(main.contains(r#"#![cfg_attr(all(windows, feature = "shipped"), windows_subsystem = "windows")]"#));
    let tool = read("crates/app/src/ai_tool/mod.rs");
    let command = &tool[tool.find("pub fn run_command").expect("run_command")..];
    let known = command.find("if !known").expect("the check for a known command");
    let attach = command.find("attach_parent_console();").expect("run_command attaches the console");
    let first_command = command.find("run_describe(rest)").expect("the commands run");
    assert!(known < attach && attach < first_command, "attach after knowing it is a command, before running it");
    assert!(tool.contains("#[cfg(windows)]\nfn attach_parent_console()") && tool.contains("AttachConsole(ATTACH_PARENT_PROCESS)"));
    let manifest = read("crates/app/Cargo.toml");
    assert!(manifest.contains("[target.'cfg(windows)'.dependencies]") && manifest.contains("windows-sys"));
}

#[test]
fn windows_gets_a_console_twin_that_a_shell_waits_for() {
    // Windows only and never run here.
    let twin = read("crates/app/src/bin/stagepack-studio-console.rs");
    assert!(twin.contains("stagepack_studio::run_main()") && !twin.contains("windows_subsystem = \"windows\"]"), "the twin is always a console program");
    let main = read("crates/app/src/main.rs");
    assert!(main.contains("stagepack_studio::run_main()"));
    let manifest = read("crates/app/Cargo.toml");
    assert!(manifest.contains("name = \"stagepack-studio-console\"") && manifest.contains("required-features = [\"windows-console\"]"));
    // Staged only on Windows, as stagepack-studio.com, and built with the feature that makes it.
    let stage = read("crates/xtask/src/stage.rs");
    assert!(stage.contains("\"shipped,windows-console\"") && stage.contains("\"stagepack-studio-console.exe\"") && stage.contains("out.join(\"stagepack-studio.com\")"));
    let copy = stage.find("stagepack-studio.com").unwrap();
    assert!(stage[..copy].rfind("if cfg!(windows)").is_some_and(|at| copy - at < 400), "the twin is copied only on Windows");
    // The installer takes the whole staged folder, so it takes the twin; its comment says so.
    let text = iss();
    assert!(text.contains("stagepack-studio.com"));
    assert_eq!(section(&text, "Files").len(), 1);
}
