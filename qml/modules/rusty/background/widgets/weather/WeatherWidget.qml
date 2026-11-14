import QtQuick
import ri
import ri.services
import ri.modules.common
import ri.modules.common.functions
import ri.modules.common.widgets
import ri.modules.common.widgets.widgetCanvas
import ri.modules.rusty.background.widgets

AbstractBackgroundWidget {
    id: root

    configEntryName: "weather"

    // The original's size, and how many times larger or smaller this one is drawn: a desktop
    // box of another size (DesktopWeather) gets the shape, the temperature and the icon at
    // that size, not the original's stretched.
    readonly property real naturalSize: 200
    property real zoom: 1

    implicitHeight: backgroundShape.implicitHeight
    implicitWidth: backgroundShape.implicitWidth

    StyledDropShadow {
        target: backgroundShape
    }

    MaterialShape {
        id: backgroundShape
        anchors.fill: parent
        shape: MaterialShape.Shape.Pill
        color: Appearance.colors.colPrimaryContainer
        implicitSize: root.naturalSize * root.zoom

        StyledText {
            font {
                pixelSize: Math.round(80 * root.zoom)
                family: Appearance.font.family.expressive
                weight: Font.Medium
            }
            color: Appearance.colors.colPrimary
            text: Weather.data?.temp.substring(0,Weather.data?.temp.length - 1) ?? "--°"
            anchors {
                right: parent.right
                top: parent.top
                rightMargin: Math.round(16 * root.zoom)
                topMargin: Math.round(20 * root.zoom)
            }
        }

        MaterialSymbol {
            iconSize: Math.round(80 * root.zoom)
            color: Appearance.colors.colOnPrimaryContainer
            text: Icons.getWeatherIcon(Weather.data.wCode) ?? "cloud"
            anchors {
                left: parent.left
                bottom: parent.bottom

                leftMargin: Math.round(16 * root.zoom)
                bottomMargin: Math.round(20 * root.zoom)
            }
        }
    }
}
