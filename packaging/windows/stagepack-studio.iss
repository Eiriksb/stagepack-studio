; Stagepack Studio's Windows installer, built with Inno Setup 6.3 or newer by scripts/package-windows.ps1.
; The script passes two values on the command line: /DAppVersion=0.1.0 and /DStagedDir=<the staged app folder>
; (and, if it wants the setup somewhere else, /DOutDir=<folder>).
;
; Per user: no administrator rights, nothing outside the user's own folders, and only the HKCU file-type entries below.
; Uninstalling removes what Inno's own log says was installed, plus the registry entries flagged below. Nothing is
; ever deleted from the user's data folders.

#ifndef AppVersion
#error Pass the version: ISCC /DAppVersion=0.1.0 /DStagedDir=<staged folder> stagepack-studio.iss
#endif
#ifndef StagedDir
#error Pass the staged app folder: ISCC /DAppVersion=0.1.0 /DStagedDir=<staged folder> stagepack-studio.iss
#endif
; Inno wants up to four plain numbers for the file version (0.2.0-beta would be refused), so the script passes those apart.
#ifndef AppVersionNumeric
#define AppVersionNumeric AppVersion
#endif
#ifndef OutDir
#define OutDir SourcePath + "..\..\target\package"
#endif

[Setup]
; This GUID identifies the app to Windows so that a new version installs over the old one.
; It must NEVER change. ({{ is how Inno writes a literal brace.)
AppId={{D5D431CE-0EB9-45EB-8110-EB9518B4C9F1}
AppName=Stagepack Studio
AppVersion={#AppVersion}
AppPublisher=Stagepack Studio
VersionInfoVersion={#AppVersionNumeric}
DefaultDirName={localappdata}\Programs\Stagepack Studio
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
WizardStyle=modern
SetupIconFile={#StagedDir}\icons\stagepack-studio.ico
UninstallDisplayIcon={app}\icons\stagepack-studio.ico
ChangesAssociations=yes
CloseApplications=yes
RestartApplications=no
Compression=lzma2/max
SolidCompression=yes
OutputDir={#OutDir}
OutputBaseFilename=stagepack-studio-{#AppVersion}-windows-x86_64-setup

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[InstallDelete]
; An upgrade first takes away the old compiler bundle: Inno only adds and replaces files, and a new toolchain's
; files (named by hash) beside the old ones stop component builds. It is inside {app}, so no data is touched.
Type: filesandordirs; Name: "{app}\bundle"

[Files]
; The whole staged folder, which holds stagepack-studio.exe (the windowed program) and stagepack-studio.com (its console twin, which a shell waits for).
Source: "{#StagedDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\Stagepack Studio"; Filename: "{app}\stagepack-studio.exe"; IconFilename: "{app}\icons\stagepack-studio.ico"

[Registry]
; .sptx opens with Stagepack Studio by default. Other programs stay offered under Open with:
; we only add names to OpenWithProgids and never delete that key.
; First the program that was the default before ours (Inno reads it as it processes this line; with none, our
; own name) goes under OpenWithProgids, and stays there after uninstalling. Uninstalling doesn't put that default
; back: that would need a [Code] step.
Root: HKCU; Subkey: "Software\Classes\.sptx\OpenWithProgids"; ValueType: string; ValueName: "{reg:HKCU\Software\Classes\.sptx,|StagepackStudio.Talk}"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\.sptx"; ValueType: string; ValueName: ""; ValueData: "StagepackStudio.Talk"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.sptx\OpenWithProgids"; ValueType: string; ValueName: "StagepackStudio.Talk"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\StagepackStudio.Talk"; ValueType: string; ValueName: ""; ValueData: "Stagepack Studio talk"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\StagepackStudio.Talk\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\icons\stagepack-studio.ico"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\StagepackStudio.Talk\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\stagepack-studio.exe"" ""%1"""; Flags: uninsdeletekey

[Run]
Filename: "{app}\stagepack-studio.exe"; Description: "{cm:LaunchProgram,Stagepack Studio}"; Flags: postinstall nowait skipifsilent
