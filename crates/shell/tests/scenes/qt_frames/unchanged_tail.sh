#!/bin/sh
# unchanged_tail.sh: what Qt 6.12 does over unchanged_tail.qml under a headless sway, three
# times: how long the transition takes, how many steps it has, how many commits of its
# surface carry a buffer before and after the item is gone (WAYLAND_DEBUG), and the order of
# the animation's signals. A measurement, not a test: run in the testenv image as the tester,
# in this directory.
set -eu
if [ "${1-}" = inside ]; then
    # Run by sway once it is up, with its socket and display in the environment.
    WAYLAND_DEBUG=1 QT_QPA_PLATFORM=wayland QT_FORCE_STDERR_LOGGING=1 qml6 unchanged_tail.qml \
        > "$XDG_RUNTIME_DIR/log" 2>&1 || true
    awk '
        /qml: STARTED/ { part = 1; head = tail = frames = 0; order = "" }
        /qml: GONE/ { part = 2 }
        /qml: (RUNNING|STARTED|STOPPED|FINISHED)/ {
            sub(/.*qml: /, ""); order = order (order == "" ? "" : ", ") $1 ($1 == "RUNNING" ? " " $2 : "")
        }
        /-> wl_surface#[0-9]+\.attach\(wl_buffer/ { buffer = 1 }
        /-> wl_surface#[0-9]+\.frame\(/ && part == 2 { frames++ }
        /-> wl_surface#[0-9]+\.commit\(/ {
            if (buffer && part == 1) head++
            if (buffer && part == 2) tail++
            buffer = 0
        }
        /^FINISHED|qml: FINISHED/ {
            print $0
            print "  commits with a buffer: " head " before GONE, " tail " after; frame requests after GONE: " frames
            print "  order: " order
            part = 0
        }
    ' "$XDG_RUNTIME_DIR/log" > "$XDG_RUNTIME_DIR/times" 2>&1
    swaymsg exit
    exit
fi
export XDG_RUNTIME_DIR=$(mktemp -d)
{
    echo 'output HEADLESS-1 resolution 1920x1080'
    echo "exec cd '$PWD' && sh ./unchanged_tail.sh inside"
} > "$XDG_RUNTIME_DIR/sway.conf"
WLR_BACKENDS=headless WLR_HEADLESS_OUTPUTS=1 WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman \
    sway -c "$XDG_RUNTIME_DIR/sway.conf" > "$XDG_RUNTIME_DIR/sway.log" 2>&1 || true
cat "$XDG_RUNTIME_DIR/times"
