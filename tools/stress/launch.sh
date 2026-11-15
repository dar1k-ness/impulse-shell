#!/bin/bash
# launch.sh MODE LOG PIDFILE [NATIVE_BIN]: native is impulse-shell, orig is Quickshell on the original in
# ~/parity-original (README.md). Run by Hyprland (exec_cmd) so the shell lives in the graphical login
# session: the polkit agent registers for its own session, and pkexec from that session must find it.
cd ~; echo $$ > "$3"
if [ "$1" = native ]; then RUST_BACKTRACE=1 exec setsid "$4" >> "$2" 2>&1 < /dev/null
else exec setsid qs -p ~/parity-original >> "$2" 2>&1 < /dev/null; fi
