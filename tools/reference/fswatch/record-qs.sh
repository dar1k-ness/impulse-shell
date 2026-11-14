#!/usr/bin/env bash
# Record real Quickshell's FileView/FolderListModel reactions to tools/reference/fswatch/steps.sh into the
# table the native test compares against (crates/qml/tests/fswatch/qs-expected.txt).
# Needs `qs` (any Arch box: pacman -S quickshell); runs headless (QT_QPA_PLATFORM=offscreen).
# Usage: tools/reference/fswatch/record-qs.sh > crates/qml/tests/fswatch/qs-expected.txt
set -eu
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)
base=$(mktemp -d /tmp/fswatch.XXXXXX)
cfg=$(mktemp -d /tmp/fswatch-cfg.XXXXXX)
log=$cfg/log
trap 'kill $qs 2>/dev/null || true; rm -rf "$base" "$cfg"' EXIT
sed "s|@BASE@|$base|" "$repo/crates/qml/tests/fswatch/watch.qml" > "$cfg/shell.qml"
steps=()
while IFS=$'\t' read -r name cmd; do
    [[ -z $name || $name == \#* ]] && continue
    steps+=("$name"$'\t'"$cmd")
done < "$here/steps.sh"
(cd "$base" && eval "${steps[0]#*$'\t'}")
QT_QPA_PLATFORM=offscreen qs -p "$cfg/shell.qml" > "$log" 2>&1 &
qs=$!
sleep 2
marks=()
for s in "${steps[@]:1}"; do
    marks+=("$(date +%s%3N) ${s%%$'\t'*}")
    (cd "$base" && eval "${s#*$'\t'}")
    sleep 0.6
done
marks+=("$(date +%s%3N) end")
kill $qs
sed -n 's/.*EV \([0-9]*\) \(.*\)$/\1 \2/p' "$log" > "$cfg/ev"
printf '%s\n' "${marks[@]}" | awk -v evf="$cfg/ev" '
    BEGIN { while ((getline l < evf) > 0) ev[++ne] = l }
    { mt[NR] = $1; mn[NR] = $2 }
    END {
        print "# step a link nd ctrl self trig ad wr fl"
        fl = "-"
        for (j = 1; j <= ne; j++) { split(ev[j], f, " "); if (f[1] + 0 < mt[1] && f[2] == "fl") fl = f[3] }
        for (i = 1; i < NR; i++) {
            split("a link nd ctrl self trig ad wr", ks, " ")
            for (k in ks) c[ks[k]] = 0
            for (j = 1; j <= ne; j++) {
                split(ev[j], f, " ")
                if (f[1] + 0 < mt[i] || f[1] + 0 >= mt[i + 1]) continue
                if (f[2] == "fl") fl = f[3]; else c[f[2]]++
            }
            printf "%s %d %d %d %d %d %d %d %d %s\n", mn[i], c["a"], c["link"], c["nd"], c["ctrl"], c["self"], c["trig"], c["ad"], c["wr"], fl
        }
    }'
