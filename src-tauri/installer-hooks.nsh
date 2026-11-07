; Remove the video / PDF engines DocShift unpacked into the user's local app data on first run.
!macro NSIS_HOOK_POSTUNINSTALL
  RMDir /r "$LOCALAPPDATA\DocShift"
!macroend
