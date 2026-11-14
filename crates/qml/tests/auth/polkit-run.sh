#!/bin/bash
# Runs inside a logind session (systemd-run -p PAMName=ri-session).
# Every step waits for the event it needs: a line of the agent's output (read from its FIFO
# as it is written), a process exit, or polkitd's journal entry for the re-registration.
say() { echo "### $*"; }
say "logind session: XDG_SESSION_ID=$XDG_SESSION_ID"
rm -f /tmp/pk.in /tmp/pk.out /tmp/pk-agent.log; mkfifo /tmp/pk.in /tmp/pk.out; exec 3<>/tmp/pk.in; : > /tmp/pk-agent.log
/tmp/authcheck /tmp/polkittest.qml <&3 >/tmp/pk.out 2>&1 & AGENT=$!
exec 4</tmp/pk.out
# Copies the agent's output to its log until a line containing $1.
await() {
  local l
  while IFS= read -r l <&4; do
    printf '%s\n' "$l" >> /tmp/pk-agent.log
    case $l in *"$1"*) return 0 ;; esac
  done
  echo "agent output ended before: $1"; return 1
}
next() { await 'console: prompt'; }
await 'isRegistered true'
say "1) pkexec true: wrong password, then correct one"
( pkexec true; echo "pkexec exit=$?" ) 2>&1 & P=$!
next; echo wrong-pass >&3; next; echo ubuntu-pw-1 >&3; wait $P
say "2) pkexec true: user cancels in the agent"
( pkexec true; echo "pkexec exit=$?" ) 2>&1 & P=$!
next; echo cancel >&3; wait $P
say "3) pkcheck --allow-user-interaction, correct password"
( pkcheck --action-id org.freedesktop.policykit.exec --process $$ --allow-user-interaction; echo "pkcheck exit=$?" ) 2>&1 & P=$!
next; echo ubuntu-pw-1 >&3; wait $P
say "4) pkcheck interrupted while the prompt is open -> polkitd CancelAuthentication"
# An asynchronous command ignores SIGINT unless its disposition is reset.
env --default-signal=INT pkcheck --action-id org.freedesktop.policykit.exec --process $$ --allow-user-interaction 2>&1 & P=$!
next; kill -INT $P; wait $P; echo "pkcheck exit=$?"
await 'authenticationRequestCancelled'
say "5) restart polkitd, agent must re-register"
cur=$(sudo -n journalctl -u polkit -n 1 -o cat -q --show-cursor | sed -n 's/^-- cursor: //p')
sudo -n systemctl restart polkit
coproc J { sudo -n journalctl -u polkit -f -o cat --after-cursor="$cur"; }
while IFS= read -r l <&"${J[0]}"; do
  case $l in "Registered Authentication Agent"*) echo "polkitd: $l"; break ;; esac
done
kill $J_PID; wait $J_PID
( pkexec true; echo "pkexec exit=$?" ) 2>&1 & P=$!
next; echo ubuntu-pw-1 >&3; wait $P
say "6) two requests at once are queued"
( pkexec true; echo "pkexec A exit=$?" ) 2>&1 & A=$!
await 'authenticationRequestStarted'
( pkexec /bin/echo second; echo "pkexec B exit=$?" ) 2>&1 & B=$!
next; echo ubuntu-pw-1 >&3; next; echo ubuntu-pw-1 >&3; wait $A $B
echo quit >&3
while IFS= read -r l <&4; do printf '%s\n' "$l" >> /tmp/pk-agent.log; done
wait $AGENT
say "agent log:"; cat /tmp/pk-agent.log
