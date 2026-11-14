import QtQuick
Item {
    id: root
    function t(n, f) { var r; try { r = f() } catch (e) { r = "THROW " + e.name + ": " + e.message } console.log(n + " => " + r) }
    Component.onCompleted: {
        t("A1", function() { return "%1 %2".arg("a", "b") })
        t("A2", function() { return "%1 %2 %3".arg(1, 2.5, true) })
        t("A3", function() { return "%1".arg("a", "b") })
        t("A4", function() { return "%2 %1 %3".arg("a", "b") })
        t("A5", function() { return "%1 %2".arg("%2", "b") })
        t("A6", function() { return "%1 %2".arg(1234567.5, null) })
        t("A7", function() { return "%1 %2 %3".arg(undefined, [1,2], {}) })
        t("A8", function() { return "%1".arg() })
        t("A9", function() { return "%1%1 %5".arg("x", "y") })
        t("A10", function() { return "%1 %2".arg(Symbol("s"), "b") })
        t("A11", function() { return "%1 %2".arg("b", Symbol("s")) })
        t("A12", function() { return String.prototype.arg.call(null, 1) + "|" + String.prototype.arg.call(undefined, 1) + "|" + String.prototype.arg.call(5, 1) + "|" + String.prototype.arg.call("%1", 1) })
        t("A13", function() { return "%1 %2".arg(-0, 1e21) })
        t("A15", function() { return "%L1 %2 %L3".arg("a", "b") + "|" + "%L1 %2".arg("a") + "|" + "%1 %L1".arg(1234567.5, 2) + "|" + "%1 %01 %001 %10".arg("a", "b", "c") + "|" + "%3 %1".arg("a", "b", "c", "d") })
        t("A16", function() { return "%1 %2".arg(2147483648, -2147483649) + "|" + "%2 %1".arg(false, 0.1) + "|" + "%1".arg(new String("%1"), 2) })
        t("A14", function() { return "".arg.length + " " + "".arg.name })
    }
}
