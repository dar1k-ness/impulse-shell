#!/bin/sh
# The file the original's Hyprland keybind runs to choose a wallpaper while no shell answers
# (Ctrl+Super+T): `switchwall.sh` asks for the image with kdialog and applies it, colours
# included; `--image FILE`, `--mode`, `--type`, `--color`, `--noswitch` are the original's script's.
# rusty-impulse does the work of the original's script itself; the shell need not be running.
# rusty-impulse is the one in PATH, else the binary of the shell that runs, else an installed one.
dir=$(cd "$(dirname "$0")" && pwd)
for bin in "$(command -v rusty-impulse)" $(for pid in $(pidof rusty-impulse 2>/dev/null); do echo "/proc/$pid/exe"; done) \
    "$HOME/.local/bin/rusty-impulse" /usr/local/bin/rusty-impulse /usr/bin/rusty-impulse; do
    [ -x "$bin" ] && exec "$bin" __ri-switchwall "$dir" "$@"
done
echo "switchwall.sh: no rusty-impulse to run" >&2
exit 127
