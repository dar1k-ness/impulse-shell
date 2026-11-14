#!/bin/bash
# provision.sh: what the scene matrix needs on top of bench/guest-setup.sh. Run once in the guest
# made by `bench/vm.sh create` (setup.sh refuses to run without these):
#   bench/vm.sh ssh 'bash rusty-impulse/tools/parity/provision.sh'
# Packages come from the archive snapshot guest-setup.sh pinned, the font from a pinned release.
set -euo pipefail
PARITY=$(cd "$(dirname "$0")" && pwd)
. "$PARITY/lib.sh"
real_clock
# wtype: key input; python-evdev: ptr.py; python-numpy/pillow: compare.py's idle check in the guest;
# slurp, wf-recorder: the original's region and screen recording (scripts/videos/record.sh);
# breeze-icons: the icon theme kdeglobals.py pins; polkit: the polkit scene's pkexec.
sudo pacman -S --noconfirm --needed wtype python-evdev python-numpy python-pillow slurp wf-recorder \
  breeze-icons polkit unzip
# Google Sans Flex, the application font of the end-4 dots (kdeglobals.py): not packaged by Arch.
FONT_VER=v5.000
FONT_SHA256=c20b00878dd801d1bfc06d771732447940c7255bf94cf538ce0102c7fb06fa85
if [ "$(fc-match -f '%{family}' 'Google Sans Flex')" != "Google Sans Flex" ]; then
  tmp=$(mktemp -d)
  curl -fL -o "$tmp/font.zip" \
    "https://github.com/googlefonts/googlesans-flex/releases/download/$FONT_VER/GoogleSansFlex-$FONT_VER.zip"
  echo "$FONT_SHA256  $tmp/font.zip" | sha256sum -c -
  sudo mkdir -p /usr/local/share/fonts/google-sans-flex
  sudo unzip -o -q "$tmp/font.zip" -d /usr/local/share/fonts/google-sans-flex
  sudo chmod -R a+rX /usr/local/share/fonts/google-sans-flex
  rm -rf "$tmp"
  sudo fc-cache -f
fi
# ptr.py and wf-recorder's pkill need /dev/uinput at every boot.
echo uinput | sudo tee /etc/modules-load.d/parity-uinput.conf >/dev/null
echo 'KERNEL=="uinput", MODE="0660", GROUP="input"' | sudo tee /etc/udev/rules.d/60-parity-uinput.rules >/dev/null
sudo modprobe uinput
sudo udevadm control --reload
sudo udevadm trigger --name-match=uinput --settle
echo "provisioned"
