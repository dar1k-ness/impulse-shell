// QQuickControl::setBackground puts a background whose z is 0 at z -1, under the control's
// other children, whenever it is made: deferred, with an id inside, or written by a script.
// A z that was written and is not 0 stays. A contentItem and an indicator keep z 0.
// qt: -1 0 -1 -1 2 0 -1 -1 0 -1 -1 -1 0 -1 0 -1 4 0 3
import QtQuick
import QtQuick.Controls
Item {
    id: root
    property string result: ""
    Component { id: late; Rectangle { } }
    Button { id: plain; background: Rectangle { } contentItem: Item { } }
    Button { id: withId; background: Rectangle { id: inner } }
    Button { id: zero; background: Rectangle { z: 0 } }
    Button { id: two; background: Rectangle { z: 2 } }
    Button { id: bound; property real level: 0; background: Rectangle { z: bound.level } }
    Button { id: script }
    Control { id: control; background: Item { } contentItem: Item { } }
    TextField { id: field; background: Rectangle { } }
    TextArea { id: area; background: Rectangle { } }
    Switch { id: sw; indicator: Rectangle { } background: Item { } }
    ComboBox { id: combo; background: Rectangle { } indicator: Item { } }
    Pane { id: pane; background: Rectangle { } }
    Component.onCompleted: {
        script.background = late.createObject(null)
        var made = late.createObject(null, { z: 3 })
        var s = [plain.background.z, plain.contentItem.z, withId.background.z, zero.background.z,
                 two.background.z, bound.background.z, script.background.z, control.background.z,
                 control.contentItem.z, field.background.z, area.background.z, sw.background.z,
                 sw.indicator.z, combo.background.z, combo.indicator.z, pane.background.z]
        bound.level = 4
        s.push(bound.background.z)
        bound.level = 0
        s.push(bound.background.z)
        script.background = made
        s.push(script.background.z)
        result = s.join(" ")
    }
}
