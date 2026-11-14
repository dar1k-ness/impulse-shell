// `parent` is a property of an Item (and of a Timer): an animation, a QtObject, a Behavior or
// a Connections has none, so the name there is the `parent` of the component's root.
// qt: anim=null qo=null tm=icon bh=null cn=null | undefined undefined false | TypeError 10,20,30 | loader,loader,50,60,0
import QtQuick
Item {
    id: root
    property string result: ""
    Item {
        id: outer
        rotation: 10
        Item {
            id: comp
            rotation: 20
            Item {
                id: icon
                rotation: 30
                NumberAnimation on rotation {
                    id: anim
                    running: false
                    property string who: parent === icon ? "icon" : "" + parent
                    function reset() { parent.rotation = 0 }
                }
                QtObject { id: qo; property string who: parent === icon ? "icon" : "" + parent }
                Timer { id: tm; property string who: parent === icon ? "icon" : "" + parent }
                Behavior on x { id: bh; property string who: parent === icon ? "icon" : "" + parent; NumberAnimation {} }
                Connections { id: cn; property string who: parent === icon ? "icon" : "" + parent }
            }
        }
    }
    Loader {
        id: ld
        rotation: 40
        sourceComponent: Item {
            id: croot
            rotation: 50
            property string res: ""
            function go() { a2.reset(); res = [a2.who, q2.who, croot.rotation, inner.rotation, ld.rotation].join(",") }
            Item {
                id: inner
                rotation: 60
                RotationAnimation on rotation {
                    id: a2
                    running: false
                    property string who: parent === ld ? "loader" : parent === inner ? "inner" : "" + parent
                    function reset() { parent.rotation = 0 }
                }
                QtObject { id: q2; property string who: parent === ld ? "loader" : parent === inner ? "inner" : "" + parent }
            }
        }
    }
    Component.onCompleted: {
        var out = "anim=" + anim.who + " qo=" + qo.who + " tm=" + tm.who + " bh=" + bh.who + " cn=" + cn.who
        out += " | " + typeof anim.parent + " " + typeof qo.parent + " " + ("parent" in anim) + " | "
        try { anim.reset() } catch (e) { out += e.name }
        out += " " + [outer.rotation, comp.rotation, icon.rotation]
        ld.item.go()
        result = out + " | " + ld.item.res
    }
}
