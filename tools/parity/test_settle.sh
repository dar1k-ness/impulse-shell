#!/bin/bash
# Checks when lib.sh takes a shot, on recorded screens, without a guest or a clock:
#   bash tools/parity/test_settle.sh
#
# COLD is the original's password field after `x` was typed on the lock screen, on the first run
# of a freshly made guest: the time each new frame was first seen, in ms after the key. The symbol
# is drawn by a Canvas whose first paint is slow there, so the field stands still on the way: with
# the hint gone and no symbol yet, then with the symbol half grown, 0.1 to 0.3 s each time. A shot
# of any of these is a real-looking difference from native that the next run does not have.
# STALL is COLD with the steps left out and the symbol later still: the field without it for 3.5 s,
# longer than the shot's delay and QUIET_MS together. No such pause was recorded, and no rule about
# time tells it from the end; typing a second time does (shot_twice). WARM is every later typing.
COLD="0:hint 500:empty 590:cursor-moved 910:symbol-1 1020:symbol-2 1310:symbol-3 1440:symbol-4
1600:symbol-5 1810:symbol-6 1920:symbol-7 2120:symbol"
STALL="0:hint 500:empty 590:cursor-moved 4120:symbol"
WARM="0:hint 40:cursor-moved 90:symbol-3 400:symbol"

export HYPRLAND_INSTANCE_SIGNATURE=test TMPF OUT
TMPF=$(mktemp -d); OUT=$TMPF/out; mkdir "$OUT"; trap 'rm -rf "$TMPF"' EXIT
here=$(dirname "$0")
. "$here/lib.sh"; . "$here/scenes.sh"

GRAB_MS=60   # a full-output grim in the guest takes 60 ms and more
CLOCK=0
mono_ms() { echo "$CLOCK"; }
sleep() { CLOCK=$(( CLOCK + $(awk -v s="$1" 'BEGIN { printf "%d", s * 1000 }') )); }
log() { :; }
on_screen() { :; }   # no Hyprland here to ask for the mapped layers
# play REC: the screen shows REC from now on. grim writes the frame on screen when the grab starts.
play() { local e; SCREEN=; for e in $1; do SCREEN="$SCREEN $(( CLOCK + ${e%%:*} )):${e#*:}"; done; }
grim() {
  local f=${*: -1} e frame
  for e in $SCREEN; do [ "${e%%:*}" -le "$CLOCK" ] && frame=${e#*:}; done
  echo "$frame" > "$f"; CLOCK=$(( CLOCK + GRAB_MS ))
}

fail=0
bad() { echo "FAIL: $*"; fail=1; }

# settle returns the last frame of the recording whenever it starts looking.
wrong=
for start in $(seq 0 10 2500); do
  CLOCK=0; play "$COLD"; CLOCK=$start
  settle "$TMPF/shot" || bad "start +$start ms: settle gave up on a screen that stops"
  [ "$(cat "$TMPF/shot")" = symbol ] || wrong="$wrong +$start:$(cat "$TMPF/shot")"
done
[ -z "$wrong" ] || bad "settle took a frame of the animation for the still screen, start:frame$wrong"

# A still screen is still.
CLOCK=0; play "0:still"
settle "$TMPF/shot" || bad "settle gave up on a still screen"
# A screen that never stops: fails after MAX_S with the last frame.
CLOCK=0; play "$(for t in $(seq 0 50 20000); do echo "$t:f$t"; done)"
settle "$TMPF/shot" 3 && bad "settle called a screen that changes every 50 ms still"
[ "$CLOCK" -ge 3000 ] && [ "$CLOCK" -lt 4000 ] || bad "settle FILE 3 gave up after $CLOCK ms, not 3 s"

# The lock scene on a shell whose first typed symbol is drawn as FIRST and whose lock goes away
# UNLOCK_MS after Return: lock_typing is the field with its symbol, lock_after_unlock the desktop.
# 6 s is the unlock that was shot still locked once, the password already accepted.
ipc() { play "0:desktop 300:hint"; locked_until=; }
park() { :; }
die() { bad "the scene stopped: $*"; exit 1; }
locked() {
  local is=true; [ -n "$locked_until" ] && [ "$CLOCK" -ge "$locked_until" ] && is=false
  [ "$is" = "${1:-true}" ]
}
key() {
  case "$*" in
    x) typings=$(( typings + 1 )); if [ "$typings" = 1 ]; then play "$FIRST"; else play "$WARM"; fi ;;
    "-k BackSpace") play "0:hint" ;;
    "-k Return") play "0:hint $UNLOCK_MS:desktop"; locked_until=$(( CLOCK + UNLOCK_MS )) ;;
  esac
}
lock_scene() {
  FIRST=$2 UNLOCK_MS=$3 typings=0 CLOCK=0
  rm -f "$OUT"/*; play "0:desktop"
  (scene_lock) || return
  [ "$(cat "$OUT/lock.ppm")" = hint ] || bad "$1: lock is '$(cat "$OUT/lock.ppm")', not the empty field"
  [ "$(cat "$OUT/lock_typing.ppm")" = symbol ] ||
    bad "$1: lock_typing is '$(cat "$OUT/lock_typing.ppm")', not the field with its symbol"
  [ "$(cat "$OUT/lock_after_unlock.ppm")" = desktop ] ||
    bad "$1: lock_after_unlock is '$(cat "$OUT/lock_after_unlock.ppm")', not the desktop"
  [ ! -e "$OUT/unsettled.txt" ] || bad "$1: listed as still changing: $(tr '\n' ' ' < "$OUT/unsettled.txt")"
}
lock_scene "warm shell" "$WARM" 300
lock_scene "cold start as recorded" "$COLD" 300
lock_scene "symbol 3.5 s late" "$STALL" 300
lock_scene "unlock 6 s after the password" "$WARM" 6000

[ "$fail" = 0 ] && echo "test_settle: ok"
exit "$fail"
