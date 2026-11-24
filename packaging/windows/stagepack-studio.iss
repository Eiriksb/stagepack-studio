; Stagepack Studio's Windows installer, built with Inno Setup 6.3 or newer by scripts/package-windows.ps1.
; The script passes two values on the command line: /DAppVersion=0.1.0 and /DStagedDir=<the staged app folder>
; (and, if it wants the setup somewhere else, /DOutDir=<folder>).
;
; Per user: no administrator rights, nothing outside the user's own folders, and only HKCU: the file-type entries
; below, the user Path (the app folder is added, so a new terminal runs stagepack-studio), and
; HKCU\Software\Stagepack Studio (the earlier .sptx default, kept until uninstalling). [Code] does those two.
; Uninstalling removes what Inno's own log says was installed, plus the registry entries flagged below, takes the app
; folder off the Path and gives the earlier default back. Nothing is ever deleted from the user's data folders.

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
ChangesEnvironment=yes
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
; own name) goes under OpenWithProgids, and stays there after uninstalling.
Root: HKCU; Subkey: "Software\Classes\.sptx\OpenWithProgids"; ValueType: string; ValueName: "{reg:HKCU\Software\Classes\.sptx,|StagepackStudio.Talk}"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\.sptx"; ValueType: string; ValueName: ""; ValueData: "StagepackStudio.Talk"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.sptx\OpenWithProgids"; ValueType: string; ValueName: "StagepackStudio.Talk"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\StagepackStudio.Talk"; ValueType: string; ValueName: ""; ValueData: "Stagepack Studio talk"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\StagepackStudio.Talk\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\icons\stagepack-studio.ico"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\StagepackStudio.Talk\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\stagepack-studio.exe"" ""%1"""; Flags: uninsdeletekey

[Run]
Filename: "{app}\stagepack-studio.exe"; Description: "{cm:LaunchProgram,Stagepack Studio}"; Flags: postinstall nowait skipifsilent

[Code]
// Kept short and plain: Windows can't be run where this is written, so each step is checked by hand (docs/checks/plan-e.md).

// S without one trailing backslash, so that C:\A and C:\A\ are the same Path entry.
function WithoutTrailingSlash(S: string): string;
begin
  Result := S;
  if (Length(Result) > 0) and (Result[Length(Result)] = '\') then
    Result := Copy(Result, 1, Length(Result) - 1);
end;

// True if one of Path's ;-separated entries is Entry, ignoring case and one trailing backslash.
function PathHasEntry(Path, Entry: string): Boolean;
var
  Rest, Part: string;
  At: Integer;
begin
  Result := False;
  Rest := Path + ';';
  while Rest <> '' do
  begin
    At := Pos(';', Rest);
    Part := Copy(Rest, 1, At - 1);
    Rest := Copy(Rest, At + 1, Length(Rest));
    if CompareText(WithoutTrailingSlash(Part), WithoutTrailingSlash(Entry)) = 0 then
      Result := True;
  end;
end;

// Path without the entries equal to Entry (as PathHasEntry compares) and without empty entries.
// The other entries keep their order and their text, %USERPROFILE% and all.
function PathWithout(Path, Entry: string): string;
var
  Rest, Part: string;
  At: Integer;
begin
  Result := '';
  Rest := Path + ';';
  while Rest <> '' do
  begin
    At := Pos(';', Rest);
    Part := Copy(Rest, 1, At - 1);
    Rest := Copy(Rest, At + 1, Length(Rest));
    if (Part <> '') and (CompareText(WithoutTrailingSlash(Part), WithoutTrailingSlash(Entry)) <> 0) then
    begin
      if Result <> '' then
        Result := Result + ';';
      Result := Result + Part;
    end;
  end;
end;

// Adds the app folder to the end of the user Path, unless it is there already (a reinstall).
// With no Path value at all, the new value is just the app folder. It is written back as REG_EXPAND_SZ.
procedure AddAppToPath();
var
  Path, App: string;
begin
  App := ExpandConstant('{app}');
  if not RegQueryStringValue(HKCU, 'Environment', 'Path', Path) then
    Path := '';
  if not PathHasEntry(Path, App) then
  begin
    Path := PathWithout(Path, App);
    if Path <> '' then
      Path := Path + ';';
    RegWriteExpandStringValue(HKCU, 'Environment', 'Path', Path + App);
  end;
end;

// Takes the app folder off the user Path and leaves every other entry as it was.
// If nothing else is left, the Path value goes, as it was before a first install onto a user without one.
procedure RemoveAppFromPath();
var
  Path, App: string;
begin
  App := ExpandConstant('{app}');
  if RegQueryStringValue(HKCU, 'Environment', 'Path', Path) and PathHasEntry(Path, App) then
  begin
    Path := PathWithout(Path, App);
    if Path = '' then
      RegDeleteValue(HKCU, 'Environment', 'Path')
    else
      RegWriteExpandStringValue(HKCU, 'Environment', 'Path', Path);
  end;
end;

// Before [Registry] makes Stagepack Studio the .sptx default, keeps the program that was the default.
// Our own ProgID is never kept, so a reinstall or an upgrade leaves the earlier one saved.
procedure SavePreviousDefault();
var
  Previous: string;
begin
  Previous := '';
  if RegQueryStringValue(HKCU, 'Software\Classes\.sptx', '', Previous) and (Previous <> '') and (CompareText(Previous, 'StagepackStudio.Talk') <> 0) then
    RegWriteStringValue(HKCU, 'Software\Stagepack Studio', 'PreviousSptxDefault', Previous);
end;

// After uninstalling (our own default is gone by now), makes the saved program the default again, if .sptx has
// no default since and that program is still installed. Then forgets the saved value.
procedure RestorePreviousDefault();
var
  Previous, Current: string;
begin
  Previous := '';
  Current := '';
  if RegQueryStringValue(HKCU, 'Software\Stagepack Studio', 'PreviousSptxDefault', Previous) and (Previous <> '') then
  begin
    if (not RegQueryStringValue(HKCU, 'Software\Classes\.sptx', '', Current)) or (Current = '') then
    begin
      if RegKeyExists(HKCU, 'Software\Classes\' + Previous) or RegKeyExists(HKCR, Previous) then
        RegWriteStringValue(HKCU, 'Software\Classes\.sptx', '', Previous);
    end;
  end;
  RegDeleteKeyIncludingSubkeys(HKCU, 'Software\Stagepack Studio');
end;

// Installing: the earlier default is saved before the files and [Registry] go in; the Path changes after.
procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssInstall then
    SavePreviousDefault();
  if CurStep = ssPostInstall then
    AddAppToPath();
end;

// Uninstalling: the Path changes first; the earlier default comes back after Inno has removed ours.
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    RemoveAppFromPath();
  if CurUninstallStep = usPostUninstall then
    RestorePreviousDefault();
end;
