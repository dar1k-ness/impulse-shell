import QtQuick
import QtQuick.Controls

Item {
    id: root

    property bool on: true
    property int st: 0

    function s(n, c) {
        console.log(n, c.checked, c.checkState, c.tristate);
    }

    // Two literals that contradict each other: the one written first stands.
    CheckBox { id: a; checked: true; checkState: Qt.Unchecked }
    CheckBox { id: b; checkState: Qt.Unchecked; checked: true }
    CheckBox { id: c; checked: false; checkState: Qt.Checked }
    CheckBox { id: d; checkState: Qt.Checked; checked: false }
    CheckBox { id: e; checked: true; checkState: Qt.PartiallyChecked }
    CheckBox { id: f; checkState: Qt.PartiallyChecked; checked: true }
    CheckBox { id: g; checked: false; checkState: Qt.PartiallyChecked }
    CheckBox { id: h; checkState: Qt.PartiallyChecked; checked: false }
    CheckBox { id: i; tristate: true; checked: true; checkState: Qt.PartiallyChecked }
    CheckBox { id: j; checkState: Qt.PartiallyChecked; tristate: true; checked: true }
    CheckBox { id: i2; checked: true; checkState: Qt.Unchecked; tristate: true }
    // A binding is evaluated after the literals.
    CheckBox { id: k; checked: root.on; checkState: Qt.Unchecked }
    CheckBox { id: l; checkState: Qt.Unchecked; checked: root.on }
    CheckBox { id: m; checked: true; checkState: root.st }
    CheckBox { id: n; checkState: root.st; checked: true }
    CheckBox { id: o; checked: root.on; checkState: root.st }
    CheckBox { id: p; checkState: root.st; checked: root.on }
    CheckBox { id: p2; checked: 1 > 0; checkState: Qt.Unchecked }
    CheckBox { id: p3; checked: true; checkState: Qt.Checked - 2 }
    CheckBox { id: p4; checked: true; checkState: Qt.Unchecked + 0 }
    // No handler runs for a literal.
    CheckBox {
        id: q
        checked: true
        checkState: Qt.Unchecked
        onCheckedChanged: console.log("q checked", checked)
        onCheckStateChanged: console.log("q state", checkState)
    }
    CheckBox {
        id: r
        checkState: Qt.Unchecked
        checked: true
        onCheckedChanged: console.log("r checked", checked)
        onCheckStateChanged: console.log("r state", checkState)
    }
    // The literals of the type's own document are stored before those of its user.
    OnBox { id: t; checkState: Qt.Unchecked }
    OnBox { id: u; checkState: Qt.PartiallyChecked }
    PartBox { id: v; checked: true }
    PartBox { id: w; checked: false }
    PartBox { id: x; checked: true; checkState: Qt.Unchecked }
    OnBox { id: y; checkState: Qt.Unchecked; checked: false }

    Component.onCompleted: {
        const boxes = { a, b, c, d, e, f, g, h, i, j, i2, k, l, m, n, o, p, p2, p3, p4, q, r, t, u, v, w, x, y };
        for (const name in boxes)
            s(name, boxes[name]);
        Qt.quit();
    }
}
