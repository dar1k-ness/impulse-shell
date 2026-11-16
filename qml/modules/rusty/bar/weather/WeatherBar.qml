pragma ComponentBehavior: Bound

import ri.modules.common
import ri.modules.common.widgets
import ri.services
import Rustshell
import QtQuick
import QtQuick.Layouts
import Impulse

MouseArea {
    id: root
    property bool hovered: false
    // The length the bar fixes it at, -1 when it takes its own: the temperature goes when it
    // has no room there.
    property real room: -1
    implicitWidth: rowLayout.implicitWidth + 10 * 2
    implicitHeight: Appearance.sizes.barHeight

    acceptedButtons: Qt.LeftButton | Qt.RightButton
    hoverEnabled: !Config.options.bar.tooltips.clickToShow

    onPressed: {
        if (mouse.button === Qt.RightButton) {
            Weather.getData();
            Notify.send(
                Translation.tr("Weather"),
                Translation.tr("Refreshing (manually triggered)")
            )
            mouse.accepted = false
        }
    }

    TextMetrics {
        id: tempLength
        font: temp.font
        text: temp.text
    }

    RowLayout {
        id: rowLayout
        anchors.centerIn: parent

        MaterialSymbol {
            id: icon
            fill: 0
            text: Icons.getWeatherIcon(Weather.data.wCode) ?? "cloud"
            iconSize: Appearance.font.pixelSize.large
            color: Appearance.colors.colOnLayer1
            Layout.alignment: Qt.AlignVCenter
        }

        StyledText {
            id: temp
            // Measured apart from the text itself, which shows or hides by it.
            visible: root.room < 0 || root.room >= icon.implicitWidth + tempLength.width + rowLayout.spacing
            font.pixelSize: Appearance.font.pixelSize.small
            color: Appearance.colors.colOnLayer1
            text: Weather.data?.temp ?? "--°"
            Layout.alignment: Qt.AlignVCenter
        }
    }

    WeatherPopup {
        id: weatherPopup
        hoverTarget: root
    }
}
