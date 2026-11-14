#!/bin/sh
# Prints expected.qt: run in the fixture's directory, in the image of tools/testenv. The vnc
# platform is the one without a display whose services are QGenericUnixServices; with no
# session bus there is no portal, and Qt starts the `xdg-open` found first in PATH.
d=$(mktemp -d)
mkdir -p "$d/bin" "$d/up/doc"
mkfifo "$d/calls"
printf '#!/bin/sh\nprintf "%%s|%%s\\n" "$#" "$1" > %s/calls\n' "$d" > "$d/bin/xdg-open"
chmod +x "$d/bin/xdg-open"
cp Open.qml "$d/up/doc/"
here=$(pwd)
export PATH="$d/bin:$PATH" XDG_RUNTIME_DIR=$(mktemp -d) HOME=$(mktemp -d) QT_QPA_PLATFORM=vnc LANG=C.UTF-8
unset DBUS_SESSION_BUS_ADDRESS
cd "$d/up/doc"
timeout 120 /opt/qv4-venv/bin/python "$here/qt.py" "$d/up/doc/Open.qml" "$d/calls" 2>/dev/null | sed "s|$d/up/doc|DIR|g; s|$d/up|UP|g"
