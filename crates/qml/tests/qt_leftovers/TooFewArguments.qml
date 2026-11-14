import QtQuick
import QtQuick.Controls
Item {
    id: root
    Item { id: it; width: 10; height: 10 }
    Timer { id: tm }
    ListModel { id: lm; ListElement { a: 1 } }
    ListView { id: lv; model: 3; delegate: Item {} }
    TextInput { id: ti; text: "hello" }
    StackView { id: sv }
    Container { id: ct; contentItem: Item {} }
    Component { id: comp; Item {} }
    function t(name, f) {
        try { var r = f(); console.log(name, "ok", r); }
        catch (e) { console.log(name, "threw", e.name, e.message.split("\n    ").join(" | ")); }
    }
    Component.onCompleted: {
        t("mapToItem()", function() { return it.mapToItem() });
        t("mapToItem(it)", function() { return it.mapToItem(it) });
        t("mapFromItem()", function() { return it.mapFromItem() });
        t("mapToGlobal()", function() { return it.mapToGlobal() });
        t("mapFromGlobal()", function() { return it.mapFromGlobal() });
        t("contains()", function() { return it.contains() });
        t("childAt()", function() { return it.childAt() });
        t("childAt(1)", function() { return it.childAt(1) });
        t("grabToImage()", function() { return it.grabToImage() });
        t("forceActiveFocus(1,2,3)", function() { return it.forceActiveFocus(1, 2, 3) });
        t("tm.start(1)", function() { return tm.start(1) });
        t("tm.restart(1,2)", function() { return tm.restart(1, 2) });
        t("lm.get()", function() { return lm.get() });
        t("lm.append()", function() { return lm.append() });
        t("lm.insert()", function() { return lm.insert() });
        t("lm.insert(0)", function() { return lm.insert(0) });
        t("lm.remove()", function() { return lm.remove() });
        t("lm.set()", function() { return lm.set() });
        t("lm.set(0)", function() { return lm.set(0) });
        t("lm.setProperty(0)", function() { return lm.setProperty(0) });
        t("lm.move(0)", function() { return lm.move(0) });
        t("lv.positionViewAtIndex()", function() { return lv.positionViewAtIndex() });
        t("lv.positionViewAtIndex(0)", function() { return lv.positionViewAtIndex(0) });
        t("lv.itemAtIndex()", function() { return lv.itemAtIndex() });
        t("lv.indexAt()", function() { return lv.indexAt() });
        t("lv.indexAt(1)", function() { return lv.indexAt(1) });
        t("lv.itemAt()", function() { return lv.itemAt() });
        t("ti.select()", function() { return ti.select() });
        t("ti.select(1)", function() { return ti.select(1) });
        t("ti.getText()", function() { return ti.getText() });
        t("ti.getText(1)", function() { return ti.getText(1) });
        t("ti.insert()", function() { return ti.insert() });
        t("ti.insert(0)", function() { return ti.insert(0) });
        t("ti.remove()", function() { return ti.remove() });
        t("ti.positionAt()", function() { return ti.positionAt() });
        t("ti.positionToRectangle()", function() { return ti.positionToRectangle() });
        t("ti.moveCursorSelection()", function() { return ti.moveCursorSelection() });
        t("ct.itemAt()", function() { return ct.itemAt() });
        t("ct.addItem()", function() { return ct.addItem() });
        t("ct.insertItem()", function() { return ct.insertItem() });
        t("ct.insertItem(0)", function() { return ct.insertItem(0) });
        t("ct.moveItem()", function() { return ct.moveItem() });
        t("ct.moveItem(0)", function() { return ct.moveItem(0) });
        t("ct.removeItem()", function() { return ct.removeItem() });
        t("ct.takeItem()", function() { return ct.takeItem() });
        t("ct.setCurrentIndex()", function() { return ct.setCurrentIndex() });
        t("ct.setCurrentIndex(1,2)", function() { return ct.setCurrentIndex(1, 2) });
        t("sv.get()", function() { return sv.get() });
        t("sv.find()", function() { return sv.find() });
        t("comp.createObject()", function() { return comp.createObject() !== null });
        t("it.destroy('x')", function() { var o = comp.createObject(root); return o.destroy("x") });
        t("root.t()", function() { return typeof root.toString() });
        t("it.objectNameChanged.connect()", function() { return it.objectNameChanged.connect() });
        t("it.objectNameChanged.disconnect()", function() { return it.objectNameChanged.disconnect() });
        Qt.quit();
    }
}
