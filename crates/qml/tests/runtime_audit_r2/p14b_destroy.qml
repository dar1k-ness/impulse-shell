import QtQuick
Item {
    id: r
    Component {
        id: c
        Item {
            id: me
            Component.onCompleted: {
                try { me.destroy(); console.log("in onCompleted: no throw") } catch (e) { console.log("in onCompleted threw:", e.message) }
            }
            Component.onDestruction: console.log("dyn destroyed")
        }
    }
    Loader { sourceComponent: c }
    Component.onCompleted: {
        c.createObject(r)
        console.log("created")
    }
}
