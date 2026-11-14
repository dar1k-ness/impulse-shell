#!/usr/bin/env bash
# Regenerate crates/colorgen/tests/kde/qcolor.txt (see qcolor.cpp) in the kmyc-golden image.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)
if [[ ! -d /usr/include/KF6/KGuiAddons ]]; then
  engine=${ENGINE:-$(command -v podman || command -v docker)}
  "$engine" build -q -t kmyc-golden -f "$here/Containerfile" "$here" >/dev/null
  exec "$engine" run --rm -v "$repo:/src" -w /src -u "$(id -u):$(id -g)" kmyc-golden tools/reference/kmyc-golden/qcolor.sh
fi
# Arch's own CFLAGS (makepkg.conf), as plasma-workspace was built with.
g++ -std=c++20 -O2 -march=x86-64 -mtune=generic -fPIC -o /tmp/qcolor "$here/qcolor.cpp" \
  -I/usr/include/qt6 -I/usr/include/qt6/QtCore -I/usr/include/qt6/QtGui -I/usr/include/KF6/KGuiAddons \
  -lKF6GuiAddons -lQt6Gui -lQt6Core
QT_QPA_PLATFORM=offscreen /tmp/qcolor >"$repo/crates/colorgen/tests/kde/qcolor.txt"
