pragma Singleton
import QtQuick
QtObject {
    property QtObject colors: QtObject {
        property color colPrimary: "#336699"
        property color colOnLayer0: "#ffffff"
    }
    property QtObject animation: QtObject {
        property QtObject elementMove: QtObject { property Component numberAnimation: Component { NumberAnimation {} } }
        property QtObject elementMoveFast: QtObject { property Component numberAnimation: Component { NumberAnimation {} } }
        property QtObject elementResize: QtObject { property Component numberAnimation: Component { NumberAnimation {} } }
    }
}
