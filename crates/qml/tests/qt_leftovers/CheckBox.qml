import QtQuick
import QtQuick.Controls
Item {
    CheckBox { id: a }
    CheckBox { id: b; checked: true }
    CheckBox { id: c; checkState: Qt.PartiallyChecked }
    CheckBox { id: d; tristate: true
        onCheckStateChanged: console.log("d checkState", checkState)
        onCheckedChanged: console.log("d checked", checked)
        onToggled: console.log("d toggled")
        onClicked: console.log("d clicked")
        onTristateChanged: console.log("d tristate", tristate)
    }
    CheckBox { id: e; checkState: Qt.Checked }
    CheckBox { id: f; tristate: true; nextCheckState: function() { console.log("f next from", checkState); return checkState === Qt.Checked ? Qt.Unchecked : Qt.Checked } }
    CheckBox { id: g
        onCheckStateChanged: console.log("g checkState", checkState)
        onCheckedChanged: console.log("g checked", checked)
        onTristateChanged: console.log("g tristate", tristate)
    }
    Component.onCompleted: {
        console.log("enum", Qt.Unchecked, Qt.PartiallyChecked, Qt.Checked);
        console.log("a", a.checked, a.checkState, a.tristate, typeof a.nextCheckState);
        console.log("b", b.checked, b.checkState, b.tristate);
        console.log("c", c.checked, c.checkState, c.tristate);
        console.log("e", e.checked, e.checkState, e.tristate);
        console.log("--d click"); d.click(); console.log("d", d.checked, d.checkState);
        console.log("--d click"); d.click(); console.log("d", d.checked, d.checkState);
        console.log("--d click"); d.click(); console.log("d", d.checked, d.checkState);
        console.log("--d toggle"); d.toggle(); console.log("d", d.checked, d.checkState);
        console.log("--d toggle"); d.toggle(); console.log("d", d.checked, d.checkState);
        console.log("--d checked=true"); d.checked = true; console.log("d", d.checked, d.checkState);
        console.log("--d partial"); d.checkState = Qt.PartiallyChecked; console.log("d", d.checked, d.checkState);
        console.log("--d checked=false"); d.checked = false; console.log("d", d.checked, d.checkState);
        console.log("--f click"); f.click(); console.log("f", f.checked, f.checkState);
        console.log("--f click"); f.click(); console.log("f", f.checked, f.checkState);
        console.log("--g partial"); g.checkState = Qt.PartiallyChecked; console.log("g", g.checked, g.checkState, g.tristate);
        console.log("--g click"); g.click(); console.log("g", g.checked, g.checkState);
        console.log("--g click"); g.click(); console.log("g", g.checked, g.checkState);
        console.log("--g click"); g.click(); console.log("g", g.checked, g.checkState);
        console.log("--g checked"); g.checkState = Qt.Checked; console.log("g", g.checked, g.checkState);
        console.log("--g tristate=false"); g.tristate = false; console.log("g", g.checked, g.checkState);
        console.log("--a click"); a.click(); console.log("a", a.checked, a.checkState);
        console.log("--a click"); a.click(); console.log("a", a.checked, a.checkState);
        Qt.quit();
    }
}
