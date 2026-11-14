#!/bin/bash
# lock.sh RUN [N]: lock via IPC, type the password, unlock; screenshots locked/unlocked; unlocked frame must match the pre-lock frame.
# The password is the one tools/stress/setup.sh gives the user ($PARITY_PW, as tools/parity/setup.sh).
# Exit 1 unless every round unlocked.
RUN=$1; N=${2:-15}; . ~/h/common.sh
need wtype grim compare faillock
# locked STATE: the compositor's answer to whether it holds a session lock (`hyprctl -j locked`) is
# STATE; asked until it is, for 20 s at most (or $2 times). A fixed wait counted an unlock that
# took longer than it as a round not unlocked.
locked() {
  local i
  for i in $(seq 1 ${2:-100}); do
    hyprctl -j locked | grep -q "\"locked\": $1" && return
    sleep 0.2
  done
  return 1
}
# Three passwords refused within 15 minutes and pam_faillock refuses the right one for 10 more;
# polkit.sh's cancelled requests are counted as refused ones. The rounds start from no record.
faillock --user "$(id -un)" --reset
# A lock left by an earlier scenario (a round that did not unlock, a shell killed while it held
# the lock) would make every round here look the same before and after.
locked false 1 || { echo "lock.sh: the session is locked before the first round" >&2; exit 1; }
snap lock.before; mkdir -p $RUN/lock; grim $RUN/lock/pre.png
ok=0
for i in $(seq 1 $N); do
  $QS ipc call lock activate >/dev/null
  # A round unlocked when the compositor held the lock, holds it no longer, and the screen shows
  # the frame it had before. Without a lock the password is not typed: it would go to the desktop.
  d=not_locked
  if locked true; then
    sleep 3; grim $RUN/lock/locked$i.png 2>/dev/null
    wtype "${PARITY_PW:-parity}"; sleep 0.3; wtype -k Return
    d=still_locked
    # The desktop is drawn again a moment after the lock is gone.
    if locked false; then
      sleep 1; grim $RUN/lock/un$i.png 2>/dev/null
      d=$(compare -metric RMSE $RUN/lock/pre.png $RUN/lock/un$i.png null: 2>&1 | sed 's/.*(\(.*\))/\1/')
      awk -v d="$d" 'BEGIN { exit !(d+0 < 0.05) }' && ok=$((ok+1))
    fi
  fi
  echo "$(date +%s) lock.$i unlocked_rmse=$d" >> $RUN/events.log; snap lock.$i
done
sleep 5; snap lock.after
echo "$(date +%s) lock unlocked_ok=$ok/$N native_log_locked=$(grep -c 'session lock: locked' $LOG) released=$(grep -c 'session lock: released' $LOG)" >> $RUN/events.log
[ $ok = $N ] || { echo "lock.sh: $ok of $N rounds unlocked" >&2; exit 1; }
