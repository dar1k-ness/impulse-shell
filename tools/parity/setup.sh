#!/bin/bash
# setup.sh: make the guest deterministic for both shells (idempotent, guest side).
# - everything the scenes call is installed (provision.sh), checked here so a missing tool stops
#   the run instead of leaving a scene that shoots nothing
# - wall clock pinned to 2026-01-15 12:34:30 (clock/date text identical between runs)
# - fixed generated wallpaper + Material colours (identical inputs for both shells)
# - a password on the test account so the lock scene can unlock
# - the platform theme state Qt reads: KDE_SESSION_VERSION=6 in the Hyprland env (QKdeTheme) and
#   the dots' kdeglobals application font + icon theme, with the font installed
# - the original's Python environment in the Hyprland env (region detection of the region selector)
# - one audio sink at a known volume (volume OSDs)
set -u
PARITY=$(cd "$(dirname "$0")" && pwd)
. "$PARITY/lib.sh"
SRC=${SRC:-$HOME/impulse-shell}
need="run tools/parity/provision.sh in the guest"
for c in wtype grim magick jq slurp wf-recorder pkexec notify-send wl-copy matugen wpctl pactl; do
  command -v "$c" >/dev/null || die "$c is not installed: $need"
done
python3 -c "import evdev, numpy, PIL" 2>/dev/null || die "python-evdev, python-numpy or python-pillow is not installed: $need"
[ "$(fc-match -f '%{family}' 'Google Sans Flex')" = "Google Sans Flex" ] ||
  die "Google Sans Flex (kdeglobals application font) is not installed: $need"
[ -w /dev/uinput ] || die "/dev/uinput is not writable (uinput module of the running kernel, group input): $need"
[ -f "$HOME/.local/share/impulse-shell/shell.qml" ] || die "no impulse-shell config at ~/.local/share/impulse-shell: bench/vm.sh sync"
# The region selector finds content regions with scripts/images/find_regions.py in this environment;
# without it the original shows none and the scene compares nothing.
VENV=/opt/ri-venv
"$VENV/bin/python" -c "import cv2, numpy" 2>/dev/null || die "the original's Python environment $VENV has no opencv (bench/guest-setup.sh builds it)"

# No NTP, or systemd-timesyncd steps the clock back from the time clock.sh pins. It stays off over a
# reboot, and the image waits for a synchronized clock (systemd-time-wait-sync.service) before
# pacman-init.service, which is ordered before sshd: with NTP off that wait never ended and the
# rebooted guest had no ssh. The guest's clock at boot is the RTC, which QEMU keeps at the host's time.
sudo timedatectl set-ntp false 2>/dev/null
sudo systemctl disable -q systemd-time-wait-sync.service 2>/dev/null
pgrep -f "[p]arity/clock.sh" >/dev/null || sudo setsid -f bash "$PARITY/clock.sh" >/dev/null 2>&1 < /dev/null
echo "$(id -un):${PARITY_PW:-parity}" | sudo chpasswd
# hl.env reaches every process exec'd from Hyprland afterwards, i.e. both shells.
# QT_QPA_PLATFORMTHEME=kde only selects QKdeTheme (kdeglobals font/icons) with KDE_SESSION_VERSION >= 4.
hyprctl eval 'hl.env("QT_QPA_PLATFORMTHEME","kde")' >/dev/null
hyprctl eval 'hl.env("KDE_SESSION_VERSION","6")' >/dev/null
hyprctl eval "hl.env(\"IMPULSE_SHELL_VIRTUAL_ENV\",\"$VENV\")" >/dev/null
# The original's scripts read it under the original's name.
hyprctl eval "hl.env(\"$(printf %s_%s_VIRTUAL_ENV ILLOGICAL IMPULSE)\",\"$VENV\")" >/dev/null
python3 "$PARITY/kdeglobals.py" "$HOME/.config/kdeglobals" || die "kdeglobals"

mkdir -p "$(dirname "$CFG")" "$HOME/.local/state/quickshell/user" "$HOME/.cache/quickshell"
# The original and this shell read the same settings, colours and history here, as they
# did while they had the same names for them.
for d in "$HOME/.local/state" "$HOME/.cache"; do
  # (The link of a guest set up before this shell's directories had one name.)
  [ ! -L "$d/rustshell" ] || rm "$d/rustshell"
  if [ ! -L "$d/impulse-shell" ] && [ -d "$d/impulse-shell" ]; then
    cp -a "$d/impulse-shell/." "$d/quickshell/" && rm -rf "$d/impulse-shell"
  fi
  ln -sfn quickshell "$d/impulse-shell"
done
old=$HOME/.config/$(printf %s-%s illogical impulse)
[ -L "$old" ] || rm -rf "$old"
ln -sfn "$(basename "$(dirname "$CFG")")" "$old"
[ -s "$CFG" ] || echo '{}' > "$CFG"
# The matugen configuration switchwall.sh needs (the one the colour tests use).
[ -d "$HOME/.config/matugen" ] ||
  cp -r "$SRC/crates/colorgen/tests/switchwall/fixture/home/.config/matugen" "$HOME/.config/matugen" ||
  die "no matugen configuration (needs the checkout at $SRC: bench/vm.sh sync)"
WALL=$HOME/Pictures/parity-wall.png
if [ ! -f "$WALL" ] || [ ! -f "$HOME/.local/state/quickshell/user/generated/colors.json" ]; then
  mkdir -p "$(dirname "$WALL")"
  magick -size 1920x1080 -seed 7 plasma:"#3b5b8c-#c08a5a" -blur 0x8 -depth 8 "$WALL"
  # switchwall.sh and its helpers are kept with the tests (the shell does not run them); the
  # terminal templates they read are in the installed tree.
  SW=$(mktemp -d)
  # the original's switchwall.sh goes over the installed one, which hands the work to impulse-shell.
  cp -r "$HOME/.local/share/impulse-shell/scripts" "$SW/"
  cp -r "$SRC/crates/qml/tests/fixtures/original-unshipped/scripts/." "$SW/scripts/" || die "original-unshipped scripts (needs the checkout at $SRC: bench/vm.sh sync)"
  IMPULSE_SHELL_VIRTUAL_ENV=$VENV bash "$SW/scripts/colors/switchwall.sh" \
    --image "$WALL" --mode dark > "$HOME/.cache/parity-switchwall.log" 2>&1
  rm -rf "$SW"
  [ -f "$HOME/.local/state/quickshell/user/generated/colors.json" ] ||
    die "switchwall.sh made no colours, see ~/.cache/parity-switchwall.log"
fi
# screenRecord.savePath: without it the original's record.sh reads "null" from the config and records into ./null.
# osd.timeout: the OSD stays 1 s by default, less than a shot waits for a still screen (QUIET_MS).
jq --arg w "$WALL" --arg v "$HOME/Videos" '.background.wallpaperPath = $w | .bar.vertical = false | .panelFamily = "ii"
  | .screenRecord.savePath = $v | .osd.timeout = 5000' "$CFG" > "$CFG.tmp" && mv "$CFG.tmp" "$CFG"
touch "$HOME/.local/state/quickshell/user/first_run.txt"

# "Open recordings folder" hands the folder to the desktop's file manager. The guest has none:
# opened.sh is registered as the one for folders and records what it is asked to open.
mkdir -p "$HOME/.local/share/applications" "$HOME/Videos"
printf '[Desktop Entry]\nType=Application\nName=parity-opened\nExec=%s %%u\nMimeType=inode/directory;\n' \
  "$PARITY/opened.sh" > "$HOME/.local/share/applications/parity-opened.desktop"
xdg-mime default parity-opened.desktop inode/directory || die "cannot register the folder handler"

# The guest has no sound card: one null sink as the default, so volume OSDs have a volume to show.
pactl list short sinks 2>/dev/null | grep -q "parity-sink" ||
  pactl load-module module-null-sink sink_name=parity-sink sink_properties=device.description=parity-sink >/dev/null ||
  die "cannot create the null audio sink (pipewire-pulse)"
# The first pactl of a boot is what starts PipeWire and WirePlumber, and the default sink is kept in
# WirePlumber's "default" metadata: until that object exists, set-default-sink answers "Failure: Not
# supported". pw-metadata -m prints a line when the object appears.
grep -q -m1 'Found "default" metadata' < <(timeout 10 pw-metadata -n default -m 2>&1) ||
  die "WirePlumber has no default metadata (wireplumber)"
pactl set-default-sink parity-sink || die "cannot select the null audio sink"
wait_for 10 reset_volume || die "cannot set the sink volume"

"$PY" "$PARITY/ptr.py" move 1300 700 || die "pointer injection broken (ptr.py)"
[ "$(cursor_at)" = "1300 700" ] || die "pointer injection: asked for 1300 700, the cursor is at $(cursor_at)"
