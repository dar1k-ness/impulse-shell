import QtQuick
Rectangle {
    id: root
    property int declared: 7
    property string log: ""
    signal poked()
    function ping() { return "pong" }
    // A key built at run time: no name of the program's text.
    function k(path) { return "ev_absent_" + path }
    function t(what, f) {
        var r
        try { r = String(f()) } catch (e) { r = e.name + ": " + String(e.message).replace(/0x[0-9a-f]+/, "0x") }
        log += what + " " + r + "\n"
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
    Component.onCompleted: { run(); console.log("LOG<" + log + ">LOG"); Qt.quit() }
}
