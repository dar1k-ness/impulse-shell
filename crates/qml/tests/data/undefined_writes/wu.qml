import QtQuick
import QtQuick.Layouts
import QtQuick.Controls
Item {
    id: root
    function tt(f) { try { var v = f(); return typeof v + ":" + v } catch (e) { return "!" + e } }
    Item { id: q }
    Image { id: r }
    ListView { id: t }
    MouseArea { id: u }
    Rectangle { id: r1 }
    Text { id: g }
    Item { id: n }
    Item { id: k; property bool on: true; x: on ? 5 : undefined; opacity: on ? 0.5 : undefined; anchors.leftMargin: on ? 4 : undefined; Layout.preferredWidth: on ? 30 : undefined }
    Rectangle { id: k2; property bool on: true; border.width: on ? 3 : undefined; radius: on ? 2 : undefined; color: on ? "red" : undefined }
    Text { id: k3; property bool on: true; font.pixelSize: on ? 33 : undefined; text: on ? "t" : undefined; horizontalAlignment: on ? Text.AlignRight : undefined }
    Control { id: k4; property bool on: true; padding: on ? 6 : undefined; topPadding: on ? 2 : undefined; implicitWidth: 40; width: on ? 9 : undefined; hoverEnabled: on ? true : undefined; font.pixelSize: on ? 31 : undefined }
    Item { id: k5; width: 10; anchors.fill: undefined; anchors.left: undefined }
    Item { id: k6; property bool on: true; anchors.fill: on ? root : undefined; anchors.margins: on ? 2 : undefined }
    Item { id: k7; property list<int> li: undefined; property var jv: 4; property Item it2: k5; property alias al: k5.width; Component.onCompleted: { jv = undefined; try { it2 = undefined } catch (e) { objectName = "" + e } try { al = undefined } catch (e) { objectName += "/" + e } } }
    Item { id: k8; x: 3; opacity: 0.25; Component.onCompleted: { try { opacity = undefined } catch (e) { objectName = "" + e } try { anchors.leftMargin = undefined } catch (e) { objectName += "/" + e } try { visible = undefined } catch (e) { objectName += "/" + e } try { Layout.fillWidth = undefined } catch (e) { objectName += "/" + e } } }
    Control { id: k9; padding: 7; topPadding: 3; Component.onCompleted: { topPadding = undefined; try { padding = undefined } catch (e) { objectName = "" + e } try { hoverEnabled = undefined } catch (e) { objectName += "/" + e }  } }
    Item { id: k10; width: 12; anchors.fill: root; Component.onCompleted: { anchors.fill = undefined; try { state = undefined } catch (e) { objectName = "" + e } } }
    Component.onCompleted: { k.on = false; k2.on = false; k3.on = false; k4.on = false; k6.on = false }
    property int tick: 0
    Timer { interval: 1; running: true; onTriggered: root.tick++ }
    property var cases: [
        () => r.source,
        () => r1.border.color,
        () => q.anchors.margins,
        () => q.anchors.fill,
        () => k.x,
        () => k.opacity,
        () => k.anchors.leftMargin,
        () => k.Layout.preferredWidth,
        () => k2.border.width,
        () => k2.radius,
        () => k2.color,
        () => k3.font.pixelSize,
        () => k3.text,
        () => k4.padding,
        () => k4.topPadding,
        () => k4.width,
        () => k4.hoverEnabled,
        () => k4.font.pixelSize === k3.font.pixelSize,
        () => k5.anchors.fill,
        () => k5.anchors.left === undefined,
        () => k5.width,
        () => k6.anchors.fill,
        () => k6.anchors.margins,
        () => k7.li,
        () => k7.jv,
        () => k7.it2 === k5,
        () => k7.objectName,
        () => k7.al,
        () => k8.opacity,
        () => k9.padding,
        () => k9.topPadding,
        () => k9.objectName,
        () => k10.anchors.fill,
        () => k10.objectName
    ]
    property string result: (tick, cases.map(f => tt(f)).join(" | "))
}
