#!/bin/bash
# restart.sh [ENV=VAL ...]: restart native shell with extra env, wait for it
. ~/h/common.sh; stop_shell impulse-shell; cd ~
(env "$@" setsid ${BIN:-$HOME/impulse-shell/target/release/impulse-shell} > ~/impulse-shell.log 2>&1 < /dev/null &)
sleep 25; pgrep -x impulse-shell >/dev/null && echo started || echo FAILED
