#!/bin/bash
# Checks that lib.sh follows and stops the shell it started whatever the binary's file is named,
# without a guest:
#   bash tools/parity/test_shell.sh
export HYPRLAND_INSTANCE_SIGNATURE=test XDG_RUNTIME_DIR
XDG_RUNTIME_DIR=$(mktemp -d); trap 'kill "$pid" 2>/dev/null; rm -rf "$XDG_RUNTIME_DIR"' EXIT
here=$(dirname "$0")
. "$here/lib.sh"
# No shell is found or stopped by name here: the machine's own qs or rusty-impulse is not this test's.
pgrep() { return 1; }
pkill() { return 1; }
# A "native binary" that stays up and is not named rusty-impulse, started as start_shell starts one.
# launch.sh takes the step that pins the uptime (a mount namespace through sudo) as done: that step
# needs root, and the shell it starts is then a child of sudo, not the process started here.
export BIN=$XDG_RUNTIME_DIR/any-name SHELL_PID PARITY_UPTIME_PINNED=1
printf '#!/bin/bash\nexec sleep 600\n' > "$BIN"; chmod +x "$BIN"
bash "$here/launch.sh" native "$XDG_RUNTIME_DIR/log" & pid=$!
sleep 0.5

fail=0
bad() { echo "FAIL: $*"; fail=1; }
[ "$(shell_pid)" = "$pid" ] || bad "the running shell is pid $pid, shell_pid says '$(shell_pid)'"
stop_shell
! kill -0 "$pid" 2>/dev/null || { bad "stop_shell left the shell running"; kill "$pid"; }
wait "$pid" 2>/dev/null
[ -z "$(shell_pid)" ] || bad "the shell is gone, shell_pid still says '$(shell_pid)'"
# Another process that got the pid of a shell that is gone is not that shell.
echo "$$ 1" > "$SHELL_PID"
[ -z "$(shell_pid)" ] || bad "a process with another start time is taken for the shell"
[ $fail = 0 ] && echo "ok"
exit $fail
