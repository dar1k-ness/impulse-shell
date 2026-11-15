#!/bin/bash
# launch.sh orig|native LOG: exec the shell under test (run from the Hyprland session).
# Env: BIN (native binary), ORIG_TREE (the original QML tree; lib.sh), SHELL_PID (the file this
# process's pid and start time go to; lib.sh).
# The shell reads its uptime from /proc/uptime ("Up 1h, 23m" in the right sidebar, the clock popup),
# and that file cannot be pinned for the guest: lib.sh takes its durations from it. So the shell alone
# gets a mount namespace of its own in which /proc/uptime is a file that says 1 h 23 min 45 s, and
# everything else about it (user, groups, environment) stays as Hyprland started it.
UPTIME=${XDG_RUNTIME_DIR:-/tmp}/parity-uptime
if [ -z "${PARITY_UPTIME_PINNED:-}" ]; then
  echo "5025.00 39000.00" > "$UPTIME"
  mapfile -d '' -t ENVIRON < <(env -0)
  exec sudo -n unshare -m --propagation slave -- sh -c 'mount --bind "$1" /proc/uptime && shift && exec "$@"' sh "$UPTIME" \
    setpriv --reuid="$(id -u)" --regid="$(id -g)" --init-groups \
    env -i "${ENVIRON[@]}" PARITY_UPTIME_PINNED=1 bash "$0" "$@" > "$2" 2>&1 < /dev/null
fi
unset PARITY_UPTIME_PINNED
echo "$$ $(cut -d' ' -f22 "/proc/$$/stat")" > "$SHELL_PID"
if [ "$1" = native ]; then exec "${BIN:-$HOME/impulse-shell/target/release/impulse-shell}" > "$2" 2>&1 < /dev/null
else exec qs -p "$ORIG_TREE" > "$2" 2>&1 < /dev/null; fi
