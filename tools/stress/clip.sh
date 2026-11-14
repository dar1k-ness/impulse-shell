#!/bin/bash
# clip.sh RUN [N]: clipboard churn (cliphist store + cliphistService update via the wl-paste watcher), clipboard overview open
RUN=$1; N=${2:-200}; . ~/h/common.sh
need wl-copy
snap clip.before
cliphist wipe 2>/dev/null
for i in $(seq 1 $N); do
  printf 'clip %d %s' $i "$(head -c 300 /dev/urandom | base64 -w0)" | wl-copy; sleep 0.1
  [ $((i%50)) = 0 ] && { $QS ipc call search clipboardToggle >/dev/null; sleep 1.5; $QS ipc call search close >/dev/null; snap clip.$i; }
done
sleep 5; cliphist wipe; $QS ipc call cliphistService update >/dev/null; sleep 5; snap clip.after
