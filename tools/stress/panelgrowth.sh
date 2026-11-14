#!/bin/bash
# panelgrowth.sh TARGET [rounds] [cycles]: native only; anon RSS, live items/objs (ri::wstat) and malloc in-use per round
T=$1; R=${2:-5}; N=${3:-30}; D=${D:-0.1}
export RUN=~/stress/growth-$T-$(date +%m%d-%H%M); mkdir -p $RUN; . ~/h/common.sh
export RI_LOG=ri::wstat; start_shell_proc native $RUN/shell.log; sleep 15; p=$(spid)
st() { sleep 3; local w=$(grep -a '^WSTAT' $RUN/shell.log | tail -1) g=$(grep -a '^GC live=' $RUN/shell.log | tail -1)
  echo "$1 anon=$(awk '/RssAnon/{print int($2/1024)}' /proc/$p/status)MB $(grep -o ' items=[0-9]* live=[0-9]* dead=[0-9]*' <<<"$w" | tail -1) objs=$(grep -o 'objs=[0-9]*' <<<"$w" | cut -d= -f2) bmade=$(grep -o 'bmade=[0-9]*' <<<"$w" | cut -d= -f2) $(grep -o 'allocated=[0-9]*M' <<<"$g")" | tee -a $RUN/growth.txt; }
tg() { rusty-impulse -c rusty-impulse ipc call $T toggle >/dev/null 2>&1; sleep $D; rusty-impulse -c rusty-impulse ipc call $T toggle >/dev/null 2>&1; sleep $D; }
for i in 1 2 3; do tg; done; sleep 3; st r0
for r in $(seq $R); do for i in $(seq $N); do tg; done; sleep 5; st r$r; done
stop_shell
