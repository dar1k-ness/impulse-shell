#!/usr/bin/env bash
# Regenerates tests/fixtures/magick/golden/ with the real `magick` (taken with
# ImageMagick 7.1.2-35 Q16-HDRI). Only runs `magick` in a throwaway temp dir.
#   crates/imgtools/tests/magick_golden.sh                                      # local magick
#   MAGICK_SSH="ssh -p 22822 user@host" crates/imgtools/tests/magick_golden.sh  # remote magick
set -euo pipefail
fx="$(cd "$(dirname "$0")" && pwd)/fixtures/magick"
script='set -euo pipefail
d=$(mktemp -d); trap "rm -rf $d" EXIT
mkdir "$d/in" "$d/golden"; tar -C "$d/in" -x; cd "$d"
magick -version | head -1 > golden/VERSION
for f in in/*; do
  b=${f#in/}
  sizes="128 256"
  case $b in small*|onepx*) sizes="$sizes 512 1024" ;; esac
  echo "$b $(magick identify -format "%m %r %z %[channels]" "$f")" >> golden/INFO
  for s in $sizes; do magick "$f" -resize "${s}x${s}" "golden/$b.$s.png"; done
done
tar -C "$d" -c golden'
files=$(cd "$fx" && find . -maxdepth 1 -type f -printf '%f\n' | sort)
rm -rf "$fx/golden"
# shellcheck disable=SC2086
(cd "$fx" && tar -c $files) | if [ -n "${MAGICK_SSH:-}" ]; then $MAGICK_SSH "bash -c $(printf %q "$script")"; else bash -c "$script"; fi | tar -C "$fx" -x
echo "goldens: $(ls "$fx/golden" | wc -l) files, $(cat "$fx/golden/VERSION")"
