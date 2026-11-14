// The instances of the Components a document declares (a delegate of a Repeater, a ListView
// and an Instantiator, `createObject`, a Loader's `sourceComponent`) have the document
// instance's JS files (Qt 6.12; QQmlObjectCreator::create hands a Component's instance the
// creation context's imported scripts): one set of the file's variables, whose code sees the
// document's root object and ids and nothing of the delegate. An instance of an inline
// component and of another document has files of its own, also when it is made again and
// again from one Component.
// qt: root bump 1;root bump 2;d0 get 2;d0 bump 3;d1 get 3;root get 3;d0 inner get 3;made get 3;made bump 4;loader get 4;loader bump 5;root get 5;ic1 get 0;ic1 bump 1;ic2 get 0;root get 5;o1 bump 1;o2 get 0;root get 5;root who root;d0 who root;d1 who root;d0 inner who root;made who root;loader who root;ic1 who inline;o1 who other;root w 11;d0 w 11;made w 11;ic1 w 33;d1 idx none;d1 md none;d1 cellId none;d1 rootId root;ic1 rootId root;root idx none;row get 5;row bump 6;row who root;nested get 6;nested who root;obj get 6;obj bump 7;obj who root;root get 7;ic cell get 1;ic cell bump 2;ic1 get 2;ic2 cell get 0;ic cell who inline;file get 0;file bump 1;file who other;root get 7;made docs bump 1,1,1,1,1;made docs bump 3 2,3;made docs get 1,1,1,3,1;made docs who doc0,doc1,doc2,doc3,doc4;cell docs bump 2 1,2;cell docs get 0,0,2,0;cell docs who cell0,cell1,cell2,cell3;root get 7
import QtQuick
import "js_scope_instances.js" as Plain
Item {
    id: root
    property string result: ""
    property string tag: "root"
    property var seen: []
    width: 11
    function t(name, fn) {
        var v
        try { v = fn() } catch (e) { v = e }
        seen.push(name + " " + v)
    }
    component Inline: Item {
        id: ic
        property string tag: "inline"
        width: 33
        function bump() { return Plain.bump() }
        function get() { return Plain.get() }
        function who() { return Plain.who() }
        function w() { return Plain.w() }
        function rootId() { return Plain.rootId() }
        property alias cells: icRep
        Repeater {
            id: icRep
            model: 2
            Item {
                property string tag: "iccell"
                function bump() { return Plain.bump() }
                function get() { return Plain.get() }
                function who() { return Plain.who() }
            }
        }
    }
    Inline { id: ic1 }
    Inline { id: ic2 }
    JsScopeInstancesOther { id: o1 }
    JsScopeInstancesOther { id: o2 }
    Repeater {
        id: rep
        model: ["a", "b"]
        Item {
            id: cell
            property string tag: "cell" + index
            width: 22
            function bump() { return Plain.bump() }
            function get() { return Plain.get() }
            function who() { return Plain.who() }
            function w() { return Plain.w() }
            function idx() { return Plain.idx() }
            function md() { return Plain.md() }
            function cellId() { return Plain.cellId() }
            function rootId() { return Plain.rootId() }
            Item {
                id: inner
                property string tag: "inner"
                function who() { return Plain.who() }
                function get() { return Plain.get() }
            }
            property Item innerItem: inner
        }
    }
    Component {
        id: comp
        Item {
            property string tag: "made"
            width: 44
            function bump() { return Plain.bump() }
            function get() { return Plain.get() }
            function who() { return Plain.who() }
            function w() { return Plain.w() }
        }
    }
    Loader { id: ld; sourceComponent: comp }
    Component {
        id: holder
        Item {
            property alias doc: nestedDoc
            property int n: 0
            JsScopeInstancesOther { id: nestedDoc; tag: "doc" + parent.n }
        }
    }
    Repeater {
        id: docs
        model: 4
        Item {
            property alias doc: cellDoc
            JsScopeInstancesOther { id: cellDoc; tag: "cell" + index }
        }
    }
    Loader { id: ldFile; source: "JsScopeInstancesOther.qml" }
    ListView {
        id: view
        width: 100; height: 100
        model: ListModel { ListElement { name: "x" } ListElement { name: "y" } }
        delegate: Item {
            width: 10; height: 10
            property string tag: "row"
            function bump() { return Plain.bump() }
            function get() { return Plain.get() }
            function who() { return Plain.who() }
            property alias nested: nestedRep
            Repeater {
                id: nestedRep
                model: 1
                Item {
                    property string tag: "nested"
                    function get() { return Plain.get() }
                    function who() { return Plain.who() }
                }
            }
        }
    }
    Instantiator {
        id: inst
        model: 2
        QtObject {
            property string tag: "obj"
            function bump() { return Plain.bump() }
            function get() { return Plain.get() }
            function who() { return Plain.who() }
        }
    }
    function run() {
        var d0 = rep.itemAt(0), d1 = rep.itemAt(1)
        var made = comp.createObject(root)
        t("root bump", () => Plain.bump())
        t("root bump", () => Plain.bump())
        t("d0 get", () => d0.get())
        t("d0 bump", () => d0.bump())
        t("d1 get", () => d1.get())
        t("root get", () => Plain.get())
        t("d0 inner get", () => d0.innerItem.get())
        t("made get", () => made.get())
        t("made bump", () => made.bump())
        t("loader get", () => ld.item.get())
        t("loader bump", () => ld.item.bump())
        t("root get", () => Plain.get())
        t("ic1 get", () => ic1.get())
        t("ic1 bump", () => ic1.bump())
        t("ic2 get", () => ic2.get())
        t("root get", () => Plain.get())
        t("o1 bump", () => o1.bump())
        t("o2 get", () => o2.get())
        t("root get", () => Plain.get())
        t("root who", () => Plain.who())
        t("d0 who", () => d0.who())
        t("d1 who", () => d1.who())
        t("d0 inner who", () => d0.innerItem.who())
        t("made who", () => made.who())
        t("loader who", () => ld.item.who())
        t("ic1 who", () => ic1.who())
        t("o1 who", () => o1.who())
        t("root w", () => Plain.w())
        t("d0 w", () => d0.w())
        t("made w", () => made.w())
        t("ic1 w", () => ic1.w())
        t("d1 idx", () => d1.idx())
        t("d1 md", () => d1.md())
        t("d1 cellId", () => d1.cellId())
        t("d1 rootId", () => d1.rootId())
        t("ic1 rootId", () => ic1.rootId())
        t("root idx", () => Plain.idx())
        t("row get", () => view.itemAtIndex(0).get())
        t("row bump", () => view.itemAtIndex(1).bump())
        t("row who", () => view.itemAtIndex(0).who())
        t("nested get", () => view.itemAtIndex(0).nested.itemAt(0).get())
        t("nested who", () => view.itemAtIndex(0).nested.itemAt(0).who())
        t("obj get", () => inst.objectAt(0).get())
        t("obj bump", () => inst.objectAt(1).bump())
        t("obj who", () => inst.objectAt(0).who())
        t("root get", () => Plain.get())
        t("ic cell get", () => ic1.cells.itemAt(0).get())
        t("ic cell bump", () => ic1.cells.itemAt(1).bump())
        t("ic1 get", () => ic1.get())
        t("ic2 cell get", () => ic2.cells.itemAt(0).get())
        t("ic cell who", () => ic1.cells.itemAt(0).who())
        t("file get", () => ldFile.item.get())
        t("file bump", () => ldFile.item.bump())
        t("file who", () => ldFile.item.who())
        t("root get", () => Plain.get())
        var hs = []
        for (var i = 0; i < 5; i++) hs.push(holder.createObject(root, { n: i }))
        t("made docs bump", () => hs.map(h => h.doc.bump()).join(","))
        t("made docs bump 3", () => hs[3].doc.bump() + "," + hs[3].doc.bump())
        t("made docs get", () => hs.map(h => h.doc.get()).join(","))
        t("made docs who", () => hs.map(h => h.doc.who()).join(","))
        t("cell docs bump 2", () => docs.itemAt(2).doc.bump() + "," + docs.itemAt(2).doc.bump())
        t("cell docs get", () => [0, 1, 2, 3].map(i => docs.itemAt(i).doc.get()).join(","))
        t("cell docs who", () => [0, 1, 2, 3].map(i => docs.itemAt(i).doc.who()).join(","))
        t("root get", () => Plain.get())
        result = seen.join(";")
    }
    Component.onCompleted: Qt.callLater(run)
}
