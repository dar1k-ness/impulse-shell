#!/bin/bash
export XDG_RUNTIME_DIR=/run/user/1000 WAYLAND_DISPLAY=wayland-1 HYPRLAND_INSTANCE_SIGNATURE=$(ls -t /run/user/1000/hypr | head -1)
export DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus
# SIGTERM every shell process and return when they have exited (SIGKILL after 10 s without an exit)
stop_shell() { local n='qs|quickshell|rusty-impulse'; pkill -x "$n" || return 0
  timeout 10 pidwait -x "$n"; [ $? != 124 ] || { pkill -9 -x "$n"; pidwait -x "$n"; }; return 0; }
pkill hyprlock; stop_shell
cd ~/rusty-impulse; RI_LOG=ri::dprof,ri::hprof,ri::stats setsid target/release/rusty-impulse -c rusty-impulse > /tmp/vb.log 2>&1 < /dev/null & sleep 20
P=$(pgrep -x rusty-impulse | head -1); [ -n "$P" ] || { echo "no rusty-impulse process, see /tmp/vb.log" >&2; exit 1; }; L=$(wc -l < /tmp/vb.log)
a=$(awk '{print $14+$15}' /proc/$P/stat)
for i in $(seq 1 40); do notify-send -a app$((i%4)) "burst $i" "body text number $i with some words"; sleep 0.05; done; sleep 8
b=$(awk '{print $14+$15}' /proc/$P/stat); echo "burst $((b-a))"
sed -n "${L},\$p" /tmp/vb.log > /tmp/vb_seg.txt
echo "== handlers (total us, n, handler, type)"
grep ^hprof /tmp/vb_seg.txt | awk '{n[$4" "$5" "$6]++; s[$4" "$5" "$6]+=$2} END{for(k in n) print s[k]"us", n[k], k}' | sort -rn | head -12
echo "== draws by window (n, layout, paint, gpu)"
grep "^draw" /tmp/vb_seg.txt | awk '{k=$2" "$3; n[k]++; gsub("ms","",$5); gsub("ms","",$7); gsub("ms","",$9); l[k]+=$5; p[k]+=$7; g[k]+=$9} END{for(k in n) print k, n[k], l[k], p[k], g[k]}'
echo "== loopstat sum"
grep ^loopstat /tmp/vb_seg.txt | awk '{for(i=2;i<=NF;i+=2) s[$i]+=$(i+1)} END{for(k in s) printf "%s %s  ", k, s[k]; print ""}'
grep dprof /tmp/vb_seg.txt | tail -2; grep dprof /tmp/vb.log | head -1
grep -E "local settle|relayout hidden|slow t=" /tmp/vb_seg.txt | sed 's/[0-9]\+/N/g' | sort | uniq -c | sort -rn | head -6
