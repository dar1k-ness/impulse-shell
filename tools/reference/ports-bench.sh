#!/bin/bash
# Measurements for crates/qml/src/ports: per operation, wall time and CPU (process +
# children) of the original shell command vs the port, A/B alternated ROUNDS times, and
# exec()s per operation counted with strace. Fake wf-recorder/songrec (exit at once /
# print one match) keep record.sh and recognize-music.sh measurable without recording.
# Usage: tools/reference/ports-bench.sh TEST_BIN [N] [ROUNDS]   (ROUNDS=0: exec counts only)
set -e
TEST_BIN=${1:?qml lib test binary}; N=${2:-20}; ROUNDS=${3:-3}
FAKE=$(mktemp -d)
printf '#!/bin/sh\nexit 0\n' >"$FAKE/wf-recorder"
printf '#!/bin/sh\necho "{\\"matches\\": [{\\"id\\":1}], \\"track\\": {\\"title\\":\\"t\\"}}"\nexec sleep 30\n' >"$FAKE/songrec"
chmod +x "$FAKE"/*
export PATH="$FAKE:$PATH" RI_PORTS_WAYLAND=1
run() { RI_PORTS_BENCH=$1 RI_PORTS_N=$N "$TEST_BIN" --exact ports::tests::bench_ports --include-ignored --nocapture --test-threads 1 2>/dev/null | grep -o 'BENCH.*'; }
for r in $(seq "$ROUNDS"); do run orig; run port; done
# exec()s of one run of the test binary (N iterations of $2); wl-copy stays up to serve the
# clipboard (as under the original), so it is ended once the test process itself has exited.
count() {
    RI_PORTS_BENCH=$1 RI_PORTS_ONLY=$2 RI_PORTS_N=$3 strace -f -q -e trace=execve -o /tmp/st.$$ \
        "$TEST_BIN" --exact ports::tests::bench_ports --include-ignored --test-threads 1 >/dev/null 2>&1 </dev/null &
    local sp=$! main
    sleep 0.2
    main=$(head -1 /tmp/st.$$ | cut -d' ' -f1)
    while ! grep -q "^$main +++ exited" /tmp/st.$$; do sleep 0.1; done
    pkill -x wl-copy || true
    wait $sp
    # Successful exec()s only (PATH lookups fail with ENOENT first).
    grep -cE '(execve\(.*|<\.\.\. execve resumed>)\) += 0$' /tmp/st.$$
}
command -v strace >/dev/null || { echo "--- no strace: exec() counts skipped"; exit 0; }
echo "--- exec() per operation (strace, N=1)"
for op in "cliphist copy" "cliphist image decode" "temp screenshot" "screenshot copy" "region copy" "key press+release" "paste key" "record.sh --region" "recognize-music.sh"; do
    for m in orig port; do
        base=$(count "$m" "$op" 0); one=$(count "$m" "$op" 1)
        echo "EXEC $m $op: $((one - base))"
    done
done
rm -f /tmp/st.$$
