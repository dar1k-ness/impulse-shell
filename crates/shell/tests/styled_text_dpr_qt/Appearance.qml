pragma Singleton
import QtQuick

// What StyledText.qml and StyledTextInput.qml read of qml/modules/common/Appearance.qml, with
// the values it has before a theme is loaded.
QtObject {
    property QtObject m3colors: QtObject {
        property color m3onBackground: "#e6e1e1"
        property color m3primary: "#cbc4cb"
        property color m3onSecondaryContainer: "#ece6e9"
    }
    property QtObject colors: QtObject {
        property color colOnLayer1: "#cbc5ca"
        property color colSecondaryContainer: "#4d4b4d"
    }
    property QtObject font: QtObject {
        property QtObject family: QtObject {
            property string main: "Google Sans Flex"
            property string numbers: "Google Sans Flex"
        }
        property QtObject variableAxes: QtObject {
            property var main: ({
                "wght": 450,
                "wdth": 100
            })
        }
        property QtObject pixelSize: QtObject {
            property int small: 15
        }
    }
    property QtObject animation: QtObject {
        property QtObject elementMoveFast: QtObject {
            property list<real> bezierCurve: [0.34, 0.80, 0.34, 1.00, 1, 1]
        }
    }
}
