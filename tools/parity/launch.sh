#!/bin/bash
# launch.sh orig|native LOG: exec the shell under test (run from the Hyprland session).
# Env: BIN (native binary), ORIG_TREE (the original QML tree; lib.sh), SHELL_PID (the file this
# process's pid and start time go to; lib.sh).
echo "$$ $(cut -d' ' -f22 "/proc/$$/stat")" > "$SHELL_PID"
if [ "$1" = native ]; then exec "${BIN:-$HOME/rusty-impulse/target/release/rusty-impulse}" > "$2" 2>&1 < /dev/null
else exec qs -p "$ORIG_TREE" > "$2" 2>&1 < /dev/null; fi
