import QtQuick
import QtQuick.Layouts
import ri.services
import ri.modules.common
import ri.modules.common.widgets
import ri.modules.rusty.bar.weather

// Sidebar widget "weather": the current conditions on a block card, with the original's weather popup
// metrics. In a row of toggles it is as high as they are: the conditions' icon and the
// temperature, with the city beside them where there is room, scaled down to the cell.
SidebarCard {
    id: root
    implicitWidth: root.inGrid ? root.cellsWidth : content.implicitWidth + root.padding * 2
    implicitHeight: root.inGrid ? root.baseCellHeight : content.implicitHeight + root.padding * 2
    readonly property real padding: 16

    Row {
        id: compact
        visible: root.inGrid
        anchors.centerIn: parent
        spacing: 8
        scale: Math.min(1, (root.width - 20) / Math.max(1, compact.implicitWidth), (root.height - 12) / Math.max(1, compact.implicitHeight))

        MaterialSymbol {
            anchors.verticalCenter: parent.verticalCenter
            fill: 0
            text: Icons.getWeatherIcon(Weather.data.wCode) ?? "cloud"
            iconSize: Appearance.font.pixelSize.hugeass * 1.4
            color: Appearance.colors.colOnLayer2
        }
        StyledText {
            anchors.verticalCenter: parent.verticalCenter
            text: Weather.data?.temp ?? "--°"
            font.pixelSize: Appearance.font.pixelSize.hugeass
            font.weight: Font.Medium
            color: Appearance.colors.colOnLayer2
        }
        StyledText {
            visible: root.cellSize >= 4 && text.length > 0
            anchors.verticalCenter: parent.verticalCenter
            width: Math.min(implicitWidth, 130)
            elide: Text.ElideRight
            text: Weather.data?.city ?? ""
            font.pixelSize: Appearance.font.pixelSize.small
            color: Appearance.colors.colSubtext
        }
    }

    ColumnLayout {
        id: content
        visible: !root.inGrid
        anchors {
            left: parent.left
            right: parent.right
            verticalCenter: parent.verticalCenter
            margins: root.padding
        }
        spacing: 10

        RowLayout {
            spacing: 12
            MaterialSymbol {
                fill: 0
                text: Icons.getWeatherIcon(Weather.data.wCode) ?? "cloud"
                iconSize: Appearance.font.pixelSize.hugeass * 2
                color: Appearance.colors.colOnLayer1
            }
            ColumnLayout {
                spacing: 0
                StyledText {
                    text: Weather.data?.temp ?? "--°"
                    font.pixelSize: Appearance.font.pixelSize.hugeass
                    font.weight: Font.Medium
                    color: Appearance.colors.colOnLayer1
                }
                StyledText {
                    text: Translation.tr("Feels like %1").arg(Weather.data?.tempFeelsLike ?? "--°")
                    font.pixelSize: Appearance.font.pixelSize.smaller
                    color: Appearance.colors.colSubtext
                }
            }
            Item {
                Layout.fillWidth: true
            }
            MaterialSymbol {
                text: "location_on"
                iconSize: Appearance.font.pixelSize.normal
                color: Appearance.colors.colSubtext
            }
            StyledText {
                Layout.maximumWidth: content.width / 3
                elide: Text.ElideRight
                text: Weather.data?.city ?? ""
                font.pixelSize: Appearance.font.pixelSize.small
                color: Appearance.colors.colSubtext
            }
        }

        GridLayout {
            Layout.fillWidth: true
            columns: 2
            rowSpacing: 5
            columnSpacing: 5
            uniformCellWidths: true

            WeatherCard {
                title: Translation.tr("Humidity")
                symbol: "humidity_low"
                value: Weather.data?.humidity ?? ""
            }
            WeatherCard {
                title: Translation.tr("Wind")
                symbol: "air"
                value: `(${Weather.data?.windDir ?? ""}) ${Weather.data?.wind ?? ""}`
            }
            WeatherCard {
                title: Translation.tr("UV Index")
                symbol: "wb_sunny"
                value: Weather.data?.uv ?? ""
            }
            WeatherCard {
                title: Translation.tr("Precipitation")
                symbol: "rainy_light"
                value: Weather.data?.precip ?? ""
            }
        }
    }
}
