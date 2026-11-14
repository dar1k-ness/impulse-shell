#!/bin/sh
# What Quickshell 0.3.2 leaves in each scene's `result`: the `// qs:` line of the scene.
#   tools/ci/gate --run as-tester sh /src/crates/shell/tests/scenes/script_model_current/qs.sh [NAME...]
# Each scene runs as the one window of a shell of its own, under the image's sway.
here=$(cd "$(dirname "$0")" && pwd)
[ $# -gt 0 ] || set -- $(cd "$here" && ls *.qml | sed 's/\.qml$//')
run=$(mktemp -d)
for name; do
    mkdir "$run/$name"
    cp "$here/$name.qml" "$run/$name/Scene.qml"
    cat > "$run/$name/shell.qml" <<QML
import QtQuick
import Quickshell
Scope {
    Scene { onResultChanged: if (result !== "") { console.log("// qs: " + result); Qt.quit() } }
}
QML
    LIBGL_ALWAYS_SOFTWARE=1 timeout 30 qs --no-color -p "$run/$name" 2>&1 | sed -n "s|.*// qs: |// qs: $name: |p"
done
rm -rf "$run"
