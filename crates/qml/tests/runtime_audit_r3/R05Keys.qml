import QtQuick
import QtQuick.Controls
Item {
    id: root
    property string log: ""
    function p(...a) { log += a.join(" ") + "\n"; }
    function t(name, f) { try { p(name, f()); } catch (e) { p(name, "THROW", e.message); } }
    component Base: QtObject { property int b1: 1; property alias al: sub.x; property var b2; signal bs(); function bf() {}
        property QtObject sub: QtObject { id: sub; property int x: 3 } }
    Base { id: d; property int d1: 2; signal ds(int q); function df() {} property alias dal: d.b1; property string d2: "s"; enum E { A, B } }
    Timer { id: tm }
    Rectangle { id: rc; property int extra: 1; color: "red" }
    Component.onCompleted: {
        t("derived", () => Object.keys(d).join(" "));
        t("timer", () => Object.keys(tm).join(" "));
        t("rect", () => Object.keys(rc).join(" "));
        t("json-d", () => JSON.stringify(d));
        t("json-tm", () => { try { return JSON.stringify(tm); } catch (e) { return e.name; } });
        t("hasOwn", () => [d.hasOwnProperty("b1"), d.hasOwnProperty("bf"), d.hasOwnProperty("bs"), d.hasOwnProperty("destroy"), d.hasOwnProperty("toString"), d.hasOwnProperty("nope"), d.hasOwnProperty("objectName"), tm.hasOwnProperty("start"), tm.hasOwnProperty("ticked")].join(" "));
        t("in", () => ["destroy" in d, "toString" in d, "ticked" in tm, "bf" in d, "hasOwnProperty" in d, "nope" in d].join(" "));
        t("typeof", () => [typeof tm.ticked, typeof d.destroy, typeof d.hasOwnProperty, typeof d.bs, typeof d.bf, typeof d.b1Changed].join(" "));
        t("entries", () => Object.entries(d).map(e => e[0] + ":" + typeof e[1]).join(" "));
        t("assign", () => Object.keys(Object.assign({}, d)).join(" "));
        t("values", () => Object.values(d).length);
        t("getOwnNames", () => Object.getOwnPropertyNames(d).join(" "));
        t("propIsEnum", () => [d.propertyIsEnumerable("b1"), d.propertyIsEnumerable("bf")].join(" "));
        t("valueOf", () => d.valueOf() === d);
    }
}
