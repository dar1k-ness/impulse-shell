#!/bin/bash
# run.sh [SCENE...]: host side. Copies tools/parity into the guest, builds native rusty-impulse there from
# the checkout `bench/vm.sh sync` put in ~/rusty-impulse, captures every scene under the original
# (Quickshell running the tree ORIG names) and under native rusty-impulse, pulls both sets back and
# compares them (compare.py).
# Exit code: 0 every shot passed, 1 a shot differs for real, 2 the tool itself failed (a scene
# that shot nothing or left something open, a stale build, a missing shot).
# Env: SSH (guest ssh command, default `bench/vm.sh ssh`), OUT (host result dir, default /tmp/parity),
#      A, B (the two sides, default `orig native`; `A=orig B=orig` measures the noise of one shell),
#      ORIG (REVISION:PATH of the original in git history; orig.sh),
#      BIN (native binary in the guest, default the one build.sh builds; with BIN nothing is built),
#      REV (revision the native binary must be from, default this checkout's).
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
SSH=${SSH:-$REPO/bench/vm.sh ssh}
OUT=${OUT:-/tmp/parity}
A=${A:-orig} B=${B:-native}
REV=${REV:-$(git -C "$REPO" describe --always --dirty --abbrev=12)}
G=parity
tar -C "$HERE" -c . | $SSH "rm -rf ~/$G && mkdir -p ~/$G && tar -x -m -C ~/$G"
# The original is not in the checkout: orig.sh takes it from git history (ORIG) and unpacks it
# in the guest (lib.sh, ORIG_TREE).
case "$A $B" in *orig*) SSH=$SSH "$HERE/orig.sh" ;; esac
case "$A $B" in *native*)
  [ -n "${BIN:-}" ] || $SSH "bash ~/$G/build.sh" ;;
esac
$SSH "bash ~/$G/setup.sh"
da=$A db=$B; [ "$A" != "$B" ] || db=${B}2
rm -rf "$OUT"; mkdir -p "$OUT"
for side in "$A $da" "$B $db"; do
  set -- $side "$@"; mode=$1 dir=$2; shift 2
  $SSH "${BIN:+BIN=$BIN }REV=$REV bash ~/$G/capture.sh $mode /tmp/$G-$dir $*"
  $SSH "tar -C /tmp/$G-$dir -c ." | (mkdir -p "$OUT/$dir" && tar -x -C "$OUT/$dir")
done
python3 "$HERE/compare.py" "$OUT/$da" "$OUT/$db" "$OUT/diff"
