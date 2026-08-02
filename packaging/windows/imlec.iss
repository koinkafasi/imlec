; Inno Setup script for the imlec one-click Windows installer.
; Built in CI by .github/workflows/release.yml; produces imlec-setup-x64.exe.
;   iscc /DAppVersion=0.1.0 packaging\windows\imlec.iss

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif

#define AppName "imlec"
#define AppPublisher "koinkafasi"
#define AppURL "https://github.com/koinkafasi/imlec"
#define AppExe "imlec.exe"

[Setup]
AppId={{7C4C1F9E-3B2A-4E51-9A0D-6F1B8C2D5E30}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppURL}
AppSupportURL={#AppURL}/issues
AppUpdatesURL={#AppURL}/releases
VersionInfoVersion={#AppVersion}

; Per-user install: no UAC prompt, no admin rights needed.
PrivilegesRequired=lowest
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
DisableDirPage=auto
LicenseFile=..\..\LICENSE
OutputDir=..\..\dist
OutputBaseFilename=imlec-setup-x64
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern

; x86_64 only. Refuses to install on 32-bit or ARM hosts.
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "turkish"; MessagesFile: "compiler:Languages\Turkish.isl"

[Tasks]
Name: "startup"; Description: "{cm:AutoStartProgram,{#AppName}}"; GroupDescription: "{cm:AdditionalIcons}"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\..\dist\payload\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\dist\payload\default.toml"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{group}\{cm:UninstallProgram,{#AppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon
Name: "{userstartup}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: startup

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; The config lives outside {app}; leave it alone unless the user asks.
Type: dirifempty; Name: "{app}"

[Code]
// A running instance holds imlec.exe open, so stop it before copying files.
function InitializeSetup(): Boolean;
var
  ResultCode: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM imlec.exe', '',
       SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Result := True;
end;

function InitializeUninstall(): Boolean;
var
  ResultCode: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM imlec.exe', '',
       SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Result := True;
end;
