#!/usr/bin/env bash
# Record the app results real Quickshell gives for the tree's LauncherSearch (crates/qml/tests/appsearch/shell.qml)
# for tools/reference/appsearch/queries.txt and after each step of tools/reference/appsearch/steps.sh into the table
# the native test compares against (crates/qml/tests/appsearch/qs-expected.txt).
# Needs `qs` with the original's Qt modules and `weston` (Arch: pacman -S quickshell weston qt6-positioning
# qt6-5compat qt6-svg qt6-multimedia qt6-imageformats qt6-virtualkeyboard); runs on headless weston.
# Usage: tools/reference/appsearch/record-qs.sh > crates/qml/tests/appsearch/qs-expected.txt
set -eu
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)
base=$(mktemp -d /tmp/appsearch.XXXXXX)
cfg=$(mktemp -d /tmp/appsearch-cfg.XXXXXX)
export XDG_RUNTIME_DIR=$base/run
mkdir -m 700 "$XDG_RUNTIME_DIR"
trap 'kill $qs $weston 2>/dev/null || true; rm -rf "$base" "$cfg"' EXIT
cp -r "$repo/qml/." "$cfg"
cp "$repo/crates/qml/tests/appsearch/shell.qml" "$cfg/shell.qml"
steps=()
while IFS=$'\t' read -r name cmd; do
    [[ -z $name || $name == \#* ]] && continue
    steps+=("$name"$'\t'"$cmd")
done < "$here/steps.sh"
(cd "$base" && eval "${steps[0]#*$'\t'}")
export HOME=$base/home XDG_DATA_HOME=$base/home XDG_DATA_DIRS=$base/sys XDG_CONFIG_HOME=$base/config \
    XDG_STATE_HOME=$base/state XDG_CACHE_HOME=$base/cache LANG=C.UTF-8 LC_ALL=C.UTF-8
weston --backend=headless --socket=appsearch > "$base/weston.log" 2>&1 &
weston=$!
sleep 2
WAYLAND_DISPLAY=appsearch QT_QPA_PLATFORM=wayland qs -p "$cfg" > "$base/qs.log" 2>&1 &
qs=$!
for _ in $(seq 120); do grep -q "Configuration Loaded" "$base/qs.log" && break; sleep 0.5; done
sleep 2
q() { qs ipc --pid $qs call t q "$1"; }
echo "# query results"
while IFS= read -r s; do printf '%s\t%s\n' "${s:0:40}" "$(q "$s")"; done < "$here/queries.txt"
echo "# step results (foo | fire | kit)"
row() { echo "$1 $(q foo) | $(q fire) | $(q kit)"; }
row start
for s in "${steps[@]:1}"; do
    (cd "$base" && eval "${s#*$'\t'}")
    sleep 0.8
    row "${s%%$'\t'*}"
done
