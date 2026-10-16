#define AppVersion "0.1.0"
#ifndef RepoDir
#define RepoDir "..\.."
#endif
#ifndef OutputDir
#define OutputDir "..\..\dist"
#endif
[Setup]
AppId={{E7D8E728-4A6E-4CC6-8A08-ROADWAVES0001}
AppName=RoadWaves
AppVersion={#AppVersion}
AppPublisher=RoadWaves contributors
DefaultDirName={localappdata}\Programs\RoadWaves
DefaultGroupName=RoadWaves
OutputDir={#OutputDir}
OutputBaseFilename=RoadWaves-{#AppVersion}-windows-x64
Compression=lzma
SolidCompression=yes
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
UninstallDisplayName=RoadWaves
LicenseFile={#RepoDir}\LICENSE

[Files]
Source: "{#RepoDir}\target\x86_64-pc-windows-gnu\release\roadwaves.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoDir}\target\x86_64-pc-windows-gnu\release\roadwaves-gui.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoDir}\target\x86_64-pc-windows-gnu\release\roadwaves_scs.dll"; DestDir: "{app}\plugins"; Flags: ignoreversion
Source: "{#RepoDir}\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoDir}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoDir}\docs\IMPLEMENTATION_REPORT.md"; DestDir: "{app}\docs"; Flags: ignoreversion

[Icons]
Name: "{group}\RoadWaves Doctor"; Filename: "{app}\roadwaves.exe"; Parameters: "doctor"; WorkingDir: "{app}"
Name: "{group}\RoadWaves"; Filename: "{app}\roadwaves-gui.exe"; WorkingDir: "{app}"
Name: "{commondesktop}\RoadWaves Doctor"; Filename: "{app}\roadwaves.exe"; Parameters: "doctor"; WorkingDir: "{app}"; Tasks: desktopicon

[Tasks]
Name: desktopicon; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"
