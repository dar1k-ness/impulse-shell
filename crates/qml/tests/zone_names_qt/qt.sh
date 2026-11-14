#!/bin/sh
# Writes what Qt 6.12 answers at each `show` of steps.txt into answers.qt. Run in the
# tools/testenv image:
#   tools/ci/gate --run as-tester /src/crates/qml/tests/zone_names_qt/qt.sh
# The steps write /etc/localtime, /etc/TZ and /etc/timezone, so they run over an /etc of
# their own (an empty tmpfs in user and mount namespaces), as the test does.
cd "$(dirname "$0")" || exit 1
if [ -z "$ZONE_NAMES_OWN_ETC" ]; then
    exec unshare --user --map-root-user --mount env ZONE_NAMES_OWN_ETC=1 sh "$PWD/qt.sh"
fi
mount -t tmpfs tmpfs /etc || exit 1
# What the steps name: a copy of the database, with a zone CLDR has no name for, and the
# database under a path with two `zoneinfo` in it.
cp -rL /usr/share/zoneinfo /etc/tzcopy || exit 1
mkdir -p /etc/tzcopy/Test /etc/zoneinfo/usr/share
cp /etc/tzcopy/Asia/Tokyo /etc/tzcopy/Test/Zone
ln -s /usr/share/zoneinfo /etc/zoneinfo/usr/share/zoneinfo
unset TZ TZDIR LANG LANGUAGE LC_TIME LC_NUMERIC LC_MESSAGES
export LC_ALL=C.UTF-8
group=0
: > answers.qt
while :; do
    rm -f /etc/localtime /etc/TZ /etc/timezone /etc/lt2 /etc/c[0-9]*
    /opt/qv4-venv/bin/python qt.py "$group" >> answers.qt
    case $? in
    0) group=$((group + 1)) ;;
    3) break ;;
    *) exit 1 ;;
    esac
done
echo "$(wc -l < answers.qt) answers of $group groups"
