import QtQuick
import QtQuick.Controls
Item {
    id: root
    width: 200; height: 200
    // The children of p: the objectName, "src" for a layer's ShaderEffectSource.
    function kids(p) {
        var s = [];
        for (var i = 0; i < p.children.length; i++) {
            var c = p.children[i];
            s.push((c.objectName || (c.sourceItem !== undefined ? "src" : "?")) + (c.visible ? "" : "(hidden)"));
        }
        return s.join(",");
    }
    function names(list) {
        var s = [];
        for (var i = 0; i < list.length; i++)
            s.push(list[i].objectName || (list[i].sourceItem !== undefined ? "src" : "?"));
        return s.join(",");
    }
    function geo(o) {
        return o.x + "," + o.y + " " + o.width + "x" + o.height + " z" + o.z + " o" + o.opacity
            + " s" + o.scale + " r" + o.rotation + " " + o.transformOrigin + " " + o.visible;
    }
    property Item fx: null
    property bool over: false
    Item {
        id: box
        objectName: "box"
        x: 5; y: 6
        Rectangle { objectName: "before"; width: 10; height: 10 }
        Rectangle {
            id: a
            objectName: "a"
            x: 20; y: 30; width: 40; height: 50; z: 2; opacity: 0.5
            layer.effect: ShaderEffect {
                objectName: "fx"
                property var source
                Component.onCompleted: {
                    root.fx = this;
                    console.log("fx completed", parent ? parent.objectName : parent, source.sourceItem === a,
                                source.visible, root.geo(this), root.kids(box));
                }
                Component.onDestruction: if (!root.over) console.log("fx gone", root.kids(box))
            }
        }
        Rectangle { objectName: "after"; width: 10; height: 10 }
    }
    Row {
        id: row
        objectName: "row"
        Rectangle { objectName: "r1"; width: 10; height: 10 }
        Rectangle {
            id: r2; objectName: "r2"; width: 10; height: 10
            layer.enabled: true
            layer.samplerName: "tex"
            layer.effect: ShaderEffect {
                objectName: "rfx"
                property var source
                property var tex
                Component.onCompleted: console.log("rfx completed", parent.objectName, source, tex.sourceItem === r2, root.kids(row))
            }
        }
        Rectangle { id: r3; objectName: "r3"; width: 10; height: 10; layer.enabled: true }
    }
    SwipeView {
        id: swipe
        objectName: "swipe"
        onCountChanged: console.log("swipe count", count)
        onCurrentIndexChanged: console.log("swipe index", currentIndex)
        Rectangle { id: p0; objectName: "p0"; layer.enabled: true; layer.effect: ShaderEffect { objectName: "pfx" } }
        Rectangle { id: p1; objectName: "p1" }
    }
    Item { id: other; objectName: "other"; visible: false }
    Component {
        id: dyn
        Rectangle {
            objectName: "dyn"
            layer.enabled: true
            layer.effect: ShaderEffect { objectName: "dfx"; Component.onDestruction: if (!root.over) console.log("dfx gone") }
            Component.onDestruction: if (!root.over) console.log("dyn gone")
        }
    }
    Timer {
        interval: 1; running: true
        onTriggered: {
            console.log("destroyed", root.kids(box));
            root.over = true;
            Qt.quit();
        }
    }
    Component.onCompleted: {
        console.log("start", kids(box), "|", kids(row));
        console.log("swipe", swipe.count, names(swipe.contentChildren), swipe.currentIndex, swipe.currentItem.objectName, p0.parent === p1.parent);
        swipe.addItem(dyn.createObject(null));
        console.log("swipe added", swipe.count, names(swipe.contentChildren), swipe.currentIndex);
        p1.layer.enabled = true;
        console.log("swipe layer", swipe.count, names(swipe.contentChildren), swipe.currentIndex);
        p1.layer.effect = dyn;
        console.log("swipe effect", swipe.count, names(swipe.contentChildren), swipe.currentIndex);
        swipe.takeItem(0);
        console.log("swipe taken", swipe.count, names(swipe.contentChildren), swipe.currentIndex, p0.parent);
        p1.layer.enabled = false;
        console.log("swipe layer off", swipe.count, names(swipe.contentChildren), swipe.currentIndex);
        var lone = dyn.createObject(null);
        console.log("lone", lone.parent, lone.layer.enabled);
        lone.parent = other;
        console.log("lone in", kids(other));
        a.layer.enabled = true;
        console.log("on", kids(box), kids(a).length);
        var src = fx.source;
        console.log("fx", fx.parent.objectName, geo(fx), src.sourceItem === a, src.hideSource, src.parent.objectName, geo(src));
        a.x = 21; a.width = 41; a.z = 3; a.opacity = 0.25; a.scale = 2; a.rotation = 10; a.transformOrigin = Item.TopLeft;
        console.log("moved", geo(fx), "|", geo(src));
        a.visible = false;
        console.log("hidden", kids(box), fx.visible);
        a.visible = true;
        box.visible = false;
        console.log("box hidden", kids(box), fx.visible);
        box.visible = true;
        a.parent = other;
        console.log("reparented", kids(box), "|", kids(other), fx.parent.objectName, fx.visible);
        a.parent = box;
        console.log("back", kids(box));
        a.parent = null;
        console.log("no parent", kids(box), fx.parent, src.parent, fx.visible);
        a.parent = box;
        console.log("back again", kids(box));
        a.layer.effect = null;
        console.log("no effect", kids(box), geo(src), src === box.children[box.children.length - 1]);
        a.y = 31; a.opacity = 1;
        console.log("moved source", geo(src));
        a.layer.effect = dyn;
        console.log("other effect", kids(box), geo(box.children[box.children.length - 1]), geo(src));
        a.layer.enabled = false;
        console.log("off", kids(box));
        a.layer.enabled = true;
        console.log("on again", kids(box));
        var d = dyn.createObject(box);
        console.log("created", kids(box));
        d.destroy();
        a.destroy();
    }
}
