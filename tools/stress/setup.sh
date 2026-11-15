#!/bin/bash
# setup.sh: run in the guest, from the checkout in ~/impulse-shell; `bench/vm.sh sync` runs it.
# Copies these scripts to ~/h and writes ~/henv: the environment of the Hyprland session, which
# a script started over ssh does not have.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
mkdir -p ~/h
cp "$here"/* ~/h/
cat >~/henv <<'ENV'
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export WAYLAND_DISPLAY=$(ls $XDG_RUNTIME_DIR | grep -m1 '^wayland-[0-9]*$')
export HYPRLAND_INSTANCE_SIGNATURE=$(ls -t $XDG_RUNTIME_DIR/hypr | head -1)
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus
ENV
# The password lock.sh types: the one tools/parity/setup.sh gives the user.
echo "$(id -un):${PARITY_PW:-parity}" | sudo chpasswd
# mpris_fake.py speaks D-Bus through python-gobject, windows.sh opens kitty windows; the rest
# of what the scenarios run comes from bench/guest-setup.sh and tools/parity/provision.sh.
pkgs=
python3 -c 'import gi' 2>/dev/null || pkgs="$pkgs python-gobject"
command -v kitty >/dev/null || pkgs="$pkgs kitty"
[ -z "$pkgs" ] || sudo pacman -S --noconfirm --needed $pkgs
missing=
for p in wtype grim compare python3 pkexec wl-copy notify-send gdbus nmcli kitty wpctl pactl; do
    command -v "$p" >/dev/null || missing="$missing $p"
done
python3 -c 'import gi' 2>/dev/null || missing="$missing python-gobject"
python3 -c 'import evdev' 2>/dev/null || missing="$missing python-evdev"
[ -w /dev/uinput ] || missing="$missing /dev/uinput(tools/parity/provision.sh)"
[ -z "$missing" ] || { echo "stress: not in the guest:$missing" >&2; exit 1; }
echo "stress: scripts in ~/h, environment in ~/henv"
