import QtQuick
Item {
    // The url of this directory with its slash: Quickshell resolves "." to the directory
    // without one.
    readonly property string here: String(Qt.resolvedUrl("x")).slice(0, -1)
    function there(text) { return text.split(here).join("") }
    Loader {
        id: loader
        active: false
        source: "Broken.qml"
        onStatusChanged: console.log("loader status", status)
    }
    Component.onCompleted: {
        for (const url of ["Broken.qml", "C5.qml", "BrokenRepeater.qml", "C9.qml", "Uses.qml"]) {
            const c = Qt.createComponent(url);
            console.log(url, c.status, JSON.stringify(there(c.errorString())));
            console.log(c.createObject(null));
        }
        console.log("loader", loader.status, loader.item);
        loader.active = true;
        console.log("loader", loader.status, loader.item);
        const texts = [
            ["import QtQuick; Item { Component { NopeText {} } }"],
            ["import QtQuick; Item { Component { Broken {} } }"],
            ["import QtQuick; Item { NopeText {} }", "sub/Dyn.qml"],
            ["import QtQuick; Item { Broken {} }"],
            ["import QtQuick; Item { Component { Broken {} } }", "x.qml"],
            ["import QtQuick; Item { Component { Item {} } }", "fine.qml"],
        ];
        for (const [text, path] of texts) {
            try {
                const o = path ? Qt.createQmlObject(text, loader, path) : Qt.createQmlObject(text, loader);
                console.log("created", o !== null);
            } catch (e) {
                console.log(JSON.stringify(there(e.message)));
            }
        }
        Qt.quit();
    }
}
