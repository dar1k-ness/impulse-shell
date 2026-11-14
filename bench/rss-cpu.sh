#!/bin/bash
# Memory and CPU of rusty-impulse and of the original in Quickshell, in three states: idle (bar and
# background), panels open (both sidebars) and a wallpaper switch. Runs in the benchmark guest
# (bench/vm.sh) after `bench/vm.sh sync`; method and results: docs/benchmarks.md.
#   bench/rss-cpu.sh [OUT]     OUT (default ~/rss-cpu/<date>) gets samples.tsv (one line per
#                              second), phases.tsv (one line per run and state), summary.md,
#                              env.txt, and per run the shell's log and per state a
#                              screenshot and the processes of the shell's cgroup
#   ROUNDS=3   runs per shell; the two shells alternate and swap order every round
#   SETTLE=30  seconds between the shell answering IPC and the first sample
#   IDLE=60 PANELS=60 WALL=60   seconds sampled per state
set -euo pipefail

export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
export DBUS_SESSION_BUS_ADDRESS=${DBUS_SESSION_BUS_ADDRESS:-unix:path=$XDG_RUNTIME_DIR/bus}
export WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-wayland-1}
HYPRLAND_INSTANCE_SIGNATURE=${HYPRLAND_INSTANCE_SIGNATURE:-$(ls -t "$XDG_RUNTIME_DIR/hypr" | head -1)}
export HYPRLAND_INSTANCE_SIGNATURE
export LANG=C.UTF-8

self=$(realpath "$0")
repo=$(dirname "$(dirname "$self")")
tree=$HOME/.config/rustshell/rusty-impulse
# The original, which Quickshell runs: `bench/vm.sh sync` unpacks it here (tools/parity/orig.sh).
# The checkout's own tree, `qml`, imports the Impulse module, which Quickshell does not have.
orig=${ORIG_TREE:-$HOME/parity-original}
unit=ri-bench.scope

# The shell under test, exec'd by Hyprland inside the scope: `rss-cpu.sh --exec SHELL LOG PIDFILE`.
if [ "${1:-}" = --exec ]; then
    echo $$ >"$4"
    case $2 in
    rusty-impulse) exec rusty-impulse -c rusty-impulse >"$3" 2>&1 </dev/null ;;
    quickshell) exec qs -p "$orig" >"$3" 2>&1 </dev/null ;;
    esac
fi

ROUNDS=${ROUNDS:-3}
SETTLE=${SETTLE:-30}
IDLE=${IDLE:-60}
PANELS=${PANELS:-60}
WALL=${WALL:-60}
out=${1:-$HOME/rss-cpu/$(date +%Y%m%d-%H%M%S)}
walls=$HOME/Pictures/Wallpapers
cfg=$HOME/.config/rusty-impulse/config.json
generated=$HOME/.local/state/quickshell/user/generated
tick=$(getconf CLK_TCK)

die() {
    echo "rss-cpu.sh: $*" >&2
    exit 1
}

ipc() {
    case $shell in
    rusty-impulse) timeout 10 rusty-impulse -c rusty-impulse ipc "$@" ;;
    quickshell) timeout 10 qs -p "$orig" ipc "$@" ;;
    esac
}

# wait_for SECS CMD...: until CMD succeeds, checked every 0.2 s.
wait_for() {
    local end=$((SECONDS + $1))
    shift
    until "$@" >/dev/null 2>&1; do
        [ $SECONDS -lt $end ] || return 1
        sleep 0.2
    done
}

# The shell maps a layer surface with this namespace (hyprctl layers).
layer() { hyprctl layers -j | jq -e --arg n "$1" '[.. | .namespace? // empty] | (index("quickshell:" + $n) // index("rustshell:" + $n))' >/dev/null; }
no_layer() { ! layer "$1"; }

# SIGTERM every shell process and return when they have exited (pidwait(1)), and Hyprland
# has dropped their bar: it does so on its own turn after the client is gone, and `hyprctl`
# has no request that waits for it.
stop_shell() {
    local n='rusty-impulse|quickshell|qs' rc=0
    systemctl --user stop "$unit" 2>/dev/null || true
    if pkill -x "$n"; then
        timeout 20 pidwait -x "$n" || rc=$?
        [ $rc != 124 ] || die "a shell did not exit"
    fi
    wait_for 20 no_layer bar || die "a shell is still on screen"
}

# Both shells start from the same files: no state or cache from an earlier run, the original's default
# configuration with the first wallpaper, and the matugen configuration switchwall.sh needs.
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
    # A transient scope (systemd-run(1)) gives the shell and everything it starts its own
    # cgroup, while Hyprland stays the parent, as with `exec-once`.
    hyprctl dispatch "hl.dsp.exec_cmd(\"systemd-run --user --scope --quiet --collect --unit=$unit $self --exec $shell $run/shell.log $run/pid\")" >/dev/null
    wait_for 20 test -s "$run/pid" || die "$shell did not start"
    pid=$(cat "$run/pid")
    cg=/sys/fs/cgroup$(cut -d: -f3 "/proc/$pid/cgroup")
    [[ $cg == */$unit ]] || die "$shell is not in $unit: $cg"
    wait_for 120 ipc show || die "$shell does not answer IPC; see $run/shell.log"
    wait_for 30 layer bar || die "$shell has no bar"
    wait_for 30 layer background || die "$shell has no background"
}

# Own CPU time of the shell process in clock ticks: utime + stime, proc_pid_stat(5) fields 14
# and 15, counted after the ")" that ends the command name.
own_ticks() {
    local stat f
    read -r stat <"/proc/$pid/stat"
    read -ra f <<<"${stat##*) }"
    echo $((f[11] + f[12]))
}

# CPU time of every process that ran in the scope, exited ones included, in microseconds.
all_usec() { awk '$1 == "usage_usec" { print $2 }' "$cg/cpu.stat"; }

status_kb() { awk -v k="$1:" '$1 == k { print $2 }' "/proc/$2/status" 2>/dev/null; }

# The state's window starts here: the clock and both CPU counters.
begin() {
    t0=${EPOCHREALTIME/./}
    c0=$(own_ticks)
    u0=$(all_usec)
}

# sample STATE SECS, after begin: one line per second in samples.tsv (memory now, CPU time since
# begin), then the state's line in phases.tsv (memory over the samples, CPU time over the
# window), the processes of the scope in STATE.procs and a screenshot.
sample() {
    local state=$1 secs=$2 i p kb all t1 c1 u1 rss anon
    for ((i = 1; i <= secs; i++)); do
        sleep 1
        [ -d "/proc/$pid" ] || die "$shell died during $state; see $run/shell.log"
        rss=$(status_kb VmRSS "$pid")
        anon=$(status_kb RssAnon "$pid")
        all=0
        # A helper can exit between the list and its status (awk then fails): it counts as 0.
        for p in $(<"$cg/cgroup.procs"); do
            kb=$(status_kb VmRSS "$p") || kb=0
            all=$((all + ${kb:-0}))
        done
        printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$shell" "$round" "$state" "$i" "$rss" "$anon" "$all" \
            $(($(own_ticks) - c0)) $(($(all_usec) - u0)) >>"$out/samples.tsv"
    done
    t1=${EPOCHREALTIME/./}
    c1=$(own_ticks)
    u1=$(all_usec)
    gawk -F'\t' -v sh="$shell" -v r="$round" -v st="$state" -v wall=$((t1 - t0)) \
        -v own=$((c1 - c0)) -v tick="$tick" -v all=$((u1 - u0)) '
        $1 == sh && $2 == r && $3 == st { n++; rss[n] = $5; anon[n] = $6; grp[n] = $7; last = $5 }
        function med(a,   m) { m = asort(a); return m % 2 ? a[(m + 1) / 2] : (a[m / 2] + a[m / 2 + 1]) / 2 }
        function max(a,   i, m) { for (i in a) if (a[i] > m) m = a[i]; return m }
        END {
            w = wall / 1e6; o = own / tick; a = all / 1e6
            printf "%s\t%s\t%s\t%.1f\t%.1f\t%.1f\t%.1f\t%.1f\t%.1f\t%.1f\t%.2f\t%.2f\t%.2f\t%.2f\n", sh, r, st, w,
                med(rss) / 1024, max(rss) / 1024, last / 1024, med(anon) / 1024, med(grp) / 1024, max(grp) / 1024,
                o, a, 100 * o / w, 100 * a / w
        }' "$out/samples.tsv" >>"$out/phases.tsv"
    tail -n 1 "$out/phases.tsv"
    for p in $(<"$cg/cgroup.procs"); do
        printf '%s\t%s\t%s\n' "$p" "$(status_kb VmRSS "$p")" "$(tr '\0' ' ' <"/proc/$p/cmdline" 2>/dev/null)"
    done >"$run/$state.procs"
    # The screenshot is taken by the compositor, after the counters were read.
    grim "$run/$state.png"
}

one_run() {
    run=$out/$shell-$round
    mkdir -p "$run"
    start_shell
    sleep "$SETTLE"

    begin
    sample idle "$IDLE"

    ipc call sidebarLeft open >/dev/null
    ipc call sidebarRight open >/dev/null
    wait_for 20 layer sidebarLeft || die "$shell: sidebarLeft did not open"
    wait_for 20 layer sidebarRight || die "$shell: sidebarRight did not open"
    sleep 5
    begin
    sample panels "$PANELS"
    ipc call sidebarLeft close >/dev/null
    ipc call sidebarRight close >/dev/null
    wait_for 20 no_layer sidebarLeft || die "$shell: sidebarLeft did not close"
    wait_for 20 no_layer sidebarRight || die "$shell: sidebarRight did not close"
    sleep 5

    # The window starts before the request, so it holds the whole switch: color generation,
    # the theme reload and the new image.
    begin
    ipc call wallpapers apply "$walls/bench-b.jpg" >/dev/null
    sample wallpaper "$WALL"
    [ "$(jq -r .background.wallpaperPath "$cfg")" = "$walls/bench-b.jpg" ] || die "$shell did not set the wallpaper"
    [ -s "$generated/colors.json" ] && [ -s "$generated/material_colors.scss" ] ||
        die "$shell: the switch did not generate the colors"

    stop_shell
}

command -v rusty-impulse >/dev/null || die "rusty-impulse is not installed: run bench/vm.sh sync"
diff -rq "$repo/qml" "$tree" >/dev/null || die "$tree differs from $repo/qml: run bench/vm.sh sync"
[ -f "$orig/shell.qml" ] && [ -s "$orig/.rev" ] || die "no original at $orig: run bench/vm.sh sync"
mkdir -p "$out" "$walls"
# The shell gets its log path through Hyprland, whose working directory is not this one.
out=$(realpath "$out")
# Two 4K wallpapers: ImageMagick's seeded plasma fractal, the same pixels for every run.
[ -f "$walls/bench-a.jpg" ] || magick -size 3840x2160 -seed 7 plasma:'#3b5b8c-#c08a5a' -blur 0x8 -quality 92 "$walls/bench-a.jpg"
[ -f "$walls/bench-b.jpg" ] || magick -size 3840x2160 -seed 11 plasma:'#7a3b8c-#5ac08a' -blur 0x8 -quality 92 "$walls/bench-b.jpg"

{
    echo "commit: $(cat "$repo/.commit" 2>/dev/null || echo unknown)"
    echo "QML tree: sha256 $(cd "$tree" && find . -type f -print0 | sort -z | xargs -0 sha256sum | sha256sum | cut -d' ' -f1)"
    echo "original: $(cat "$orig/.rev")"
    pacman -Q quickshell qt6-base qt6-declarative hyprland mesa linux matugen
    echo "cpus: $(nproc), $(awk -F': ' '/model name/ { print $2; exit }' /proc/cpuinfo)"
    echo "memory: $(awk '/MemTotal/ { printf "%.1f GiB", $2 / 1048576 }' /proc/meminfo)"
    echo "output: $(hyprctl monitors -j | jq -r '.[0] | "\(.width)x\(.height)@\(.refreshRate)"')"
    echo "renderer: $(grep -m1 -oP '(?<= Renderer: ).*' "$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/hyprland.log")"
    echo "rounds=$ROUNDS settle=$SETTLE idle=$IDLE panels=$PANELS wallpaper=$WALL"
} >"$out/env.txt"
printf 'shell\tround\tstate\tsecond\trss_kb\trss_anon_kb\tall_rss_kb\tcpu_own_ticks\tcpu_all_usec\n' >"$out/samples.tsv"
printf 'shell\tround\tstate\tseconds\trss_median_mb\trss_max_mb\trss_last_mb\trss_anon_median_mb\tall_rss_median_mb\tall_rss_max_mb\tcpu_own_s\tcpu_all_s\tcpu_own_pct\tcpu_all_pct\n' >"$out/phases.tsv"

for round in $(seq 1 "$ROUNDS"); do
    order="rusty-impulse quickshell"
    [ $((round % 2)) = 1 ] || order="quickshell rusty-impulse"
    for shell in $order; do one_run; done
done
# bash drops out of the loops on an expansion error without exiting: a short table is not a result.
rows=$(($(wc -l <"$out/phases.tsv") - 1))
[ "$rows" = $((ROUNDS * 6)) ] || die "$rows of $((ROUNDS * 6)) states measured; $out is incomplete"

# Per shell and state: the median over the rounds, with the smallest and largest round.
gawk -F'\t' '
    NR > 1 { k = $1 SUBSEP $3; n[k]++; for (c = 5; c <= NF; c++) v[k, c, n[k]] = $c }
    function stat(k, c, fmt,   a, i, m) {
        for (i = 1; i <= n[k]; i++) a[i] = v[k, c, i]
        m = asort(a)
        return sprintf(fmt " (" fmt "–" fmt ")", m % 2 ? a[(m + 1) / 2] : (a[m / 2] + a[m / 2 + 1]) / 2, a[1], a[m])
    }
    END {
        print "| State | Shell | RSS, MiB | RSS max, MiB | RssAnon, MiB | RSS with helpers, MiB | CPU shell, % | CPU with helpers, % | CPU shell, s | CPU with helpers, s |"
        print "|---|---|---|---|---|---|---|---|---|---|"
        split("idle panels wallpaper", st, " "); split("rusty-impulse quickshell", sh, " ")
        for (i = 1; i <= 3; i++) for (j = 1; j <= 2; j++) {
            k = sh[j] SUBSEP st[i]
            if (k in n) printf "| %s | %s | %s | %s | %s | %s | %s | %s | %s | %s |\n", st[i], sh[j],
                stat(k, 5, "%.0f"), stat(k, 6, "%.0f"), stat(k, 8, "%.0f"), stat(k, 9, "%.0f"),
                stat(k, 13, "%.2f"), stat(k, 14, "%.2f"), stat(k, 11, "%.2f"), stat(k, 12, "%.2f")
        }
    }' "$out/phases.tsv" >"$out/summary.md"
cat "$out/env.txt" "$out/summary.md"
echo "results: $out"
