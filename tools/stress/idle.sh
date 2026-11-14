#!/bin/bash
# idle.sh RUN [SECS]: long idle (bar only), then idle with the right sidebar and the overview open; CPU % per phase
RUN=$1; S=${2:-${IDLE:-600}}; . ~/h/common.sh
phase() { local p=$(spid) c0=$(cputicks $(spid)) t0=$(date +%s); sleep $2; local c1=$(cputicks $(spid)) t1=$(date +%s)
  echo "$(date +%s) idle.$1 secs=$((t1-t0)) cpu_pct=$(awk -v c=$((c1-c0)) -v t=$((t1-t0)) 'BEGIN{printf "%.2f", c/t}')" >> $RUN/events.log; snap idle.$1; }
snap idle.before
phase bar $S
$QS ipc call sidebarRight open >/dev/null; sleep 2; phase sidebarRight $((S/5)); $QS ipc call sidebarRight close >/dev/null
hglobal overviewWorkspacesToggle; sleep 2; phase overview $((S/5)); hglobal overviewWorkspacesToggle
sleep 5; snap idle.after
