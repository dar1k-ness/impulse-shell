// `undefined` written to a property whose RESET function does more than put the default back:
// a RectangularShadow's corner follows `radius` again and its `antialiasing` the implicit
// value, a Grid's `rowSpacing` keeps its value, a padding has the wider padding, a popup's
// margin its `margins`, `margins` is -1, `closePolicy` 17.
// qt: 5 5 true 5 5 true 3 3 5 5 0 1 -1 -1 17 5 -1 -1
import QtQuick
import QtQuick.Controls
import QtQuick.Effects
Item {
    id: root
    property bool on: true
    RectangularShadow { id: s1; radius: 5; topLeftRadius: 2; bottomRightRadius: 6; antialiasing: false }
    RectangularShadow { id: s2; radius: 5; topLeftRadius: root.on ? 2 : undefined; bottomRightRadius: root.on ? 6 : undefined; antialiasing: root.on ? false : undefined }
    Grid { id: g1; rowSpacing: 3 }
    Grid { id: g2; rowSpacing: root.on ? 3 : undefined }
    Control { id: c1; padding: 5; horizontalPadding: 8; leftPadding: 1 }
    Control { id: c2; padding: 5; spacing: 7; leftPadding: 1 }
    Popup { id: p1; margins: 6; leftMargin: 1; closePolicy: Popup.NoAutoClose }
    Popup { id: p2; padding: root.on ? 9 : undefined; topPadding: 5; margins: root.on ? 6 : undefined }
    property string result
    Component.onCompleted: {
        s1.topLeftRadius = undefined
        s1.bottomRightRadius = undefined
        s1.antialiasing = undefined
        g1.rowSpacing = undefined
        c1.leftPadding = undefined
        c1.horizontalPadding = undefined
        c2.padding = undefined
        p1.leftMargin = undefined
        root.on = false
        p1.margins = undefined
        p1.closePolicy = undefined
        result = [s1.topLeftRadius, s1.bottomRightRadius, s1.antialiasing, s2.topLeftRadius, s2.bottomRightRadius, s2.antialiasing,
            g1.rowSpacing, g2.rowSpacing, c1.leftPadding, c1.horizontalPadding, c2.padding, c2.leftPadding,
            p1.leftMargin, p1.margins, p1.closePolicy, p2.topPadding, p2.margins, p2.leftMargin].map(String).join(" ")
    }
}
