#!/usr/bin/env bash
# Regenerate crates/colorgen/tests/kde/expected/ from the original kde-material-you-colors.
# Usage: tools/reference/kmyc-golden/gen.sh [case...]   (default: every dir in tests/kde/cases)
# On the host it builds the kmyc-golden image and re-runs itself inside it.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)
kde=crates/colorgen/tests/kde

if [[ ! -x /opt/kmyc/bin/kde-material-you-colors ]]; then
  engine=${ENGINE:-$(command -v podman || command -v docker)}
  "$engine" build -q -t kmyc-golden -f "$here/Containerfile" "$here" >/dev/null
  exec "$engine" run --rm -v "$repo:/src" -w /src \
    -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" \
    kmyc-golden tools/reference/kmyc-golden/gen.sh "$@"
fi

cd "$repo"
cases=("$@")
if [[ ${#cases[@]} -eq 0 ]]; then
  mapfile -t cases < <(ls "$kde/cases")
fi

# sha1 + path of every file under $1, relative, sorted
snapshot() { (cd "$1" && find . -type f -print0 | sort -z | xargs -0r sha1sum); }

run_case() {
  local c=$1 src=$kde/cases/$1 out=$kde/expected/$1
  local work home tmp
  work=$(mktemp -d /tmp/kmyc.XXXXXX)
  home=$work/home tmp=$work/tmp
  mkdir -p "$home" "$tmp"
  if [[ -d $src/home ]]; then cp -a "$src/home/." "$home/"; fi
  if [[ -f $src/config.conf ]]; then
    mkdir -p "$home/.config/kde-material-you-colors"
    cp "$src/config.conf" "$home/.config/kde-material-you-colors/config.conf"
  fi
  snapshot "$home" >"$work/before"
  mapfile -t args <"$src/args"

  env -i PATH="$PATH" HOME="$home" USER=kmyc LOGNAME=kmyc TMPDIR="$tmp" \
    LANG=C.UTF-8 QT_QPA_PLATFORM=offscreen OPENBLAS_NUM_THREADS=1 OMP_NUM_THREADS=1 \
    KMYC_OUTCOME="$work/outcome" \
    dbus-run-session -- bash -c '
      dbus-monitor --session >"$0/dbus.raw" 2>/dev/null & mon=$!
      until [[ -s $0/dbus.raw ]]; do sleep 0.05; done
      python /src/tools/reference/kmyc-golden/once.py "$@" >"$0/stdout" 2>"$0/stderr"
      sleep 0.3; kill $mon; wait $mon 2>/dev/null || true
    ' "$work" "${args[@]}"

  snapshot "$home" >"$work/after"
  rm -rf "$out"
  mkdir -p "$out" "$kde/expected"
  # created or changed files; the log has timestamps and is not state
  comm -13 <(sort "$work/before") <(sort "$work/after") | sed 's/^[0-9a-f]*  \.\///' |
    grep -v '^\.local/share/kde-material-you-colors/' | sort |
    while IFS= read -r f; do
      mkdir -p "$out/$(dirname "$f")"
      cp "$home/$f" "$out/$f"
      chmod 644 "$out/$f"
    done
  # files the run deleted
  comm -23 <(sed 's/^[0-9a-f]*  \.\///' "$work/before" | sort) \
           <(sed 's/^[0-9a-f]*  \.\///' "$work/after" | sort) >"$work/deleted"
  if [[ -s $work/deleted ]]; then cp "$work/deleted" "$out/deleted.txt"; fi
  cp "$tmp/kde-material-you-colors-kmyc.json" "$out/export.json" 2>/dev/null || true
  python /src/tools/reference/kmyc-golden/normdbus.py <"$work/dbus.raw" >"$out/dbus.log"
  cp "$work/outcome" "$out/outcome.txt" 2>/dev/null || echo "no apply" >"$out/outcome.txt"
  if [[ -n ${KEEP_WORK:-} ]]; then echo "$c: kept $work"; else rm -rf "$work"; fi
  echo "$c: $(cat "$out/outcome.txt")"
}

python tools/reference/kmyc-golden/dedup.py expand "$kde/expected" 2>/dev/null || true
for c in "${cases[@]}"; do run_case "$c"; done
python tools/reference/kmyc-golden/dedup.py dedup "$kde/expected"
if [[ -n ${HOST_UID:-} ]]; then chown -R "$HOST_UID:$HOST_GID" "$kde/expected"; fi
