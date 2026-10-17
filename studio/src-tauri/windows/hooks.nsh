; Setup steps for Klipp's two programs (Tauri runs these around installing
; and uninstalling; see installer.nsi). The window (klipp.exe) is closed by
; Tauri's own check; the recorder lives in the tray, so it's stopped here.

!macro StopRecorder
  nsExec::Exec 'taskkill /IM klipp-recorder.exe /F'
  Pop $0
  ; Windows releases the file a moment after the process is gone.
  Sleep 800
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro StopRecorder
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; Start with Windows points at this copy (it may have moved).
  ReadRegStr $0 HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Klipp"
  ${If} $0 != ""
    WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Klipp" '"$INSTDIR\klipp-recorder.exe"'
  ${EndIf}
  ; Already set up (an update): recording carries on right away. A fresh
  ; install starts recording when setup finishes in the Klipp window.
  ${If} ${FileExists} "$APPDATA\Klipp\settings.toml"
    nsis_tauri_utils::RunAsUser "$INSTDIR\klipp-recorder.exe" ""
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro StopRecorder
  ; A real uninstall (not an update) also removes what Klipp set up outside
  ; its folder: Start with Windows and the CS2 integration file. Clips and
  ; settings stay.
  ${If} $UpdateMode <> 1
    nsExec::Exec '"$INSTDIR\klipp-recorder.exe" --cleanup'
    Pop $0
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; Someone uninstalled Klipp themselves (not an update, not a silent or
  ; scripted uninstall): open the short "what didn't work?" page.
  ${If} $UpdateMode <> 1
  ${AndIf} $PassiveMode <> 1
  ${AndIfNot} ${Silent}
    ExecShell "open" "https://klipp.dev/uninstall?v=${VERSION}"
  ${EndIf}
!macroend
