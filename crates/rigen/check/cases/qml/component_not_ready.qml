// A component that is not Ready (status Error: no such file; status Loading: asked for with
// Component.Asynchronous) gives null for createObject and incubateObject and warns
// "QQmlComponent: Component is not ready" each time; a Ready one makes its object.
// The `errors` of the line below are the eight warnings Qt prints, in order.
// qt: 3 null true object null null null null 0;2 null null null 0;1 5 9 1 8 3 errors ["QQmlComponent: Component is not ready", "QQmlComponent: Component is not ready", "QQmlComponent: Component is not ready", "QQmlComponent: Component is not ready", "QQmlComponent: Component is not ready", "QQmlComponent: Component is not ready", "QQmlComponent: Component is not ready", "QQmlComponent: Component is not ready"]
import QtQuick
Item {
    id: root
    property string result: ""
    Component { id: ready; Item { property int n: 5 } }
    Component.onCompleted: {
        var out = []
        var missing = Qt.createComponent("ComponentNotReadyMissing.qml")
        var o = missing.createObject(root)
        out.push([missing.status, o, o === null, typeof o, missing.createObject(), missing.createObject(null, {n: 1}),
            missing.incubateObject(root), missing.incubateObject(root, {n: 2}, Qt.Synchronous), root.children.length].map(String).join(" "))
        var loading = Qt.createComponent("DirectoryImportAliasType.qml", Component.Asynchronous)
        out.push([loading.status, loading.createObject(root), loading.incubateObject(root),
            loading.incubateObject(root, {n: 2}, Qt.Synchronous), root.children.length].map(String).join(" "))
        var made = ready.incubateObject(root, {n: 8}, Qt.Synchronous)
        out.push([ready.status, ready.createObject(root).n, ready.createObject(root, {n: 9}).n, made.status, made.object.n,
            root.children.length].join(" "))
        result = out.join(";")
    }
}
