#!/bin/sh
# output_off.sh: how long Qt 6.12 takes over output_off.qml under a headless sway, with the
# output on and with it powered off (no frame callbacks). A measurement, not a test: run in
# the testenv image as the tester, in this directory.
set -eu
if [ "${1-}" = inside ]; then
    # Run by sway once it is up, with its socket and display in the environment.
    run() {
        QT_QPA_PLATFORM=wayland QT_FORCE_STDERR_LOGGING=1 qml6 output_off.qml 2>&1 |
            while read -r line; do
                case $line in
                    *IDLE*)
                        # The window is drawn and still; the transition starts 0.8 s after.
                        if [ "$1" = off ]; then
                            swaymsg output HEADLESS-1 power off > /dev/null
                        fi ;;
                    *FINISHED*) echo "output $1: $line" ;;
                esac
            done
    }
    for i in 1 2 3; do
        run on
        run off
        swaymsg output HEADLESS-1 power on > /dev/null
    done > "$XDG_RUNTIME_DIR/times" 2>&1
    swaymsg exit
    exit
fi
export XDG_RUNTIME_DIR=$(mktemp -d)
{
    echo 'output HEADLESS-1 resolution 1920x1080'
    echo "exec cd '$PWD' && sh ./output_off.sh inside"
} > "$XDG_RUNTIME_DIR/sway.conf"
WLR_BACKENDS=headless WLR_HEADLESS_OUTPUTS=1 WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman \
    sway -c "$XDG_RUNTIME_DIR/sway.conf" > "$XDG_RUNTIME_DIR/sway.log" 2>&1 || true
cat "$XDG_RUNTIME_DIR/times"
