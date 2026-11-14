#!/bin/bash
# ipcprobe.sh RUNDIR interval: IPC round-trip latency (ms, -1 = timeout 10 s) into ipc.txt
RUN=$1; . ~/h/common.sh; I=${2:-2}
while true; do echo "$(date +%s) $(ipcms 10)" >> $RUN/ipc.txt; sleep $I; done
