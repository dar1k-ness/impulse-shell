// A TextInput with an input mask filled in part, in whole and not at all: the positions of
// select(), getText(), insert(), remove() and the selection are in the text shown, which is
// as long as the mask; cursorPosition takes a position only up to the length of `text`;
// selectAll() selects that length; insert() and remove() replace in place.
import QtQuick
Item {
    id: root
    property string log: ""
    function st(t) { return "(" + t.text + "|" + t.displayText + "|" + t.length + "|" + t.cursorPosition + "," + t.selectionStart + "," + t.selectionEnd + "|" + t.selectedText + ")" }
    TextInput { id: tm; inputMask: "AAAAA-AAAAA;_" }
    function reset(txt) { tm.readOnly = false; tm.text = ""; tm.text = txt; tm.deselect() }
    function step(n, f) { f(); log += n + " " + st(tm) + "\n" }
    Component.onCompleted: {
        var texts = ["ab", "hello-wo", "", "hello-world"]
        for (var i = 0; i < texts.length; i++) {
            var x = texts[i]
            reset(x); step("set " + x, () => {})
            reset(x); step("selectAll", () => tm.selectAll())
            reset(x); step("select(2,4)", () => tm.select(2, 4))
            reset(x); step("select(4,2)", () => tm.select(4, 2))
            reset(x); step("select(0,11)", () => tm.select(0, 11))
            reset(x); step("select(0,12)", () => tm.select(0, 12))
            reset(x); step("select(8,10) deselect", () => { tm.select(8, 10); tm.deselect() })
            for (var c = 0; c <= 12; c++) { reset(x); step("cp=" + c, () => { tm.cursorPosition = c }) }
            reset(x); step("cp=1,cp=0", () => { tm.cursorPosition = 1; tm.cursorPosition = 0 })
            reset(x); step("sel(2,4) cp=1", () => { tm.select(2, 4); tm.cursorPosition = 1 })
            reset(x); step("sel(2,9) cp=7", () => { tm.select(2, 9); tm.cursorPosition = 7 })
            reset(x); log += "getText(0,11)=" + tm.getText(0, 11) + " getText(1,4)=" + tm.getText(1, 4) + " getText(6,20)=" + tm.getText(6, 20) + "\n"
            reset(x); step("select(1,7) ro", () => { tm.select(1, 7); tm.readOnly = true })
            step("ro cp=5", () => { tm.cursorPosition = 5 })
            step("ro=false", () => { tm.readOnly = false })
            step("cp=2", () => { tm.cursorPosition = 2 })
            reset(x); step("insert(1,X)", () => tm.insert(1, "X"))
            reset(x); step("remove(0,1)", () => tm.remove(0, 1))
            reset(x); step("sel(1,3) insert(2,Xy)", () => { tm.select(1, 3); tm.insert(2, "Xy") })
            reset(x); step("sel(1,3) insert(0,9)", () => { tm.select(1, 3); tm.insert(0, "9") })
            reset(x); step("insert(11,X)", () => tm.insert(11, "X"))
            reset(x); step("insert(12,X)", () => tm.insert(12, "X"))
            reset(x); step("sel(1,3) remove(7,2)", () => { tm.select(1, 3); tm.remove(7, 2) })
            reset(x); step("remove(4,40)", () => tm.remove(4, 40))
            reset(x); step("cp=1 remove(-3,1)", () => { tm.cursorPosition = 1; tm.remove(-3, 1) })
        }
        console.log("LOG<" + log + ">LOG"); Qt.quit()
    }
}
