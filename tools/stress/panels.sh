#!/bin/bash
# rapid open/close of every panel via IPC + global shortcuts
RUN=$1; R=${2:-50}; D=${3:-0.08}; . ~/h/common.sh
snap panels.before
T="sidebarRight sidebarLeft session cheatsheet mediaControls osk overlay wallpaperSelector"
for r in $(seq 1 $R); do
  for t in $T; do $QS ipc call $t toggle >/dev/null 2>&1; sleep $D; $QS ipc call $t toggle >/dev/null 2>&1; sleep $D; done
  $QS ipc call search toggle >/dev/null; sleep $D; $QS ipc call search close >/dev/null; sleep $D
  $QS ipc call search workspacesToggle >/dev/null; sleep $D; $QS ipc call search workspacesToggle >/dev/null; sleep $D
  $QS ipc call search clipboardToggle >/dev/null; sleep $D; $QS ipc call search close >/dev/null; sleep $D
  $QS ipc call osdVolume trigger >/dev/null; $QS ipc call osd trigger >/dev/null
  $QS ipc call bar toggle >/dev/null; sleep $D; $QS ipc call bar open >/dev/null; sleep $D
  for g in sidebarRightToggle sidebarLeftToggle overviewWorkspacesToggle overviewEmojiToggle cheatsheetToggle sessionToggle; do hglobal $g; sleep $D; hglobal $g; sleep $D; done
  [ $((r%10)) = 0 ] && snap panels.r$r
done
for t in $T; do $QS ipc call $t close >/dev/null 2>&1; done; $QS ipc call search close >/dev/null
sleep 10; snap panels.after
