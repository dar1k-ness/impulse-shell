// The JS object of a QML type (QQmlTypeWrapper). Types with no attached type are all equal
// to each other, by `===`, `Object.is`, `indexOf`, `switch`, Set and Map alike; a type with
// one equals itself only. It converts to `Module/Name`, the type of a QML file and a name on
// a directory qualifier to the bare name, a module qualifier to `Unknown Type`; a directory
// qualifier does not convert. Its enums are not enumerable.
// qt: true true true true false true false true true 0 1 item 1 3 1  | QtQuick/Item QtQuick/Item Button DirectoryImportAliasType DirectoryImportAliasType Nope QML/QtObject QtQml.Models/ListModel QQuickKeysAttached(0x) QtQuick/Item1 QtQuick/Item,Nope true | TypeError: Type error TypeError: Type error Unknown Type Unknown Type Unknown Type function true  Nope | 0 0 {} 4 true
import QtQuick
import QtQuick as Q
import QtQuick.Controls as C
import "." as Here
Item {
    id: root
    function t(f) { try { return String(f()).replace(/0x[0-9a-f]+/, "0x") } catch (e) { return String(e) } }
    function sw(x) { switch (x) { case Keys: return "keys"; case Item: return "item"; default: return "default" } }
    property string result: [Here.Nope === Here.Other, Here.Nope === Here.DirectoryImportAliasType,
        Here.Nope === Item, Item === Rectangle, Item === Keys, Keys === Q.Keys, Keys === Component,
        Q.Item === C.Button, Object.is(Item, Text), [Item].indexOf(Rectangle), [Keys, Item].indexOf(Rectangle),
        sw(Rectangle), new Set([Item, Rectangle, Item]).size, new Set([Item, Rectangle, Keys, Keys, Component]).size,
        new Map([[Item, 1]]).get(Rectangle), new Map([[Keys, 1]]).get(Item), "|",
        String(Item), String(Q.Item), String(C.Button), String(DirectoryImportAliasType),
        String(Here.DirectoryImportAliasType), String(Here.Nope), String(QtObject), String(ListModel),
        t(() => Keys), Item + 1, [Item, Here.Nope].join(), Item == "QtQuick/Item", "|",
        t(() => String(Here)), t(() => "" + Here), String(Q), "" + C, Q.toString(), typeof Q.toString,
        Item.toString === Q.toString, Item.toString.call(root), Item.toString.call(Here.Nope), "|",
        Object.keys(Item).length, Object.keys(Text.HAlignment).length, JSON.stringify(Text),
        Q.Text.AlignHCenter, Item.hasOwnProperty("toString")].join(" ")
}
