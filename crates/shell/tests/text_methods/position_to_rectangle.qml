// TextEdit.positionToRectangle the way qml/modules/rusty/overlay/notes/NotesContent.qml:121-122
// calls it: the rectangle of the cursor at the start and at the end of each line of a wrapped
// plain-text TextEdit with padding, and what the caller reads of it (y, height).
import QtQuick

Item {
    id: root
    width: 320
    height: 400

    TextEdit {
        id: edit
        anchors.left: parent.left
        anchors.right: parent.right
        wrapMode: TextEdit.Wrap
        textFormat: TextEdit.PlainText
        padding: 24
        font.pixelSize: 16
        text: "Sheep fricker\n- 4x Slab\n- 1x Boat\n- 4x Redstone Dust and a line long enough to wrap twice at this width for sure\n\n- 1x Sheep"
    }

    function show(r) {
        return "x=" + r.x + " y=" + r.y + " width=" + r.width + " height=" + r.height;
    }
    function report() {
        console.log("typeof", typeof edit.positionToRectangle);
        var start = 0;
        var lines = edit.text.split("\n");
        for (var i = 0; i < lines.length; i++) {
            var end = start + lines[i].length;
            try {
                var a = edit.positionToRectangle(start);
                var b = edit.positionToRectangle(end);
                console.log("line " + i + " [" + start + "," + end + "]: start " + show(a) + "; end " + show(b)
                    + "; finite " + isFinite(a.y) + " " + isFinite(b.y) + "; bottom " + (b.y + b.height));
            } catch (e) {
                console.log("line " + i + " [" + start + "," + end + "]: throws " + e.message);
            }
            start = end + 1;
        }
        for (var p of [-1, edit.length, edit.length + 5]) {
            try {
                console.log("position " + p + ": " + show(edit.positionToRectangle(p)));
            } catch (e) {
                console.log("position " + p + ": throws " + e.message);
            }
        }
        Qt.quit();
    }
    // After the first layout, as NotesContent.qml calls it (Qt.callLater and change handlers):
    // this engine applies anchors in the window's layout pass, so inside Component.onCompleted
    // the TextEdit is still 0 wide here and 320 wide in Qt.
    Component.onCompleted: Qt.callLater(report)
}
