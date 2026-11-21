Unicode true
RequestExecutionLevel user
ManifestDPIAware true
SetCompressor /SOLID lzma
!include "MUI2.nsh"
!include "x64.nsh"

!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\Tervix"
Name "Tervix"
OutFile "${OUTPUT}"
InstallDir "$LOCALAPPDATA\Programs\Tervix"
ShowInstDetails show
ShowUninstDetails show

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "Russian"

Function .onInit
  ${IfNot} ${RunningX64}
    MessageBox MB_OK|MB_ICONSTOP "Tervix requires 64-bit Windows." /SD IDOK
    Abort
  ${EndIf}
  SetRegView 64
  SetShellVarContext current
  ReadRegStr $0 HKCU "${UNINSTALL_KEY}" "InstallLocation"
  StrCmp $0 "" +2
  StrCpy $INSTDIR $0
FunctionEnd

Section "Tervix"
  ReadRegStr $0 HKCU "${UNINSTALL_KEY}" "InstallLocation"
  StrCmp $0 "" new_install
  StrCmp $0 $INSTDIR install_files
  MessageBox MB_OK|MB_ICONSTOP "Uninstall the existing copy before changing its location." /SD IDOK
  Abort
new_install:
  IfFileExists "$INSTDIR\tervix.exe" conflict
  IfFileExists "$INSTDIR\LICENSE.txt" conflict
  IfFileExists "$INSTDIR\Uninstall.exe" conflict install_files
conflict:
  MessageBox MB_OK|MB_ICONSTOP "This folder contains files with the same names. Choose another folder." /SD IDOK
  Abort
install_files:
  SetOutPath "$INSTDIR"
  File /oname=tervix.exe "${PAYLOAD}"
  File /oname=LICENSE.txt "LICENSE"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateShortcut "$SMPROGRAMS\Tervix.lnk" "$INSTDIR\tervix.exe"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "Tervix"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\tervix.exe"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
SectionEnd

Section "Uninstall"
  SetRegView 64
  SetShellVarContext current
  ReadRegStr $0 HKCU "${UNINSTALL_KEY}" "InstallLocation"
  StrCmp $0 $INSTDIR +2
  Abort
  ClearErrors
  Delete "$INSTDIR\tervix.exe"
  IfErrors 0 +3
  MessageBox MB_OK|MB_ICONSTOP "Close Tervix and retry." /SD IDOK
  Abort
  Delete "$INSTDIR\LICENSE.txt"
  Delete "$INSTDIR\Uninstall.exe"
  Delete "$SMPROGRAMS\Tervix.lnk"
  DeleteRegKey HKCU "${UNINSTALL_KEY}"
  RMDir "$INSTDIR"
SectionEnd
