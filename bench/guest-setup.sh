#!/bin/bash
# Provisions the benchmark guest: run once by `bench/vm.sh create`, in the guest, as dar1k,
# from the checkout it copied to ~/impulse-shell.
#   guest-setup.sh YYYY/MM/DD   the Arch Linux Archive snapshot every package comes from
set -euo pipefail
snapshot=$1
repo=$(cd "$(dirname "$0")/.." && pwd)

# One archive snapshot for the whole system (ArchWiki "Arch Linux Archive"): -Syyuu also
# downgrades what the image has newer.
echo "Server = https://archive.archlinux.org/repos/$snapshot/\$repo/os/\$arch" |
    sudo tee /etc/pacman.d/mirrorlist >/dev/null
# Build dependencies of impulse-shell, the compositor, Quickshell with the Qt modules the original imports, the
# fonts the original's defaults name that Arch packages, and the tools the original's services call.
sudo pacman -Syyuu --noconfirm --needed \
    base-devel rustup pkgconf clang cmake nasm git jq \
    wayland wayland-protocols libxkbcommon mesa libglvnd libdrm libinput \
    freetype2 harfbuzz fontconfig noto-fonts noto-fonts-emoji ttf-dejavu libjpeg-turbo \
    ttf-material-symbols-variable ttf-jetbrains-mono-nerd \
    pam systemd-libs \
    hyprland quickshell matugen \
    kirigami qt6-positioning qt6-5compat qt6-svg qt6-wayland \
    qt6-virtualkeyboard qt6-tools qt6-shadertools qt6-imageformats \
    syntax-highlighting \
    pipewire pipewire-audio pipewire-pulse wireplumber libnotify upower networkmanager \
    cliphist wl-clipboard cava brightnessctl ddcutil libqalculate translate-shell \
    gsettings-desktop-schemas adwaita-icon-theme \
    imagemagick libheif libavif bc xdg-user-dirs xdg-utils python grim
# Qt needs a UTF-8 locale; C.UTF-8 is the one glibc has without generating any.
echo LANG=C.UTF-8 | sudo tee /etc/locale.conf >/dev/null

# The venv the original's switchwall.sh runs generate_colors_material.py in
# (IMPULSE_SHELL_VIRTUAL_ENV), built as tools/testenv/Containerfile builds it:
# hash-checked wheels only.
sudo python -m venv --system-site-packages /opt/ri-venv
sudo /opt/ri-venv/bin/pip install --no-cache-dir --require-hashes --only-binary=:all: \
    -r "$repo/tools/testenv/build-requirements.txt"
sudo /opt/ri-venv/bin/pip install --no-cache-dir --require-hashes --only-binary=:all: \
    --no-binary=materialyoucolor --no-build-isolation -r "$repo/tools/testenv/requirements.txt"

# Hyprland on tty1 at boot: an autologin (ArchWiki "getty", "Automatic login to virtual
# console") whose login shell starts the compositor the way its session file does.
sudo mkdir -p /etc/systemd/system/getty@tty1.service.d
printf '%s\n' '[Service]' 'ExecStart=' \
    "ExecStart=-/usr/bin/agetty -o '-p -f -- \\\\u' --noclear --autologin dar1k %I \$TERM" |
    sudo tee /etc/systemd/system/getty@tty1.service.d/autologin.conf >/dev/null
printf '%s\n' '[ -f ~/.bashrc ] && . ~/.bashrc' \
    '[ "$(tty)" = /dev/tty1 ] && exec start-hyprland' >~/.bash_profile
mkdir -p ~/.config/hypr
cp "$repo/bench/hyprland.lua" ~/.config/hypr/hyprland.lua
