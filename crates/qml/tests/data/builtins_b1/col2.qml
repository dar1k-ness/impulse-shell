import QtQuick
Item {
    id: root
    property color c: "#336699"
    function t(name, f) { var s; try { s = f() } catch (e) { s = "THROW " + e.name + ": " + e.message } console.log(name + " => " + s) }
    Component.onCompleted: {
        t("K1", function() { var x = root.c.lighter(NaN); return x.valid + " " + x.a + " " + (x === undefined) + " " + typeof x })
        t("K2", function() { return [root.c.lighter(21474836.47), root.c.lighter(21474836.48), root.c.lighter(-1e9), root.c.darker(1e9), root.c.lighter(Infinity), root.c.darker(-Infinity), root.c.lighter(0.005), root.c.lighter(0.015), root.c.lighter(1.005), root.c.darker(0.5), root.c.lighter(0.5), root.c.lighter(true), root.c.lighter([2]), root.c.lighter("")].join(" ") })
        t("K3", function() { return String(root.c.tint(null)) })
        t("K4", function() { return String(root.c.tint(5)) })
        t("K5", function() { return String(root.c.tint({})) })
        t("K6", function() { return String(root.c.tint("")) + " " + String(root.c.tint("#80ff0000", 1)) + " " + String(root.c.tint(" red")) + " " + String(root.c.tint(Qt.rgba(1, 0, 0, 0.5))) })
        t("K7", function() { return String(root.c.tint(true)) })
        t("K8", function() { return String(root.c.tint([1])) })
        t("K9", function() { return [root.c.alpha(0.5).valid, root.c.alpha(0.3).a, root.c.alpha(Infinity), root.c.alpha(true), root.c.alpha([0.5]), root.c.alpha(1e-9), root.c.alpha(0.999999)].join(" ") })
        t("K10", function() { var x = Qt.color("#80336699"); return [x.rgba, x.lighter().rgba, Qt.rgba(0.2, 0.4, 0.6, 1).rgba, Qt.hsla(0.3, 0.4, 0.5, 0.6).rgba, Qt.rgba(0.5, 0.5, 0.5, 0.5).rgba].join(" ") })
        t("K11", function() { var x = root.c; x.rgba = 5; return String(x) + " " + x.rgba })
        Qt.quit()
    }
}
