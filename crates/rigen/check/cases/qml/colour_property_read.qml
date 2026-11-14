// A property of type `color` holds a colour whatever was written: a name, `#rgb`, `#aarrggbb`,
// `Qt.rgba()`, "transparent" or a string that names no colour (an invalid colour, black), in the
// document, by a script or as the result of a binding. A script reads an object whose text is
// `#rrggbb` or `#aarrggbb`: it has `r`, `g`, `b`, `a`, is not `===` a string and is `==` the
// string of its own text only; a `var` property keeps the string.
// qt: document name: object #0000ff 0 1 false false true true true | bound: object #0000ff 0 1 false false true true true | binding to a var: object #0000ff 0 1 false false true true true | nothing set: object #ffffff 1 1 false false false false true | own: object #0000ff 0 1 false false true true true | var: string blue   true true false true  | #rgb in a document: object #aabbcc 0.6666666865348816 1 false false false false true | transparent in a document: object #00000000 0 0 false false false false true | border: object #ff0000 1 1 false false false false true | name: object #0000ff 0 1 false false true true true | name own: object #0000ff 0 1 false false true true true | name border: object #0000ff 0 1 false false true true true | name text: object #0000ff 0 1 false false true true true | name binding: object #0000ff 0 1 false false true true true | name binding border: object #0000ff 0 1 false false true true true | #rgb: object #0000ff 0 1 false false true true true | #rgb own: object #0000ff 0 1 false false true true true | #rgb border: object #0000ff 0 1 false false true true true | #rgb text: object #0000ff 0 1 false false true true true | #rgb binding: object #0000ff 0 1 false false true true true | #rgb binding border: object #0000ff 0 1 false false true true true | #rrggbb: object #0000ff 0 1 false false true true true | #rrggbb own: object #0000ff 0 1 false false true true true | #rrggbb border: object #0000ff 0 1 false false true true true | #rrggbb text: object #0000ff 0 1 false false true true true | #rrggbb binding: object #0000ff 0 1 false false true true true | #rrggbb binding border: object #0000ff 0 1 false false true true true | #aarrggbb: object #800000ff 0 0.501960813999176 false false false false true | #aarrggbb own: object #800000ff 0 0.501960813999176 false false false false true | #aarrggbb border: object #800000ff 0 0.501960813999176 false false false false true | #aarrggbb text: object #800000ff 0 0.501960813999176 false false false false true | #aarrggbb binding: object #800000ff 0 0.501960813999176 false false false false true | #aarrggbb binding border: object #800000ff 0 0.501960813999176 false false false false true | Qt.rgba: object #800000ff 0 0.5000076293945312 false false false false true | Qt.rgba own: object #800000ff 0 0.5000076293945312 false false false false true | Qt.rgba border: object #800000ff 0 0.5000076293945312 false false false false true | Qt.rgba text: object #800000ff 0 0.501960813999176 false false false false true | Qt.rgba binding: object #800000ff 0 0.5000076293945312 false false false false true | Qt.rgba binding border: object #800000ff 0 0.5000076293945312 false false false false true | transparent: object #00000000 0 0 false false false false true | transparent own: object #00000000 0 0 false false false false true | transparent border: object #00000000 0 0 false false false false true | transparent text: object #00000000 0 0 false false false false true | transparent binding: object #00000000 0 0 false false false false true | transparent binding border: object #00000000 0 0 false false false false true | wrong string: object #000000 0 1 false false false false false | wrong string own: object #000000 0 1 false false false false false | wrong string border: object #000000 0 1 false false false false false | wrong string text: object #000000 0 1 false false false false true | wrong string binding: object #000000 0 1 false false false false false | wrong string binding border: object #000000 0 1 false false false false false | empty string: object #000000 0 1 false false false false false | empty string own: object #000000 0 1 false false false false false | empty string border: object #000000 0 1 false false false false false | empty string text: object #000000 0 1 false false false false true | empty string binding: object #000000 0 1 false false false false false | empty string binding border: object #000000 0 1 false false false false false | number: object #000000 0 1 false false false false false | number own: object #000000 0 1 false false false false false | number border: object #000000 0 1 false false false false false | number text: object #000000 0 1 false false false false true | number binding: object #000000 0 1 false false false false false | number binding border: object #000000 0 1 false false false false false | a colour of another property: object #0000ff 0 1 false false true true true | a colour of another property own: object #0000ff 0 1 false false true true true | a colour of another property border: object #0000ff 0 1 false false true true true | a colour of another property text: object #0000ff 0 1 false false true true true | a colour of another property binding: object #0000ff 0 1 false false true true true | a colour of another property binding border: object #0000ff 0 1 false false true true true | same value twice: true true true true true | in a string: #0000ff {"r":0,"g":0,"b":1,"a":1,"hsvHue":0.6666666865348816,"hsvSaturation":1,"hsvValue":1,"hslHue":0.6666666865348816,"hslSaturation":1,"hslLightness":0.5000076293945312,"valid":true,"rgba":-16776961} #0000ff #0000ff1 | fields: 0 0 1 1 0.6666666865348816 0.5000076293945312 r,g,b,a,hsvHue,hsvSaturation,hsvValue,hslHue,hslSaturation,hslLightness,valid,rgba
import QtQuick

Item {
    id: root
    property string result: ""
    property color own: "blue"
    property var held: "blue"
    Rectangle { id: plain; color: "blue" }
    Rectangle { id: bound; color: root.own }
    Rectangle { id: given; color: root.held; border.color: root.held }
    Rectangle { id: fresh }
    Text { id: words; color: "#abc"; styleColor: "transparent" }
    Rectangle { id: target; border.color: "red" }

    function look(c) {
        return [typeof c, String(c), c.r, c.a, c === "blue", c == "blue", c == "#0000ff",
            Qt.colorEqual(c, "blue"), c.valid].join(" ")
    }
    function line(label, c) { result += label + ": " + look(c) + " | " }
    function written(label, v) {
        try { target.color = v } catch (e) { result += label + " throws " + e + " | " }
        line(label, target.color)
        try { root.own = v } catch (e) { result += label + " own throws " + e + " | " }
        line(label + " own", root.own)
        try { target.border.color = v } catch (e) { result += label + " border throws " + e + " | " }
        line(label + " border", target.border.color)
        try { words.color = v } catch (e) { result += label + " text throws " + e + " | " }
        line(label + " text", words.color)
        root.held = v
        line(label + " binding", given.color)
        line(label + " binding border", given.border.color)
    }
    Component.onCompleted: {
        line("document name", plain.color)
        line("bound", bound.color)
        line("binding to a var", given.color)
        line("nothing set", fresh.color)
        line("own", root.own)
        line("var", root.held)
        line("#rgb in a document", words.color)
        line("transparent in a document", words.styleColor)
        line("border", target.border.color)
        written("name", "blue")
        written("#rgb", "#00f")
        written("#rrggbb", "#0000ff")
        written("#aarrggbb", "#800000ff")
        written("Qt.rgba", Qt.rgba(0, 0, 1, 0.5))
        written("transparent", "transparent")
        written("wrong string", "no such colour")
        written("empty string", "")
        written("number", 255)
        written("a colour of another property", plain.color)
        const c = plain.color
        result += "same value twice: " + (plain.color === plain.color) + " " + (plain.color == plain.color)
            + " " + (c === c) + " " + (plain.color == bound.color) + " " + (plain.color === bound.color) + " | "
        result += "in a string: " + ("" + plain.color) + " " + JSON.stringify(plain.color) + " "
            + plain.color.toString() + " " + (plain.color + 1) + " | "
        result += "fields: " + [c.r, c.g, c.b, c.a, c.hsvHue, c.hslLightness].join(" ") + " "
            + Object.keys(c).join(",")
    }
}
