// A write to an object V4 keeps nothing new in (Qt 6.12): a ListModel row without
// `dynamicRoles` (QV4::ModelObject), the `Qt` object, a type and its enums
// (QQmlTypeWrapper::virtualPut), a type's attached object. Sloppy code drops the write, strict
// code gets `TypeError: Type error`, a method, a signal and a read-only property say so, and
// `root.font` on an object without one is a missing property.
// qt: elem new undefined,undefined;elem new strict undefined;elem new [k] undefined;elem role z,z;elem role number 2.5;elem role compound 7;elem index 1;elem delete true,z;elem appended 7,undefined;elem set 9,undefined;elem dynamicRoles 1,1;model new Error: Cannot assign to non-existent property "foo";model count TypeError: Cannot assign to read-only property "count";Qt new undefined,false;Qt new strict TypeError: Type error;Qt new [k] undefined;Qt compound undefined;Qt method TypeError: Cannot assign to read-only property "rgba";Qt method strict TypeError: Cannot assign to read-only property "rgba";Qt.callLater TypeError: Cannot assign to read-only property "callLater";Qt enum 1;Qt enum strict TypeError: Type error;Qt.Key_A 65;Qt.platform TypeError: Cannot assign to read-only property "platform";Qt.application TypeError: Cannot assign to read-only property "application";Qt.styleHints TypeError: Cannot assign to read-only property "styleHints";Qt.inputMethod TypeError: Cannot assign to read-only property "inputMethod";Qt.application new 1;Qt.uiLanguage xx;Qt delete true,function;type new undefined,false;type new strict TypeError: Type error;type new [k] undefined;type enum 0;type enum strict TypeError: Type error;type enum compound 0;type enum delete true,0;type property undefined;type cpp property undefined;type with attached 1;enum type object;enum type member 1;enum type new undefined;Component.status undefined;Component.foo undefined;Component.Ready 1;Component.objectName x;comp.status TypeError: Cannot assign to read-only property "status";comp.progress TypeError: Cannot assign to read-only property "progress";comp.foo Error: Cannot assign to non-existent property "foo";Keys.enabled false;Keys.foo undefined;Keys.foo strict TypeError: Type error;Keys.enabledChanged TypeError: Cannot assign to read-only property "enabledChanged";Keys.onPressed TypeError: Cannot assign to read-only property "pressed";Keys.pressed TypeError: Cannot assign to read-only property "pressed";Keys.priority 1;Keys.AfterItem 1;ListView.foo undefined;delegate wAttached TypeError: Cannot assign to read-only property "isCurrentItem";delegate wView TypeError: Cannot assign to read-only property "view";delegate wNew undefined;delegate wDelay true;root.font Error: Cannot assign to non-existent property "font";root.anchors TypeError: Cannot assign to read-only property "anchors";root.childrenRect TypeError: Cannot assign to read-only property "childrenRect";root.implicitWidth 5;Math new 1;JSON new 1;console new 1;
import QtQuick
Item {
    id: root
    property string result: ""
    function t(what, f) {
        var r
        try { r = String(f()) } catch (e) { r = e.name + ": " + e.message }
        result += what + " " + r + ";"
    }
    ListModel { id: lm; ListElement { name: "a"; n: 1 } ListElement { name: "b"; n: 2 } }
    ListModel { id: dyn; dynamicRoles: true }
    ListModel { id: empty }
    Component { id: comp; Item {} }
    ListView { id: lv; width: 100; height: 100; model: lm
        delegate: Item { height: 10
            function wAttached() { ListView.isCurrentItem = false; return typeof ListView.isCurrentItem }
            function wView() { ListView.view = null; return typeof ListView.view }
            function wNew() { ListView.foo = 1; return ListView.foo }
            function wDelay() { ListView.delayRemove = true; return ListView.delayRemove }
        }
    }
    Component.onCompleted: run()
    function run() {
        t("elem new", () => { var e = lm.get(0); e.foo = 1; return e.foo + "," + lm.get(0).foo })
        t("elem new strict", () => { "use strict"; var e = lm.get(0); e.foo = 1; return e.foo })
        t("elem new [k]", () => { var e = lm.get(0); var k = "dyn"; e[k] = 1; return e[k] })
        t("elem role", () => { var e = lm.get(0); e.name = "z"; return e.name + "," + lm.get(0).name })
        t("elem role number", () => { var e = lm.get(0); e.n = 2.5; return e.n })
        t("elem role compound", () => { var e = lm.get(1); e.n += 5; return e.n })
        t("elem index", () => { var e = lm.get(0); e[0] = 1; return e[0] })
        t("elem delete", () => { var e = lm.get(0); return (delete e.name) + "," + e.name })
        t("elem appended", () => { empty.append({ a: 1 }); var e = empty.get(0); e.b = 2; e.a = 7; return e.a + "," + e.b })
        t("elem set", () => { lm.set(0, { extra: 5 }); var e = lm.get(0); e.extra = 9; e.other = 1; return e.extra + "," + e.other })
        t("elem dynamicRoles", () => { dyn.append({ a: 1 }); var e = dyn.get(0); e.foo = 1; return e.foo + "," + dyn.get(0).foo })
        t("model new", () => { lm.foo = 1; return lm.foo })
        t("model count", () => { lm.count = 5; return lm.count })
        t("Qt new", () => { Qt.foo = 1; return Qt.foo + "," + ("foo" in Qt) })
        t("Qt new strict", () => { "use strict"; Qt.foo2 = 1; return Qt.foo2 })
        t("Qt new [k]", () => { var k = "dyn2"; Qt[k] = 1; return Qt[k] })
        t("Qt compound", () => { Qt.foo3 += 1; return Qt.foo3 })
        t("Qt method", () => { Qt.rgba = 1; return typeof Qt.rgba })
        t("Qt method strict", () => { "use strict"; Qt.rgba = 1; return typeof Qt.rgba })
        t("Qt.callLater", () => { Qt.callLater = 1; return typeof Qt.callLater })
        t("Qt enum", () => { Qt.AlignLeft = 99; return Qt.AlignLeft })
        t("Qt enum strict", () => { "use strict"; Qt.AlignLeft = 99; return Qt.AlignLeft })
        t("Qt.Key_A", () => { Qt.Key_A = 0; return Qt.Key_A })
        t("Qt.platform", () => { Qt.platform = 1; return typeof Qt.platform })
        t("Qt.application", () => { Qt.application = 1; return typeof Qt.application })
        t("Qt.styleHints", () => { Qt.styleHints = 1; return typeof Qt.styleHints })
        t("Qt.inputMethod", () => { Qt.inputMethod = 1; return typeof Qt.inputMethod })
        t("Qt.application new", () => { Qt.application.foo = 1; return Qt.application.foo })
        t("Qt.uiLanguage", () => { var o = Qt.uiLanguage; Qt.uiLanguage = "xx"; var n = Qt.uiLanguage; Qt.uiLanguage = o; return n })
        t("Qt delete", () => (delete Qt.rgba) + "," + typeof Qt.rgba)
        t("type new", () => { Item.foo = 1; return Item.foo + "," + ("foo" in Item) })
        t("type new strict", () => { "use strict"; Item.foo2 = 1; return Item.foo2 })
        t("type new [k]", () => { var k = "dyn3"; Item[k] = 1; return Item[k] })
        t("type enum", () => { Item.TopLeft = 99; return Item.TopLeft })
        t("type enum strict", () => { "use strict"; Item.TopLeft = 5; return Item.TopLeft })
        t("type enum compound", () => { Item.TopLeft += 1; return Item.TopLeft })
        t("type enum delete", () => (delete Item.TopLeft) + "," + Item.TopLeft)
        t("type property", () => { Item.objectName = "x"; return Item.objectName })
        t("type cpp property", () => { Item.width = 5; return Item.width })
        t("type with attached", () => { Text.AlignLeft = 99; return Text.AlignLeft })
        t("enum type", () => { Text.HAlignment = 1; return typeof Text.HAlignment })
        t("enum type member", () => { Text.HAlignment.AlignLeft = 99; return Text.HAlignment.AlignLeft })
        t("enum type new", () => { Text.HAlignment.foo = 99; return Text.HAlignment.foo })
        t("Component.status", () => { Component.status = 1; return Component.status })
        t("Component.foo", () => { Component.foo = 1; return Component.foo })
        t("Component.Ready", () => { Component.Ready = 7; return Component.Ready })
        t("Component.objectName", () => { Component.objectName = "x"; return Component.objectName })
        t("comp.status", () => { comp.status = 5; return comp.status })
        t("comp.progress", () => { comp.progress = 5; return comp.progress })
        t("comp.foo", () => { comp.foo = 5; return comp.foo })
        t("Keys.enabled", () => { Keys.enabled = false; return Keys.enabled })
        t("Keys.foo", () => { Keys.foo = 1; return Keys.foo })
        t("Keys.foo strict", () => { "use strict"; Keys.foo2 = 1; return Keys.foo2 })
        t("Keys.enabledChanged", () => { Keys.enabledChanged = 1; return typeof Keys.enabledChanged })
        t("Keys.onPressed", () => { Keys.onPressed = 1; return typeof Keys.onPressed })
        t("Keys.pressed", () => { Keys.pressed = 1; return typeof Keys.pressed })
        t("Keys.priority", () => { Keys.priority = Keys.AfterItem; return Keys.priority })
        t("Keys.AfterItem", () => { Keys.AfterItem = 9; return Keys.AfterItem })
        t("ListView.foo", () => { ListView.foo = 1; return ListView.foo })
        var d = lv.itemAtIndex(1)
        for (var k of ["wAttached", "wView", "wNew", "wDelay"]) t("delegate " + k, d[k])
        t("root.font", () => { root.font = 1; return root.font })
        t("root.anchors", () => { root.anchors = 1; return typeof root.anchors })
        t("root.childrenRect", () => { root.childrenRect = 1; return typeof root.childrenRect })
        t("root.implicitWidth", () => { root.implicitWidth = 5; return root.implicitWidth })
        t("Math new", () => { Math.foo = 1; return Math.foo })
        t("JSON new", () => { JSON.foo = 1; return JSON.foo })
        t("console new", () => { console.foo = 1; return console.foo })
    }
}
