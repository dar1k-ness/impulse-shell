. ~/henv
# The shell under test and what addresses its IPC. native: impulse-shell on ~/.local/share/impulse-shell (a
# copy of this checkout's qml), `impulse-shell -c impulse-shell` (Quickshell's `qs` does not find it). orig: Quickshell on the original in ~/parity-original
# (README.md: tools/parity/orig.sh puts it there; Quickshell cannot load qml, which imports the
# Impulse module), `qs -p ~/parity-original`. The mode is $MODE of the script that starts the shell;
# a scenario started for that run reads it from $RUN/mode, where start_shell_proc wrote it.
ORIG_TREE=$HOME/parity-original
MODE=${MODE:-$(cat $RUN/mode 2>/dev/null)}
if [ "$MODE" = orig ]; then QS="qs -p $ORIG_TREE" NS=quickshell; else QS="impulse-shell -c impulse-shell" NS=rustshell; fi
NATIVE_BIN=${BIN:-$HOME/impulse-shell/target/release/impulse-shell}
spid() { local p=$(cat $RUN/pid 2>/dev/null); [ -n "$p" ] && [ -d /proc/$p ] && echo $p; }
# CPU ticks of process $1; nothing for no process ("/proc//stat" would be /proc/stat, many lines).
cputicks() { [ -n "$1" ] && awk '{print $14+$15}' /proc/$1/stat 2>/dev/null; }
# errors logged by the shell so far (warnings, JS errors, panics)
errcount() { grep -caE 'ERROR|WARN|TypeError|ReferenceError|panicked|Cannot read|is not defined|^qml: ' ${LOG:-$RUN/shell.log} 2>/dev/null; }
snap() { # label
  local p=$(spid); [ -z "$p" ] && { echo "$(date +%s) $1 DEAD" >> $RUN/events.log; return; }
  local rss=$(awk '/VmRSS/{print $2}' /proc/$p/status) anon=$(awk '/RssAnon/{print $2}' /proc/$p/status)
  local fds=$(ls /proc/$p/fd | wc -l) thr=$(awk '/Threads/{print $2}' /proc/$p/status) maps=$(wc -l < /proc/$p/maps)
  echo "$(date +%s) $1 pid=$p rss=$rss anon=$anon fds=$fds thr=$thr maps=$maps cpu=$(cputicks $p) err=$(errcount)" >> $RUN/events.log
}
# IPC round trip in ms (-1 = no answer within $1 s)
ipcms() { local s=$(date +%s%N); timeout ${1:-10} $QS ipc call bar open >/dev/null 2>&1 || { echo -1; return; }; echo $(( ($(date +%s%N)-s)/1000000 )); }
alive() { [ -d /proc/$1 ] && timeout 5 $QS ipc call bar open >/dev/null 2>&1; }
# Hyprland 0.56 Lua dispatchers
hglobal() { hyprctl dispatch "hl.dsp.global(\"$NS:$1\")" >/dev/null; }
hws() { hyprctl dispatch "hl.dsp.focus({ workspace = $1 })" >/dev/null; }
start_shell_proc() { # mode logfile -> pid in $RUN/pid
  [ "$1" = "${MODE:-native}" ] || { echo "start_shell_proc: mode $1, but \$MODE is ${MODE:-native}" >&2; exit 1; }
  [ "$1" != orig ] || [ -f $ORIG_TREE/shell.qml ] || { echo "no original in $ORIG_TREE: see README.md" >&2; exit 1; }
  echo $1 > $RUN/mode
  [ -n "$CLEAN" ] && rm -f ~/.cache/$NS/notifications/notifications.json
  . ~/henv; stop_shell
  rm -f $RUN/pid; hyprctl dispatch "hl.dsp.exec_cmd(\"env ${RI_LOG:+RI_LOG=$RI_LOG} ${RI_DEBUG:+RI_DEBUG=$RI_DEBUG} bash $HOME/h/launch.sh $1 $2 $RUN/pid $NATIVE_BIN\")" >/dev/null
  local i; for i in $(seq 1 20); do [ -s $RUN/pid ] && break; sleep 0.25; done
  local t0=$(date +%s%N)
  for i in $(seq 1 120); do timeout 3 $QS ipc call bar open >/dev/null 2>&1 && break; sleep 0.5; done
  echo "$(date +%s) startup_ipc_ms=$(( ($(date +%s%N)-t0)/1000000 ))" >> $RUN/events.log
}
# stop_shell [NAMES]: SIGTERM every shell process (or those named NAMES, a pattern) and return when they have exited
# (SIGKILL after 10 s without an exit)
stop_shell() { local n=${1:-'qs|quickshell|impulse-shell'}; pkill -x "$n" || return 0
  timeout 10 pidwait -x "$n"; [ $? != 124 ] || { pkill -9 -x "$n"; pidwait -x "$n"; }; return 0; }
# need PROGRAM...: a scenario names what it runs, and stops when one is missing: without it the
# scenario would do nothing and the shell, left alone, would pass. `bench/vm.sh sync` installs
# them all (tools/stress/setup.sh lists them).
need() {
  local p
  for p in "$@"; do
    command -v "$p" >/dev/null || { echo "${0##*/}: $p is not installed (bench/vm.sh sync installs it)" >&2; exit 1; }
  done
}
# tap X Y: a real pointer click at output pixel (X,Y), through the uinput device of
# tools/parity/ptr.py (python-evdev and /dev/uinput: tools/parity/provision.sh). A click that
# was not made ends the scenario: its rounds would count a shell nothing touched.
PTR=${PTR:-$HOME/impulse-shell/tools/parity/ptr.py}
tap() {
  python3 "$PTR" click "$1" "$2" || { echo "${0##*/}: tap $1 $2: python3 $PTR failed" >&2; exit 1; }
}
