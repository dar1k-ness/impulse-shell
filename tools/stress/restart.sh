#!/bin/bash
# restart.sh [ENV=VAL ...]: restart native shell with extra env, wait for it
. ~/h/common.sh; stop_shell rusty-impulse; cd ~
(env "$@" setsid ${BIN:-$HOME/rusty-impulse/target/release/rusty-impulse} > ~/rusty-impulse.log 2>&1 < /dev/null &)
sleep 25; pgrep -x rusty-impulse >/dev/null && echo started || echo FAILED
