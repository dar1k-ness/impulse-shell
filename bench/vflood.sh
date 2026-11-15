#!/bin/bash
# usage: vflood.sh ORIG|NATIVE [N] [K] -> wall/CPU for K quick notifications with N in history, then IPC latency
export XDG_RUNTIME_DIR=/run/user/1000 WAYLAND_DISPLAY=wayland-1 HYPRLAND_INSTANCE_SIGNATURE=$(ls -t /run/user/1000/hypr | head -1)
export DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus
N=${2:-9400}; K=${3:-20}
# The original, which ORIG runs: `bench/vm.sh sync` unpacks it here (tools/parity/orig.sh). The
# `impulse-shell` config is the project's own tree, which Quickshell cannot load.
T=${ORIG_TREE:-$HOME/parity-original}
# SIGTERM every shell process and return when they have exited (SIGKILL after 10 s without an exit)
stop_shell() { local n='qs|quickshell|impulse-shell'; pkill -x "$n" || return 0
  timeout 10 pidwait -x "$n"; [ $? != 124 ] || { pkill -9 -x "$n"; pidwait -x "$n"; }; return 0; }
pkill hyprlock; pkill hypridle; stop_shell
python3 - "$N" <<'PY'
import json,sys
n=int(sys.argv[1])
d=[{"notificationId":i,"actions":[],"appIcon":"","appName":"histapp%d"%(i%4),"body":"history body %d"%i,"image":"","summary":"history %d"%i,"time":1793000000000+i*1000,"urgency":"1"} for i in range(1,n+1)]
json.dump(d,open("/home/dar1k/.cache/quickshell/notifications/notifications.json","w"),indent=2)
PY
# The native shell reads its history under its own name for the directory.
mkdir -p ~/.cache/impulse-shell/notifications && cp ~/.cache/quickshell/notifications/notifications.json ~/.cache/impulse-shell/notifications/
if [ "$1" = ORIG ]; then setsid quickshell -p "$T" > /tmp/vf.log 2>&1 < /dev/null &
else cd ~/impulse-shell; setsid ${BIN:-target/release/impulse-shell} -c impulse-shell > /tmp/vf.log 2>&1 < /dev/null & fi
[ "$1" = ORIG ] && { S='quickshell|qs'; C="-p $T"; Q=qs; } || { S=impulse-shell; C="-c impulse-shell"; Q=${BIN:-target/release/impulse-shell}; }
sleep ${WAIT:-40}; P=$(pgrep -x "$S" | head -1); [ -n "$P" ] || { echo "$1: no $S process, see /tmp/vf.log" >&2; exit 1; }
t(){ awk '{print $14+$15}' /proc/$P/stat; }
ipc(){ local s=$(date +%s%N); timeout 300 $Q $C ipc call cliphistService update >/dev/null 2>&1; echo $(( ($(date +%s%N)-s)/1000000 )); }
echo "idle ipc: $(ipc) ms"
a=$(t); s=$(date +%s%N)
pids=(); for i in $(seq 1 $K); do notify-send -a flood$((i%3)) "flood $i" "flood body $i" & pids+=($!); done; wait "${pids[@]}"
sent=$(( ($(date +%s%N)-s)/1000000 ))
mx=0; for j in $(seq 1 ${POLL:-40}); do l=$(ipc); [ $l -gt $mx ] && mx=$l; sleep 0.3; done; b=$(t)
echo "$1 N=$N K=$K: sent ${sent} ms, max ipc over window ${mx} ms, window $(( ($(date +%s%N)-s)/1000000 )) ms, cpu $((b-a)) ticks, alive $(kill -0 $P && echo 1)"
