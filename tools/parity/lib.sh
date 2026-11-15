# Shared helpers for tools/parity (sourced inside the Hyprland guest).
export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
export WAYLAND_DISPLAY=${WAYLAND_DISPLAY:-wayland-1}
export HYPRLAND_INSTANCE_SIGNATURE=${HYPRLAND_INSTANCE_SIGNATURE:-$(ls -t "$XDG_RUNTIME_DIR/hypr" | head -1)}
export LANG=C.UTF-8
PARITY=${PARITY:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}
CFG=$HOME/.config/impulse-shell/config.json
SETTLE=${SETTLE:-1.5}

# The wall clock is pinned by setup.sh, so durations and the log's times come from /proc/uptime
# (10 ms ticks): ms since the guest booted.
mono_ms() { local u; read -r u _ < /proc/uptime; echo $(( ${u/./} * 10 )); }
log() { echo "[$(mono_ms)] $*" >&2; }
die() { log "FATAL: $*"; exit 1; }
# The original: run.sh unpacks it here from git history (ORIG there). The checkout's own tree,
# `qml`, imports the Impulse module, which Quickshell does not have, so `qs` cannot run it.
ORIG_TREE=${ORIG_TREE:-$HOME/parity-original}
# The shell under test is the original run from that tree, or native serving the `impulse-shell` config.
ipc() {
  if [ "${MODE:-}" = orig ]; then timeout 5 qs -p "$ORIG_TREE" ipc call "$@" >/dev/null 2>&1
  else timeout 5 "${BIN:-$HOME/impulse-shell/target/release/impulse-shell}" -c impulse-shell ipc call "$@" >/dev/null 2>&1; fi
}
# What the shell under test puts before the names of its layers.
ns() { if [ "${MODE:-}" = orig ]; then echo quickshell; else echo rustshell; fi; }
hdsp() { hyprctl dispatch "$1" >/dev/null; }

# wait_for SECS CMD...: until CMD succeeds, checked every 0.2 s.
wait_for() {
  local end=$(( $(mono_ms) + $1 * 1000 )); shift
  until "$@" >/dev/null 2>&1; do [ "$(mono_ms)" -lt "$end" ] || return 1; sleep 0.2; done
}
# The shell under test has a layer surface with this namespace mapped (hyprctl layers).
layer() { hyprctl layers -j | jq -e --arg n "$(ns):$1" '[.. | .namespace? // empty] | index($n)' >/dev/null; }
no_layer() { ! layer "$1"; }
bar_layer() { layer bar || layer verticalBar; }

# shell_pid: the pid of the shell start_shell launched, while that process lives. launch.sh writes its
# pid and start time to SHELL_PID and execs the shell, which keeps both: the binary's file may have any
# name, and a later process that got the same pid is not taken for the shell.
SHELL_PID=${SHELL_PID:-$XDG_RUNTIME_DIR/parity-shell.pid}
shell_pid() {
  local p t
  [ -f "$SHELL_PID" ] && read -r p t < "$SHELL_PID" && [ -n "$t" ] || return 1
  [ "$(sed 's/.*) //' "/proc/$p/stat" 2>/dev/null | cut -d' ' -f20)" = "$t" ] && echo "$p"
}
# A shell something else started (the session's own, a bench script's) is found by its name.
other_shell() { pgrep -x qs >/dev/null || pgrep -x quickshell >/dev/null || pgrep -x impulse-shell >/dev/null; }
stop_shell() {
  local p
  p=$(shell_pid) && kill "$p"; pkill -x qs; pkill -x quickshell; pkill -x impulse-shell
  for _ in $(seq 30); do shell_pid >/dev/null || other_shell || return 0; sleep 0.2; done
  p=$(shell_pid) && kill -9 "$p"; pkill -9 -x qs; pkill -9 -x quickshell; pkill -9 -x impulse-shell
}

# start_shell orig|native LOG [first]: launch from the Hyprland seat session and wait until the bar is
# up and the screen has stopped changing. Both shells share states.json, so every start gets the same
# one: default sidebar bottom group, default overlay widget positions (ptr_overlay drags them), and
# - by default the state of a restart inside one login: this Hyprland instance's signature,
#   "Keep awake" off (KEEP_INHIBIT=1: left on);
# - with `first` the state the first start after a login finds: the signature of another Hyprland
#   instance and "Keep awake" left on by it, which the original drops for a new instance (services/Idle.qml).
# relaunch TAG [first]: restart the shell under test (MODE/OUT from capture.sh), log to shell-TAG.log.
relaunch() { start_shell "$MODE" "$OUT/shell-$1.log" "${2:-}"; }

STATES=$HOME/.local/state/quickshell/states.json
start_shell() {
  local sig=$HYPRLAND_INSTANCE_SIGNATURE inhibit=false
  [ "${3:-}" = first ] && sig=parity-previous-login inhibit=true
  [ -n "${KEEP_INHIBIT:-}" ] && inhibit=true
  stop_shell
  mkdir -p "$(dirname "$STATES")"; [ -s "$STATES" ] || echo '{}' > "$STATES"
  jq --arg s "$sig" --argjson i "$inhibit" '.sidebar.bottomGroup = {"collapsed": false, "tab": 0} | del(.overlay)
    | .hyprlandInstanceSignature = $s | .idle.inhibit = $i' "$STATES" > "$STATES.tmp" && mv "$STATES.tmp" "$STATES" ||
    die "cannot reset $STATES"
  rm -rf "$HOME/.cache/quickshell/notifications"
  reset_volume
  pin_home_times
  hdsp "hl.dsp.exec_cmd(\"BIN=${BIN:-} ORIG_TREE=$ORIG_TREE SHELL_PID=$SHELL_PID bash $PARITY/launch.sh $1 $2\")"
  wait_for 60 ipc bar open || { log "shell did not answer IPC"; return 1; }
  wait_for 30 bar_layer && wait_for 30 layer background || { log "shell has no bar or background"; return 1; }
  sleep "${BOOT_SETTLE:-3}"
  settle "$TMPF/boot.ppm" 30 || log "screen still changing 30 s after start"
}

# Pointer: real events from an absolute uinput device (ptr.py, python-evdev), in output pixels. The
# compositor puts the pointer exactly there, and a miss stops the run: a click somewhere else
# silently changes what every later scene shows.
PY=${PARITY_PY:-python3}
cursor_at() { hyprctl cursorpos | tr -d ','; }
ptr_do() {
  local want="${*: -2}"; [ "$1" = click ] && want="$2 $3"
  "$PY" "$PARITY/ptr.py" "$@" || die "ptr.py $* failed"
  [ "$(cursor_at)" = "$want" ] || die "ptr.py $*: the cursor is at $(cursor_at), not at $want"
}
park() { ptr 1300 700; }
ptr() { ptr_do move "$1" "$2"; }
click() { ptr_do click "$1" "$2" "${3:-left}"; }
# drag X Y DX DY: press at (X,Y), move by (DX,DY) in 20 steps, release.
drag() { ptr_do drag "$1" "$2" $(($1 + $3)) $(($2 + $4)); }
# hold X Y: press the left button at (X,Y) and keep it down; release lets it go at the same place.
# Between the two the scene shoots what only a held button shows (the ripple that has filled it).
hold() {
  coproc HOLD { "$PY" "$PARITY/ptr.py" hold "$1" "$2"; }
  local down
  read -r -t 10 -u "${HOLD[0]}" down && [ "$down" = down ] || die "ptr.py hold $1 $2 failed"
  [ "$(cursor_at)" = "$1 $2" ] || die "ptr.py hold $1 $2: the cursor is at $(cursor_at), not at $1 $2"
}
release() {
  local pid=$HOLD_PID
  echo >&"${HOLD[1]}"
  wait "$pid" || die "ptr.py hold did not release the button"
}
key() { wtype "$@"; }

# cfg JQ_PATH VALUE: edit the live config (both shells watch config.json) and wait for reload.
cfg() {
  jq "$1 = $2" "$CFG" > "$CFG.tmp" && mv "$CFG.tmp" "$CFG"
  sleep "${CFG_SETTLE:-3}"
}

# settle FILE [MAX_S]: screenshots until the screen has shown one frame for QUIET_MS, i.e. it has
# stopped changing (pointer not drawn). FILE is that frame; after MAX_S (default 8) it is the last
# one and settle fails. Two equal screenshots in a row are not enough: an animation stands still
# in the middle when the shell is busy (the original's first password symbol on the lock screen is
# 0.3 s late on a cold start, test_settle.sh), and QUIET_MS has to be longer than any such pause.
# The time counts from after the first screenshot of the frame to before the last one. SETTLED is
# how long settle looked and the longest pause it saw between two changes.
QUIET_MS=${QUIET_MS:-1000}
TMPF=${TMPF:-/dev/shm/parity}; mkdir -p "$TMPF"
settle() {
  local f=$1 prev=$TMPF/prev.ppm t0 now since end pause=0 ok=1
  t0=$(mono_ms); end=$(( t0 + ${2:-8} * 1000 ))
  grim -t ppm "$f"; since=$(mono_ms)
  while :; do
    mv "$f" "$prev"; sleep 0.1; now=$(mono_ms); grim -t ppm "$f"
    if ! cmp -s "$prev" "$f"; then
      [ $(( now - since )) -le "$pause" ] || pause=$(( now - since ))
      since=$(mono_ms)
    elif [ $(( now - since )) -ge "$QUIET_MS" ]; then ok=0; break
    fi
    [ "$now" -lt "$end" ] || break
  done
  SETTLED="$(( $(mono_ms) - t0 )) ms, longest pause $pause ms"
  return $ok
}

# shot NAME [DELAY]: full-output screenshot once the screen is still, looking DELAY seconds after
# the action (what it starts may take that long to begin). A screen that never stops (a live graph,
# a recording timer) is shot as it is and listed in unsettled.txt.
shot() {
  sleep "${2:-$SETTLE}"
  settle "$OUT/$1.ppm" || echo "$1" >> "$OUT/unsettled.txt"
  log "shot $1 ($SETTLED)"
}

# shot_twice NAME DELAY UNDO DO: the shot of what DO draws, taken when doing it twice in a row ended
# on the same frame; UNDO takes it back in between. For a step that can be drawn later than any
# QUIET_MS the first time (a first paint on a cold start): no rule about time tells that pause from
# the end, a second go that ends on another frame does. After 4 goes the last frame is the shot and
# NAME is listed in unsettled.txt: a shell that draws the step differently every time, or late
# every time, shows it.
shot_twice() {
  local name=$1 delay=$2 undo=$3 act=$4 f=$OUT/$1.ppm i same=
  for i in 1 2 3 4; do
    [ "$i" = 1 ] || { mv "$f" "$TMPF/once.ppm"; "$undo"; }
    "$act"; sleep "$delay"; settle "$f" || break
    [ "$i" = 1 ] || ! cmp -s "$f" "$TMPF/once.ppm" || { same=1; break; }
  done
  [ -n "$same" ] || echo "$name" >> "$OUT/unsettled.txt"
  log "shot $name (go $i, $SETTLED)"
}

# idle_check SCENE: every scene must leave the shell as start_shell left it, or what it left open
# is on every later shot. Compares the still screen with the one after the first start (idle.png)
# outside the live-data masks; a scene that fails is listed in not-idle.txt (compare.py reports
# it as a tool error), its screen kept as idle-SCENE.png, and the shell restarted.
idle_check() {
  park; sleep 0.5
  settle "$TMPF/idle.ppm" || true
  if [ ! -f "$OUT/idle.ppm" ]; then cp "$TMPF/idle.ppm" "$OUT/idle.ppm"; return; fi
  local px
  px=$("$PY" "$PARITY/compare.py" --same "$OUT/idle.ppm" "$TMPF/idle.ppm") && return
  log "scene $1 did not return to idle: $px"
  echo "$1 $px" >> "$OUT/not-idle.txt"
  cp "$TMPF/idle.ppm" "$OUT/idle-$1.ppm"
  relaunch "idle-$1"
}

# layer_anim NAMESPACE on|off: Hyprland's own open/close animation of a quickshell layer (popin + fade,
# run on the compositor's wall clock and identical for both shells). grim frames ~50 ms apart catch it at
# a different phase every run (orig-vs-orig up to ~90k px >32 on anim_sidebarRight), so anim scenes turn
# it off and their frames show only what the shell draws. Rules are last-match-wins, so "on" restores the
# default; use only for namespaces without a no_anim rule in the dots config.
layer_anim() {
  local v=false; [ "$2" = off ] && v=true
  hyprctl eval "hl.layer_rule({ match = { namespace = \"$(ns):$1\" }, no_anim = $v })" >/dev/null
}

# frames NAME N CMD...: run CMD then grab N screenshots back to back (animation frames).
frames() {
  local name=$1 n=$2; shift 2
  "$@"
  local t0=$(mono_ms)
  for i in $(seq -w 0 $((n - 1))); do
    grim -l 0 -t ppm "$OUT/$name.f$i.ppm"
    echo "$name.f$i $(( $(mono_ms) - t0 ))" >> "$OUT/frames.txt"
  done
  log "frames $name x$n"
}

# real_clock: stop setup.sh's clock pin and take the time back from the RTC, which QEMU keeps at
# the host's time. Package and crate downloads need it: under the pinned date every TLS
# certificate is "not yet valid".
real_clock() {
  sudo pkill -f "[p]arity/clock.sh" || true
  sudo hwclock --hctosys
}

# The null sink setup.sh creates, back at the volume every run starts from.
# The wallpaper selector lists $HOME (the guest has no ~/Pictures/Wallpapers) by modification time,
# and under the pinned clock whatever a run writes to gets the same second: the order of the folders
# was a matter of milliseconds. Every entry gets its own minute, in name order.
pin_home_times() { local f i=0; for f in "$HOME"/*; do i=$((i + 1)); touch -d "@$((1767225600 + i * 60))" "$f"; done; }
reset_volume() { wpctl set-mute @DEFAULT_AUDIO_SINK@ 0 2>/dev/null && wpctl set-volume @DEFAULT_AUDIO_SINK@ 0.40 2>/dev/null; }
