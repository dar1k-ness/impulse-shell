pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import ri.services
import ri.modules.common
import ri.modules.common.widgets

/**
 * RAM, swap and CPU as a row of Material You gauges: what the original's bar resources widget and its
 * popup show (usage ring with the resource's icon, percentage, used of total), with the bar's
 * warning thresholds. Swap shows only when the system has some.
 *
 * `fitTo(width, height)` arranges the gauges for a box of that size: side by side or one above
 * another, each with its label under the ring or beside it or, in a box too small for labels,
 * the rings alone, whichever draws them largest, and `zoom` times the size they have in the bar's popup, so text stays sharp in a large box.
 */
Item {
    id: root
    property color colOnCard: Appearance.colors.colOnLayer1
    property color colSubtext: Appearance.colors.colSubtext
    property real gaugeSize: 72
    // The gauges one above another, not side by side.
    property bool stacked: false
    // Each gauge's label and detail beside its ring, not under it.
    property bool labelsBeside: false
    // The rings alone, with the resource's icon in each: no percentage, label or detail.
    property bool ringsOnly: false
    property real zoom: 1
    readonly property int gaugeCount: ResourceUsage.swapTotal > 0 ? 3 : 2
    implicitWidth: row.implicitWidth
    implicitHeight: row.implicitHeight

    // About what the gauges take at zoom 1 in each arrangement: a gauge with its label under
    // the ring is 110 by 125, one with the label beside it 175 by 72, a ring alone 72 by 72.
    function arrangementSize(stacked, labelsBeside, ringsOnly) {
        const n = root.gaugeCount;
        const w = ringsOnly ? 72 : labelsBeside ? 175 : 110;
        const h = ringsOnly || labelsBeside ? 72 : 125;
        return stacked ? Qt.size(w, n * h + (n - 1) * 12) : Qt.size(n * w + (n - 1) * 20, h);
    }
    // The arrangement that draws the gauges largest in a box of this size, and how much larger
    // than in the popup (never smaller: a box too small for them is the caller's to scale).
    // The popup's own arrangement wins unless another is clearly larger, and the rings go
    // without their labels only in a box where the labels would be too small to read.
    function fitTo(width, height) {
        let best = {
            "stacked": false,
            "labelsBeside": false,
            "ringsOnly": false,
            "zoom": 1
        };
        let bestFit = 0;
        for (const ringsOnly of [false, true]) {
            if (ringsOnly && bestFit >= 0.6)
                break;
            for (const stacked of [false, true]) {
                for (const labelsBeside of ringsOnly ? [false] : [false, true]) {
                    const size = root.arrangementSize(stacked, labelsBeside, ringsOnly);
                    const fit = Math.min(width / size.width, height / size.height);
                    const own = !stacked && !labelsBeside && !ringsOnly;
                    if (fit * (own ? 1.25 : 1) > bestFit) {
                        bestFit = fit * (own ? 1.25 : 1);
                        best = {
                            "stacked": stacked,
                            "labelsBeside": labelsBeside,
                            "ringsOnly": ringsOnly,
                            "zoom": Math.max(1, fit)
                        };
                    }
                }
            }
        }
        return best;
    }

    function formatKB(kb) {
        return (kb / (1024 * 1024)).toFixed(1);
    }

    Demand {
        source: ResourceUsage
        active: root.visible
    }

    GridLayout {
        id: row
        anchors.centerIn: parent
        columns: root.stacked ? 1 : -1
        columnSpacing: Math.round(20 * root.zoom)
        rowSpacing: Math.round(12 * root.zoom)

        Gauge {
            icon: "memory"
            label: Translation.tr("RAM")
            value: ResourceUsage.memoryUsedPercentage
            detail: `${root.formatKB(ResourceUsage.memoryUsed)} / ${root.formatKB(ResourceUsage.memoryTotal)} GB`
            warningThreshold: Config.options.bar.resources.memoryWarningThreshold
        }
        Gauge {
            visible: ResourceUsage.swapTotal > 0
            icon: "swap_horiz"
            label: Translation.tr("Swap")
            value: ResourceUsage.swapUsedPercentage
            detail: `${root.formatKB(ResourceUsage.swapUsed)} / ${root.formatKB(ResourceUsage.swapTotal)} GB`
            warningThreshold: Config.options.bar.resources.swapWarningThreshold
        }
        Gauge {
            icon: "planner_review"
            label: Translation.tr("CPU")
            value: ResourceUsage.cpuUsage
            detail: ResourceUsage.maxAvailableCpuString
            warningThreshold: Config.options.bar.resources.cpuWarningThreshold
        }
    }

    component Gauge: GridLayout {
        id: gauge
        required property string icon
        required property string label
        required property real value
        required property string detail
        property int warningThreshold: 100
        readonly property bool warning: gauge.value * 100 >= gauge.warningThreshold
        readonly property int textAlignment: root.labelsBeside ? Qt.AlignLeft : Qt.AlignHCenter
        columns: root.labelsBeside ? 2 : 1
        rowSpacing: Math.round(4 * root.zoom)
        columnSpacing: Math.round(8 * root.zoom)

        CircularProgress {
            id: ring
            Layout.alignment: Qt.AlignHCenter | Qt.AlignVCenter
            implicitSize: Math.round(root.gaugeSize * root.zoom)
            lineWidth: Math.round(6 * root.zoom)
            value: gauge.value
            colPrimary: gauge.warning ? Appearance.colors.colError : Appearance.colors.colPrimary
            colSecondary: gauge.warning ? Appearance.colors.colErrorContainer : Appearance.colors.colSecondaryContainer

            Column {
                anchors.centerIn: parent
                spacing: -2

                MaterialSymbol {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: gauge.icon
                    fill: 1
                    iconSize: Math.round(Appearance.font.pixelSize.larger * root.zoom * (root.ringsOnly ? 1.6 : 1))
                    color: gauge.warning ? Appearance.colors.colError : root.colOnCard
                }
                StyledText {
                    visible: !root.ringsOnly
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: `${Math.round(gauge.value * 100)}%`
                    font.pixelSize: Math.round(Appearance.font.pixelSize.small * root.zoom)
                    font.weight: Font.DemiBold
                    color: root.colOnCard
                }
            }
        }
        ColumnLayout {
            visible: !root.ringsOnly
            Layout.alignment: Qt.AlignVCenter | gauge.textAlignment
            spacing: gauge.rowSpacing

            StyledText {
                Layout.alignment: gauge.textAlignment
                text: gauge.label
                font.pixelSize: Math.round(Appearance.font.pixelSize.normal * root.zoom)
                font.weight: Font.DemiBold
                color: root.colOnCard
            }
            StyledText {
                Layout.alignment: gauge.textAlignment
                text: gauge.detail
                font.pixelSize: Math.round(Appearance.font.pixelSize.smaller * root.zoom)
                color: root.colSubtext
            }
        }
    }
}
