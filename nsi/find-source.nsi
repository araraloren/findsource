; example2.nsi
;
; This script is based on example1.nsi, but it remember the directory, 
; has uninstall support and (optionally) installs start menu shortcuts.
;
; It will install example2.nsi into a directory that the user selects,

;--------------------------------

; The name of the installer
Name "FindSource"

; The file to write
OutFile "findsource-bin.exe"

; The default installation directory
InstallDir $PROGRAMFILES\FindSource

; Registry key to check for directory (so if you install again, it will 
; overwrite the old one automatically)
InstallDirRegKey HKLM "Software\LOREN_FindSource" "Install_Dir"

; Request application privileges for Windows Vista
RequestExecutionLevel admin

;--------------------------------

; Pages

Page components
Page directory
Page instfiles

UninstPage uninstConfirm
UninstPage instfiles

;--------------------------------

; The stuff to install
Section "FindSource (required)"

  SectionIn RO
  
  ; Set output path to the installation directory.
  SetOutPath $INSTDIR
  
  ; Put file there
  File "fs.exe"
  File "c.json"
  File "rs.json"
  File "cpp.json"
  File "cfg.json"
  File "make.json"
  File "unity.json"
  
  ; Write the installation path into the registry
  WriteRegStr HKLM SOFTWARE\LOREN_FindSource "Install_Dir" "$INSTDIR"
  
  ; Write the uninstall keys for Windows
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\LOREN_FindSource" "DisplayName" "LOREN_FindSource"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\LOREN_FindSource" "UninstallString" '"$INSTDIR\uninstall.exe"'
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\LOREN_FindSource" "NoModify" 1
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\LOREN_FindSource" "NoRepair" 1
  WriteUninstaller "uninstall.exe"
  
SectionEnd

;--------------------------------

; Uninstaller

Section "Uninstall"
  
  ; Remove registry keys
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\LOREN_FindSource"
  DeleteRegKey HKLM SOFTWARE\LOREN_FindSource

  ; Remove files and uninstaller
  Delete $INSTDIR\fs.exe
  Delete $INSTDIR\c.json
  Delete $INSTDIR\rs.json
  Delete $INSTDIR\cpp.json
  Delete $INSTDIR\cfg.json
  Delete $INSTDIR\make.json
  Delete $INSTDIR\unity.json
  Delete $INSTDIR\uninstall.exe

  RMDir "$INSTDIR"

SectionEnd
