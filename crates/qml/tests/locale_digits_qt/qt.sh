#!/bin/sh
# Writes what Qt 6.12 answers show.qml into answers.qt. Run in the tools/testenv image:
#   tools/ci/gate --run as-tester /src/crates/qml/tests/locale_digits_qt/qt.sh
cd "$(dirname "$0")" || exit 1
unset TZDIR LANG LANGUAGE LC_TIME LC_NUMERIC LC_MESSAGES LC_MONETARY LC_ALL
export TZ=Asia/Kolkata QT_QPA_PLATFORM=offscreen
LC_ALL=C.UTF-8 /opt/qv4-venv/bin/python qt.py > answers.qt || exit 1
# Each line of systems.txt is the environment of a system locale.
while read -r vars; do
    # shellcheck disable=SC2086 # the variables of a line are words
    env $vars /opt/qv4-venv/bin/python qt.py --system "$vars" >> answers.qt 2>/dev/null || exit 1
done < systems.txt
echo "$(wc -l < answers.qt) answers of $(cut -f1 answers.qt | uniq | wc -l) locales"
