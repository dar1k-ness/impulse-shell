import QtQuick
import Quickshell
import Quickshell.Widgets

Scope {
    id: root
    function say(label, image) {
        console.log(label + ": " + image.sourceSize.width + "x" + image.sourceSize.height);
    }
    IconImage { id: half; width: 19.5; height: 19.5 }
    IconImage { id: over; width: 19.7; height: 30 }
    IconImage { id: under; width: 30; height: 20.2 }
    IconImage { id: implied; implicitSize: 21.99 }
    IconImage { id: whole; implicitSize: 24 }
    // The same binding in a document that is not compiled ahead of time.
    Item {
        id: own
        width: 19.5; height: 19.5
        readonly property real actualSize: Math.min(width, height)
        Image { id: interpreted; sourceSize.width: own.actualSize; sourceSize.height: own.actualSize }
    }
    Component.onCompleted: {
        say("IconImage 19.5", half.backer);
        say("IconImage 19.7 by 30", over.backer);
        say("IconImage 30 by 20.2", under.backer);
        say("IconImage implicitSize 21.99", implied.backer);
        say("IconImage implicitSize 24", whole.backer);
        say("Image 19.5", interpreted);
        console.log("done");
        Qt.quit();
    }
}
