#!/bin/bash
# vfperf.sh ORIG|NATIVE OUT [N] [K]: vflood.sh scenario; perf record (cpu-clock, fp) of the shell over the flood window
export XDG_RUNTIME_DIR=/run/user/1000 WAYLAND_DISPLAY=wayland-1 HYPRLAND_INSTANCE_SIGNATURE=$(ls -t /run/user/1000/hypr | head -1)
export DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus
export LANG=en_US.UTF-8
M=$1; O=$2; N=${3:-50}; K=${4:-300}; mkdir -p $O
# The original, which ORIG runs: `bench/vm.sh sync` unpacks it here (tools/parity/orig.sh). The
# `impulse-shell` config is the project's own tree, which Quickshell cannot load.
T=${ORIG_TREE:-$HOME/parity-original}
# SIGTERM every shell process and return when they have exited (SIGKILL after 10 s without an exit)
stop_shell() { local n='qs|quickshell|impulse-shell'; pkill -x "$n" || return 0
  timeout 10 pidwait -x "$n"; [ $? != 124 ] || { pkill -9 -x "$n"; pidwait -x "$n"; }; return 0; }
stop_shell
python3 - "$N" <<'PY'
import json,sys
n=int(sys.argv[1])
d=[{"notificationId":i,"actions":[],"appIcon":"","appName":"histapp%d"%(i%4),"body":"history body %d"%i,"image":"","summary":"history %d"%i,"time":1793000000000+i*1000,"urgency":"1"} for i in range(1,n+1)]
json.dump(d,open("/home/dar1k/.cache/quickshell/notifications/notifications.json","w"),indent=2)
PY
# The native shell reads its history under its own name for the directory.
mkdir -p ~/.cache/rustshell/notifications && cp ~/.cache/quickshell/notifications/notifications.json ~/.cache/rustshell/notifications/
if [ "$M" = ORIG ]; then setsid quickshell ${QSARGS} -p "$T" > $O/shell.log 2>&1 < /dev/null &
else setsid ${BIN:-$HOME/bin/impulse-shell} ~/.config/rustshell/impulse-shell > $O/shell.log 2>&1 < /dev/null & fi
[ "$M" = ORIG ] && { S='quickshell|qs'; C="-p $T"; Q=qs; } || { S=impulse-shell; C="-c impulse-shell"; Q=${BIN:-$HOME/bin/impulse-shell}; }
sleep ${WAIT:-40}; P=$(pgrep -x "$S" | head -1); [ -n "$P" ] || { echo "$M: no $S process, see $O/shell.log" >&2; exit 1; }
t(){ awk '{print $14+$15}' /proc/$P/stat; }
ipc(){ local s=$(date +%s%N); timeout 300 $Q $C ipc call cliphistService update >/dev/null 2>&1; echo $(( ($(date +%s%N)-s)/1000000 )); }
echo "idle ipc: $(ipc) ms"
rm -f /tmp/stopperf; [ -n "$PERF" ] && { perf record -q -e cpu-clock -F ${FREQ:-2000} --call-graph fp -p $P -o $O/perf.data -- bash -c "while [ ! -e /tmp/stopperf ]; do sleep 0.1; done" > $O/perf.log 2>&1 & PP=$!; sleep 1; }
[ -n "$PRE" ] && eval "$PRE"
a=$(t); s=$(date +%s%N)
pids=(); for i in $(seq 1 $K); do notify-send -a flood$((i%3)) "flood $i" "flood body $i" & pids+=($!); done; wait "${pids[@]}"
sent=$(( ($(date +%s%N)-s)/1000000 ))
mx=0; for j in $(seq 1 ${POLL:-40}); do l=$(ipc); [ $l -gt $mx ] && mx=$l; sleep 0.3; done; b=$(t)
[ -n "$POST" ] && eval "$POST"
[ -n "$PERF" ] && { touch /tmp/stopperf; wait $PP; }
echo "$M N=$N K=$K perf=${PERF:-0}: sent ${sent} ms, max ipc ${mx} ms, window $(( ($(date +%s%N)-s)/1000000 )) ms, cpu $((b-a)) ticks, alive $(kill -0 $P && echo 1)" | tee $O/res.txt
for th in /proc/$P/task/*; do echo "$(cat $th/comm) $(awk '{print $14+$15}' $th/stat)"; done > $O/threads.txt
