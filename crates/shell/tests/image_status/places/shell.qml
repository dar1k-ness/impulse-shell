import QtQuick
import qs.modules.common
import qs.modules.common.widgets

// The widgets of the shell that depend on the status a picture has when it is created:
// StyledImage (its `opacity` follows the status under a Behavior), ThumbnailImage as
// WallpaperDirectoryItem.qml uses it (`cache: false`), and `bg` of WaffleLock.qml as written
// there (`onStatusChanged` moves it and starts the opening animation).
Window {
    id: root
    visible: true
    width: 200
    height: 200

    function say(...words) {
        console.log(words.join(" "));
    }

    Item {
        id: stage
        anchors.fill: parent

        // These put the files into the pixmap cache at the sizes asked below.
        Image { source: "p.png"; sourceSize: Qt.size(20, 20) }
        Image { source: "q.png"; sourceSize: Qt.size(200, 200); fillMode: Image.PreserveAspectCrop }
    }

    Timer {
        interval: 400
        running: true
        onTriggered: {
            styled.createObject(stage);
            thumb.createObject(stage);
            lock.createObject(stage);
        }
    }
    Timer {
        interval: 3000
        running: true
        onTriggered: Qt.quit()
    }

    Component {
        id: styled
        StyledImage {
            property bool faded: false
            width: 20
            height: 20
            source: "p.png"
            onStatusChanged: root.say("StyledImage: status", status)
            onOpacityChanged: {
                if (opacity > 0 && opacity < 1)
                    faded = true;
                else if (opacity === 1)
                    root.say("StyledImage: opaque", faded ? "after a fade" : "at once");
            }
            Component.onCompleted: root.say("StyledImage: completed with status", status, "opacity", opacity)
        }
    }

    Component {
        id: thumb
        ThumbnailImage {
            property bool faded: false
            width: 20
            height: 20
            sourcePath: ""
            source: "p.png"
            cache: false
            onStatusChanged: root.say("ThumbnailImage: status", status)
            onOpacityChanged: {
                if (opacity > 0 && opacity < 1)
                    faded = true;
                else if (opacity === 1)
                    root.say("ThumbnailImage: opaque", faded ? "after a fade" : "at once");
            }
            Component.onCompleted: root.say("ThumbnailImage: completed with status", status, "opacity", opacity)
        }
    }

    Component {
        id: lock
        Item {
            id: lockSurfaceItem
            width: 200
            height: 200
            Component.onCompleted: root.say("WaffleLock: surface completed, bg status", bg.status, "y", bg.y, "opening", openAnim.running)
            StyledImage {
                id: bg
                z: 0
                width: parent.width
                height: parent.height
                onStatusChanged: {
                    if (status === Image.Ready) {
                        y = -lockSurfaceItem.height;
                        openAnim.restart();
                    }
                    root.say("WaffleLock: bg status", status, "y", y, "opening", openAnim.running);
                }
                source: "q.png"
                fillMode: Image.PreserveAspectCrop

                PropertyAnimation {
                    id: openAnim
                    target: bg
                    property: "y"
                    to: 0
                    duration: 350
                    onFinished: root.say("WaffleLock: opened, y", bg.y)
                }
            }
        }
    }
}
