// Qt reference for delegator_complete_and_destruction_order_matches_qt (runtime.rs):
// QT_QPA_PLATFORM=offscreen qs -p static.qml | grep -o "ST.*"
import QtQuick
import Rustshell
ShellRoot {
FloatingWindow { id: win; implicitWidth: 200; implicitHeight: 400; visible: true
Item { id: root; anchors.fill: parent
  property string log: ""
  property var names: ["a"]
  function add(s) { log += s + " " }
  Item { id: top
    Repeater { model: 2; delegate: Item { Component.onCompleted: root.add("TRc" + index) } }
    Loader { active: true; sourceComponent: Item { Component.onCompleted: root.add("TLc") } }
    Item { Component.onCompleted: root.add("TSc") }
  }
  Repeater { id: rep
    model: ScriptModel { values: root.names }
    delegate: Item { id: d; required property var modelData
      Item { Component.onDestruction: root.add("s1d"); Component.onCompleted: root.add("s1c")
         Item { Component.onDestruction: root.add("s11d"); Component.onCompleted: root.add("s11c") } }
      Loader { active: true; sourceComponent: Item { Component.onCompleted: root.add("L1c"); Component.onDestruction: root.add("L1d") } }
      Item { Component.onDestruction: root.add("s2d"); Component.onCompleted: root.add("s2c") }
      Repeater { model: 2; delegate: Item { Component.onCompleted: root.add("Rc" + index); Component.onDestruction: root.add("Rd" + index) } }
      Loader { active: true; sourceComponent: Item { Component.onCompleted: root.add("L2c"); Component.onDestruction: root.add("L2d") } }
      Component.onCompleted: root.add("c:" + modelData)
      Component.onDestruction: root.add("d:" + modelData) } }
  Timer { interval: 300; running: true; onTriggered: { console.log("ST0 " + root.log); root.log = ""; root.names = [] } }
  Timer { interval: 600; running: true; onTriggered: { console.log("ST1 " + root.log); Qt.quit() } }
} } }
