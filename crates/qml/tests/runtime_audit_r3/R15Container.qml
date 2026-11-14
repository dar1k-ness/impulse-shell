import QtQuick
import QtQuick.Controls
Item {
    id: root
    property string log: ""
    function p(...a) { log += a.join(" ") + "\n"; }
    function t(name, f) { try { p(name, f()); } catch (e) { p(name, "THROW", e.message); } }
    width: 300; height: 300
    function names(c) { var a = []; for (var i = 0; i < c.count; i++) a.push(c.itemAt(i).objectName); return a.join(""); }
    Component { id: mk; Item {} }
    SwipeView { id: sv; onCurrentIndexChanged: root.p("sv.ci", currentIndex)
        Item { objectName: "a" } Item { objectName: "b" } Item { objectName: "c" } }
    TabBar { id: tb; TabButton { objectName: "x"; text: "a" } TabButton { objectName: "y"; text: "b" } }
    SwipeView { id: empty }
    Component.onCompleted: {
        t("count", () => [sv.count, tb.count, empty.count, sv.currentIndex].join(" "));
        t("set5", () => { sv.setCurrentIndex(5); return sv.currentIndex + " " + (sv.currentItem === null); });
        t("inc@5", () => { sv.incrementCurrentIndex(); return sv.currentIndex; });
        t("dec@5", () => { sv.decrementCurrentIndex(); return sv.currentIndex; });
        t("set-1", () => { sv.setCurrentIndex(-1); return sv.currentIndex; });
        t("dec@-1", () => { sv.decrementCurrentIndex(); return sv.currentIndex; });
        t("inc@-1", () => { sv.incrementCurrentIndex(); return sv.currentIndex; });
        t("inc@last", () => { sv.setCurrentIndex(2); sv.incrementCurrentIndex(); return sv.currentIndex; });
        t("dec@0", () => { sv.setCurrentIndex(0); sv.decrementCurrentIndex(); return sv.currentIndex; });
        t("tb.set7", () => { tb.setCurrentIndex(7); return tb.currentIndex; });
        t("set-str", () => { sv.setCurrentIndex("1"); return sv.currentIndex; });
        t("itemAt", () => [sv.itemAt(0).objectName, sv.itemAt(2).objectName, sv.itemAt(3), sv.itemAt(-1), tb.itemAt(1).objectName].join(" "));
        t("itemAt-eq", () => sv.itemAt(1) === sv.contentChildren[1]);
        t("add", () => { var d = mk.createObject(null, {objectName: "d"}); sv.addItem(d); return names(sv) + " " + sv.count + " " + sv.currentIndex + " " + (d.parent !== null) + " " + sv.contentChildren.length; });
        t("insert0", () => { var e = mk.createObject(null, {objectName: "e"}); sv.insertItem(0, e); return names(sv) + " " + sv.currentIndex; });
        t("insert99", () => { var f = mk.createObject(null, {objectName: "f"}); sv.insertItem(99, f); return names(sv) + " " + sv.currentIndex; });
        t("insert-1", () => { var g = mk.createObject(null, {objectName: "g"}); sv.insertItem(-1, g); return names(sv) + " " + sv.currentIndex; });
        t("insert-existing", () => { sv.insertItem(1, sv.itemAt(4)); return names(sv) + " " + sv.currentIndex; });
        t("insert-null", () => { sv.insertItem(0, null); return names(sv); });
        t("move-cur", () => { sv.setCurrentIndex(0); sv.moveItem(0, 3); return names(sv) + " " + sv.currentIndex; });
        t("move-over", () => { sv.moveItem(0, 5); return names(sv) + " " + sv.currentIndex; });
        t("move-back", () => { sv.moveItem(6, 1); return names(sv) + " " + sv.currentIndex; });
        t("move-bad", () => { sv.moveItem(-1, 2); sv.moveItem(9, 0); return names(sv) + " " + sv.currentIndex; });
        t("move-to99", () => { sv.moveItem(0, 99); return names(sv) + " " + sv.currentIndex; });
        t("take1", () => { var x = sv.takeItem(1); return x.objectName + " " + names(sv) + " " + sv.currentIndex + " " + sv.count + " " + x.parent; });
        t("take-bad", () => [sv.takeItem(-1), sv.takeItem(99)].join(" "));
        t("take-cur", () => { sv.setCurrentIndex(2); var x = sv.takeItem(2); return x.objectName + " " + names(sv) + " " + sv.currentIndex; });
        t("take-before", () => { sv.setCurrentIndex(3); var x = sv.takeItem(0); return x.objectName + " " + names(sv) + " " + sv.currentIndex; });
        t("take-last-cur", () => { sv.setCurrentIndex(sv.count - 1); var x = sv.takeItem(sv.count - 1); return x.objectName + " " + names(sv) + " " + sv.currentIndex; });
        t("remove", () => { var x = sv.itemAt(0); sv.removeItem(x); return names(sv) + " " + sv.currentIndex + " " + sv.count; });
        t("remove-null", () => { sv.removeItem(null); return names(sv); });
        t("remove-foreign", () => { sv.removeItem(root); return names(sv); });
        t("empty-add", () => { var d = mk.createObject(null, {objectName: "z"}); empty.addItem(d); return names(empty) + " " + empty.currentIndex; });
        t("empty-take", () => { empty.takeItem(0); return names(empty) + " " + empty.currentIndex + " " + empty.count; });
        t("tb.add", () => { var b = Qt.createQmlObject('import QtQuick.Controls; TabButton { objectName: "w" }', root); tb.addItem(b); return names(tb) + " " + tb.count + " " + tb.currentIndex; });
    }
}
