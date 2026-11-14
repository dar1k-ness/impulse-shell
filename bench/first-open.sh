#!/bin/bash
# How long a panel takes to appear the first time after a fresh start of the shell, and the
# second time. Runs in the benchmark guest (bench/vm.sh) after `bench/vm.sh sync`.
#   bench/first-open.sh [OUT]  OUT (default ~/first-open/<date>) gets runs.tsv (one line per
#                              start: shell, run, first open ms, second open ms), summary.md,
#                              env.txt and per start the shell's log
#   RUNS=20            starts per shell; with two shells they alternate
#   SHELLS=rusty-impulse       or "rusty-impulse quickshell"
#   WAIT=180           seconds from the shell's exec to the first open
#   PANEL=sidebarRight the panel: its shortcuts are <PANEL>Open / <PANEL>Close, its layer
#                      namespace rustshell:<PANEL> (the original's: quickshell:<PANEL>)
#   SHELL_ENV=         environment for rusty-impulse, e.g. "RI_LOG=ri::stats,ri::frames"
#   RI_BIN=rusty-impulse        the rusty-impulse binary, e.g. another release under /usr/local/lib/rusty-impulse/releases
# One open is the time from just before `hyprctl dispatch` of the panel's global shortcut to
# Hyprland's `openlayer` event for the panel's layer (its first buffer is committed), read from
# Hyprland's event socket by a blocking read. A start is fresh: the shell is stopped and the original's
# state, cache and configuration are deleted, as in rss-cpu.sh.
set -euo pipefail

export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
export DBUS_SESSION_BUS_ADDRESS=${DBUS_SESSION_BUS_ADDRESS:-unix:path=$XDG_RUNTIME_DIR/bus}
export WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-wayland-1}
HYPRLAND_INSTANCE_SIGNATURE=${HYPRLAND_INSTANCE_SIGNATURE:-$(ls -t "$XDG_RUNTIME_DIR/hypr" | head -1)}
export HYPRLAND_INSTANCE_SIGNATURE
export LANG=C.UTF-8

self=$(realpath "$0")
repo=$(dirname "$(dirname "$self")")
orig=${ORIG_TREE:-$HOME/parity-original}
unit=ri-bench.scope

# The shell under test, exec'd by Hyprland inside the scope: `first-open.sh --exec SHELL LOG PIDFILE`.
if [ "${1:-}" = --exec ]; then
    echo $$ >"$4"
    case $2 in
    # SHELL_ENV comes through a file: the command goes through Hyprland's own quoting.
    rusty-impulse) exec env $(<"$3.env") "$(<"$3.bin")" -c rusty-impulse >"$3" 2>&1 </dev/null ;;
    quickshell) exec qs -p "$orig" >"$3" 2>&1 </dev/null ;;
    esac
fi

RUNS=${RUNS:-20}
SHELLS=${SHELLS:-rusty-impulse}
WAIT=${WAIT:-180}
PANEL=${PANEL:-sidebarRight}
out=${1:-$HOME/first-open/$(date +%Y%m%d-%H%M%S)}
walls=$HOME/Pictures/Wallpapers
cfg=$HOME/.config/rusty-impulse/config.json

die() {
    echo "first-open.sh: $*" >&2
    exit 1
}

ipc() {
    case $shell in
    rusty-impulse) timeout 10 rusty-impulse -c rusty-impulse ipc "$@" ;;
    quickshell) timeout 10 qs -p "$orig" ipc "$@" ;;
    esac
}

wait_for() {
    local end=$((SECONDS + $1))
    shift
    until "$@" >/dev/null 2>&1; do
        [ $SECONDS -lt $end ] || return 1
        sleep 0.2
    done
}

layer() { hyprctl layers -j | jq -e --arg n "$1" '[.. | .namespace? // empty] | (index("quickshell:" + $n) // index("rustshell:" + $n))' >/dev/null; }
no_layer() { ! layer "$1"; }

stop_shell() {
    local n='rusty-impulse|quickshell|qs' rc=0
    systemctl --user stop "$unit" 2>/dev/null || true
    if pkill -x "$n"; then
        timeout 20 pidwait -x "$n" || rc=$?
        [ $rc != 124 ] || die "a shell did not exit"
    fi
    wait_for 20 no_layer bar || die "a shell is still on screen"
}

reset_state() {
    local old; old=~/.config/$(printf %s-%s illogical impulse)
    rm -rf ~/.config/rusty-impulse ~/.local/state/quickshell ~/.cache/quickshell ~/.config/matugen \
        ~/.local/state/rustshell ~/.cache/rustshell "$old"
    mkdir -p ~/.config/rusty-impulse ~/.local/state/quickshell/user ~/.cache/quickshell
    # Each shell reads these under its own name for them.
    ln -s quickshell ~/.local/state/rustshell
    ln -s quickshell ~/.cache/rustshell
    ln -s rusty-impulse "$old"
    cp -r "$repo/crates/colorgen/tests/switchwall/fixture/home/.config/matugen" ~/.config/matugen
    jq -n --arg w "$walls/bench-a.jpg" '{background: {wallpaperPath: $w}}' >"$cfg"
    # Without it the original opens its welcome window.
    touch ~/.local/state/quickshell/user/first_run.txt
}

start_shell() {
    stop_shell
    reset_state
    rm -f "$run/pid"
    echo "${SHELL_ENV:-}" >"$run/shell.log.env"
    echo "${RI_BIN:-rusty-impulse}" >"$run/shell.log.bin"
    started=$SECONDS
    hyprctl dispatch "hl.dsp.exec_cmd(\"systemd-run --user --scope --quiet --collect --unit=$unit $self --exec $shell $run/shell.log $run/pid\")" >/dev/null
    wait_for 20 test -s "$run/pid" || die "$shell did not start"
    pid=$(cat "$run/pid")
    wait_for 120 ipc show || die "$shell does not answer IPC; see $run/shell.log"
    wait_for 30 layer bar || die "$shell has no bar"
    wait_for 30 layer background || die "$shell has no background"
}

# event_ms SHORTCUT EVENT: milliseconds from just before the dispatch of the global shortcut to
# Hyprland's EVENT. The event socket is connected before the dispatch, and read by a blocking
# read: nothing polls.
event_ms() {
    NS=$ns python3 - "$1" "$2" <<'PY'
import os, socket, subprocess, sys, time
shortcut, event = sys.argv[1], sys.argv[2].encode()
s = socket.socket(socket.AF_UNIX)
s.connect(f"{os.environ['XDG_RUNTIME_DIR']}/hypr/{os.environ['HYPRLAND_INSTANCE_SIGNATURE']}/.socket2.sock")
s.settimeout(30)
t = time.monotonic_ns()
subprocess.run(["hyprctl", "dispatch", f'hl.dsp.global("{os.environ["NS"]}:{shortcut}")'],
               stdout=subprocess.DEVNULL, check=True)
buf = b""
while True:
    d = s.recv(4096)
    if not d:
        sys.exit("Hyprland closed its event socket")
    e = time.monotonic_ns()
    buf += d
    *lines, buf = buf.split(b"\n")
    if event in lines:
        print(f"{(e - t) / 1e6:.3f}")
        break
PY
}

# open_close: the open's milliseconds; the panel is closed again and has left the screen.
open_close() {
    # What the shell under test puts before the names of its shortcuts and of its layers.
    local ns=rustshell
    [ "$shell" != quickshell ] || ns=quickshell
    event_ms "${PANEL}Open" "openlayer>>$ns:$PANEL" || die "$shell: $PANEL did not open"
    sleep 3
    event_ms "${PANEL}Close" "closelayer>>$ns:$PANEL" >/dev/null || die "$shell: $PANEL did not close"
    sleep 3
}

one_run() {
    local first second
    run=$out/$shell-$n
    mkdir -p "$run"
    start_shell
    sleep $((started + WAIT - SECONDS))
    [ -d "/proc/$pid" ] || die "$shell died; see $run/shell.log"
    first=$(open_close)
    second=$(open_close)
    printf '%s\t%s\t%s\t%s\n' "$shell" "$n" "$first" "$second" | tee -a "$out/runs.tsv"
    stop_shell
}

command -v rusty-impulse >/dev/null || die "rusty-impulse is not installed: run bench/vm.sh sync"
mkdir -p "$out" "$walls"
out=$(realpath "$out")
[ -f "$walls/bench-a.jpg" ] || magick -size 3840x2160 -seed 7 plasma:'#3b5b8c-#c08a5a' -blur 0x8 -quality 92 "$walls/bench-a.jpg"
{
    echo "commit: $(cat "$repo/.commit" 2>/dev/null || echo unknown)"
    pacman -Q quickshell qt6-base hyprland mesa linux
    echo "cpus: $(nproc), $(awk -F': ' '/model name/ { print $2; exit }' /proc/cpuinfo)"
    echo "renderer: $(grep -m1 -oP '(?<= Renderer: ).*' "$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/hyprland.log")"
    echo "runs=$RUNS shells=$SHELLS wait=$WAIT panel=$PANEL shell_env=${SHELL_ENV:-}"
} >"$out/env.txt"
printf 'shell\trun\tfirst_open_ms\tsecond_open_ms\n' >"$out/runs.tsv"

for n in $(seq 1 "$RUNS"); do
    order=$SHELLS
    # Two shells swap order every run.
    [ $((n % 2)) = 1 ] || order=$(echo $SHELLS | tr ' ' '\n' | tac | tr '\n' ' ')
    for shell in $order; do one_run; done
done
rows=$(($(wc -l <"$out/runs.tsv") - 1))
want=$((RUNS * $(echo $SHELLS | wc -w)))
[ "$rows" = "$want" ] || die "$rows of $want starts measured; $out is incomplete"

gawk -F'\t' '
    NR > 1 { n[$1]++; a[$1, 3, n[$1]] = $3; a[$1, 4, n[$1]] = $4 }
    function stat(s, c,   v, i, m) {
        for (i = 1; i <= n[s]; i++) v[i] = a[s, c, i] + 0
        m = asort(v)
        return sprintf("%.0f | %.0f | %.0f", v[1], m % 2 ? v[(m + 1) / 2] : (v[m / 2] + v[m / 2 + 1]) / 2, v[m])
    }
    END {
        print "| Shell | Open | Starts | Min, ms | Median, ms | Max, ms |"
        print "|---|---|---|---|---|---|"
        for (s in n) {
            printf "| %s | first | %d | %s |\n", s, n[s], stat(s, 3)
            printf "| %s | second | %d | %s |\n", s, n[s], stat(s, 4)
        }
    }' "$out/runs.tsv" | tee "$out/summary.md"
