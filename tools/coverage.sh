#!/bin/sh
# Test coverage with cargo-llvm-cov (rustc's LLVM source-based coverage,
# -C instrument-coverage). ri-testenv has cargo-llvm-cov and llvm-tools; elsewhere:
#   rustup component add llvm-tools && cargo install --locked cargo-llvm-cov@0.9.1
#
#   tools/coverage.sh                          # whole workspace, per-file table
#   tools/coverage.sh -p qsg --lib             # any cargo-llvm-cov arguments instead
#   tools/coverage.sh --html                   # + target/llvm-cov/html/index.html
#   COVERAGE_FAIL_UNDER=60 tools/coverage.sh   # exit 1 under 60% line coverage
#
# Test failures still produce a report (--ignore-run-fail): passing tests is ri-gate's
# job, this only measures what they reach. Run it as ri-gate runs cargo (as-tester in
# the container) so the equivalence tests find their reference tools.
set -eu
if ! cargo llvm-cov --version >/dev/null 2>&1; then
    echo "coverage.sh: cargo-llvm-cov missing (see the header of $0)" >&2
    exit 2
fi
[ $# -gt 0 ] || set -- --workspace --features shell/interp
exec cargo llvm-cov --release --ignore-run-fail \
    ${COVERAGE_FAIL_UNDER:+--fail-under-lines "$COVERAGE_FAIL_UNDER"} "$@"
