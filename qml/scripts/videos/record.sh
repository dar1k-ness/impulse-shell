#!/bin/sh
# The file the original's Hyprland keybinds run to record the screen: `record.sh` (a region picked
# with slurp), `record.sh --region AREA`, `record.sh --fullscreen [--sound]` (the focused
# monitor). A recording that runs is stopped instead. impulse-shell does the work of the original's script
# itself and starts wf-recorder; the shell need not be running.
# impulse-shell is the one in PATH, else the binary of the shell that runs, else an installed one.
for bin in "$(command -v impulse-shell)" $(for pid in $(pidof impulse-shell 2>/dev/null); do echo "/proc/$pid/exe"; done) \
    "$HOME/.local/bin/impulse-shell" /usr/local/bin/impulse-shell /usr/bin/impulse-shell; do
    [ -x "$bin" ] || continue
    # The keybinds for a region run this file when `qs` does not say that a shell is alive.
    # With no `qs` installed that is every press, also the ones a shell took: impulse-shell is asked
    # instead, and a shell that answers has the press.
    [ $# = 0 ] && ! command -v qs >/dev/null 2>&1 &&
        "$bin" -c "${qsConfig:-impulse-shell}" ipc call TEST_ALIVE >/dev/null 2>&1 && exit 0
    exec "$bin" __ri-record "$@"
done
echo "record.sh: no impulse-shell to run" >&2
exit 127
