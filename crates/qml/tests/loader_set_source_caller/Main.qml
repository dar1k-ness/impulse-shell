import QtQuick
import "sub"
import "lib/l.js" as Lib
import "lib/p.js" as PLib

// Who calls `setSource` decides where a relative url is: the Loader is written in sub/.
Item {
    Holder { id: h }
    function row(name) { return name + " | " + h.loader.source + " | " + h.loader.status + " " + (h.loader.item ? h.loader.item.objectName : "no item") }
    function all() {
        var out = []
        h.loader.setSource("Leaf.qml"); out.push(row("called in the root document"))
        h.own("Leaf.qml"); out.push(row("called in the Loader's document"))
        Lib.set(h.loader, "Leaf.qml"); out.push(row("called in a js file"))
        PLib.set(h.loader, "Leaf.qml"); out.push(row("called in a library js file"))
        h.loader.setSource("Leaf.qml", { objectName: "given" }); out.push(row("called in the root document with properties"))
        h.loader.setSource(Qt.resolvedUrl("lib/Leaf.qml")); out.push(row("given a resolved url"))
        h.loader.source = "Leaf.qml"; out.push(row("source written in the root document"))
        return out.join("\n")
    }
}
