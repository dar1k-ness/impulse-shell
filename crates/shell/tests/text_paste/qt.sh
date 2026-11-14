#!/bin/sh
# Writes what Qt prints for paste.qml into paste.qt. Run in the tools/testenv image:
#   tools/ci/gate --run as-tester /src/crates/shell/tests/text_paste/qt.sh
cd "$(dirname "$0")" || exit 1
QT_QPA_PLATFORM=offscreen QT_FORCE_STDERR_LOGGING=1 timeout 60 qml6 paste.qml 2>&1 |
    sed -n '/LOG</,/>LOG/p' | sed -e 's/^.*LOG<//' -e '/^>LOG$/d' > paste.qt
echo "paste: $(wc -l < paste.qt) lines"
