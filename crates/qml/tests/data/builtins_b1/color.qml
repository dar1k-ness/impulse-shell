import QtQuick
Item {
    id: root
    property color c: "#336699"
    property color zz
    property color h: Qt.hsva(0.3, 0.5, 0.7, 0.9)
    property color l: Qt.hsla(0.3, 0.5, 0.7, 0.4)
    property color tr: "transparent"
    function t(name, f) { var s; try { s = f() } catch (e) { s = "THROW " + e.name + ": " + e.message } console.log(name + " => " + s) }
    Component.onCompleted: {
        t("L1", function() { return [root.c.lighter(), root.c.lighter(1.2), root.c.lighter(0), root.c.lighter(-1), root.c.lighter("2"), root.c.lighter(undefined), root.c.lighter(null), root.c.lighter(NaN), root.c.lighter(1, 2), root.c.lighter("x"), root.c.lighter(0.004), root.c.lighter(1e9)].join(" ") })
        t("L2", function() { return [root.c.darker(), root.c.darker(3), root.c.darker(0), root.c.darker(-1), root.c.darker("2"), root.c.darker(undefined), root.c.darker(null), root.c.darker(NaN), root.c.darker(1.005), root.c.darker({})].join(" ") })
        t("L3", function() { return [root.h.lighter(), root.h.darker(), root.l.lighter(), root.l.darker(), root.tr.lighter(), root.zz.lighter(), root.zz.darker()].join(" ") })
        t("A1", function() { return [root.c.alpha(0.5), root.c.alpha(2), root.c.alpha(-1), root.c.alpha("0.25"), root.c.alpha(0), root.c.alpha(1), root.h.alpha(0.5), root.zz.alpha(0.5), root.c.alpha(0.5, 1)].join(" ") })
        t("A2", function() { return String(root.c.alpha()) })
        t("A3", function() { return String(root.c.alpha(undefined)) })
        t("A4", function() { return String(root.c.alpha(NaN)) + " " + String(root.c.alpha(null)) + " " + String(root.c.alpha("x")) })
        t("T1", function() { return [root.c.tint("#80ff0000"), root.c.tint("red"), root.c.tint("transparent"), root.c.tint(Qt.rgba(0, 1, 0, 0.25)), root.h.tint("#40000000"), root.c.tint(root.l), root.c.tint("light blue")].join(" ") })
        t("T2", function() { return String(root.c.tint()) })
        t("T3", function() { return String(root.c.tint("bogus")) })
        t("T4", function() { return String(root.c.tint(undefined)) + " " + String(root.c.tint(null)) + " " + String(root.c.tint(5)) })
        t("T5", function() { return String(root.zz.tint("red")) + " " + String(root.c.tint(root.zz)) })
        t("R1", function() { return [root.c.rgba, root.zz.rgba, root.h.rgba, root.l.rgba, root.tr.rgba, Qt.rgba(1, 1, 1, 1).rgba, Qt.rgba(0, 0, 0, 0.5).rgba, Qt.color("red").rgba].join(" ") })
        t("R2", function() { return typeof root.c.rgba })
        t("M1", function() { return [typeof root.c.lighter, typeof root.c.darker, typeof root.c.alpha, typeof root.c.tint, typeof root.c.toString, typeof root.c.bogus].join(" ") })
        t("M2", function() { return String(root.c.lighter) })
        t("M3", function() { var f = root.c.lighter; return String(f(2)) })
        t("M4", function() { return root.c.lighter.length + " " + root.c.lighter.name })
        t("M5", function() { return String(root.c.lighter.call(root.c, 2)) })
        t("M6", function() { return String(root.c.toString()) + " " + root.c.toString.length })
        t("M7", function() { return String(root.c.lighter?.(2)) + " " + String(root.c.bogus?.(2)) })
        t("M8", function() { return String(root.c.equals) + " " + typeof root.c.valueOf + " " + typeof root.c.hasOwnProperty })
        t("M9", function() { var k = []; for (var p in root.c) k.push(p); return k.join() })
        var names = ["light blue", "LIGHT BLUE", " red", "red ", "r e d", "\tred", "red\n", " #fff", "#fff ", "# fff", "#FFF", "#ffff", "transparent", "", " ", "light  blue", "Light Blue", "dark slate gray", "darkslategrey", "TRANSPARENT", "trans parent", "#12345", "#1234567", "#123456789", "#123456789abc", "#g00", "rgb(1,2,3)", "red;", "\u00a0red", "re\u00a0d", "re\u3000d", "ｒｅｄ", "#１２３"]
        for (var i = 0; i < names.length; i++) {
            (function(n, i) {
                t("P" + i, function() { root.zz = "#010203"; root.zz = n; return JSON.stringify(n) + " " + String(root.zz) + " " + root.zz.valid })
                t("Q" + i, function() { return String(Qt.color(n)) + " " + String(Qt.lighter(n)) + " " + String(Qt.alpha(n, 0.5)) + " " + String(Qt.tint(n, "red")) + " " + Qt.colorEqual(n, "red") })
            })(names[i], i)
        }
        Qt.quit()
    }
}
