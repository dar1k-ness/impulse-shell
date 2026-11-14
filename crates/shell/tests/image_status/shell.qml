import QtQuick

// One picture after another: the next one is made once this one has become opaque, so what a
// picture finds in the pixmap cache is what the pictures before it left there.
Window {
    id: root
    visible: true
    width: 64
    height: 64

    Item {
        id: stage
        anchors.fill: parent
    }

    property var pictures: [
        { name: "synchronous", asynchronous: false },
        { name: "cached", },
        { name: "cache off", cache: false },
        { name: "not cached", source: "b.png" },
        { name: "cached by an asynchronous one", source: "b.png" },
        { name: "cached at another size", sourceSize: Qt.size(10, 10) },
        { name: "first with cache off", source: "c.png", cache: false },
        { name: "after one with cache off", source: "c.png" },
        { name: "synchronous with a bound size", asynchronous: false, sourceSize: Qt.binding(() => Qt.size(12, 12)) },
        { name: "synchronous with the size of the item", asynchronous: false, width: 8, height: 8, sourceSize: Qt.binding(function () {
                return Qt.size(this.width, this.height);
            }) },
        { name: "bound size, not cached", source: "b.png", sourceSize: Qt.binding(() => Qt.size(6, 6)) },
        { name: "bound size, cached", source: "b.png", sourceSize: Qt.binding(() => Qt.size(6, 6)) },
    ]
    property list<Component> groups: [sized, pair, ends]
    property int made: 0
    property var kept: []

    function say(...words) {
        console.log(words.join(" "));
    }

    function next() {
        if (made < pictures.length)
            kept.push(picture.createObject(stage, pictures[made++]));
        else if (made < pictures.length + groups.length)
            kept.push(groups[made++ - pictures.length].createObject(stage));
        else
            Qt.quit();
    }

    Timer {
        id: step
        interval: 1
        onTriggered: root.next()
    }

    Component.onCompleted: next()

    component Picture: Image {
        property string name
        property bool faded: false
        asynchronous: true
        source: "a.png"
        opacity: status === Image.Ready ? 1 : 0
        Behavior on opacity {
            NumberAnimation {
                duration: 160
            }
        }
        onStatusChanged: root.say(name + ": status", status, "progress", progress, "implicitWidth", implicitWidth, "opacity", opacity)
        onOpacityChanged: {
            if (opacity > 0 && opacity < 1)
                faded = true;
            else if (opacity === 1)
                root.say(name + ": opaque", faded ? "after a fade" : "at once");
        }
        Component.onCompleted: root.say(name + ": completed with status", status, "opacity", opacity)
    }

    Component {
        id: picture
        Picture {
            onOpacityChanged: if (opacity === 1) step.start()
        }
    }

    // A synchronous picture declared with a size: `Qt.size()` makes it a binding.
    Component {
        id: sized
        Picture {
            name: "synchronous, declared with a size"
            asynchronous: false
            sourceSize: Qt.size(7, 7)
            onOpacityChanged: if (opacity === 1) step.start()
        }
    }

    // Two pictures of one file made by one creation: the asynchronous one is declared first.
    Component {
        id: pair
        Item {
            Picture {
                name: "declared before its synchronous twin"
                source: "d.png"
            }
            Picture {
                name: "synchronous twin"
                source: "d.png"
                asynchronous: false
            }
            Picture {
                name: "declared after its synchronous twin"
                source: "d.png"
                onOpacityChanged: if (opacity === 1) step.start()
            }
        }
    }

    // The end of a load tells the status, then the size the picture was asked at.
    component Told: Image {
        property string name
        source: "a.png"
        onStatusChanged: root.say(name + ": status", status)
        onSourceSizeChanged: root.say(name + ": sourceSize", sourceSize.width)
        Component.onCompleted: root.say(name + ": completed")
    }

    Component {
        id: ends
        Item {
            Told {
                name: "asked at the size of the file"
            }
            Told {
                name: "asked at a size"
                sourceSize.width: 10
                sourceSize.height: 10
            }
            Told {
                name: "asked at a size, cache off"
                asynchronous: true
                cache: false
                sourceSize.width: 10
                sourceSize.height: 10
                onSourceSizeChanged: if (status === Image.Ready) step.start()
            }
        }
    }
}
