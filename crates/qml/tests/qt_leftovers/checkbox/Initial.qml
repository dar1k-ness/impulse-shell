import QtQuick
import QtQuick.Controls

Item {
    id: root

    function s(n, c) {
        console.log(n, c.checked, c.checkState);
    }

    Component { id: on; CheckBox { checked: true } }
    Component { id: checkedState; CheckBox { checkState: Qt.Checked } }
    Component { id: partial; CheckBox { checkState: Qt.PartiallyChecked } }
    Component { id: plain; CheckBox { } }
    Component { id: both; CheckBox { checked: true; checkState: Qt.Unchecked } }
    Component {
        id: names
        Item {
            property int b
            property int a
            property int bb
            property int a1
            property int _z
            property int zz
            property int checked
            property int checkState
            onBChanged: console.log("b")
            onAChanged: console.log("a")
            onBbChanged: console.log("bb")
            onA1Changed: console.log("a1")
            on_ZChanged: console.log("_z")
            onZzChanged: console.log("zz")
            onCheckedChanged: console.log("checked")
            onCheckStateChanged: console.log("checkState")
            onXChanged: console.log("x")
            onWidthChanged: console.log("width")
            onOpacityChanged: console.log("opacity")
        }
    }

    Component.onCompleted: {
        // The initial properties of createObject() are set in the order of their names.
        s("1", on.createObject(root, { checked: false }));
        s("2", on.createObject(root, { checkState: Qt.Unchecked }));
        s("3", checkedState.createObject(root, { checked: false }));
        s("4", partial.createObject(root, { checked: true }));
        s("5", plain.createObject(root, { checked: true }));
        s("6", plain.createObject(root, { checkState: Qt.PartiallyChecked }));
        s("7", plain.createObject(root, { checkState: Qt.PartiallyChecked, checked: true }));
        s("8", plain.createObject(root, { checked: true, checkState: Qt.PartiallyChecked }));
        // The same component made again and again.
        s("9", both.createObject(root));
        s("10", both.createObject(root, { checkState: Qt.PartiallyChecked }));
        s("11", both.createObject(root, { checked: false }));
        s("12", both.createObject(root, { checked: false }));
        s("13", both.createObject(root, { checkState: Qt.PartiallyChecked }));
        s("14", both.createObject(root));
        names.createObject(root, { zz: 1, b: 1, a: 1, width: 3, bb: 1, x: 2, a1: 1, _z: 1, checked: 1, opacity: 0.5, checkState: 1 });
        console.log("--");
        names.createObject(root, { checkState: 1, opacity: 0.5, checked: 1, _z: 1, a1: 1, x: 2, bb: 1, width: 3, a: 1, b: 1, zz: 1 });
        console.log("--");
        const later = {};
        later.zz = 1;
        later.a = 1;
        later.b = 1;
        names.createObject(root, later);
        Qt.quit();
    }
}
