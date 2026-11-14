#!/bin/bash
# dumpipc.sh BASE_RI_BIN HEAD_RI_BIN [ROOT]: the `ipc call ri-debug dump` files (RI_DEBUG=dump) of HEAD against the
# `touch /tmp/ri-dump` files of BASE, under a headless sway. Each side runs twice; a file passes when
# BASE vs HEAD differs no more than a side differs from itself (clock text, timing of rebuilds).
# SWAY=<bin> overrides the compositor (e.g. a copy without file capabilities inside a container).
set -u
B=$1 H=$2 ROOT=${3:-$(cd "$(dirname "$0")/../../.." && pwd)/qml}
T=$(mktemp -d); fail=0
export XDG_RUNTIME_DIR=$T/run; mkdir -m700 -p $XDG_RUNTIME_DIR
WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_HEADLESS_OUTPUTS=1 ${SWAY:-sway} -c /dev/null >$T/sway.log 2>&1 &
sp=$!; for _ in $(seq 50); do ls $XDG_RUNTIME_DIR/wayland-* >/dev/null 2>&1 && break; sleep 0.2; done
export WAYLAND_DISPLAY=$(basename $(ls $XDG_RUNTIME_DIR/wayland-? | head -1))
files="ri-tree.txt ri-click.txt ri-live.txt ri-ws.txt"
shot() { # shot BIN MODE OUT
  rm -f /tmp/ri-dump /tmp/ri-{tree,click,live,ws}.txt
  RI_RENDER=cpu RI_DEBUG=dump "$1" "$ROOT" >$3.log 2>&1 & local p=$!; sleep ${SETTLE:-25}
  if [ "$2" = touch ]; then touch /tmp/ri-dump; sleep 3; else "$1" -p "$ROOT" ipc call ri-debug dump >$3.reply 2>&1; fi
  [ -f /tmp/ri-dump ] && echo "$3: /tmp/ri-dump left in place"
  # HEAD writes into its private run directory and replies with the paths.
  mkdir -p $3; for f in $files; do
    src=$(grep -m1 "/$f\$" $3.reply 2>/dev/null || echo /tmp/$f)
    cp "$src" $3/ 2>/dev/null || echo "$3: no $f"
  done
  kill $p; wait $p 2>/dev/null
}
shot $B touch $T/b1; shot $B touch $T/b2; shot $H ipc $T/h; shot $H ipc $T/h2
# The old trigger does nothing now: HEAD must not write the files on touch.
rm -f /tmp/ri-{tree,click,live,ws}.txt
RI_RENDER=cpu "$H" "$ROOT" >$T/t.log 2>&1 & p=$!; sleep ${SETTLE:-25}; touch /tmp/ri-dump; sleep 3
[ -f /tmp/ri-tree.txt ] && { echo "FAIL head still polls /tmp/ri-dump"; fail=1; } || echo "ok    head ignores /tmp/ri-dump"
rm -f /tmp/ri-dump; kill $p; wait $p 2>/dev/null
echo "reply: $(tr '\n' ' ' <$T/h.reply)"
for f in $files; do
  [ -f $T/b1/$f ] && [ -f $T/h/$f ] || { echo "FAIL  $f missing"; fail=1; continue; }
  nd() { diff $T/$1/$f $T/$2/$f | grep -c '^[<>]'; }
  n=$(nd b1 b2); x=$(nd h h2); [ $x -gt $n ] && n=$x
  d=$(nd b1 h); x=$(nd b2 h2); [ $x -lt $d ] && d=$x
  v=ok; [ "$d" -gt "$n" ] && { v=FAIL; fail=1; }
  printf '%-5s %-13s lines=%-6s noise=%-4s diff=%s\n' $v $f "$(wc -l <$T/b1/$f)" $n $d
done
kill $sp; [ -n "${KEEP:-}" ] && echo "kept $T" || rm -rf "$T"; exit $fail
