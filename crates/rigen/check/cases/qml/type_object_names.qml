// What the JS object of a QML type has of a function (QQmlTypeWrapper's prototype is over
// Function.prototype): `call`, `bind`, `apply`, an empty `name`, `length` 0; none can be
// called or written. `QtObject` is a function, `new QtObject()` an object. `Qt` converts to
// `Qt(0x...)`. `ListView` and `GridView` as names are each its own attached object, the one
// their name is on an import qualifier.
// qt: function function function string  0 true true true false TypeError: Type error TypeError: Type error undefined TypeError: Type error | function QML/QtObject object true TypeError: Function can only be called with |new|. | Qt(0x) Qt(0x) true false undefined true function | false true true false QQuickListViewAttached(0x) QQuickGridViewAttached(0x) true 1 undefined
import QtQuick
import QtQuick as Q
Item {
    id: root
    function t(f) { try { return String(f()).replace(/0x[0-9a-f]+/, "0x") } catch (e) { return String(e) } }
    function strict() { "use strict"; Item.foo = 1 }
    property string result: [typeof Item.call, typeof Item.bind, typeof Item.apply, typeof Item.name, Item.name, Item.length,
        Item.constructor === Function, Item instanceof Function, Object.getPrototypeOf(Object.getPrototypeOf(Item)) === Function.prototype,
        Object.getPrototypeOf(Item) === Function.prototype, t(() => Item.call(null)), t(() => new Item()),
        t(() => (Item.foo = 1, Item.foo)), t(() => strict()), "|",
        typeof QtObject, String(QtObject), typeof new QtObject(), Qt.isQtObject(new QtObject()), t(() => QtObject()), "|",
        t(() => Qt), t(() => Qt.toString()), Qt instanceof Function, Qt === Item, typeof Qt.call, "call" in Qt, typeof Qt.rgba, "|",
        ListView === GridView, ListView === Q.ListView, GridView === Q.GridView, ListView === Item,
        t(() => ListView), t(() => GridView), ListView === ListView, ListView.Horizontal, typeof GridView.Horizontal].join(" ")
}
