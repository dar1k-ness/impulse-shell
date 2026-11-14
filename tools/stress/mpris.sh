#!/bin/bash
# mpris.sh RUN [ROUNDS]: fake MPRIS players appear, change tracks/state quickly and vanish; media controls open meanwhile
RUN=$1; R=${2:-10}; . ~/h/common.sh
need python3
python3 -c 'import gi' 2>/dev/null || { echo "mpris.sh: python-gobject is not installed (bench/vm.sh sync installs it)" >&2; exit 1; }
snap mpris.before
for r in $(seq 1 $R); do
  pids=""; for k in $(seq 1 $((r%3+1))); do python3 ~/h/mpris_fake.py stress$k 0.2 & pids="$pids $!"; done
  sleep 2; $QS ipc call mediaControls toggle >/dev/null; sleep 4; $QS ipc call mediaControls toggle >/dev/null; sleep 2
  kill $pids; wait $pids 2>/dev/null; sleep 1; snap mpris.r$r
done
sleep 5; snap mpris.after
