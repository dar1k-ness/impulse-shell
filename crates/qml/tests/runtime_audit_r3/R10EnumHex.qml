import QtQuick
Item {
    id: root
    property string log: ""
    function p(...a) { log += a.join(" ") + "\n"; }
    enum Mode { A, B = 5, C, Neg = -2, Hex = 0x10, After, Up = 0XfF, Last }
    Component.onCompleted: {
        p("values", [R10EnumHex.Mode.A, R10EnumHex.Mode.B, R10EnumHex.Mode.C, R10EnumHex.Mode.Neg, R10EnumHex.Mode.Hex, R10EnumHex.Mode.After, R10EnumHex.Mode.Up, R10EnumHex.Mode.Last].join(" "));
        p("unscoped", R10EnumHex.Hex, R10EnumHex.After, root.Hex === undefined);
    }
}
