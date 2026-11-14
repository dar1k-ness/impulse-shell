// Qt reference for script_view_log / SCRIPT_VIEW_QT in crates/qml/src/runtime.rs:
// KIND=lv|rp H=20 QT_QPA_PLATFORM=offscreen qs -p seqs.qml | grep -o "SEQ.*"
import QtQuick
import Rustshell
ShellRoot {
  id: root
  property var seqs: [
    [["a","c"],["c","a"],[]],
    [["a","b"],["q","r","a","b"],[]],
    [["a","b"],["a","b","q","r"],[]],
    [["a","b","c"],["a","c"],[]],
    [["a","c"],["c","a"],["q","r"]],
    [["c","a"],["q","r"]],
    [["a","b","c"],["a","x","b","c"],["a","c"],["c","a"],["q","r"],[]],
    [["a","b","c"],["c","a","b"],[]],
    [["a","b","c","d"],["a","d","b","c"],[]],
    [["a","b","c"],["q","r"],[]]
  ]
  property int h: Number(Rustshell.env("H") || "0")
  property string kind: Rustshell.env("KIND") || "lv"
  FloatingWindow { id: win; implicitWidth: 200; implicitHeight: 400; visible: true
  Instantiator {
    model: root.seqs.length
    delegate: QtObject {
      id: t
      required property int index
      property int step: 0
      property var names: root.seqs[index][0]
      property string log: ""
      property ScriptModel sm: ScriptModel { values: t.names }
      property Component dc: Component { Item { required property string modelData; required property int index; height: root.h; width: 10
          onIndexChanged: t.log += "i:" + modelData + index + " "
          Component.onCompleted: t.log += "c:" + modelData + " "
          Component.onDestruction: t.log += "d:" + modelData + " " } }
      property Item v: root.kind == "lv" ? lvc.createObject(win.contentItem, {model: sm, delegate: dc}) : rpc.createObject(win.contentItem, {model: sm, delegate: dc})
      property Timer tm: Timer { interval: 100; repeat: true; running: true; onTriggered: { t.step++; if (t.step < root.seqs[t.index].length) { t.log += "| "; t.names = root.seqs[t.index][t.step] } } }
      property Timer tm2: Timer { interval: 1200 + t.index * 20; running: true; onTriggered: console.log("SEQ" + t.index + " " + JSON.stringify(root.seqs[t.index]) + " " + t.log) }
    }
  }
  Component { id: lvc; ListView { width: 100; height: 300 } }
  Component { id: rpc; Column { property alias model: r.model; property alias delegate: r.delegate; Repeater { id: r } } }
  }
}
