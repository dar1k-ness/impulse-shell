#!/bin/sh
# Writes scene.qt: what Qt 6.12 (qml6) gives for scene.qml's rows() in each locale of `locales`.
# Run in the testenv image: tools/ci/gate --run as-tester crates/shell/tests/scenes/locale_compare/dump.sh
set -eu
dir=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cat > "$tmp/main.qml" <<QML
import QtQml
QtObject {
    property Component scene: Qt.createComponent("file://$dir/scene.qml")
    Component.onCompleted: { console.log("ROWS\n" + scene.createObject(this).rows() + "\nEND"); Qt.quit() }
}
QML
: > "$tmp/out"
while read -r name vars; do
    case "$name" in ''|'#'*) continue ;; esac
    env -u LC_ALL -u LC_COLLATE -u LANG -u LC_NUMERIC $vars QT_QPA_PLATFORM=offscreen QT_FORCE_STDERR_LOGGING=1 \
        QT_MESSAGE_PATTERN='%{message}' qml6 "$tmp/main.qml" 2> "$tmp/log"
    sed -n '/^ROWS$/,/^END$/p' "$tmp/log" | sed '1d;$d' > "$tmp/rows"
    test -s "$tmp/rows" || { cat "$tmp/log" >&2; exit 1; }
    sed "s/^/$name\t/" "$tmp/rows" >> "$tmp/out"
done < "$dir/locales"
cp "$tmp/out" "$dir/scene.qt"
