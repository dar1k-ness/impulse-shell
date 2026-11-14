import QtQuick
import QtQuick.Controls
Item {
    id: root
    property string log: ""
    function p(...a) { log += a.join(" ") + "\n"; }
    function t(name, f) { try { p(name, f()); } catch (e) { p(name, "THROW", e.message); } }
    Holder {
        id: h
        header: Rectangle { objectName: "hdr"; width: 7 }
        footer: QtObject { objectName: "ftr" }
        delegate: Item { objectName: "dlg" }
        highlight: Item { objectName: "hl" }
        made: Item { objectName: "tpl" }
        viaAlias: Item { objectName: "aliased" }
    }
    ListView { id: lv; width: 100; height: 100; model: 2
        header: Item { objectName: "lvh"; height: 3 }
        delegate: Item { height: 10; property int idx: index } }
    Component.onCompleted: {
        t("header", () => h.header.objectName + " " + h.header.width);
        t("footer", () => h.footer.objectName);
        t("delegate", () => h.delegate.objectName);
        t("highlight", () => h.highlight.objectName);
        t("Component property", () => h.made.objectName + " " + typeof h.made.createObject + " " + h.made.createObject(root).objectName);
        t("alias to Component property", () => h.viaAlias.objectName + " " + typeof h.viaAlias.createObject);
        t("ListView", () => typeof lv.header.createObject + " " + typeof lv.delegate.createObject + " " + lv.count);
    }
}
