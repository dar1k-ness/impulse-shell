#!/bin/bash
# Key-event equivalence of the virtual keyboard port (crates/qml/src/ports/vkbd.rs) with
# ydotool, under Hyprland: a focused `wev` window logs the key/modifier events of the
# shipped sequences sent by ydotool, then by the port; the logs must match.
# Needs ydotoold running, wev, foot-free session. Usage: tools/reference/ports-keys.sh TEST_BIN
set -e
TEST_BIN=${1:?qml lib test binary (cargo test -p qml --lib --no-run)}
LOG=$(mktemp -d)
events() { grep -E '^\[[0-9]+\] *wl_keyboard: (key|modifiers)|^ *(key|sym|depressed|latched|locked|group|state):' "$1" \
    | sed -E 's/time: [0-9]+;? ?//; s/serial: [0-9]+;? ?//' | grep -v '^\s*$' \
    | awk '/depressed:/ { m = $0; next } /latched:/ { m = m "|" $0; next }
           /locked:/ { m = m "|" $0; if (m != last) print m; last = m; next } { print }'; }
# Hyprland 0.56 sends a virtual keyboard's `modifiers` request on to clients twice; a
# repeat of an unchanged modifier state changes nothing for a client, so it is collapsed
# (in both logs) and only changes of state are compared.
run() { # $1 = name, rest = command
    local name=$1; shift
    # wev block-buffers into a file and loses it on SIGTERM: give it a pty.
    script -qfc "wev -f wl_keyboard" "$LOG/$name.raw" >/dev/null 2>&1 & local pid=$!
    sleep 1.5
    hyprctl dispatch focuswindow class:wev >/dev/null 2>&1 || true
    sleep 0.5
    "$@"
    sleep 0.5
    pkill -x wev || true; wait $pid 2>/dev/null || true
    tr -d '\r' <"$LOG/$name.raw" >"$LOG/$name.txt"; events "$LOG/$name.txt" >"$LOG/$name.ev"
}
yd() {
    ydotool key -d 1 29:1 47:1 47:0 29:0
    ydotool key --key-delay 0 42:1; ydotool key --key-delay 0 30:1; ydotool key --key-delay 0 30:0
    ydotool key --key-delay 0 42:0; ydotool key --key-delay 0 30:1; ydotool key --key-delay 0 30:0
    ydotool key --key-delay 0 56:1; ydotool key --key-delay 0 29:1
    ydotool key --key-delay 0 $(seq -f '%g:0' 0 248)
}
run ydotool yd
run port env RI_PORTS_VKBD=1 "$TEST_BIN" --exact ports::tests::vkbd_sequence --include-ignored --nocapture
echo "ydotool: $(wc -l <"$LOG/ydotool.ev") lines, port: $(wc -l <"$LOG/port.ev") lines"
if diff -u "$LOG/ydotool.ev" "$LOG/port.ev"; then echo "KEYS EQUAL"; else echo "KEYS DIFFER"; exit 1; fi
