#!/usr/bin/env bash
# Print the reference vectors of the `kde::accent` unit tests (crates/colorgen/src/kde/accent.rs):
# accent_vectors.cpp on the real Qt + KConfig + KGuiAddons, then the real plasma-apply-colorscheme
# on the Material You schemes kde-material-you-colors wrote for the original's seed #5E7A91
# (tests/kde/expected/seed-5e7a91-dark) with an `[General] AccentColor` in kdeglobals.
# On the host it builds the kmyc-golden image, re-runs itself inside it (as root: see plasma.sh) and
# writes crates/colorgen/tests/kde/accent_vectors.txt.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)
if [[ ! -d /usr/include/KF6/KGuiAddons ]] || ! command -v plasma-apply-colorscheme >/dev/null; then
  engine=${ENGINE:-$(command -v podman || command -v docker)}
  "$engine" build -q -t kmyc-golden -f "$here/Containerfile" "$here" >/dev/null
  out=$repo/crates/colorgen/tests/kde/accent_vectors.txt
  "$engine" run --rm -v "$repo:/src:ro" -w /src kmyc-golden tools/reference/kmyc-golden/accent_vectors.sh >"$out.new"
  exec mv "$out.new" "$out"
fi
pacman -Q qt6-base kconfig kguiaddons plasma-workspace | sed 's/^/# /'
g++ -std=c++20 -O2 -march=x86-64 -mtune=generic -fPIC -o /tmp/accent_vectors "$here/accent_vectors.cpp" \
  -I/usr/include/qt6 -I/usr/include/qt6/QtCore -I/usr/include/qt6/QtGui \
  -I/usr/include/KF6/KGuiAddons -I/usr/include/KF6/KConfig -I/usr/include/KF6/KConfigCore -I/usr/include/KF6/KConfigGui \
  -Wl,--no-as-needed -lKF6GuiAddons -lKF6ConfigGui -lKF6ConfigCore -lQt6Gui -lQt6Core
QT_QPA_PLATFORM=offscreen /tmp/accent_vectors

schemes=$repo/crates/colorgen/tests/kde/expected/seed-5e7a91-dark/.local/share/color-schemes
# case scheme accent [General] lines of the scheme...
cases=(
  "dark-plain|MaterialYouDark|61,174,233|"
  "dark-lowlink|MaterialYouDark|20,40,90|"
  "dark-tint-titlebar|MaterialYouDark|200,30,90|TintFactor=0.3;TitlebarIsAccentColored=true"
  "light-plain|MaterialYouLight|61,174,233|"
  "light-lowlink|MaterialYouLight|250,240,120|"
  "light-titlebar|MaterialYouLight|146,110,228|TitlebarIsAccentColored=true"
  "light-tint|MaterialYouLight|200,30,90|TintFactor=0.15"
)
for c in "${cases[@]}"; do
  IFS='|' read -r name scheme accent general <<<"$c"
  work=$(mktemp -d /tmp/accent.XXXXXX)
  mkdir -p "$work/home/.config" "$work/home/.local/share/color-schemes" "$work/tmp"
  awk -v extra="$general" '{ print } /^\[General\]$/ { n = split(extra, l, ";"); for (i = 1; i <= n; i++) if (l[i] != "") print l[i] }' \
    "$schemes/$scheme.colors" | sed 's/^ColorScheme=.*/ColorScheme=Test/' >"$work/home/.local/share/color-schemes/Test.colors"
  printf '[General]\nAccentColor=%s\n' "$accent" >"$work/home/.config/kdeglobals"
  env -i PATH="$PATH" HOME="$work/home" USER=kmyc LOGNAME=kmyc TMPDIR="$work/tmp" LANG=C.UTF-8 QT_QPA_PLATFORM=offscreen \
    dbus-run-session -- plasma-apply-colorscheme Test >"$work/out" 2>&1 || true
  echo "## $name $scheme $accent $general"
  echo "# plasma-apply-colorscheme: $(tr '\n' ' ' <"$work/out")"
  awk '/^\[/ { g = $0; next } /=/ && g ~ /^\[(Colors:|WM)/ { print g " " $0 }' "$work/home/.config/kdeglobals"
  rm -rf "$work"
done
