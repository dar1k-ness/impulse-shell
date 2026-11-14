#!/bin/sh
# Writes what Qt prints for each fixture into its .qt file. Run in the tools/testenv image:
#   tools/ci/gate --run as-tester /src/crates/qml/tests/text_read_only/qt.sh
cd "$(dirname "$0")" || exit 1
for f in read_only becomes_read_only mask_positions; do
    QT_QPA_PLATFORM=offscreen QT_FORCE_STDERR_LOGGING=1 timeout 60 qml6 $f.qml 2>&1 |
        sed -n '/LOG</,/>LOG/p' | sed -e 's/^.*LOG<//' -e '/^>LOG$/d' > $f.qt
    echo "$f: $(wc -l < $f.qt) lines"
done
