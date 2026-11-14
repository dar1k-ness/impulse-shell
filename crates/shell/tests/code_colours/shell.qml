//@ pragma Env QS_NO_RELOAD_POPUP=1
// What Quickshell draws for a code block. $CASES is a JSON list; a case with `lang` is its
// `text` under org.kde.syntaxhighlighting's SyntaxHighlighter set up as the original's
// MessageCodeBlock.qml sets it up, a case without is rich text. Each is saved as
// $OUT/<index>.png and the place of each of its characters goes to the log.
import QtQuick
import Quickshell
import Quickshell.Io
import org.kde.syntaxhighlighting

FloatingWindow {
    id: win
    visible: true
    color: "#000000"
    implicitWidth: 640
    implicitHeight: 480
    readonly property var cases: JSON.parse(casesFile.text())
    property int at: 0
    property int frames: 0

    FileView {
        id: casesFile
        path: Quickshell.env("CASES")
        blockLoading: true
    }

    Repeater {
        id: boxes
        model: win.cases
        Rectangle {
            id: box
            required property var modelData
            readonly property bool rich: modelData.lang === undefined
            property alias edit: edit
            color: "#000000"
            // Two pictures of one size are drawn alike to the last bit; a case whose pair is
            // of another width (bold text is wider) names the size of both.
            width: modelData.width ?? edit.implicitWidth
            height: modelData.height ?? edit.implicitHeight
            TextEdit {
                id: edit
                textFormat: box.rich ? TextEdit.RichText : TextEdit.PlainText
                text: box.modelData.text
                // No theme has this colour: text the highlighter left alone shows in it.
                color: "#12fe56"
                font.family: "Noto Sans Mono"
                font.pixelSize: 20
                // A character's ink stays inside its own box; a `native` case is drawn as
                // the block of MessageCodeBlock.qml is, with neither.
                font.letterSpacing: box.modelData.native ? 0 : 12
                renderType: box.modelData.native ? TextEdit.NativeRendering : TextEdit.QtRendering
                SyntaxHighlighter {
                    textEdit: box.rich ? null : edit
                    repository: Repository
                    definition: Repository.definitionForName(box.modelData.lang || "plaintext")
                    theme: box.modelData.theme ?? ""
                }
            }
        }
    }

    // The compositor asks for a frame only after a change on screen, and a grab is drawn with
    // the window's next frame: this keeps the frames coming.
    Rectangle {
        width: 4
        height: 4
        color: win.frames % 2 ? "#000001" : "#000000"
    }

    // The highlighter colours the document from an event queued when it got the document; the
    // second frame is drawn after that.
    FrameAnimation {
        running: true
        onTriggered: if (++win.frames == 2) win.next()
    }

    function next() {
        if (at >= cases.length) {
            console.log("PROBE DONE")
            return
        }
        const box = boxes.itemAt(at)
        box.grabToImage(result => {
            result.saveToFile(Quickshell.env("OUT") + "/" + at + ".png")
            const e = box.edit
            const rects = []
            for (let i = 0; i <= e.length; i++) {
                const r = e.positionToRectangle(i)
                rects.push([r.x, r.y, r.height])
            }
            const name = box.rich ? "" : Repository.definitionForName(box.modelData.lang || "plaintext").name
            console.log("PROBE CASE " + JSON.stringify({ at: at, name: name, rects: rects }))
            at++
            Qt.callLater(next)
        })
    }
}
