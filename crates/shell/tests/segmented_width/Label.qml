// As qml/modules/common/widgets/StyledText.qml: the family the file sets is the one the text
// has already, and bindings of the font come after it.
import QtQuick

Text {
    id: root
    property bool numbers: /^\d+$/.test(root.text)
    property var family: numbers ? "DejaVu Sans Mono" : Qt.application.font.family

    font {
        hintingPreference: Font.PreferDefaultHinting
        family: root.family
        pixelSize: 15
        variableAxes: numbers ? ({}) : ({ "wght": 500 })
    }
}
