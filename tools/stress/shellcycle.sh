#!/bin/bash
# shellcycle.sh MODE [N]: start the shell, wait for IPC, SIGTERM it; N times. Logs startup ms, RSS/fds when up,
# exit time after SIGTERM (HANG if > 10 s) and any coredumps. `pidwait -F` returns when the shell has exited.
MODE=$1; N=${2:-20}; export RUN=~/stress/$MODE-cycle-$(date +%m%d-%H%M); mkdir -p $RUN; . ~/h/common.sh; export LOG=$RUN/shell.log
for i in $(seq 1 $N); do
  start_shell_proc $MODE $LOG; sleep 5; snap cycle.$i; p=$(spid)
  t0=$(date +%s%N); kill -TERM $p; timeout 10 pidwait -F $RUN/pid
  if [ $? = 124 ]; then echo "$(date +%s) cycle.$i exit=HANG" >> $RUN/events.log; kill -9 $p; pidwait -F $RUN/pid
  else echo "$(date +%s) cycle.$i exit_ms=$(( ($(date +%s%N)-t0)/1000000 ))" >> $RUN/events.log; fi
done
coredumpctl list --since "-30min" --no-pager > $RUN/cores.txt 2>&1; echo DONE >> $RUN/events.log
