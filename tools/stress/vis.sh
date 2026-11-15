#!/bin/bash
# vis.sh OUTDIR: (arch-pc, from ~/h; needs ydotool, grim, python3) visual checks on the running native: overview preview icons, clock popup, tray menu, sidebar,
# Settings About (switch time + logo), waffle Creativity folder. Restores config.json and closes what it opened.
. ~/henv; . ~/h/common.sh; V=$1; mkdir -p $V; export YDOTOOL_SOCKET=/run/user/1000/.ydotool_socket
Q=${BIN:-$HOME/.local/bin/impulse-shell}; C=~/.config/impulse-shell/config.json; cp -p $C $V/config.bak
cat > $V/traypos.py <<'PYEOF'
#!/usr/bin/env python3
"""traypos.py: print "X Y" of the right-most tray icon on the bar, or nothing.

Reads a binary PPM of the bar's right part (grim -t ppm -g "X0,0 WxH") on stdin;
argv[1] = X0. The tray sits just left of the bar's 3-px "unpinned" dot; the
indicator group (net/volume/battery) is right of the dot.
"""
import sys
from collections import Counter


def main():
    x0 = int(sys.argv[1]) if len(sys.argv) > 1 else 0
    d = sys.stdin.buffer.read()
    f = []
    i = 0
    while len(f) < 4:  # P6 W H MAX
        while d[i:i + 1].isspace():
            i += 1
        if d[i:i + 1] == b"#":
            i = d.index(b"\n", i)
            continue
        j = i
        while not d[j:j + 1].isspace():
            j += 1
        f.append(d[i:j])
        i = j
    w, h = int(f[1]), int(f[2])
    px = d[i + 1:]

    def at(x, y):
        k = (y * w + x) * 3
        return px[k], px[k + 1], px[k + 2]

    ys = range(4, min(h, 36))
    bg = Counter(at(x, y) for y in ys for x in range(0, w, 3)).most_common(1)[0][0]
    fg = [[sum(abs(a - b) for a, b in zip(at(x, y), bg)) > 30 for y in ys] for x in range(w)]
    runs, x = [], 0
    while x < w - 15:  # skip the rounded right corner
        if any(fg[x]):
            s = x
            while x < w and any(fg[x]):
                x += 1
            rows = [k for c in range(s, x) for k, v in enumerate(fg[c]) if v]
            runs.append([s, x - 1, min(rows), max(rows)])
        else:
            x += 1
    merged = []
    for r in runs:  # an icon can split into column runs a few px apart
        if merged and r[0] - merged[-1][1] <= 4:
            m = merged[-1]
            merged[-1] = [m[0], r[1], min(m[2], r[2]), max(m[3], r[3])]
        else:
            merged.append(r)
    for k in range(len(merged) - 1, 0, -1):
        s, e, t, b = merged[k]
        if e - s <= 6 and b - t <= 6:  # the dot
            s2, e2, t2, b2 = merged[k - 1]
            if e2 - s2 >= 8 and s - e2 <= 30:
                print(x0 + (s2 + e2) // 2, 4 + (t2 + b2) // 2)
            return


main()
PYEOF
mv_() { hyprctl dispatch "hl.dsp.cursor.move({x=$1,y=$2})" >/dev/null; sleep 0.3; ydotool mousemove -x 1 -y 0 >/dev/null; }
click() { mv_ $1 $2; ydotool click ${3:-0xC0} >/dev/null; }
k0=$(pgrep -x kitty | tr '\n' ' ')
hyprctl dispatch 'hl.dsp.exec_cmd("kitty")' >/dev/null; sleep 3
hglobal overviewWorkspacesToggle; sleep 3; grim $V/overview.png; hglobal overviewWorkspacesToggle; sleep 1
for p in $(pgrep -x kitty); do case " $k0 " in *" $p "*) ;; *) kill $p;; esac; done
mv_ 1230 19; sleep 2; grim $V/clock_popup.png; mv_ 960 600; sleep 1
# tray icon: found on screen (left of the bar's unpinned dot); vis7's fixed 1789,20 hit empty bar
T=$(grim -t ppm -g "1200,0 720x40" - | python3 $V/traypos.py 1200); echo "vis.tray at=${T:-none}" >> $V/vis.txt
[ -n "$T" ] && click $T 0xC1; sleep 2; grim $V/tray_menu.png; ydotool key 1:1 1:0 >/dev/null; sleep 1
$Q -c impulse-shell ipc call sidebarRight open >/dev/null; sleep 2; grim $V/sidebar.png; $Q -c impulse-shell ipc call sidebarRight close >/dev/null; sleep 1
hyprctl dispatch "hl.dsp.exec_cmd(\"setsid $Q -p $HOME/.config/rustshell/impulse-shell/settings.qml\")" >/dev/null; sleep 6
SP=$(for p in $(pgrep -f "(bin/qs|impulse-shell) -p .*settings.qml"); do [ "$(readlink /proc/$p/exe)" = $HOME/.local/bin/impulse-shell ] && echo $p; done | head -1)
mv_ 507 779; c0=$(cputicks $SP); t0=$(date +%s%N); ydotool click 0xC0 >/dev/null; last=; lt=0
while :; do m=$(grim -g "760,240 600x120" - | md5sum | cut -c1-8); ms=$(( ($(date +%s%N)-t0)/1000000 ))
  [ "$m" != "$last" ] && { last=$m; lt=$ms; }; [ $((ms-lt)) -gt 3000 ] || [ $ms -gt 30000 ] && break; sleep 0.1; done
echo "$(date +%s) vis.about settle_ms=$lt cpu=$(( $(cputicks $SP)-c0 ))" | tee -a $V/vis.txt; grim $V/settings_about.png; kill $SP
$Q -c impulse-shell ipc call panelFamily cycle >/dev/null; sleep 8; grim $V/waffle_bar.png
# Creativity: click its label button (opens the folder like Qt); vis7's 1085,647 hit the gap under the tile
click 872 1055; sleep 3; grim $V/waffle_start.png; m0=$(grim -g "740,400 440x240" - | md5sum)
click 1052 664; sleep 2; grim $V/waffle_creativity.png
[ "$(grim -g "740,400 440x240" - | md5sum)" != "$m0" ] && echo "vis.creativity opened" >> $V/vis.txt || echo "vis.creativity NOT opened" >> $V/vis.txt
$Q -c impulse-shell ipc call search close >/dev/null; sleep 1; $Q -c impulse-shell ipc call panelFamily cycle >/dev/null; sleep 6
cmp -s $V/config.bak $C && echo "vis.config same" >> $V/vis.txt || { cp -p $C $V/config.post; cp -p $V/config.bak $C; echo "vis.config restored" >> $V/vis.txt; }
grim $V/back_rusty.png; echo VISDONE >> $V/vis.txt
