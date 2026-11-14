import QtQuick
import QtQuick.Controls
Item {
    id: root
    property string log: ""
    function p(...a) { log += a.join(" ") + "\n"; }
    function t(name, f) { try { p(name, f()); } catch (e) { p(name, "THROW", e.message); } }
    width: 200; height: 200
    Button {
        id: b; checkable: true
        onPressed: root.p("pressed checked=" + checked + " pressed=" + pressed + " down=" + down)
        onReleased: root.p("released checked=" + checked + " pressed=" + pressed)
        onClicked: root.p("clicked checked=" + checked)
        onToggled: root.p("toggled", checked)
        onCheckedChanged: root.p("checkedChanged", checked)
        onPressedChanged: root.p("pressedChanged", pressed)
        onDownChanged: root.p("downChanged", down)
        onCanceled: root.p("canceled")
    }
    Button { id: plain; onToggled: root.p("plain toggled"); onClicked: root.p("plain clicked"); onPressed: root.p("plain pressed"); onReleased: root.p("plain released") }
    Button { id: d; enabled: false; checkable: true; onClicked: root.p("disabled clicked"); onPressed: root.p("disabled pressed"); onToggled: root.p("disabled toggled"); onCheckedChanged: root.p("disabled checked", checked) }
    Item { enabled: false; Button { id: d2; onClicked: root.p("d2 clicked"); onPressed: root.p("d2 pressed") } }
    RadioButton { id: r1; checked: true; onToggled: root.p("r1 toggled", checked); onClicked: root.p("r1 clicked", checked); onCheckedChanged: root.p("r1 checked", checked) }
    RadioButton { id: r2; onToggled: root.p("r2 toggled", checked); onClicked: root.p("r2 clicked", checked); onCheckedChanged: root.p("r2 checked", checked) }
    Switch { id: sw; onToggled: root.p("sw toggled", checked); onClicked: root.p("sw clicked", checked) }
    TabBar { TabButton { id: tb1; onToggled: root.p("tb1 toggled", checked); onClicked: root.p("tb1 clicked", checked) }
             TabButton { id: tb2; onToggled: root.p("tb2 toggled", checked); onClicked: root.p("tb2 clicked", checked) } }
    Button { id: ed; onPressedChanged: root.p("ed pressedChanged", pressed); onDownChanged: root.p("ed downChanged", down); onPressed: root.p("ed pressed down=" + down); onClicked: root.p("ed clicked down=" + down) }
    Component.onCompleted: {
        root.p("--b"); b.click(); root.p("--b again"); b.click();
        root.p("--plain"); plain.click();
        root.p("--d"); d.click(); d2.click();
        root.p("--r1"); r1.click(); root.p("--r2"); r2.click();
        root.p("--sw"); sw.click();
        root.p("--tb"); tb2.click(); tb2.click(); tb1.click();
        root.p("--down set"); ed.down = true; ed.down = false; ed.click();
        root.p("--end");
    }
}
