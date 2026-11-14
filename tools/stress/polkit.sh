#!/bin/bash
# polkit.sh RUN [N]: pkexec through the shell's polkit agent, N rounds alternating the two ways a request ends without
# a password: Cancel clicked in the dialog (user cancel) and pkexec killed (polkitd's CancelAuthentication). Each round
# must end (pkexec rc 126 / gone, dialog closed) and leave no polkit-agent-helper-1 behind; the shell's socket count is
# logged. Exit 1 when a round did not end. pkexec is spawned by Hyprland: an ssh session has no agent (rc 127). Typed passwords are not covered: neither
# wtype nor a uinput keyboard reaches the dialog's field in the guest (both shells).
RUN=$1; N=${2:-30}; . ~/h/common.sh
need python3 pkexec
printf '#!/bin/bash\npkexec --disable-internal-agent true >/dev/null 2>&1; echo $? > %s/pk.rc\n' "$RUN" > $RUN/pkrun.sh
helpers() { pgrep -c -f polkit-agent-helper-1; }
socks() { ls -l /proc/$(spid)/fd 2>/dev/null | grep -c socket; }
snap polkit.before; ok=0; stuck=0; h0=$(helpers); s0=$(socks)
for i in $(seq 1 $N); do
  rm -f $RUN/pk.rc; hyprctl dispatch "hl.dsp.exec_cmd(\"bash $RUN/pkrun.sh\")" >/dev/null; sleep 2.5
  if [ $((i%2)) = 1 ]; then tap 1076 656; else pkill -x pkexec; echo killed > $RUN/pk.rc; fi
  for w in $(seq 1 20); do [ -f $RUN/pk.rc ] && break; sleep 0.5; done; sleep 1
  rc=$(cat $RUN/pk.rc 2>/dev/null || echo STUCK)
  [ $rc = STUCK ] && { stuck=$((stuck+1)); pkill -x pkexec; sleep 1; } || ok=$((ok+1))
  echo "$(date +%s) polkit.$i rc=$rc helpers=$(helpers) socks=$(socks)" >> $RUN/events.log
  [ $((i%10)) = 0 ] && snap polkit.$i
done
# pam_faillock counts a cancelled request as a refused password, and after three the user's
# password is refused for 10 minutes (lock screen, sudo): leave no record behind. The same with
# the original on Quickshell (README.md).
faillock --user "$(id -un)" --reset
sleep 5; snap polkit.after
echo "$(date +%s) polkit ended_ok=$ok/$N stuck=$stuck helpers=$h0->$(helpers) socks=$s0->$(socks) leftover_pkexec=$(pgrep -c pkexec)" >> $RUN/events.log
[ $stuck = 0 ] || { echo "polkit.sh: $stuck of $N requests did not end" >&2; exit 1; }
