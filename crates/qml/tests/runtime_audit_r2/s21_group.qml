import QtQuick
import QtQuick.Controls
Item {
    ButtonGroup { id: g; buttons: col.children; onClicked: (b) => console.log("group clicked", b.text) }
    Column { id: col
        Button { id: a; text: "a"; checkable: true; checked: true }
        Button { id: b; text: "b"; checkable: true }
        Button { id: c; text: "c"; checkable: true }
    }
    ButtonGroup { id: g2 }
    Button { id: d; checkable: true; checked: true; ButtonGroup.group: g2 }
    Button { id: e; checkable: true; ButtonGroup.group: g2 }
    RadioButton { id: r1; checked: true } 
    RadioButton { id: r2 }
    Button { id: x1; checkable: true; autoExclusive: true; checked: true }
    Button { id: x2; checkable: true; autoExclusive: true }
    Component.onCompleted: {
        b.checked = true; console.log("g:", a.checked, b.checked, c.checked, "checkedButton", g.checkedButton ? g.checkedButton.text : g.checkedButton, g.buttons.length)
        c.toggle(); console.log("g toggle:", a.checked, b.checked, c.checked)
        c.checked = false; console.log("g uncheck:", a.checked, b.checked, c.checked, g.checkedButton, g.checkState)
        e.checked = true; console.log("g2:", d.checked, e.checked, g2.checkedButton === e)
        g2.exclusive = false; d.checked = true; console.log("g2 nonexcl:", d.checked, e.checked)
        r2.checked = true; console.log("radio:", r1.checked, r2.checked)
        r2.checked = false; console.log("radio none:", r1.checked, r2.checked)
        x2.checked = true; console.log("autoexcl:", x1.checked, x2.checked)
    }
}
