#!/bin/bash
# usage: vswipe.sh ORIG|NATIVE [N]  -> CPU of discarding 5 notifications with N in history, sidebar closed / open
export XDG_RUNTIME_DIR=/run/user/1000 WAYLAND_DISPLAY=wayland-1 HYPRLAND_INSTANCE_SIGNATURE=$(ls -t /run/user/1000/hypr | head -1)
export DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus
N=${2:-300}
# The original, which ORIG runs: `bench/vm.sh sync` unpacks it here (tools/parity/orig.sh). The
# `rusty-impulse` config is the project's own tree, which Quickshell cannot load.
T=${ORIG_TREE:-$HOME/parity-original}
# SIGTERM every shell process and return when they have exited (SIGKILL after 10 s without an exit)
stop_shell() { local n='qs|quickshell|rusty-impulse'; pkill -x "$n" || return 0
  timeout 10 pidwait -x "$n"; [ $? != 124 ] || { pkill -9 -x "$n"; pidwait -x "$n"; }; return 0; }
pkill hyprlock; pkill hypridle; stop_shell
python3 - "$N" <<'PY'
import json,sys
n=int(sys.argv[1])
d=[{"notificationId":i,"actions":[],"appIcon":"","appName":"histapp%d"%(i%int(__import__("os").environ.get("NG","4"))),"body":"history body %d"%i,"image":"","summary":"history %d"%i,"time":1793000000000+i*1000,"urgency":"1"} for i in range(1,n+1)]
json.dump(d,open("/home/dar1k/.cache/quickshell/notifications/notifications.json","w"),indent=2)
PY
# The native shell reads its history under its own name for the directory.
mkdir -p ~/.cache/rustshell/notifications && cp ~/.cache/quickshell/notifications/notifications.json ~/.cache/rustshell/notifications/
G(){ hyprctl eval "return hl.dispatch(hl.dsp.global(\"$NS:$1\"))" >/dev/null; }
hyprctl monitors | grep -o "dpmsStatus: [01]"
if [ "$1" = ORIG ]; then setsid quickshell -p "$T" > /tmp/vs.log 2>&1 < /dev/null &
else cd ~/rusty-impulse; setsid ${BIN:-target/release/rusty-impulse} -c rusty-impulse > /tmp/vs.log 2>&1 < /dev/null & fi
[ "$1" = ORIG ] && { S='quickshell|qs'; NS=quickshell; } || { S=rusty-impulse; NS=rustshell; }
sleep 25; P=$(pgrep -x "$S" | head -1); [ -n "$P" ] || { echo "$1: no $S process, see /tmp/vs.log" >&2; exit 1; }
t(){ awk '{print $14+$15}' /proc/$P/stat; }
th(){ for f in /proc/$P/task/*/stat; do sed -E 's/^[0-9]+ \(([^)]*)\) /\1 /; s/ /_/' $f 2>/dev/null | awk '{print $1, $12+$13}'; done | awk '{sub(/_.*/,"",$1); gsub(/[0-9-]+$/,"",$1); a[$1]+=$2} END{for(k in a) print k, a[k]}' | sort; }
ids=(); for i in $(seq 1 10); do ids+=($(notify-send -p -a live$((i%3)) "live $i" "live body $i")); sleep 0.3; done
sleep 10
close(){ gdbus call --session --dest org.freedesktop.Notifications --object-path /org/freedesktop/Notifications --method org.freedesktop.Notifications.CloseNotification $1 >/dev/null; }
a=$(t); s=$(date +%s%N); for id in "${ids[@]:0:5}"; do close $id; sleep 1; done; b=$(t); echo "$1 N=$N closed-sidebar 5 discards: $((b-a)) ticks"
G sidebarRightToggle; sleep 3; [ -n "$SHOT" ] && grim /tmp/$SHOT.png; [ -n "$PROF" ] && { perf record -F ${PF:-999} --call-graph ${CG:-fp} -p $P -o /tmp/swipe.perf >/dev/null 2>&1 & PP=$!; } ; [ -n "$PSTAT" ] && { perf stat -e instructions -t $P -o /tmp/pstat.txt & PS=$!; }
wc -l < /tmp/vs.log > /tmp/vs_m0; th > /tmp/th0; a=$(t); for id in "${ids[@]:5:5}"; do close $id; sleep 1; done; b=$(t); wc -l < /tmp/vs.log > /tmp/vs_m1; th > /tmp/th1; join /tmp/th0 /tmp/th1 | awk '{d=$3-$2; if(d>0) printf "%s=%d ", $1, d} END{print ""}' >&2; [ -n "$PS" ] && { kill -INT $PS; wait $PS; }; [ -n "$PP" ] && { kill -INT $PP; wait $PP; }; echo "$1 N=$N open-sidebar 5 discards: $((b-a)) ticks"
sleep 2; a=$(t); sleep 5; b=$(t); echo "$1 after 5s idle: $((b-a)) ticks, alive $(pgrep -x "$S" | wc -l), rss $(awk '/VmRSS/{print $2}' /proc/$P/status)"
G sidebarRightToggle
