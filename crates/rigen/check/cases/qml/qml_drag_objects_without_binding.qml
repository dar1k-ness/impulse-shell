// `MouseArea.drag` is an object that is part of the MouseArea, and any Item has the attached
// object of `Drag` (Qt 6.12): both are there with their defaults when nothing binds a member
// of them, read through `id` or through an object, and take writes; `drag.active` and `drag`
// itself are read-only.
// qt: ma.own false;ma.viaId 2;ma.col c;inner.opacity 1;typeof drag object;drag null false;drag same true;drag.active false;drag.target null;drag.axis 3;drag.axis enum true;drag.minimumX true;drag.maximumX true;drag.minimumY true;drag.maximumY true;drag.threshold 10;drag.filterChildren false;drag.smoothed true;drag.nope undefined;drag write target true;drag write axis 2;drag write threshold 7;drag write active TypeError: Cannot assign to read-only property "active";drag write whole TypeError: Cannot assign to read-only property "drag";mb target true;mb axis 1;mb threshold 2;mb active false;mb smoothed true;typeof Drag object;Drag same true;Drag.active false;Drag.source true;Drag.target null;Drag.hotSpot 0,0;Drag.keys 0;Drag.dragType 2;Drag.supportedActions 7;Drag.proposedAction 2;Drag.nope undefined;free true;z2 1;hotSpot write 5,6;source write true;active write true;free after false;z2 after 2;active off false true 1;bound hotSpot 4,0;bound active true false;other untouched false false;
import QtQuick
Item {
    id: root
    property string result: ""
    function t(what, f) {
        var r
        try { r = String(f()) } catch (e) { r = e.name + ": " + e.message }
        result += what + " " + r + ";"
    }
    // The shape of TaskViewWindow: a MouseArea with no `drag` binding reads `drag.active`.
    MouseArea {
        id: ma
        property bool own: drag.active
        property real viaId: ma.drag.active ? 1 : 2
        property string col: drag.active ? "a" : (containsMouse ? "b" : "c")
        Item { id: inner; opacity: ma.drag.active ? 0 : 1 }
    }
    MouseArea { id: mb; drag.target: plain; drag.axis: Drag.XAxis; drag.threshold: 2 }
    // The shape of TrayButton: `Drag` of an Item with no binding to it, read in a binding
    // and written from a handler.
    Item {
        id: plain
        property bool free: !plain.Drag.active
        property int z2: Drag.active ? 2 : 1
    }
    Item { id: bound; Drag.hotSpot.x: 4 }
    Item { id: other }
    Component.onCompleted: run()
    function run() {
        t("ma.own", () => ma.own)
        t("ma.viaId", () => ma.viaId)
        t("ma.col", () => ma.col)
        t("inner.opacity", () => inner.opacity)
        t("typeof drag", () => typeof ma.drag)
        t("drag null", () => ma.drag === null)
        t("drag same", () => ma.drag === ma.drag)
        t("drag.active", () => ma.drag.active)
        t("drag.target", () => ma.drag.target)
        t("drag.axis", () => ma.drag.axis)
        t("drag.axis enum", () => ma.drag.axis === Drag.XAndYAxis)
        t("drag.minimumX", () => ma.drag.minimumX < -1e30)
        t("drag.maximumX", () => ma.drag.maximumX > 1e30)
        t("drag.minimumY", () => ma.drag.minimumY < -1e30)
        t("drag.maximumY", () => ma.drag.maximumY > 1e30)
        t("drag.threshold", () => ma.drag.threshold)
        t("drag.filterChildren", () => ma.drag.filterChildren)
        t("drag.smoothed", () => ma.drag.smoothed)
        t("drag.nope", () => ma.drag.nope)
        t("drag write target", () => { ma.drag.target = other; return ma.drag.target === other })
        t("drag write axis", () => { ma.drag.axis = Drag.YAxis; return ma.drag.axis })
        t("drag write threshold", () => { ma.drag.threshold = 7; return ma.drag.threshold })
        t("drag write active", () => { ma.drag.active = true; return ma.drag.active })
        t("drag write whole", () => { ma.drag = null; return typeof ma.drag })
        t("mb target", () => mb.drag.target === plain)
        t("mb axis", () => mb.drag.axis)
        t("mb threshold", () => mb.drag.threshold)
        t("mb active", () => mb.drag.active)
        t("mb smoothed", () => mb.drag.smoothed)
        t("typeof Drag", () => typeof plain.Drag)
        t("Drag same", () => plain.Drag === plain.Drag)
        t("Drag.active", () => plain.Drag.active)
        t("Drag.source", () => plain.Drag.source === plain)
        t("Drag.target", () => plain.Drag.target)
        t("Drag.hotSpot", () => plain.Drag.hotSpot.x + "," + plain.Drag.hotSpot.y)
        t("Drag.keys", () => plain.Drag.keys.length)
        t("Drag.dragType", () => plain.Drag.dragType)
        t("Drag.supportedActions", () => plain.Drag.supportedActions)
        t("Drag.proposedAction", () => plain.Drag.proposedAction)
        t("Drag.nope", () => plain.Drag.nope)
        t("free", () => plain.free)
        t("z2", () => plain.z2)
        t("hotSpot write", () => { plain.Drag.hotSpot.x = 5; plain.Drag.hotSpot.y = 6; return plain.Drag.hotSpot.x + "," + plain.Drag.hotSpot.y })
        t("source write", () => { plain.Drag.source = other; return plain.Drag.source === other })
        t("active write", () => { plain.Drag.active = true; return plain.Drag.active })
        t("free after", () => plain.free)
        t("z2 after", () => plain.z2)
        t("active off", () => { plain.Drag.active = false; return plain.Drag.active + " " + plain.free + " " + plain.z2 })
        t("bound hotSpot", () => bound.Drag.hotSpot.x + "," + bound.Drag.hotSpot.y)
        t("bound active", () => { bound.Drag.active = true; var a = bound.Drag.active; bound.Drag.active = false; return a + " " + bound.Drag.active })
        t("other untouched", () => other.Drag.active + " " + (other.Drag === plain.Drag))
    }
}
