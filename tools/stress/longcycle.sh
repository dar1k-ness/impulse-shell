#!/bin/bash
# longcycle.sh ROUNDS: every panel open/close once per round, RSS/fd/threads/live after each round
R=${1:-40}; . ~/henv
P="sidebarRight sidebarLeft session cheatsheet mediaControls osk overlay wallpaperSelector search"
pid=$( (pidof rusty-impulse || pidof qs) | awk '{print $1}')
# The shell must run with RI_DEBUG=dump; the call replies with the paths it wrote.
st() { local live=$(rusty-impulse -c rusty-impulse ipc call ri-debug dump | grep /ri-live.txt); echo "$1 $(date +%s) rss=$(awk '/VmRSS/{print $2}' /proc/$pid/status) anon=$(awk '/RssAnon/{print $2}' /proc/$pid/status) fd=$(ls /proc/$pid/fd | wc -l) thr=$(ls /proc/$pid/task | wc -l) $(head -1 "$live") crosshair_err=$(grep -c CrosshairContent ~/rusty-impulse.log)"; }
st r0
for r in $(seq 1 $R); do
  for p in $P; do rusty-impulse -c rusty-impulse ipc call $p toggle >/dev/null 2>&1; sleep 1.2; rusty-impulse -c rusty-impulse ipc call $p toggle >/dev/null 2>&1; sleep 0.8; done
  [ -d /proc/$pid ] || { echo CRASH round $r; exit 1; }
  st r$r
done
echo LC_DONE
