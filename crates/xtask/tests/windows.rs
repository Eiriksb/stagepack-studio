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

/// The raw text of the `[Code]` section (Pascal, so its comments are `//`, not `;`).
fn pascal(text: &str) -> String {
    let start = text.find("\n[Code]\n").expect("a [Code] section") + "\n[Code]\n".len();
    let rest = &text[start..];
    let end = rest.find("\n[").map_or(rest.len(), |e| e + 1);
    rest[..end].to_string()
}

/// One Pascal function or procedure, from its header to its `end;` at column 0.
fn routine(code: &str, header: &str) -> String {
    let at = code.find(&format!("\n{header}")).unwrap_or_else(|| panic!("[Code] lacks {header}")) + 1;
    let end = code[at..].find("\nend;\n").unwrap_or_else(|| panic!("{header} has no end;")) + "\nend;".len();
    code[at..at + end].to_string()
}

/// True if `body` has a line `if <condition> then` whose next line is just `call`.
fn then_calls(body: &str, condition: &str, call: &str) -> bool {
    let lines: Vec<&str> = body.lines().map(str::trim).collect();
    lines.windows(2).any(|w| w[0] == format!("if {condition} then") && w[1] == call)
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
    // HKCR is only read, to see whether the earlier program is still there.
    let without_hkcr_reads = code.replace("RegKeyExists(HKCR, ", "");
    assert!(!without_hkcr_reads.contains("HKCR"), "the installer writes or reads HKCR beyond RegKeyExists");
    for forbidden in ["HKLM", "HKEY_LOCAL_MACHINE", "HKEY_CLASSES_ROOT", "{commonpf", "{commonappdata}", "{commondesktop}", "{commonprograms}", "{sys}", "{autopf", "{pf"] {
        assert!(!code.contains(forbidden), "the installer uses {forbidden}");
    }
    // Every registry line is under HKCU\Software\Classes.
    for line in section(&text, "Registry") {
        assert!(line.starts_with("Root: HKCU; Subkey: \"Software\\Classes\\"), "a registry line outside HKCU\\Software\\Classes: {line}");
    }
    // Every registry call in [Code] that writes or deletes is under HKCU.
    let pascal = pascal(&text);
    for call in ["RegWriteStringValue(", "RegWriteExpandStringValue(", "RegDeleteValue(", "RegDeleteKeyIncludingSubkeys(", "RegQueryStringValue("] {
        for (at, _) in pascal.match_indices(call) {
            assert!(pascal[at + call.len()..].starts_with("HKCU, "), "{call} outside HKCU");
        }
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
    // Before an upgrade the old compiler bundle goes, so that a new toolchain's files don't sit beside the old ones.
    assert_eq!(section(&text, "InstallDelete"), [r#"Type: filesandordirs; Name: "{app}\bundle""#], "[InstallDelete] holds only the old bundle");
    // [Code] deletes no files or folders, and the only key it deletes is our own saved default's.
    let pascal = pascal(&text);
    for call in ["DelTree", "DeleteFile", "RemoveDir", "DeleteKey("] {
        assert!(!pascal.contains(call), "[Code] calls {call}");
    }
    assert_eq!(pascal.matches("RegDeleteKeyIncludingSubkeys(").count(), 1);
    assert!(pascal.contains(r"RegDeleteKeyIncludingSubkeys(HKCU, 'Software\Stagepack Studio');"));
    // The PowerShell script doesn't delete anything under the user's data either.
    let ps = read("scripts/package-windows.ps1");
    assert!(!ps.contains("$env:APPDATA") && !ps.contains("%APPDATA%") && !ps.contains("Remove-Item"), "package-windows.ps1 deletes or reaches the data");
}

#[test]
fn the_file_type_keeps_stagepack() {
    let text = iss();
    let lines = section(&text, "Registry");
    let find = |subkey: &str| lines.iter().find(|l| l.contains(&format!("Subkey: \"{subkey}\";"))).unwrap_or_else(|| panic!("no registry line for {subkey}")).clone();

    // The program that was the default before stays offered: its ProgID goes under OpenWithProgids first
    // (Inno reads the old default when it processes the line), and stays after uninstalling.
    let keep = lines.iter().position(|l| l.contains(r"{reg:HKCU\Software\Classes\.sptx,|StagepackStudio.Talk}")).expect("the earlier default is kept under OpenWithProgids");
    let keep_line = &lines[keep];
    assert!(keep_line.starts_with(r#"Root: HKCU; Subkey: "Software\Classes\.sptx\OpenWithProgids"; ValueType: string; ValueName: "{reg:"#) && keep_line.contains(r#"ValueData: """#), "{keep_line}");
    assert!(!keep_line.contains("Flags:"), "the earlier default's entry must not be removed on uninstall: {keep_line}");
    let set_default = lines.iter().position(|l| l.contains(r#"Subkey: "Software\Classes\.sptx"; ValueType: string; ValueName: """#)).unwrap();
    assert!(keep < set_default, "the earlier default must be read before ours replaces it");

    let default = find("Software\\Classes\\.sptx");
    assert!(default.contains("ValueName: \"\"") && default.contains("ValueData: \"StagepackStudio.Talk\"") && default.contains("Flags: uninsdeletevalue"), "{default}");

    let with = lines.iter().find(|l| l.contains(r#"Subkey: "Software\Classes\.sptx\OpenWithProgids"; ValueType: string; ValueName: "StagepackStudio.Talk""#)).expect("our own ProgID under OpenWithProgids").clone();
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
    assert!(ps.contains("-NoProfile -ExecutionPolicy Bypass -File") && !ps.contains("does not exist yet"), "the tests must run in their own PowerShell, and a missing script is an error");
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
    // ([Code] is Pascal and is indented as Pascal is.)
    let mut seen = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.starts_with('[') {
            assert!(line.ends_with(']') && !line.contains(' '), "line {}: {line}", n + 1);
            seen.push(line.to_string());
        } else if !line.is_empty() && !line.starts_with(';') && !line.starts_with('#') && seen.last().is_none_or(|s| s != "[Code]") {
            assert!(!line.starts_with(' '), "line {}: indented", n + 1);
        }
    }
    assert_eq!(seen, ["[Setup]", "[Languages]", "[InstallDelete]", "[Files]", "[Icons]", "[Registry]", "[Run]", "[Code]"]);
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

#[test]
fn a_component_build_shows_no_console_window() {
    let compile = read("crates/components/src/compile.rs");
    let at = compile.find("fn rustc_command").expect("rustc_command");
    let body = &compile[at..at + compile[at..].find("\n}\n").unwrap()];
    let flag = body.find("creation_flags(&mut command, 0x0800_0000)").expect("rustc is started with CREATE_NO_WINDOW");
    assert!(body[..flag].rfind("#[cfg(windows)]").is_some_and(|c| flag - c < 100), "only on Windows");
}

#[test]
fn the_bundles_macros_carry_their_c_runtime() {
    // The proc-macro DLLs load into the bundle's rustc on a PC that may have no Visual C++ runtime.
    // Cargo passes no rust flags to host builds under --target, so the host's flags go through -Zhost-config.
    let ps = read("scripts/make-bundle.ps1");
    assert!(ps.contains("host.rustflags=['-C', 'target-feature=+crt-static']") && ps.contains("-Zhost-config") && ps.contains("-Ztarget-applies-to-host"), "make-bundle.ps1 builds the macros without crt-static");
    let stage = read("crates/xtask/src/stage.rs");
    assert!(stage.contains(r#"release_flags(&repo, cfg!(all(windows, target_env = "msvc")))"#));
}

#[test]
fn nested_powershell_runs_bypass_the_execution_policy() {
    for script in ["scripts/package-windows.ps1", "scripts/test-all.ps1"] {
        let ps = read(script);
        for line in ps.lines().filter(|l| l.contains(" -File ")) {
            assert!(line.contains("-NoProfile -ExecutionPolicy Bypass -File"), "{script}: {line}");
        }
    }
}

#[test]
fn the_installer_adds_the_app_to_the_user_path() {
    let text = iss();
    assert!(section(&text, "Setup").iter().any(|l| l == "ChangesEnvironment=yes"), "[Setup] lacks ChangesEnvironment=yes");
    assert!(!text.contains("HKLM"));
    let code = pascal(&text);
    let read = "RegQueryStringValue(HKCU, 'Environment', 'Path', Path)";
    let write = "RegWriteExpandStringValue(HKCU, 'Environment', 'Path', ";
    // The Path is read as it is (an expandable %USERPROFILE% entry stays one) and written back as REG_EXPAND_SZ.
    let add = routine(&code, "procedure AddAppToPath();");
    assert!(add.contains(read) && add.contains(write) && add.contains("ExpandConstant('{app}')") && add.contains("PathHasEntry(Path, App)"), "{add}");
    // With no Path at all, it is just the app folder; the other entries come first, with no empty ones.
    assert!(add.contains("Path := '';") && add.contains("PathWithout(Path, App)"), "{add}");
    let remove = routine(&code, "procedure RemoveAppFromPath();");
    assert!(remove.contains(read) && remove.contains(write) && remove.contains("PathWithout(Path, App)") && remove.contains("ExpandConstant('{app}')"), "{remove}");
    assert!(!code.contains("RegWriteStringValue(HKCU, 'Environment'"), "Path must stay REG_EXPAND_SZ");
    let step = routine(&code, "procedure CurStepChanged(CurStep: TSetupStep);");
    assert!(then_calls(&step, "CurStep = ssPostInstall", "AddAppToPath();"), "{step}");
    let unstep = routine(&code, "procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);");
    assert!(then_calls(&unstep, "CurUninstallStep = usUninstall", "RemoveAppFromPath();"), "{unstep}");
}

#[test]
fn an_upgrade_keeps_the_saved_default() {
    let text = iss();
    let code = pascal(&text);
    let step = routine(&code, "procedure CurStepChanged(CurStep: TSetupStep);");
    assert!(then_calls(&step, "CurStep = ssInstall", "SavePreviousDefault();"), "{step}");
    let save = routine(&code, "procedure SavePreviousDefault();");
    let read = save.find(r"RegQueryStringValue(HKCU, 'Software\Classes\.sptx', '', Previous)").expect("reads the current default");
    let nonempty = save.find("(Previous <> '')").expect("only a non-empty default");
    let not_ours = save.find("(CompareText(Previous, 'StagepackStudio.Talk') <> 0)").expect("never our own ProgID");
    let write = save.find(r"RegWriteStringValue(HKCU, 'Software\Stagepack Studio', 'PreviousSptxDefault', Previous);").expect("saves it");
    assert!(read < nonempty && nonempty < not_ours && not_ours < write, "{save}");
    assert_eq!(code.matches("'PreviousSptxDefault'").count(), 2, "written once, read once");
}

#[test]
fn the_earlier_default_comes_back_only_if_it_still_exists() {
    let text = iss();
    let code = pascal(&text);
    let unstep = routine(&code, "procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);");
    assert!(then_calls(&unstep, "CurUninstallStep = usPostUninstall", "RestorePreviousDefault();"), "{unstep}");
    let restore = routine(&code, "procedure RestorePreviousDefault();");
    let at = |needle: &str| restore.find(needle).unwrap_or_else(|| panic!("RestorePreviousDefault lacks {needle}:\n{restore}"));
    let saved = at(r"RegQueryStringValue(HKCU, 'Software\Stagepack Studio', 'PreviousSptxDefault', Previous)");
    // By usPostUninstall our own default is gone, so anything there now was set by someone else and stays.
    let current = at(r"(not RegQueryStringValue(HKCU, 'Software\Classes\.sptx', '', Current)) or (Current = '')");
    let exists = at(r"RegKeyExists(HKCU, 'Software\Classes\' + Previous) or RegKeyExists(HKCR, Previous)");
    let write = at(r"RegWriteStringValue(HKCU, 'Software\Classes\.sptx', '', Previous);");
    let forget = at(r"RegDeleteKeyIncludingSubkeys(HKCU, 'Software\Stagepack Studio');");
    assert!(saved < current && current < exists && exists < write && write < forget, "{restore}");
    // The saved value goes whatever happened: the deletion is not inside the ifs.
    assert!(restore.lines().any(|l| l == "  RegDeleteKeyIncludingSubkeys(HKCU, 'Software\\Stagepack Studio');"), "{restore}");
}

#[test]
fn the_path_code_compares_without_case_or_trailing_slash() {
    let text = iss();
    let code = pascal(&text);
    let strip = routine(&code, "function WithoutTrailingSlash(S: string): string;");
    assert!(strip.contains("(Result[Length(Result)] = '\\')") && strip.contains("Copy(Result, 1, Length(Result) - 1)") && !strip.contains("while"), "strips exactly one: {strip}");
    let same = "CompareText(WithoutTrailingSlash(Part), WithoutTrailingSlash(Entry))";
    let has = routine(&code, "function PathHasEntry(Path, Entry: string): Boolean;");
    assert!(has.contains(&format!("{same} = 0")) && has.contains("Pos(';', Rest)"), "{has}");
    let without = routine(&code, "function PathWithout(Path, Entry: string): string;");
    assert!(without.contains(&format!("{same} <> 0")) && without.contains("Pos(';', Rest)") && without.contains("(Part <> '')"), "{without}");
    // Each routine is declared before it is used.
    let order: Vec<usize> = ["function WithoutTrailingSlash", "function PathHasEntry", "function PathWithout", "procedure AddAppToPath", "procedure RemoveAppFromPath", "procedure SavePreviousDefault", "procedure RestorePreviousDefault", "procedure CurStepChanged", "procedure CurUninstallStepChanged"]
        .iter()
        .map(|h| code.find(h).unwrap_or_else(|| panic!("[Code] lacks {h}")))
        .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "out of order");
}
