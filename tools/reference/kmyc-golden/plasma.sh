#!/usr/bin/env bash
# Regenerate crates/colorgen/tests/kde/plasma/expected/: the kdeglobals the real
# plasma-apply-colorscheme writes for each plasma/cases/<case> (its kdeglobals + Test.colors).
# On the host it builds the kmyc-golden image and re-runs itself inside it.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)
if ! command -v plasma-apply-colorscheme >/dev/null; then
  engine=${ENGINE:-$(command -v podman || command -v docker)}
  "$engine" build -q -t kmyc-golden -f "$here/Containerfile" "$here" >/dev/null
  exec "$engine" run --rm -v "$repo:/src" -w /src \
    -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" kmyc-golden tools/reference/kmyc-golden/plasma.sh
fi
dir=$repo/crates/colorgen/tests/kde/plasma
rm -rf "$dir/expected"
for c in "$dir"/cases/*/; do
  c=$(basename "$c")
  work=$(mktemp -d /tmp/plasma.XXXXXX)
  mkdir -p "$work/home/.config" "$work/home/.local/share/color-schemes" "$dir/expected/$c"
  cp "$dir/cases/$c/Test.colors" "$work/home/.local/share/color-schemes/"
  cp "$dir/cases/$c/kdeglobals" "$work/home/.config/kdeglobals"
  mkdir -p "$work/tmp"
  env -i PATH="$PATH" HOME="$work/home" USER=kmyc LOGNAME=kmyc TMPDIR="$work/tmp" LANG=C.UTF-8 QT_QPA_PLATFORM=offscreen \
    dbus-run-session -- plasma-apply-colorscheme Test >"$work/out" 2>&1 || true
  cp "$work/home/.config/kdeglobals" "$dir/expected/$c/kdeglobals"
  echo "$c: $(tr '\n' ' ' <"$work/out")"
  rm -rf "$work"
done
if [[ -n ${HOST_UID:-} ]]; then chown -R "$HOST_UID:$HOST_GID" "$dir/expected"; fi
