import QtQuick
// A value type group property is written once while the object is populated, after its
// signal handlers are connected: sourceSizeChanged comes for it, also when only bindings
// are in the group.
Item {
    Image { sourceSize.width: 3; onSourceSizeChanged: console.log("width", sourceSize.width, sourceSize.height) }
    Image { sourceSize.height: 3; sourceSize.width: 4; onSourceSizeChanged: console.log("both", sourceSize.width, sourceSize.height) }
    Image { sourceSize.width: 0; onSourceSizeChanged: console.log("zero", sourceSize.width, sourceSize.height) }
    Image { property int k: 2; sourceSize.width: k; onSourceSizeChanged: console.log("bound", sourceSize.width, sourceSize.height) }
    Image { id: whole; property int k: 2; sourceSize: Qt.size(k, 3); onSourceSizeChanged: console.log("whole", sourceSize.width, sourceSize.height) }
    Image { property int k: 2; sourceSize.width: k; sourceSize.height: 5; onSourceSizeChanged: console.log("mixed", sourceSize.width, sourceSize.height) }
    Image { onSourceSizeChanged: console.log("plain", sourceSize.width, sourceSize.height) }
    AnimatedImage { onSourceSizeChanged: console.log("animated", sourceSize.width, sourceSize.height) }
    AnimatedImage { sourceSize.width: 2; onSourceSizeChanged: console.log("animated width", sourceSize.width, sourceSize.height) }
    Component.onCompleted: { console.log("done"); whole.k = 7; console.log("end") }
}
