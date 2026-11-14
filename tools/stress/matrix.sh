#!/bin/bash
# matrix.sh MODE(native|orig) [scenarios...]: each scenario in turn on one shell; $RUN/results.txt gets a line
# "SCENARIO RESULT" for each (OK, FAIL, CRASH, RESTARTED, HANG) and DONE at the end. A scenario that ends other than
# OK does not stop the ones after it. Exit 0 when every scenario ended OK.
MODE=$1; shift; export RUN=~/stress/$MODE${TAG:+-$TAG}-$(date +%m%d-%H%M); mkdir -p $RUN; . ~/h/common.sh
SC=${*:-"notify_flood panels hotplug services netchurn reload windows clip volume mpris config lock polkit osk idle"}
export LOG=$RUN/shell.log
start_shell() { start_shell_proc $MODE $LOG; sleep 10; snap start; }
start_shell
( ~/h/sampler.sh $RUN 5 & echo $! > $RUN/sampler.pid )
( ~/h/ipcprobe.sh $RUN 2 & echo $! > $RUN/probe.pid )
since=$(date '+%Y-%m-%d %H:%M:%S')
bad=0
for s in $SC; do
  p0=$(spid); t0=$(date +%s); c0=$(cputicks $p0); e0=$(errcount)
  echo "== $s start pid=$p0" >> $RUN/events.log; snap $s.begin
  timeout 1800 ~/h/$s.sh $RUN; rc=$?
  p1=$(spid); t1=$(date +%s); c1=$(cputicks $p1); e1=$(errcount)
  # CRASH: the shell left during the scenario. FAIL: the shell is as it was, and the scenario
  # itself gave up (a program it needs is missing, a round did not end as it must).
  if [ -z "$p1" ]; then res=CRASH; elif [ "$p0" != "$p1" ]; then res=RESTARTED;
  elif ! timeout 10 $QS ipc call bar open >/dev/null 2>&1; then res=HANG;
  elif [ $rc != 0 ]; then res=FAIL; else res=OK; fi
  ipcmax=$(awk -v a=$t0 -v b=$t1 '$1>=a && $1<=b { if ($2<0) to++; else if ($2>m) m=$2 } END { printf "%d/%d", m, to }' $RUN/ipc.txt 2>/dev/null)
  snap $s.end
  # A shell that is gone has no CPU time to read: no figure, and no arithmetic on nothing.
  cpu=-; [ -n "$c0" ] && [ -n "$c1" ] && cpu=$((c1-c0))
  echo "== $s end rc=$rc result=$res dur=$((t1-t0))s cpu=$cpu err=$((${e1:-0}-${e0:-0})) ipcmax=$ipcmax" >> $RUN/events.log
  echo "$s $res" >> $RUN/results.txt
  [ $res = OK ] || bad=$((bad+1))
  # The next scenario gets a shell again; after FAIL it still has the one it had.
  case $res in CRASH|RESTARTED|HANG) start_shell ;; esac
done
kill $(cat $RUN/sampler.pid) $(cat $RUN/probe.pid)
coredumpctl list --since "$since" --no-pager > $RUN/cores.txt 2>&1
journalctl --user --since "$since" --no-pager | grep -iE 'qs|quickshell|rusty-impulse' | tail -50 > $RUN/journal.txt
echo DONE >> $RUN/results.txt
# 0: every scenario ended OK.
[ $bad = 0 ]
