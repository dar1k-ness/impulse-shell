#!/bin/sh
# Writes scene.qt: what Qt 6.12 (qml6, offscreen) gives for scene.qml's rows().
# Run in the testenv image: tools/ci/gate --run as-tester crates/shell/tests/scenes/url_hosts/dump.sh
set -eu
dir=$(cd "$(dirname "$0")" && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cat > "$tmp/main.qml" <<QML
import QtQuick
Item {
    Loader {
        source: "file://$dir/scene.qml"
        onLoaded: { console.log("ROWS\n" + item.rows() + "\nEND"); Qt.quit() }
    }
}
QML
QT_QPA_PLATFORM=offscreen QT_FORCE_STDERR_LOGGING=1 QT_MESSAGE_PATTERN='%{message}' qml6 "$tmp/main.qml" 2> "$tmp/log"
sed -n '/^ROWS$/,/^END$/p' "$tmp/log" | sed '1d;$d' > "$tmp/rows"
test -s "$tmp/rows" || { cat "$tmp/log" >&2; exit 1; }
cp "$tmp/rows" "$dir/scene.qt"
