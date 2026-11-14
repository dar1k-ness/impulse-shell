#!/bin/bash
# sampler.sh RUNDIR interval
RUN=$1; . ~/h/common.sh; I=${2:-5}
echo "t pid rss anon fds thr maps cpu" > $RUN/samples.txt
while true; do
  p=$(spid)
  if [ -n "$p" ]; then
    echo "$(date +%s) $p $(awk '/VmRSS/{r=$2}/RssAnon/{a=$2}/Threads/{t=$2}END{print r,a}' /proc/$p/status) $(ls /proc/$p/fd 2>/dev/null|wc -l) $(awk '/Threads/{print $2}' /proc/$p/status) $(wc -l < /proc/$p/maps) $(awk '{print $14+$15}' /proc/$p/stat)" >> $RUN/samples.txt
  else echo "$(date +%s) DEAD" >> $RUN/samples.txt; fi
  sleep $I
done
