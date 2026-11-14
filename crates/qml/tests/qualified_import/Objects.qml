import QtQuick as QQ
import QtQml as Q
import QtQml.Models as M

QQ.Item {
    id: root
    width: 200
    height: 100
    property int hits: 0
    signal ping(int n)
    property QQ.Item typed: inner
    property QQ.Component comp: QQ.Rectangle {
        width: 3
    }
    property list<QQ.Item> many: [QQ.Item {}, QQ.Rectangle {}]
    property Q.QtObject plain: Q.QtObject {
        property int n: 4
    }
    QQ.Component {
        id: explicit
        QQ.Text {
            text: "made"
        }
    }
    QQ.Connections {
        target: root
        function onPing(n) {
            root.hits += n
        }
    }
    Q.Connections {
        target: root
        function onPing(n) {
            root.hits += 10 * n
        }
    }
    QQ.Item {
        id: inner
        width: 20
        QQ.Loader {
            id: loader
            sourceComponent: explicit
        }
        QQ.Repeater {
            id: rep
            model: 3
            QQ.Item {
                required property int index
                width: index
            }
        }
    }
    QQ.ListModel {
        id: lm
        QQ.ListElement {
            name: "a"
            n: 1
        }
        QQ.ListElement {
            name: "b"
            n: 2
        }
    }
    M.ListModel {
        id: lm2
        M.ListElement {
            name: "c"
        }
    }
    Q.Instantiator {
        id: inst
        model: 2
        delegate: Q.QtObject {
            property int v: 1
        }
    }
    QQ.Row {
        id: row
        spacing: 2
        QQ.Rectangle {
            width: 10
            height: 5
        }
        QQ.Rectangle {
            id: second
            width: 10
            height: 5
        }
    }
    QQ.Rectangle {
        id: rect
        gradient: QQ.Gradient {
            QQ.GradientStop {
                position: 0
                color: "red"
            }
            QQ.GradientStop {
                position: 1
                color: "blue"
            }
        }
    }
    QQ.ListView {
        id: lv
        width: 50
        height: 50
        model: lm
        delegate: QQ.Text {
            required property string name
            text: name
            height: 10
        }
    }
    QQ.Component.onCompleted: {
        ping(1)
        console.log("connections", hits)
        console.log("typed", typed === inner, typed.width, plain.n, many.length)
        var made = comp.createObject(root)
        console.log("component", made.width, String(made).split("(")[0], String(comp).split("(")[0])
        console.log("loader", loader.status === QQ.Loader.Ready, loader.item.text)
        console.log("repeater", rep.count, rep.itemAt(2).width, inner.children.length)
        console.log("listmodel", lm.count, lm.get(1).name, lm.get(1).n, lm2.count, lm2.get(0).name)
        lm.append({"name": "z", "n": 3})
        console.log("listview", lv.count, lv.itemAtIndex(0).text)
        console.log("instantiator", inst.count, inst.objectAt(1).v)
        console.log("gradient", rect.gradient !== null)
        console.log("instanceof", root instanceof QQ.Item, rect instanceof QQ.Rectangle, rect instanceof QQ.Item, lm instanceof QQ.ListModel, lm instanceof QQ.Item, plain instanceof Q.QtObject)
        console.log("enums", QQ.Text.AlignHCenter, QQ.Easing.OutCubic, QQ.Loader.Ready, QQ.ListView.Contain, QQ.Item.TopRight)
        console.log("methods", typeof root.mapToItem, typeof lv.positionViewAtIndex, typeof lm.append, typeof loader.setSource, typeof rect.grabToImage)
        console.log("defaults", root.visible, root.opacity, rect.radius, lv.spacing, lv.orientation, row.layoutDirection, loader.active, rep.count)
        Qt.quit()
    }
}
