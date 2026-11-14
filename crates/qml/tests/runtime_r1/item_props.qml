import QtQuick
Item {
    id: root
    property string log: ""
    ListModel { id: lm }
    Component { id: cm; Item {} }
    QtObject { id: qo }
    Binding { id: bd }
    Connections { id: cn }
    Timer { id: tm }
    ObjBase { id: co }
    ItemBase { id: ci }
    Item { id: it }
    Rectangle { id: rc }
    Text { id: tx }
    MouseArea { id: ma }
    Loader { id: ld }
    Repeater { id: rp }
    Flickable { id: fl }
    ListView { id: lv }
    Column { id: cl }
    TextInput { id: tin }
    Image { id: im }
    IntValidator { id: iv }
    FontMetrics { id: fm }
    NumberAnimation { id: na }
    State { id: sta }
    Gradient { id: gr }
    Instantiator { id: ins }
    Translate { id: tr }
    property var names: ["visible", "enabled", "width", "x", "y", "z", "opacity", "scale", "rotation", "radius", "implicitWidth"]
    function row(n, o) {
        var s = n
        for (var i = 0; i < names.length; i++) s += " " + o[names[i]]
        log += s + "\n"
    }
    function bare(n, f) { try { log += n + " " + f() + "\n" } catch (e) { log += n + " " + e + "\n" } }
    function run() {
        row("lm", lm); row("cm", cm); row("qo", qo); row("bd", bd); row("cn", cn); row("tm", tm); row("co", co); row("ci", ci)
        row("it", it); row("rc", rc); row("tx", tx); row("ma", ma); row("ld", ld); row("rp", rp); row("fl", fl); row("lv", lv)
        row("cl", cl); row("tin", tin); row("im", im); row("iv", iv); row("fm", fm); row("na", na); row("sta", sta); row("gr", gr)
        row("ins", ins); row("tr", tr)
        bare("bare radius", function() { return radius })
        bare("bare visible", function() { return visible })
        bare("bare nosuch", function() { return nosuch })
        bare("in lm", function() { return ("visible" in lm) + " " + ("width" in lm) })
        bare("in it", function() { return ("visible" in it) + " " + ("radius" in it) })
        bare("typeof", function() { return typeof lm.visible + " " + typeof co.width + " " + typeof it.radius })
    }
    Component.onCompleted: { run(); console.log("LOG<" + log + ">LOG"); Qt.quit() }
}
