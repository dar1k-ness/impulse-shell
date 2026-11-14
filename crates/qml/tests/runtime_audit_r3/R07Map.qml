import QtQuick
Item {
    id: root
    property string log: ""
    function p(...a) { log += a.join(" ") + "\n"; }
    function t(name, f) { try { p(name, f()); } catch (e) { p(name, "THROW", e.message); } }
    width: 200; height: 200
    Item { id: a; x: 10; y: 20; Item { id: inner; x: 1; y: 2; width: 5; height: 6 } }
    Item { id: other; x: 100; y: 50 }
    Component.onCompleted: {
        t("to-pt", () => inner.mapToItem(root, Qt.point(3, 4)));
        t("to-xy", () => inner.mapToItem(root, 3, 4));
        t("to-rect", () => inner.mapToItem(root, Qt.rect(0, 0, 5, 6)));
        t("to-xywh", () => inner.mapToItem(root, 0, 0, 5, 6));
        t("to-other", () => inner.mapToItem(other, 0, 0));
        t("to-null-pt", () => inner.mapToItem(null, Qt.point(1, 1)));
        t("to-null-rect", () => inner.mapToItem(null, Qt.rect(1, 1, 2, 2)));
        t("from-pt", () => inner.mapFromItem(root, Qt.point(11, 22)));
        t("from-xy", () => inner.mapFromItem(root, 11, 22));
        t("from-rect", () => inner.mapFromItem(root, Qt.rect(11, 22, 3, 4)));
        t("from-xywh", () => inner.mapFromItem(root, 11, 22, 3, 4));
        t("from-null", () => inner.mapFromItem(null, 0, 0));
        t("glob-xy", () => inner.mapToGlobal(1, 1));
        t("glob-pt", () => inner.mapToGlobal(Qt.point(1, 1)));
        t("fromglob-xy", () => inner.mapFromGlobal(12, 23));
        t("fromglob-pt", () => inner.mapFromGlobal(Qt.point(12, 23)));
        t("fields", () => { var p = inner.mapToItem(root, 3, 4); var r = inner.mapToItem(root, 0, 0, 5, 6); return [typeof p, p.x, p.y, r.x, r.y, r.width, r.height, r.left, r.bottom].join(" "); });
        t("eq", () => inner.mapToItem(root, 3, 4) == Qt.point(14, 26));
        t("to-3", () => inner.mapToItem(root, 1, 2, 3));
        t("to-0", () => inner.mapToItem(root));
        t("to-none", () => inner.mapToItem());
        t("to-5extra", () => inner.mapToItem(root, 1, 2, 3, 4, 5));
        t("to-str", () => inner.mapToItem(root, "3", "4"));
        t("glob-0", () => inner.mapToGlobal());
        t("glob-rect", () => inner.mapToGlobal(Qt.rect(1, 2, 3, 4)));
        t("to-undef", () => inner.mapToItem(undefined, 1, 2));
        t("to-nonitem", () => inner.mapToItem(5, 1, 2));
    }
}
