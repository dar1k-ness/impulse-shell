import QtQuick
Item {
    id: outer
    objectName: "outer"
    width: 100
    property int src: 1
    property int src2: 1
    property string v: "v0"
    property string bound: "b" + src
    property string live: "l0"
    property string none: "n0"
    property string val: "x" + src
    property string bnd: "y" + src
    property string other: "o0"
    property bool on: false
    property bool on2: true
    property string prop: "other"
    onVChanged: console.log("v", v)
    onBoundChanged: console.log("bound", bound)
    onLiveChanged: console.log("live", live)
    onNoneChanged: console.log("none", none)
    onValChanged: console.log("val", val)
    onBndChanged: console.log("bnd", bnd)
    onOtherChanged: console.log("other", other)
    component Foo: Item {
        id: foo
        objectName: "foo"
        Item {
            objectName: "inner"
            // `parent` is not a property of Binding: it is the one of the component root.
            Binding { target: outer; property: "v"; value: "v1"; when: parent.width == 201 }
        }
    }
    Foo { id: f; width: 10 }
    Binding { target: outer; property: "bound"; value: "held"; when: outer.on }
    Binding { target: outer; property: "live"; value: "l" + outer.src2 }
    Binding { id: bn; target: outer; property: "none"; value: "held"; when: outer.on; restoreMode: Binding.RestoreNone }
    Binding { target: outer; property: "val"; value: "held"; when: outer.on; restoreMode: Binding.RestoreValue }
    Binding { target: outer; property: "bnd"; value: "held"; when: outer.on; restoreMode: Binding.RestoreBinding }
    Binding { id: mv; target: outer; property: outer.prop; value: "moved"; when: outer.on2 }
    Binding { id: unset; target: outer; property: "v" }
    Binding { id: free; value: "free" }
    Component.onCompleted: console.log("completed", v, bound, live, none, val, bnd, other)
    Timer {
        interval: 0; running: true
        onTriggered: {
            console.log("-- width"); f.width = 201; outer.width = 200; outer.width = 201; console.log(outer.v)
            outer.width = 300; console.log(outer.v)
            console.log("-- on"); outer.on = true; console.log(outer.bound, outer.none, outer.val, outer.bnd)
            console.log("-- src"); outer.src = 2; outer.src2 = 2; console.log(outer.bound, outer.live, outer.val, outer.bnd)
            console.log("-- off"); outer.on = false; console.log(outer.bound, outer.none, outer.val, outer.bnd)
            console.log("-- src"); outer.src = 3; outer.src2 = 3; console.log(outer.bound, outer.live, outer.val, outer.bnd)
            console.log("-- prop"); outer.prop = "v"; console.log(outer.other, outer.v)
            console.log("-- target"); mv.property = "objectName"; console.log(outer.v, outer.objectName); mv.target = f; console.log(outer.objectName, f.objectName)
            console.log("-- off2"); outer.on2 = false; console.log(outer.other, outer.v, f.objectName)
            console.log("-- write"); outer.on = true; outer.bound = "written"; outer.val = "w2"; outer.none = "w3"; outer.on = false; console.log(outer.bound, outer.val, outer.none)
            outer.src = 4; console.log(outer.bound, bn.when, bn.restoreMode === Binding.RestoreNone)
            console.log("-- again"); outer.on = true; console.log(outer.bound, outer.val, outer.none, outer.bnd); outer.on = false; console.log(outer.bound, outer.val, outer.none, outer.bnd)
        }
    }
}
