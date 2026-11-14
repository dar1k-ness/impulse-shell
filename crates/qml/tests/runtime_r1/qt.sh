#!/bin/sh
# Writes what Qt prints for each fixture into <name>.qt. Run in the tools/testenv image:
#   tools/ci/gate --run as-tester /src/crates/qml/tests/runtime_r1/qt.sh
cd "$(dirname "$0")" || exit 1
for f in edit_state edit_signals length handler_order item_props dynamic_keys dynamic_key_paths \
    text_cache cut clip; do
    QT_QPA_PLATFORM=offscreen QT_FORCE_STDERR_LOGGING=1 timeout 60 qml6 "$f.qml" 2>&1 |
        sed -n '/LOG</,/>LOG/p' | sed -e 's/^.*LOG<//' -e '/^>LOG$/d' > "$f.qt"
    echo "$f: $(wc -l < "$f.qt") lines"
done
