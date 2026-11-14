// A script's write to a padding, a margin, a spacing, an inset or a radius makes it a set one,
// as an assignment in the document does: the narrower ones nothing set follow it, a narrower one
// that was written keeps its own value, and `undefined` resets the ones that have a RESET.
// qt: popup.margins:-1,-1,-1,-1,-1 4,4,4,4,4 4,5,4,4,4 -1,5,-1,-1,-1 -1,-1,-1,-1,-1 6,6,6,6,6 | menu.margins:0,0,0,0,0 4,4,4,4,4 4,5,4,4,4 -1,5,-1,-1,-1 -1,-1,-1,-1,-1 6,6,6,6,6 | tip.margins:6,6,6,6,6 4,4,4,4,4 4,5,4,4,4 -1,5,-1,-1,-1 -1,-1,-1,-1,-1 6,6,6,6,6 | popupDoc.margins:3,2,3,3,3 4,2,4,4,4 4,5,4,4,4 -1,5,-1,-1,-1 -1,-1,-1,-1,-1 6,6,6,6,6 | popup.padding:12,12,12,12,12,12,12 4,4,4,4,4,4,4 4,5,4,5,4,5,4 0,5,0,5,0,5,0 0,0,0,0,0,0,0 6,6,6,6,6,6,6 | control.padding:0,0,0,0,0,0,0 4,4,4,4,4,4,4 4,5,4,5,4,5,4 0,5,0,5,0,5,0 0,0,0,0,0,0,0 6,6,6,6,6,6,6 | button.padding:6,8,6,8,6,8,6 4,6,4,6,4,6,4 4,5,4,5,4,5,4 0,5,0,5,0,5,0 0,0,0,0,0,0,0 6,6,6,6,6,6,6 | pane.horizontalPadding:12,12,12,12 4,4,4,12 4,5,4,12 12,5,12,12 12,12,12,12 6,6,6,12 | frame.verticalPadding:12,12,12,12 4,4,4,12 4,5,4,12 12,5,12,12 12,12,12,12 6,6,6,12 | column.padding:0,0,0,0,0 4,4,4,4,4 4,5,4,4,4 0,5,0,0,0 0,0,0,0,0 6,6,6,6,6 | text.padding:0,0,0,0,0 4,4,4,4,4 4,5,4,4,4 0,5,0,0,0 0,0,0,0,0 6,6,6,6,6 | field.padding:6,10,6,6,6 4,8,4,4,4 4,5,4,4,4 0,5,0,0,0 0,0,0,0,0 6,6,6,6,6 | control.spacing:0,0 4,4 5,5 0,0 0,0 6,6 | button.spacing:6,6 4,4 5,5 0,0 0,0 6,6 | column.spacing:0,0 4,4 5,5 ![undefined] to double 5,5 ![undefined] to double 5,5 6,6 | grid.spacing:0,-1,-1 4,-1,-1 4,5,-1 ![undefined] to double 4,5,-1 4,5,-1 6,5,-1 | row.spacing:5,5 4,4 5,5 ![undefined] to double 5,5 ![undefined] to double 5,5 6,6 | gridLayout.rowSpacing:5,5 4,5 4,5 ![undefined] to double 4,5 ![undefined] to double 4,5 6,5 | control.inset:0,0,0,0 4,0,0,0 4,5,0,0 0,5,0,0 0,0,0,0 6,0,0,0 | button.inset:0,0,0,0 4,0,0,0 4,5,0,0 0,5,0,0 0,0,0,0 6,0,0,0 | popup.inset:0,0,0,0 4,0,0,0 4,5,0,0 0,5,0,0 0,0,0,0 6,0,0,0 | anchors.margins:0,0,0,0,0 4,4,4,4,4 4,5,4,4,4 ![undefined] to double 4,5,4,4,4 4,4,4,4,4 6,6,6,6,6 | anchorsDoc.margins:3,2,3,3,3 4,2,4,4,4 4,5,4,4,4 ![undefined] to double 4,5,4,4,4 4,4,4,4,4 6,6,6,6,6 | Layout.margins:3,3,3,3,3 4,4,4,4,4 4,5,4,4,4 ![undefined] to double 4,5,4,4,4 4,4,4,4,4 6,6,6,6,6 | rect.radius:0,0,0,0,0 4,4,4,4,4 4,5,4,4,4 ![undefined] to double 4,5,4,4,4 4,4,4,4,4 6,6,6,6,6 | rectDoc.radius:3,2,3,3,3 4,2,4,4,4 4,5,4,4,4 ![undefined] to double 4,5,4,4,4 4,4,4,4,4 6,6,6,6,6
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
Item {
    id: root
    Popup { id: popup }
    Menu { id: menu }
    ToolTip { id: tip }
    Popup { id: popupDoc; margins: 3; leftMargin: 2 }
    Control { id: control }
    Button { id: button }
    Pane { id: pane }
    Frame { id: frame }
    Column { id: column }
    Grid { id: grid }
    Text { id: text }
    TextField { id: field }
    Rectangle { id: rect }
    Rectangle { id: rectDoc; radius: 3; topLeftRadius: 2 }
    Item { id: anchored }
    Item { id: anchoredDoc; anchors.margins: 3; anchors.leftMargin: 2 }
    RowLayout { id: row; Item { id: laid; Layout.margins: 3 } }
    GridLayout { id: gridLayout }
    property string result
    property var out: []
    // Reads `names` of `o` after each of: nothing, `wide = v`, `narrow = v + 1`,
    // `wide = undefined`, `narrow = undefined`, `wide = v + 2`.
    function probe(label, o, names, v) {
        var row = () => names.map(n => o[n]).join(",")
        var s = label + ":" + row()
        var write = (n, x) => {
            try { o[n] = x } catch (e) { s += " !" + ("" + e).replace(/.*Cannot assign /, "") }
            s += " " + row()
        }
        write(names[0], v)
        write(names[1], v + 1)
        write(names[0], undefined)
        write(names[1], undefined)
        write(names[0], v + 2)
        out.push(s)
    }
    Component.onCompleted: {
        var margins = ["margins", "leftMargin", "topMargin", "rightMargin", "bottomMargin"]
        var sides = ["leftPadding", "topPadding", "rightPadding", "bottomPadding"]
        var paddings = ["padding", "horizontalPadding", "verticalPadding"].concat(sides)
        var insets = ["topInset", "leftInset", "rightInset", "bottomInset"]
        var corners = ["radius", "topLeftRadius", "topRightRadius", "bottomLeftRadius", "bottomRightRadius"]
        probe("popup.margins", popup, margins, 4)
        probe("menu.margins", menu, margins, 4)
        probe("tip.margins", tip, margins, 4)
        probe("popupDoc.margins", popupDoc, margins, 4)
        probe("popup.padding", popup, paddings, 4)
        probe("control.padding", control, paddings, 4)
        probe("button.padding", button, paddings, 4)
        probe("pane.horizontalPadding", pane, ["horizontalPadding", "leftPadding", "rightPadding", "padding"], 4)
        probe("frame.verticalPadding", frame, ["verticalPadding", "topPadding", "bottomPadding", "padding"], 4)
        probe("column.padding", column, ["padding"].concat(sides), 4)
        probe("text.padding", text, ["padding"].concat(sides), 4)
        probe("field.padding", field, ["padding"].concat(sides), 4)
        probe("control.spacing", control, ["spacing", "spacing"], 4)
        probe("button.spacing", button, ["spacing", "spacing"], 4)
        probe("column.spacing", column, ["spacing", "spacing"], 4)
        probe("grid.spacing", grid, ["spacing", "rowSpacing", "columnSpacing"], 4)
        probe("row.spacing", row, ["spacing", "spacing"], 4)
        probe("gridLayout.rowSpacing", gridLayout, ["rowSpacing", "columnSpacing"], 4)
        probe("control.inset", control, insets, 4)
        probe("button.inset", button, insets, 4)
        probe("popup.inset", popup, insets, 4)
        probe("anchors.margins", anchored.anchors, margins, 4)
        probe("anchorsDoc.margins", anchoredDoc.anchors, margins, 4)
        probe("Layout.margins", laid.Layout, margins, 4)
        probe("rect.radius", rect, corners, 4)
        probe("rectDoc.radius", rectDoc, corners, 4)
        result = out.join(" | ")
        console.log(result)
    }
}
