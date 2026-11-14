import QtQuick
Item {
    id: root
    property string log: ""
    function p(...a) { log += a.join(" ") + "\n"; }
    function t(name, f) { try { p(name, f()); } catch (e) { p(name, "THROW", e.message); } }
    signal go()
    Timer { id: tm; interval: 50; onTriggered: root.p("tm fired") }
    Timer { id: tm2; interval: 60; onTriggered: root.p("tm2 fired") }
    Timer { id: tm3; interval: 70; onTriggered: root.p("tm3 fired") }
    Component.onCompleted: {
        t("typeof", () => [typeof tm.start, typeof tm.restart, typeof root.destroy, typeof root.mapToItem, typeof root.forceActiveFocus, typeof root.toString, typeof root.childAt, typeof root.grabToImage, typeof root.contains].join(" "));
        t("var f", () => { var f = tm.restart; f(); return tm.running; });
        t("connect", () => { root.go.connect(tm2.start); root.go(); return tm2.running; });
        t("callLater", () => { Qt.callLater(tm3.start); return "queued"; });
        t("in", () => ("start" in tm) + " " + ("width" in root) + " " + ("nope" in root));
        t("hasOwn", () => root.hasOwnProperty("width") + " " + root.hasOwnProperty("nope"));
    }
}
