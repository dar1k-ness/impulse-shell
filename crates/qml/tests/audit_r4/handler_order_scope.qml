import QtQuick
Item { id: r; property string log: ""
  Item { id: a
    Item { id: b; property bool viaScope: visible; property int n: 0
      onVisibleChanged: { n++; console.log("h" + visible) }
      onViaScopeChanged: console.log("s" + viaScope) } }
  Component.onCompleted: { a.visible = false; console.log(b.viaScope + "," + b.n);
     a.visible = true; console.log(b.viaScope + "," + b.n); console.log("END"); Qt.quit() } }
