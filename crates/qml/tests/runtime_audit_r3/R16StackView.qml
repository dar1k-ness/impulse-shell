import QtQuick
import QtQuick.Controls
Item {
    id: root
    property string log: ""
    function p(...a) { log += a.join(" ") + "\n"; }
    function t(name, f) { try { p(name, f()); } catch (e) { p(name, "THROW", e.message); } }
    width: 300; height: 300
    function names(s) { var a = []; for (var i = 0; i < s.depth; i++) a.push(on(s.get(i))[0]); return a.join(""); }
    function on(o) { return o ? o.objectName : "_"; }
    Component { id: ca; Item { objectName: "a"; property int v: 1 } }
    Component { id: cb; Item { objectName: "b"; property int v: 2 } }
    Component { id: cc; Item { objectName: "c"; property int v: 3; Component.onDestruction: root.p("destroyed c") } }
    Item { id: own; objectName: "o"; property int v: 0 }
    Item { id: own2; objectName: "q"; property int v: 0 }
    StackView { id: sv; anchors.fill: parent; onDepthChanged: root.p("depth", depth) }
    Component.onCompleted: {
        t("empty", () => [sv.depth, sv.empty, sv.currentItem, sv.get(0), sv.pop(), sv.find(function() { return true; })].join(" "));
        t("push-many", () => { var r = sv.push([ca, cb, cc], StackView.Immediate); return on(r) + " " + names(sv) + " " + [sv.get(0).visible, sv.get(1).visible, sv.get(2).visible].join(""); });
        t("push-args", () => { var r = sv.push(ca, {objectName: "d"}, cb, {objectName: "e", v: 7}, StackView.Immediate); return on(r) + " " + names(sv) + " " + r.v; });
        t("get", () => [on(sv.get(0)), on(sv.get(4)), sv.get(5), sv.get(-1)].join(" "));
        t("find", () => { var seen = []; var r = sv.find(function(item, index) { seen.push(item.objectName + index); return item.v === 3; }); return on(r) + " " + seen.join(""); });
        t("find-none", () => sv.find(function(item, index) { return false; }));
        t("pop-item", () => { var r = sv.pop(sv.get(2), StackView.Immediate); return on(r) + " " + names(sv) + " " + on(sv.currentItem); });
        t("pop-foreign", () => { var r = sv.pop(own, StackView.Immediate); return r + " " + names(sv); });
        t("pop-top", () => { var r = sv.pop(sv.currentItem, StackView.Immediate); return r + " " + names(sv); });
        t("push-item", () => { var r = sv.push(own, {v: 4}, StackView.Immediate); return on(r) + " " + names(sv) + " " + own.v + " " + own.visible + " " + (own.parent === sv); });
        t("replace-top", () => { var r = sv.replace(cb, {objectName: "f"}, StackView.Immediate); return on(r) + " " + names(sv) + " " + own.visible; });
        t("replace-target", () => { var r = sv.replace(sv.get(1), ca, {objectName: "g"}, StackView.Immediate); return on(r) + " " + names(sv); });
        t("replace-null", () => { sv.push(cb, StackView.Immediate); var r = sv.replace(null, ca, {objectName: "h"}, cb, {objectName: "i"}, StackView.Immediate); return on(r) + " " + names(sv); });
        t("pop", () => { var r = sv.pop(StackView.Immediate); return on(r) + " " + names(sv); });
        t("pop@1", () => { var r = sv.pop(StackView.Immediate); return r + " " + names(sv); });
        t("pushItem", () => { var r = sv.pushItem(cb, {objectName: "j"}, StackView.Immediate); return on(r) + " " + names(sv); });
        t("pushItem-item", () => { var r = sv.pushItem(own2, {v: 6}, StackView.Immediate); return on(r) + " " + names(sv) + " " + own2.v; });
        t("pushItems", () => { var r = sv.pushItems([ca, cc], StackView.Immediate); return on(r) + " " + names(sv); });
        t("popToIndex", () => { var r = sv.popToIndex(2, StackView.Immediate); return on(r) + " " + names(sv); });
        t("popToIndex-bad", () => { var r = sv.popToIndex(9, StackView.Immediate); return r + " " + names(sv); });
        t("popToItem", () => { var r = sv.popToItem(sv.get(1), StackView.Immediate); return on(r) + " " + names(sv); });
        t("replaceCurrentItem", () => { var r = sv.replaceCurrentItem(cc, {objectName: "k"}, StackView.Immediate); return on(r) + " " + names(sv); });
        t("popCurrentItem", () => { var r = sv.popCurrentItem(StackView.Immediate); return on(r) + " " + names(sv); });
        t("popCurrentItem@1", () => { var r = sv.popCurrentItem(StackView.Immediate); return on(r) + " " + names(sv) + " " + sv.depth; });
        t("push-url", () => { var r = sv.push("Page1.qml", {v: 8}, StackView.Immediate); return on(r) + " " + r.v + " " + sv.depth; });
        t("push-none", () => sv.push());
        t("clear", () => { sv.clear(); return sv.depth + " " + sv.currentItem + " " + sv.empty; });
    }
}
