; Setup steps for Klipp's two programs (Tauri runs these around installing
; and uninstalling; see installer.nsi).
;
; Installing forces its way. Klipp is closed without asking, the
; window first (it would start the recorder again) and then the recorder;
; the install waits until Windows lets go of both programs, closes whatever
; still holds one (Restart Manager), and as a last resort moves a program
; that's still running aside (klipp.exe.old, removed at the recorder's next
; start). A quiet update would otherwise skip a file it can't write and
; leave the old program under the new version number.
;
; Every step goes to %LOCALAPPDATA%\Klipp\install.log, which the recorder
; copies into its own log on its next start (its installlog). Lines are
; ASCII and name files, never paths: the log reaches the developer.

; One line in install.log, with the local time (stack: the text).
Function KlippLog
  Exch $R9
  Push $R0
  Push $R1
  Push $R2
  Push $R3
  Push $R4
  Push $R5
  Push $R6
  Push $R8
  ${GetTime} "" "L" $R0 $R1 $R2 $R3 $R4 $R5 $R6
  CreateDirectory "$LOCALAPPDATA\Klipp"
  ClearErrors
  FileOpen $R8 "$LOCALAPPDATA\Klipp\install.log" a
  ${IfNot} ${Errors}
    FileSeek $R8 0 END
    FileWrite $R8 "$R2-$R1-$R0 $R4:$R5:$R6 installer: $R9$\r$\n"
    FileClose $R8
  ${EndIf}
  Pop $R8
  Pop $R6
  Pop $R5
  Pop $R4
  Pop $R3
  Pop $R2
  Pop $R1
  Pop $R0
  Pop $R9
FunctionEnd

!macro KlippLog text
  Push "${text}"
  Call KlippLog
!macroend

; Closes a program by name, if it runs (stack: its file name).
Function KlippKill
  Exch $R9
  Push $R0
  nsExec::Exec 'taskkill /IM $R9 /F'
  Pop $R0
  ; taskkill: 0 closed it, 128 none was running.
  ${If} $R0 = 0
    !insertmacro KlippLog "closed $R9"
  ${ElseIf} $R0 != 128
    !insertmacro KlippLog "taskkill $R9 answered $R0"
  ${EndIf}
  Pop $R0
  Pop $R9
FunctionEnd

; Waits until a program in the install folder can be replaced (stack: its
; file name): up to 10 s, then Restart Manager closes whatever holds it,
; 5 s more, then it's moved aside.
Function KlippFree
  Exch $R9
  Push $R0
  Push $R1
  ${If} ${FileExists} "$INSTDIR\$R9"
    StrCpy $R1 0
    wait:
      ClearErrors
      FileOpen $R0 "$INSTDIR\$R9" a
      ${IfNot} ${Errors}
        FileClose $R0
        ${If} $R1 > 0
          IntOp $R0 $R1 * 250
          !insertmacro KlippLog "$R9 is free after $R0 ms"
        ${EndIf}
        Goto done
      ${EndIf}
      IntOp $R1 $R1 + 1
      ${If} $R1 = 40
        !insertmacro RestartManager_ShutdownFile "$INSTDIR\$R9" $R0
        !insertmacro KlippLog "$R9 still in use after 10 s; Restart Manager closed what held it (result $R0)"
      ${EndIf}
      ${If} $R1 < 60
        Sleep 250
        Goto wait
      ${EndIf}
    ; A running program can't be overwritten, but it can be renamed.
    Delete "$INSTDIR\$R9.old"
    ClearErrors
    Rename "$INSTDIR\$R9" "$INSTDIR\$R9.old"
    ${If} ${Errors}
      !insertmacro KlippLog "$R9 still in use after 15 s and can't be moved aside"
    ${Else}
      !insertmacro KlippLog "$R9 still in use after 15 s; moved it aside to $R9.old"
    ${EndIf}
  ${EndIf}
  done:
  Pop $R1
  Pop $R0
  Pop $R9
FunctionEnd

!macro StopRecorder
  nsExec::Exec 'taskkill /IM klipp-recorder.exe /F'
  Pop $0
  ; Windows releases the file a moment after the process is gone.
  Sleep 800
!macroend

!macro NSIS_HOOK_PREINSTALL
  Push $R0
  Push $R1
  ReadRegStr $R0 SHCTX "${UNINSTKEY}" "DisplayVersion"
  ${If} $R0 == ""
    StrCpy $R0 "nothing"
  ${EndIf}
  ${If} $UpdateMode = 1
    StrCpy $R1 "update"
  ${Else}
    StrCpy $R1 "install"
  ${EndIf}
  ${If} ${Silent}
    StrCpy $R1 "$R1, quiet"
  ${ElseIf} $PassiveMode = 1
    StrCpy $R1 "$R1, passive"
  ${Else}
    StrCpy $R1 "$R1, with its window"
  ${EndIf}
  !insertmacro KlippLog "Klipp ${VERSION} starting ($R1) over $R0"
  Pop $R1
  Pop $R0
  ; Leftovers of an install that had to move programs aside.
  Delete "$INSTDIR\klipp.exe.old"
  Delete "$INSTDIR\klipp-recorder.exe.old"
  Push "klipp.exe"
  Call KlippKill
  Push "klipp-recorder.exe"
  Call KlippKill
  Push "klipp.exe"
  Call KlippFree
  Push "klipp-recorder.exe"
  Call KlippFree
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
    ; Logged first: the recorder takes install.log in as it starts.
    !insertmacro KlippLog "Klipp ${VERSION} installed; starting the recorder"
    nsis_tauri_utils::RunAsUser "$INSTDIR\klipp-recorder.exe" ""
  ${Else}
    !insertmacro KlippLog "Klipp ${VERSION} installed; setup starts the recorder"
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
