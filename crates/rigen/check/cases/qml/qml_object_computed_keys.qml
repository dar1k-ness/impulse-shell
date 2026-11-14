// A QML object's member reached by a computed key, as in Qt 6.12, for a key no object has and
// for the names the object has: reading, writing (an object a QML document created takes no
// new member, a group object keeps one), `in`, `delete`, the Object.prototype and Reflect
// lookups, calling and `?.[]`. The interpreter's side of crates/qml/tests/runtime_r1.rs
// (`no_computed_key_path_interns_an_unknown_key`) also checks that no such key is interned.
// qt: get undefined;set Error: Cannot assign to non-existent property "ev_absent_set";compound Error: Cannot assign to non-existent property "ev_absent_compound";increment Error: Cannot assign to non-existent property "ev_absent_increment";in false;delete true;hasOwnProperty false;hasOwnProperty.call false;propertyIsEnumerable false;getOwnPropertyDescriptor undefined;Reflect.has false;Reflect.get undefined;call TypeError: undefined is not a function;optional undefined;optional call undefined;group get undefined;group in false;group set 1;known get 7;known set 8;known compound 9;known increment 10;known in true,true,true,true,true;known delete true,10;known hasOwnProperty true,true,true,true;known call pong,function,false;known optional 10,false;known group 0,true,3;
import QtQuick
Rectangle {
    id: root
    property int declared: 7
    property string result: ""
    signal poked()
    function ping() { return "pong" }
    // A key built at run time: no name of the program's text.
    function k(path) { return "ev_absent_" + path }
    function t(what, f) {
        var r
        try { r = String(f()) } catch (e) { r = e.name + ": " + String(e.message).replace(/0x[0-9a-f]+/, "0x") }
        result += what + " " + r + ";"
    }
    function run() {
        t("get", () => root[k("get")])
        t("set", () => { root[k("set")] = 1; return root[k("set")] })
        t("compound", () => { root[k("compound")] += 1; return root[k("compound")] })
        t("increment", () => { root[k("increment")]++; return root[k("increment")] })
        t("in", () => k("in") in root)
        t("delete", () => delete root[k("delete")])
        t("hasOwnProperty", () => root.hasOwnProperty(k("own")))
        t("hasOwnProperty.call", () => Object.prototype.hasOwnProperty.call(root, k("own_call")))
        t("propertyIsEnumerable", () => root.propertyIsEnumerable(k("enumerable")))
        t("getOwnPropertyDescriptor", () => Object.getOwnPropertyDescriptor(root, k("descriptor")))
        t("Reflect.has", () => Reflect.has(root, k("reflect_has")))
        t("Reflect.get", () => Reflect.get(root, k("reflect_get")))
        t("call", () => root[k("call")]())
        t("optional", () => root?.[k("optional")])
        t("optional call", () => root?.[k("optional_call")]?.())
        t("group get", () => root.anchors[k("group_get")])
        t("group in", () => k("group_in") in root.anchors)
        // A new name written to an object no QML document created is kept.
        t("group set", () => { root.anchors[k("group_set")] = 1; return root.anchors[k("group_set")] })
        // The same paths with names the object has, built at run time.
        t("known get", () => root["decl" + "ared"])
        t("known set", () => { root["decl" + "ared"] = 8; return root["decl" + "ared"] })
        t("known compound", () => { root["decl" + "ared"] += 1; return declared })
        t("known increment", () => { root["decl" + "ared"]++; return declared })
        t("known in", () => [("decl" + "ared") in root, ("pi" + "ng") in root, ("po" + "ked") in root,
                             ("onPo" + "ked") in root, ("forceActive" + "Focus") in root])
        t("known delete", () => [delete root["decl" + "ared"], declared])
        t("known hasOwnProperty", () => [root.hasOwnProperty("decl" + "ared"), root.hasOwnProperty("pi" + "ng"),
                                         root.hasOwnProperty("childrenRect" + "Changed"),
                                         root.hasOwnProperty("mapTo" + "Global")])
        t("known call", () => [root["pi" + "ng"](), typeof root["mapFrom" + "Global"], root["to" + "String"] === root.toString])
        t("known optional", () => [root?.["decl" + "ared"], root?.["activeFocusOn" + "Tab"]])
        t("known group", () => { root.anchors["left" + "Margin"] = 3
                                 return [root.anchors["baseline" + "Offset"], ("right" + "Margin") in root.anchors,
                                         anchors.leftMargin] })
    }
    Component.onCompleted: run()
}
