// The colours a file of the Basic style binds to its palette (`color: control.palette.text`
// in TextField.qml): the text, placeholder and selection colours of TextField and TextArea,
// the text and link colours of Label, the colour of ApplicationWindow. Nothing set, each is
// the colour of that role in the object's palette: the disabled group's in what is not
// enabled, the one set in the palette of the object or of what it is in, and it changes when
// that colour does. A text field of a style module (QtQuick.Controls.FluentWinUI3) has the
// same four bindings, and the palette is the one of the style in use (Basic: QtQuick.Controls
// is imported first).
// qt: #353637 #88353637 #0066ff #090909 #7f353637 #88353637 #f0f6ff #090909 #405060 #102030 #708090 #a0b0c0 #010203 #040506 #0066ff #090909 #353637 #88353637 #0066ff #090909 #7f353637 #88353637 #f0f6ff #090909 #415161 #112131 #718191 #a1b1c1 #26282a #45a7d7 #bdbebf #45a7d7 #425262 #122232 #ffffff #0a0b0c #353637 #88353637 #0066ff #090909 #7f353637 #88353637 #f0f6ff #090909 #435363 #132333 #738393 #a3b3c3 #010203 #040506 #0066ff #090909 #353637 #112233 #0066ff #090909 #223344 #45a7d7 #353637 #112233 #0066ff #090909 #000000 #000000 #0000ff 0|#778899 #88353637 #0066ff #090909 #7f353637 #88353637 #f0f6ff #090909 #405060 #102030 #708090 #a0b0c0 #010203 #040506 #0066ff #090909 #353637 #88353637 #0066ff #090909 #7f353637 #88353637 #f0f6ff #090909 #415161 #112131 #718191 #a1b1c1 #26282a #8899aa #bdbebf #45a7d7 #425262 #122232 #ffffff #0d0e0f #778899 #88353637 #0066ff #090909 #7f353637 #88353637 #f0f6ff #090909 #435363 #132333 #738393 #a3b3c3 #010203 #040506 #0066ff #090909 #353637 #99aabb #0066ff #090909 #223344 #45a7d7 #353637 #99aabb #0066ff #090909 #000000 #000000 #0000ff 10001|#353637 #88353637 #0066ff #090909 #7f353637 #88353637 #f0f6ff #090909 #405060 #102030 #708090 #a0b0c0 #010203 #040506 #0066ff #090909 #353637 #88353637 #0066ff #090909 #7f353637 #88353637 #f0f6ff #090909 #415161 #112131 #718191 #a1b1c1 #26282a #45a7d7 #bdbebf #45a7d7 #425262 #122232 #ffffff #ffffff #353637 #88353637 #0066ff #090909 #7f353637 #88353637 #f0f6ff #090909 #435363 #132333 #738393 #a3b3c3 #010203 #040506 #0066ff #090909 #353637 #88353637 #0066ff #090909 #26282a #45a7d7 #353637 #88353637 #0066ff #090909 #000000 #000000 #0000ff 20002|#7f353637 #88353637 #f0f6ff #090909 #7f353637 #88353637 #f0f6ff #090909 #405060 #102030 #708090 #a0b0c0 #010203 #040506 #0066ff #090909 #353637 #88353637 #0066ff #090909 #7f353637 #88353637 #f0f6ff #090909 #415161 #112131 #718191 #a1b1c1 #26282a #45a7d7 #bdbebf #45a7d7 #728292 #122232 #ffffff #ffffff #7f353637 #88353637 #f0f6ff #090909 #7f353637 #88353637 #f0f6ff #090909 #435363 #132333 #738393 #a3b3c3 #010203 #040506 #0066ff #090909 #7f353637 #88353637 #f0f6ff #090909 #bdbebf #45a7d7 #7f353637 #88353637 #f0f6ff #090909 #000000 #000000 #0000ff 30003
import QtQuick
import QtQuick.Controls
import QtQuick.Controls.FluentWinUI3 as Fluent
Item {
    id: root
    property string result
    property int n: 0
    TextField { id: field; onColorChanged: root.n++; onPlaceholderTextColorChanged: root.n += 100 }
    TextField { id: fieldOff; enabled: false }
    TextField { id: fieldOwn; palette.text: "#405060"; palette.placeholderText: "#102030"; palette.highlight: "#708090"; palette.highlightedText: "#a0b0c0" }
    TextField { id: fieldGiven; color: "#010203"; placeholderTextColor: "#040506"; palette.text: "#405060" }
    TextArea { id: area }
    TextArea { id: areaOff; enabled: false }
    TextArea { id: areaOwn; palette.text: "#415161"; palette.placeholderText: "#112131"; palette.highlight: "#718191"; palette.highlightedText: "#a1b1c1" }
    Label { id: label }
    Label { id: labelOff; enabled: false }
    Label { id: labelOwn; palette.windowText: "#425262"; palette.link: "#122232"; palette.disabled.windowText: "#728292" }
    ApplicationWindow { id: win }
    ApplicationWindow { id: winOwn; palette.window: "#0a0b0c" }
    Fluent.TextField { id: fluent; onColorChanged: root.n += 10000 }
    Fluent.TextField { id: fluentOff; enabled: false }
    Fluent.TextField { id: fluentOwn; palette.text: "#435363"; palette.placeholderText: "#132333"; palette.highlight: "#738393"; palette.highlightedText: "#a3b3c3" }
    Fluent.TextField { id: fluentGiven; color: "#010203"; placeholderTextColor: "#040506" }
    Pane {
        id: pane
        palette.placeholderText: "#112233"; palette.windowText: "#223344"
        TextField { id: inPane }
        Label { id: labelInPane }
        Fluent.TextField { id: fluentInPane }
    }
    // A type that is not of Qt Quick Controls has no such binding.
    TextInput { id: input; palette.text: "#445464" }
    Text { id: text; palette.windowText: "#455565" }
    function four(f) {
        return [f.color, f.placeholderTextColor, f.selectionColor, f.selectedTextColor].join(" ")
    }
    function two(l) { return l.color + " " + l.linkColor }
    function all() {
        return [four(field), four(fieldOff), four(fieldOwn), four(fieldGiven), four(area),
            four(areaOff), four(areaOwn), two(label), two(labelOff), two(labelOwn), win.color,
            winOwn.color, four(fluent), four(fluentOff), four(fluentOwn), four(fluentGiven),
            four(inPane), two(labelInPane), four(fluentInPane), input.color, text.color,
            text.linkColor, n].join(" ")
    }
    Component.onCompleted: {
        // Counted from here: creating the object gives one signal for each such binding.
        n = 0
        const out = [all()]
        field.palette.text = "#778899"
        fluent.palette.text = "#778899"
        label.palette.link = "#8899aa"
        pane.palette.placeholderText = "#99aabb"
        winOwn.palette.window = "#0d0e0f"
        out.push(all())
        field.palette.text = undefined
        fluent.palette = undefined
        label.palette.link = undefined
        pane.palette = undefined
        winOwn.palette.window = undefined
        out.push(all())
        field.enabled = false
        fluent.enabled = false
        labelOwn.enabled = false
        pane.enabled = false
        out.push(all())
        result = out.join("|")
    }
}
