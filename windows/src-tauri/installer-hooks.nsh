; A manual reinstall resets only onboarding. The built-in updater passes /UPDATE.
!macro NSIS_HOOK_POSTINSTALL
  Push $0
  Push $1
  ${If} $UpdateMode = 1
    IfFileExists "$INSTDIR\tailsync-install-id" tailsync_install_id_done
  ${EndIf}
  System::Call 'ole32::CoCreateGuid(g .r0) i .r1'
  ${If} $1 <> 0
    Abort "Could not create the TailSync installation identity."
  ${EndIf}
  ClearErrors
  FileOpen $1 "$INSTDIR\tailsync-install-id" w
  IfErrors tailsync_install_id_error
  FileWrite $1 "$0$\r$\n"
  FileClose $1
  IfErrors tailsync_install_id_error tailsync_install_id_done
tailsync_install_id_error:
  Abort "Could not save the TailSync installation identity."
tailsync_install_id_done:
  Pop $1
  Pop $0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    DeleteRegKey HKCU "Software\Classes\AppUserModelId\${BUNDLEID}"
    Delete "$INSTDIR\tailsync-install-id"
  ${EndIf}
!macroend
