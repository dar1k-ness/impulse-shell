#!/bin/bash
# summ.sh RUNDIR: per scenario result, RSS/anon MB and fds/threads first->last, CPU s, new log errors, max IPC ms/timeouts
echo "| scenario | result | RSS MB | anon MB | fds | threads | CPU s | errors | max IPC ms/timeouts | dur |"
echo "|---|---|---|---|---|---|---|---|---|---|"
awk '
function kv(l,k){ if (match(l, " "k"=[^ ]+")) return substr(l,RSTART+length(k)+2,RLENGTH-length(k)-2); return "" }
/^== .* start/ {s=$2; first=""; last=""; next}
/^== .* end/ {printf "| %s | %s | %d -> %d | %d -> %d | %s -> %s | %s -> %s | %.1f | %s | %s | %s |\n", s, kv($0,"result"), kv(first,"rss")/1024, kv(last,"rss")/1024, kv(first,"anon")/1024, kv(last,"anon")/1024, kv(first,"fds"), kv(last,"fds"), kv(first,"thr"), kv(last,"thr"), kv($0,"cpu")/100, kv($0,"err"), kv($0,"ipcmax"), kv($0,"dur"); next}
/ rss=/ { if (s!="") { if (first=="") first=$0; last=$0 } }
' $1/events.log
grep -h startup_ipc_ms $1/events.log | head -1
