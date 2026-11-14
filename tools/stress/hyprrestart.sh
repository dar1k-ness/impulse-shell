#!/bin/bash
# hyprrestart.sh MODE: exit Hyprland under the shell 3 times: shell must exit promptly (no hang, no core); getty autologin restarts Hyprland
# (without autologin it is started here via seatd once no new instance shows up within 15 s).
# `pidwait` returns when the processes it is given have exited: Hyprland gets 2 s to exit by itself, the shell 10 s.
MODE=$1; export RUN=~/stress/$MODE-hypr-$(date +%m%d-%H%M); mkdir -p $RUN; . ~/h/common.sh; export LOG=$RUN/shell.log
for i in 1 2 3; do
  start_shell_proc $MODE $LOG; sleep 10; p=$(spid); alive=$(alive $p && echo yes || echo no)
  old=$HYPRLAND_INSTANCE_SIGNATURE
  t0=$(date +%s%N); hyprctl dispatch 'hl.dsp.exit()' >/dev/null 2>&1; timeout 2 pidwait -x Hyprland; [ $? != 124 ] || pkill -x Hyprland
  timeout 10 pidwait -F $RUN/pid
  if [ $? = 124 ]; then echo "iter$i shell_alive_after_compositor_exit=HANG pid=$p" >> $RUN/events.log; kill -9 $p; pidwait -F $RUN/pid
  else echo "iter$i alive_before=$alive shell_exited_after=$(( ($(date +%s%N)-t0)/1000000 ))ms" >> $RUN/events.log; fi
  for w in $(seq 1 60); do
    [ $w = 15 ] && ! pgrep -x Hyprland >/dev/null && (cd ~; LIBSEAT_BACKEND=seatd setsid Hyprland >$RUN/hypr$i.log 2>&1 </dev/null &)
    n=$(ls -t /run/user/1000/hypr | head -1); [ -n "$n" ] && [ "$n" != "$old" ] && [ -S /run/user/1000/hypr/$n/.socket.sock ] && break; sleep 1
  done; sleep 3; . ~/henv
done
coredumpctl list --since "-10min" --no-pager > $RUN/cores.txt 2>&1; echo DONE >> $RUN/events.log
