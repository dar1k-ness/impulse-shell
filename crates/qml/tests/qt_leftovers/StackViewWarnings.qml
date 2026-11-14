import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 300; height: 300
    Component { id: ca; Item { objectName: "a" } }
    Item { id: own; objectName: "o" }
    StackView { id: empty }
    StackView { id: sv; anchors.fill: parent }
    function t(name, f) { console.log(name, f()); }
    Component.onCompleted: {
        t("empty-pop", () => empty.pop());
        t("empty-popCurrentItem", () => empty.popCurrentItem());
        t("empty-popToItem", () => empty.popToItem(own));
        t("empty-popToIndex", () => empty.popToIndex(0));
        t("empty-clear", () => empty.clear());
        t("push-none", () => sv.push());
        t("push-empty-list", () => sv.push([]));
        t("push-number", () => sv.push(3));
        t("pushItems-empty", () => sv.pushItems([]));
        t("replace-none", () => sv.replace());
        t("replace-empty-list", () => sv.replace([]));
        sv.push([ca, ca, ca], StackView.Immediate);
        t("depth", () => sv.depth);
        t("pop-foreign", () => sv.pop(own, StackView.Immediate));
        t("pop-many-args", () => sv.pop(null, StackView.Immediate, 3));
        t("pop-current", () => sv.pop(sv.currentItem, StackView.Immediate));
        t("popToIndex-high", () => sv.popToIndex(9, StackView.Immediate));
        t("popToIndex-negative", () => sv.popToIndex(-1, StackView.Immediate));
        t("popToItem-null", () => sv.popToItem(null, StackView.Immediate));
        t("popToItem-foreign", () => sv.popToItem(own, StackView.Immediate));
        t("push-in-stack", () => sv.push(sv.get(0), StackView.Immediate));
        t("depth", () => sv.depth);
        Qt.quit();
    }
}
