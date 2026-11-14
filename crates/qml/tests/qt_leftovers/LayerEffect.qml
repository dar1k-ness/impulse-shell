import QtQuick
Item {
    id: root
    width: 100; height: 100
    property int made: 0
    property int gone: 0
    Rectangle { id: a; width: 10; height: 10
        layer.effect: ShaderEffect { objectName: "fxA"; Component.onCompleted: root.made++; Component.onDestruction: root.gone++ }
    }
    Rectangle { id: b; width: 10; height: 10
        layer.enabled: true
        layer.effect: ShaderEffect { objectName: "fxB"; property int k: 7; Component.onCompleted: root.made++; Component.onDestruction: root.gone++ }
    }
    Rectangle { id: c; width: 10; height: 10 }
    Rectangle { id: d; width: 10; height: 10
        property bool on: false
        layer.enabled: on
        layer.effect: comp
    }
    Component { id: comp; ShaderEffect { objectName: "fxC"; Component.onCompleted: root.made++; Component.onDestruction: root.gone++ } }
    Component { id: comp2; ShaderEffect { objectName: "fxD"; Component.onCompleted: root.made++; Component.onDestruction: root.gone++ } }
    Component.onCompleted: {
        console.log("made", made);
        console.log("a", typeof a.layer.effect, a.layer.effect === null, a.layer.effect.status === Component.Ready);
        console.log("b", typeof b.layer.effect, b.layer.effect.status === Component.Ready, typeof b.layer.effect.createObject);
        console.log("c", c.layer.effect, c.layer.enabled);
        console.log("d", d.layer.effect === comp, d.layer.enabled);
        var o = b.layer.effect.createObject(root);
        console.log("created", o.objectName, o.k, made);
        a.layer.enabled = true;
        console.log("a on", made, gone);
        a.layer.enabled = true;
        console.log("a on again", made, gone);
        a.layer.enabled = false;
        console.log("a off", made, gone);
        c.layer.effect = comp;
        console.log("c set", c.layer.effect === comp, made, gone);
        c.layer.enabled = true;
        console.log("c on", made, gone);
        c.layer.effect = comp;
        console.log("c same", made, gone);
        c.layer.effect = comp2;
        console.log("c other", c.layer.effect === comp2, made, gone);
        b.layer.effect = null;
        console.log("b null", b.layer.effect, made, gone);
        b.layer.effect = comp;
        console.log("b comp", made, gone);
        d.on = true;
        console.log("d on", made, gone);
        d.on = false;
        console.log("d off", made, gone);
        c.layer.enabled = false;
        console.log("c off", made, gone);
        Qt.quit();
    }
}
