// QQuickLoader emits in a fixed order. A load: itemChanged, sourceComponentChanged (or
// sourceChanged), statusChanged, progressChanged, loaded, activeChanged. Left with nothing to
// load: the source's change, statusChanged, progressChanged, itemChanged. An inactive Loader
// only records its source. setSource(url, properties) loads with initial properties, which
// the next source drops.
// qt: sc | start ,0 | item:A1 sc status:1 progress:1 loaded:A1 active:true | item:B sc progress:1 loaded:B | sc status:0 progress:0 item:null | item:A1 sc status:1 progress:1 loaded:A1 item:null status:0 active:false | sc sc item:A2 sc status:1 progress:1 loaded:A2 active:true | src3 status3:1 loaded3:viaSetSource | src3 status3:0 ,0 | src3 status3:1 loaded3:default | status3:0 ,true
import QtQuick
Item {
    id: root
    property string result: ""
    property int n: 1
    function log(s) {
        result += s + " "
    }
    Component {
        id: cA
        Item {
            property string nm: "A" + root.n
        }
    }
    Component {
        id: cB
        Item {
            property string nm: "B"
        }
    }
    Loader {
        id: ld
        active: false
        sourceComponent: cA
        onLoaded: root.log("loaded:" + item.nm)
        onItemChanged: root.log("item:" + (item ? item.nm : item))
        onStatusChanged: root.log("status:" + status)
        onActiveChanged: root.log("active:" + active)
        onSourceComponentChanged: root.log("sc")
        onProgressChanged: root.log("progress:" + progress)
    }
    Loader {
        id: ld3
        onLoaded: root.log("loaded3:" + item.tag)
        onStatusChanged: root.log("status3:" + status)
        onSourceChanged: root.log("src3")
    }
    Component.onCompleted: {
        log("| start " + [ld.item, ld.status] + " |")
        ld.active = true
        log("|")
        ld.sourceComponent = cB
        log("|")
        ld.sourceComponent = undefined
        log("|")
        ld.sourceComponent = cA
        ld.active = false
        log("|")
        ld.sourceComponent = cB
        ld.sourceComponent = cA
        root.n = 2
        ld.active = true
        log("|")
        ld3.setSource("LoaderPart.qml", {tag: "viaSetSource"})
        log("|")
        ld3.source = ""
        log([ld3.item, ld3.status] + " |")
        ld3.source = "LoaderPart.qml"
        log("|")
        ld3.active = false
        result += [ld3.item, ld3.source.toString().endsWith("LoaderPart.qml")]
    }
}
