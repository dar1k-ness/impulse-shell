import QtQuick
Rectangle {
    id: root
    property int declared: 7
    property string log: ""
    signal poked()
    function run() {
        var hits = 0
        for (var i = 0; i < 2000; i++) {
            if (root["r1_absent_get_" + i] !== undefined) hits++
            if (("r1_absent_in_" + i) in root) hits++
        }
        log += hits + "\n"
        // Names the object does have still resolve when built at run time.
        var known = ["decl" + "ared", "wid" + "th", "bor" + "der", "anc" + "hors", "po" + "ked",
                     "decl" + "aredChanged", "activeFocusOn" + "Tab", "baselineOffset" + "Changed",
                     "containment" + "Mask", "r1_" + "nothing"]
        for (var j = 0; j < known.length; j++)
            log += known[j] + " " + typeof root[known[j]] + " " + (known[j] in root) + "\n"
    }
    Component.onCompleted: { run(); console.log("LOG<" + log + ">LOG"); Qt.quit() }
}
