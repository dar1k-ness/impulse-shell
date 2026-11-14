#!/bin/bash
# capture.sh orig|native OUTDIR [SCENE...]: run the scene matrix against one shell (in the guest).
# Env: BIN (native binary), REV (the revision the native binary must have been built from; run.sh
# passes the host checkout's).
set -u
PARITY=$(cd "$(dirname "$0")" && pwd)
. "$PARITY/lib.sh"
. "$PARITY/scenes.sh"
MODE=$1 OUT=$2; shift 2
rm -rf "$OUT"; mkdir -p "$OUT"
# A shell killed while locked leaves Hyprland on its "lockscreen app died" screen, and clearing
# it leaves a placeholder workspace active: back to workspace 1 so both shells see the same one.
unlock_crashed() {
  hyprctl eval 'hl.clear_crashed_lockscreen()' >/dev/null 2>&1 || true
  hdsp 'hl.dsp.focus({workspace=1})'
}
unlock_crashed
if [ "$MODE" = orig ]; then
  [ -f "$ORIG_TREE/shell.qml" ] || die "no original at $ORIG_TREE: tools/parity/run.sh unpacks it there"
  [ ! -f "$ORIG_TREE/.rev" ] || { cp "$ORIG_TREE/.rev" "$OUT/orig.txt"; log "original $(cat "$ORIG_TREE/.rev")"; }
fi
if [ "$MODE" = native ]; then
  # Stale-build guard: build.sh records which source the binary was built from and the binary's
  # checksum. Without that record, or with another binary or another revision than the one asked
  # for, the run would compare some older build without anyone noticing.
  BIN=${BIN:-$HOME/rusty-impulse/target/release/rusty-impulse}; export BIN
  [ -x "$BIN" ] || die "no native binary at $BIN: build it with tools/parity/build.sh"
  [ -f "$BIN.rev" ] || die "$BIN has no build record ($BIN.rev): build it with tools/parity/build.sh"
  read -r built sum < "$BIN.rev"
  [ "$(sha256sum < "$BIN" | cut -d' ' -f1)" = "$sum" ] ||
    die "$BIN is not the binary its build record describes: rebuild with tools/parity/build.sh"
  [ -z "${REV:-}" ] || [ "$REV" = "$built" ] ||
    die "$BIN was built from $built, the run asks for $REV: bench/vm.sh sync, then tools/parity/build.sh"
  cp "$BIN.rev" "$OUT/build.txt"; log "native $built"
fi
start_shell "$MODE" "$OUT/shell.log" || exit 1
idle_check start
for s in ${*:-$SCENES}; do
  log "scene $s"; "scene_$s"
  shell_pid >/dev/null || die "shell died in scene $s"
  idle_check "$s"
done
stop_shell
unlock_crashed
for f in "$OUT"/*.ppm; do
  magick "$f" "${f%.ppm}.png" && rm "$f" || die "cannot convert $f"
done
