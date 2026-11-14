import QtQuick
TextInput {
    property var out
    text: "x"
    onTextChanged: out.ev("base text")
    onLengthChanged: out.ev("base len")
    onCursorPositionChanged: out.ev("base cp")
}
