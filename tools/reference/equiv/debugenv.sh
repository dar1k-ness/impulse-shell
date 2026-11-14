#!/bin/bash
# debugenv.sh BASE_BIN_DIR HEAD_BIN_DIR [ROOT]: equivalence of the RI_LOG/RI_DEBUG settings to the
# II_* variables they replace. Runs riscen/riflood from BASE (II_*) and HEAD (RI_*) and diffs their
# stderr (`exact`; `shape` = digits masked, for timings; `set` = distinct masked lines, for output
# whose count depends on timing) and the riflood world dump (switches).
# Each side runs twice: a case passes when BASE vs HEAD differs no more than a side differs from
# itself (timing noise); lines are sorted and temp-dir PIDs masked first. `+N` = lines over the
# no-flag baseline (the flag produced output); for dumps `eff=N` = lines the switch changed in the
# world against the no-flag dump (the switch took effect).
set -u
B=$1 H=$2 ROOT=${3:-$(cd "$(dirname "$0")/../../.." && pwd)/qml}
T=$(mktemp -d); fail=0
norm() { sed -E 's#/tmp/ri(scen|flood)-[0-9]+#/tmp/ri\1-P#g' "$1" | sort; }
mask() { norm "$1" | sed -E 's/[0-9]+(\.[0-9]+)?/#/g' | sort; }
uniqm() { mask "$1" | uniq; }
nd() { diff "$1" "$2" | grep -c '^[<>]'; }
run() { # run DIR BIN ENV... -> stdout: stderr file
  local d=$1 bin=$2; shift 2
  local o=$T/$RANDOM$RANDOM
  env -u RI_LOG -u RI_DEBUG "$@" RI_DUMP=$o.dump timeout 300 "$d/$bin" "$ROOT" $ARGS >/dev/null 2>$o.err
  echo $o
}
cmp_case() { # cmp_case MODE BIN "OLD ENV" "NEW ENV"
  local mode=$1 bin=$2 old=$3 new=$4 f=err x r
  [ "$mode" = dump ] && f=dump
  local b1 b2 h1 h2; b1=$(run $B $bin $old); b2=$(run $B $bin $old); h1=$(run $H $bin $new); h2=$(run $H $bin $new)
  for x in $b1 $b2 $h1 $h2; do case $mode in shape) mask $x.$f ;; set) uniqm $x.$f ;; *) norm $x.$f ;; esac >$x.n; done
  local nb nh noise cross
  nb=$(nd $b1.n $b2.n); nh=$(nd $h1.n $h2.n); noise=$((nb > nh ? nb : nh))
  cross=$(nd $b1.n $h1.n); x=$(nd $b2.n $h2.n); [ $x -lt $cross ] && cross=$x
  local lines; lines=$(wc -l <$b1.n)
  r=ok; [ "$cross" -gt "$noise" ] && { r=FAIL; fail=1; }
  [ -z "${BASELINE[$bin$mode]:-}" ] && { BASELINE[$bin$mode]=$lines; cp $b1.n $T/base.$bin$mode; }
  local extra="+$((lines - BASELINE[$bin$mode]))"
  [ "$mode" = dump ] && extra="eff=$(nd $T/base.$bin$mode $b1.n)"
  printf '%-5s %-6s %-7s lines=%-6s %-8s noise=%-5s diff=%-5s %s -> %s\n' "$r" "$mode" "$bin" "$lines" "$extra" "$noise" "$cross" "$old" "$new"
}
declare -A BASELINE
ARGS="hotplug 10"
cmp_case exact riscen "X=" "X="
cmp_case exact riscen "RI_VAL=width,height" "RI_LOG=ri::val RI_DEBUG=val=width+height"
cmp_case exact riscen "RI_PLANDBG=1" "RI_LOG=ri::plandbg"
cmp_case exact riscen "RI_PLANCHECK=1" "RI_DEBUG=plancheck"
cmp_case exact riscen "RI_DEPTRACE=width" "RI_LOG=ri::deptrace RI_DEBUG=deptrace=width"
cmp_case shape riscen "RI_WSTAT=1" "RI_LOG=ri::wstat"
cmp_case shape riscen "RI_STATS=1" "RI_LOG=ri::stats"
cmp_case shape riscen "RI_PROF=0" "RI_LOG=ri::prof RI_DEBUG=prof=0"
cmp_case shape riscen "RI_AOTSTAT=1" "RI_LOG=ri::aotstat"
cmp_case shape riscen "RI_LREV=1" "RI_LOG=ri::lrev"
cmp_case shape riscen "RI_VIEW=1" "RI_LOG=ri::view"
cmp_case shape riscen "RI_DPROF=1" "RI_LOG=ri::dprof"
cmp_case shape riscen "RI_HPROF=1" "RI_LOG=ri::hprof"
# riflood without a flood (N=0 HIST=0): the loaded shell's world, dumped after it idles.
ARGS="0 0"
cmp_case shape riflood "X=" "X="
cmp_case set   riflood "RI_VAL=1" "RI_LOG=ri::val"
cmp_case dump riflood "X=" "X="
for s in NOPLAN NOPOST NOVIRT NODEFER NOLAZY NODELEGMEMO NODELEGFAST NOMOVED NOFREEZE FIFO LREVALL NOPOLISH; do
  cmp_case dump riflood "II_$s=1" "RI_DEBUG=${s,,}"
done
rm -rf "$T"
exit $fail
