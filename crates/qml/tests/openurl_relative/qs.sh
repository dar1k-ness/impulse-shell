#!/bin/sh
# Prints expected.qs: the same for Quickshell 0.3.2, whose url interceptor the resolved
# address goes through. One call every 40 ms keeps what `xdg-open` writes in the order of
# the calls. `xdg-open` writes to a pipe this script holds open, and as many lines are read
# from it as there were calls that said they opened: each is waited for, however late.
d=$(mktemp -d)
mkdir -p "$d/bin" "$d/up/doc"
mkfifo "$d/calls"
exec 7<>"$d/calls"
printf '#!/bin/sh\nprintf "%%s|%%s\\n" "$#" "$1" > %s/calls\n' "$d" > "$d/bin/xdg-open"
chmod +x "$d/bin/xdg-open"
cp Open.qml shell.qml "$d/up/doc/"
export PATH="$d/bin:$PATH" XDG_RUNTIME_DIR=$(mktemp -d) HOME=$(mktemp -d) QT_QPA_PLATFORM=vnc LANG=C.UTF-8
unset DBUS_SESSION_BUS_ADDRESS
cd "$d/up/doc"
timeout 60 quickshell -p "$d/up/doc" 2>&1 | sed 's/\x1b\[[0-9;]*m//g' | grep -o "CASE .*" | sed 's/^CASE //' > "$d/said"
timeout 60 head -n "$(awk -F'\t' '$3 == "true"' "$d/said" | wc -l)" <&7 > "$d/opened"
awk -F'\t' -v calls="$d/opened" 'BEGIN { OFS = "\t" } { arg = "-"; if ($3 == "true") getline arg < calls; print $1, $2, $3, arg }' "$d/said" \
    | sed "s|$d/up/doc|DIR|g; s|$d/up|UP|g"
