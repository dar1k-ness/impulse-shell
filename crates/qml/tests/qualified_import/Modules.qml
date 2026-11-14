import QtQuick as QQ
import QtQuick.Layouts as L
import QtQuick.Controls as C
import QtQuick.Window as W

QQ.Item {
    id: root
    width: 300
    height: 100
    L.RowLayout {
        id: row
        anchors.fill: parent
        spacing: 0
        QQ.Rectangle {
            id: fixed
            L.Layout.preferredWidth: 100
            L.Layout.fillHeight: true
        }
        QQ.Rectangle {
            id: rest
            L.Layout.fillWidth: true
            L.Layout.preferredHeight: 40
        }
    }
    L.ColumnLayout {
        id: col
        spacing: 3
        QQ.Item {
            implicitWidth: 10
            implicitHeight: 10
        }
        QQ.Item {
            id: low
            implicitWidth: 20
            implicitHeight: 10
        }
    }
    C.Button {
        id: button
        text: "go"
        checkable: true
        onClicked: console.log("clicked", checked)
    }
    C.Slider {
        id: slider
        from: 0
        to: 10
        value: 4
    }
    C.ScrollView {
        id: sv
        width: 50
        height: 50
        QQ.Item {
            implicitHeight: 200
        }
    }
    C.TextField {
        id: field
        text: "abc"
    }
    QQ.TextInput {
        id: input
        text: "hello"
    }
    QQ.MouseArea {
        id: ma
        anchors.fill: parent
    }
    QQ.Flickable {
        id: flick
        width: 40
        height: 40
        contentHeight: 100
    }
    QQ.Component.onCompleted: {
        console.log("rowlayout", fixed.L.Layout.preferredWidth, fixed.L.Layout.fillHeight, rest.L.Layout.fillWidth, rest.L.Layout.preferredHeight, row.spacing, col.spacing)
        button.click()
        console.log("button", button.text, button.checked, typeof button.toggle, button.checkable)
        console.log("slider", slider.value, slider.stepSize, typeof slider.decrease)
        console.log("scrollview", typeof sv.contentItem.contentY)
        field.selectAll()
        input.select(1, 3)
        console.log("text", field.selectedText, input.selectedText, input.cursorPosition, field.length)
        console.log("mousearea", ma.enabled, ma.acceptedButtons, ma.hoverEnabled, ma.containsMouse)
        flick.contentY = 30
        console.log("flickable", flick.contentY, flick.atYEnd, flick.interactive, typeof flick.flick)
        console.log("enums", L.Layout.fillWidth, C.ScrollBar.AlwaysOn, W.Window.FullScreen, QQ.Window.Windowed, C.Button.TextOnly)
        console.log("instanceof", row instanceof L.RowLayout, row instanceof QQ.Item, button instanceof C.Button)
        Qt.quit()
    }
}
