#!/bin/bash
# per-panel open/close RSS growth: panelleak.sh orig|native [cycles]
MODE=$1; N=${2:-40}; D=${D:-0.1}
export RUN=~/stress/panelleak-$MODE-$(date +%m%d-%H%M); mkdir -p $RUN; . ~/h/common.sh
LOG=$RUN/shell.log; start_shell_proc $MODE $LOG; sleep 15
anon() { awk '/RssAnon/{print int($2/1024)}' /proc/$(spid)/status; }
cyc() { # target
  case $1 in
    search|workspaces|clipboard) local a=${1/search/toggle}; a=${a/workspaces/workspacesToggle}; a=${a/clipboard/clipboardToggle}
      $QS ipc call search $a >/dev/null; sleep $D; $QS ipc call search close >/dev/null ;;
    g:*) hglobal ${1#g:}; sleep $D; hglobal ${1#g:} ;;
    *) $QS ipc call $1 toggle >/dev/null 2>&1; sleep $D; $QS ipc call $1 toggle >/dev/null 2>&1 ;;
  esac; sleep $D
}
echo "| target | anon MB before | after $N cycles | delta | errors |" | tee $RUN/table.md
echo "|---|---:|---:|---:|---:|" | tee -a $RUN/table.md
for t in sidebarRight sidebarLeft session cheatsheet mediaControls osk overlay wallpaperSelector search workspaces clipboard g:overviewEmojiToggle; do
  for i in 1 2 3; do cyc $t; done; sleep 3
  a=$(anon); e=$(errcount)
  for i in $(seq $N); do cyc $t; done; sleep 5
  echo "| $t | $a | $(anon) | $(( $(anon)-a )) | $(( $(errcount)-e )) |" | tee -a $RUN/table.md
done
stop_shell
