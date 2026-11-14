#!/usr/bin/env bash
# Record real Quickshell's DesktopEntries.applications after each step of tools/reference/desktopwatch/steps.sh
# into the table the native test compares against (crates/qml/tests/desktopwatch/qs-expected.txt).
# Needs `qs` (any Arch box: pacman -S quickshell); runs headless (QT_QPA_PLATFORM=offscreen).
# Usage: tools/reference/desktopwatch/record-qs.sh > crates/qml/tests/desktopwatch/qs-expected.txt
set -eu
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)
base=$(mktemp -d /tmp/desktopwatch.XXXXXX)
cfg=$(mktemp -d /tmp/desktopwatch-cfg.XXXXXX)
log=$cfg/log
trap 'kill $qs 2>/dev/null || true; rm -rf "$base" "$cfg"' EXIT
cp "$repo/crates/qml/tests/desktopwatch/apps.qml" "$cfg/shell.qml"
steps=()
while IFS=$'\t' read -r name cmd; do
    [[ -z $name || $name == \#* ]] && continue
    steps+=("$name"$'\t'"$cmd")
done < "$here/steps.sh"
(cd "$base" && eval "${steps[0]#*$'\t'}")
export XDG_DATA_HOME=$base/home XDG_DATA_DIRS=$base/sys:$base/late LANG=C LC_ALL=C
QT_QPA_PLATFORM=offscreen qs -p "$cfg/shell.qml" > "$log" 2>&1 &
qs=$!
sleep 2
apps() { sed -n 's/.*EV [0-9]* \(.*\)$/\1/p' "$log" | tail -1; }
echo "# step apps"
echo "start $(apps)"
for s in "${steps[@]:1}"; do
    (cd "$base" && eval "${s#*$'\t'}")
    sleep 0.6
    echo "${s%%$'\t'*} $(apps)"
done
