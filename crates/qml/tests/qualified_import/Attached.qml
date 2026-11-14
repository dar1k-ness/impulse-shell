import QtQuick as QQ
import QtQuick.Layouts as L
import QtQuick.Controls as C
import QtQml as Q

QQ.Item {
    id: root
    width: 100
    height: 100
    property int completed: 0
    QQ.Keys.enabled: false
    QQ.Keys.onPressed: event => console.log("key", event.key)
    QQ.LayoutMirroring.enabled: true
    QQ.LayoutMirroring.childrenInherit: true
    QQ.Accessible.name: "root"
    QQ.ListView {
        id: lv
        model: 2
        width: 100
        height: 100
        delegate: QQ.Item {
            id: d
            required property int index
            height: 10
            property bool cur: QQ.ListView.isCurrentItem
            property var view: QQ.ListView.view
            QQ.Component.onCompleted: console.log("delegate", index, cur, view === lv, QQ.ListView.view === lv)
        }
    }
    QQ.GridView {
        id: gv
        model: 1
        width: 100
        height: 100
        delegate: QQ.Item {
            property bool cur: QQ.GridView.isCurrentItem
            QQ.Component.onCompleted: console.log("grid delegate", cur, QQ.GridView.view === gv)
        }
    }
    QQ.Item {
        id: nav
        QQ.KeyNavigation.tab: root
        QQ.KeyNavigation.priority: QQ.KeyNavigation.BeforeItem
        Q.Component.onCompleted: root.completed += 10
    }
    C.Button {
        id: button
        C.ToolTip.text: "tip"
        C.ToolTip.delay: 7
    }
    L.GridLayout {
        id: grid
        columns: 2
        QQ.Item {
            id: cell
            L.Layout.row: 1
            L.Layout.column: 1
            L.Layout.columnSpan: 1
            L.Layout.minimumWidth: 11
            L.Layout.alignment: Qt.AlignRight | Qt.AlignBottom
            L.Layout.margins: 3
        }
    }
    QQ.Component.onCompleted: {
        completed++
        console.log("keys", root.QQ.Keys.enabled, typeof root.QQ.Keys.forwardTo, nav.QQ.KeyNavigation.tab === root, nav.QQ.KeyNavigation.priority)
        console.log("mirroring", root.QQ.LayoutMirroring.enabled, root.QQ.Accessible.name)
        console.log("tooltip", button.C.ToolTip.text, button.C.ToolTip.delay, button.C.ToolTip.visible)
        console.log("layout", cell.L.Layout.row, cell.L.Layout.column, cell.L.Layout.minimumWidth, cell.L.Layout.alignment, cell.L.Layout.margins, cell.L.Layout.fillWidth)
        console.log("completed", completed)
        console.log("window", root.QQ.Window.window, root.QQ.Window.width)
        Qt.quit()
    }
}
