import QtQuick as QQ
import QtQuick.Layouts as L
import QtQuick.Controls as C

QQ.Item {
    id: root
    property int combined: QQ.Text.AlignRight | QQ.Text.AlignBottom
    property int picked: QQ.Easing.InOutQuad
    QQ.Text {
        id: text
        horizontalAlignment: QQ.Text.AlignHCenter
        verticalAlignment: QQ.Text.AlignVCenter
        elide: QQ.Text.ElideRight
        wrapMode: QQ.Text.WordWrap
        textFormat: QQ.Text.PlainText
        renderType: QQ.Text.NativeRendering
        font.weight: QQ.Font.DemiBold
        font.capitalization: QQ.Font.AllUppercase
    }
    QQ.ListView {
        id: view
        orientation: QQ.ListView.Horizontal
        snapMode: QQ.ListView.SnapOneItem
        boundsBehavior: QQ.Flickable.StopAtBounds
        highlightRangeMode: QQ.ListView.StrictlyEnforceRange
        layoutDirection: Qt.RightToLeft
    }
    QQ.Image {
        id: image
        fillMode: QQ.Image.PreserveAspectCrop
        horizontalAlignment: QQ.Image.AlignLeft
    }
    QQ.NumberAnimation {
        id: anim
        easing.type: QQ.Easing.OutBack
        loops: QQ.Animation.Infinite
    }
    QQ.MouseArea {
        id: area
        cursorShape: Qt.PointingHandCursor
        acceptedButtons: Qt.LeftButton | Qt.RightButton
    }
    QQ.Flow {
        id: flow
        flow: QQ.Flow.TopToBottom
    }
    QQ.Grid {
        id: grid
        horizontalItemAlignment: QQ.Grid.AlignHCenter
    }
    QQ.TextInput {
        id: input
        echoMode: QQ.TextInput.Password
        inputMethodHints: Qt.ImhDigitsOnly
    }
    QQ.Loader {
        id: loader
        asynchronous: false
    }
    QQ.Item {
        id: origin
        transformOrigin: QQ.Item.BottomRight
    }
    C.ScrollBar {
        id: bar
        policy: C.ScrollBar.AlwaysOn
        orientation: Qt.Horizontal
    }
    C.Button {
        id: button
        display: C.AbstractButton.IconOnly
    }
    C.Popup {
        id: popup
        closePolicy: C.Popup.CloseOnEscape | C.Popup.CloseOnPressOutside
    }
    L.StackLayout {
        id: stack
    }
    function name(v) {
        switch (v) {
        case QQ.Text.AlignHCenter:
            return "center"
        case QQ.Text.AlignRight:
            return "right"
        }
        return "other"
    }
    QQ.Component.onCompleted: {
        console.log("text", text.horizontalAlignment, text.verticalAlignment, text.elide, text.wrapMode, text.textFormat, text.renderType, text.font.weight, text.font.capitalization)
        console.log("view", view.orientation, view.snapMode, view.boundsBehavior, view.highlightRangeMode, view.layoutDirection)
        console.log("image", image.fillMode, image.horizontalAlignment, image.status === QQ.Image.Null)
        console.log("anim", anim.easing.type, root.picked, root.combined)
        console.log("area", area.cursorShape, area.acceptedButtons, flow.flow, grid.horizontalItemAlignment)
        console.log("input", input.echoMode, input.inputMethodHints, loader.status === QQ.Loader.Null, origin.transformOrigin)
        console.log("controls", bar.policy, bar.orientation, button.display, popup.closePolicy)
        console.log("script", name(text.horizontalAlignment), name(QQ.Text.AlignRight), QQ.Text.ElideMiddle, QQ.Font.Bold, QQ.Easing.Linear, QQ.Item.Center)
        console.log("scoped", QQ.Text.HAlignment.AlignRight, QQ.Text.TextElideMode.ElideLeft, typeof QQ.Text.Nope, typeof QQ.Nope, typeof QQ.Text, typeof QQ)
        console.log("wrapper", String(QQ.Item).split("(")[0], QQ.Item === QQ.Item, Object.keys(QQ).length, "Item" in QQ)
        Qt.quit()
    }
}
