import QtQuick
import QtQuick.Layouts
import QtQuick.Controls.Material
import QtQuick.Controls
import Qt5Compat.GraphicalEffects

// The places of qml/ that show a ProgressBar, each with the lines that place it there and a
// block where a neighbour stands. First the five with a running bar, then the seven with a
// value. The types of qml/ are written out (`Bar`, `WBar`, `SBar`, `Clipped`, `WValueBar`,
// `SText`, `Symbol`), with what Appearance, Looks and Config say as constants:
// rounding.large 23, rounding.unsharpen 2, the fonts "Google Sans Flex", the colours.
//
// LightDarkPreferenceButton.qml:70 is a StyledProgressBar that is always `wavy`: its fill is a
// Canvas (WavyLine) whose wave starts at `Date.now() / 400`, so its frames are taken with the
// clock held (`now=` of tools/oracle/progress_oracle.py). PlayerControl.qml:263 is wavy while the
// player plays, and is here as it is when the player does not.
Rectangle {
    width: 380; height: 530
    color: "#202020"

    component Block: Rectangle { Layout.fillWidth: true; implicitHeight: 12; color: "#404040" }
    // qml/modules/common/widgets/StyledIndeterminateProgressBar.qml
    component Bar: ProgressBar {
        indeterminate: true
        Material.accent: "#cbc4cb"
    }
    // qml/modules/waffle/looks/WIndeterminateProgressBar.qml
    component WBar: Bar {
        id: progressBar
        implicitHeight: 3
        background: null
        layer.enabled: true
        layer.effect: OpacityMask {
            maskSource: Rectangle {
                width: progressBar.width
                height: progressBar.height
                radius: progressBar.height / 2
            }
        }
    }

    // qml/modules/rusty/sidebarRight/bluetoothDevices/BluetoothDialog.qml:26
    ColumnLayout {
        x: 40; y: 10; width: 300; spacing: 16
        Block {}
        Bar {
            Layout.fillWidth: true
            Layout.topMargin: -8
            Layout.bottomMargin: -8
            Layout.leftMargin: -23
            Layout.rightMargin: -23
        }
        Block {}
    }
    // qml/modules/rusty/sidebarRight/wifiNetworks/WifiDialog.qml:20
    ColumnLayout {
        x: 40; y: 76; width: 300; spacing: 16
        Block {}
        Bar {
            Layout.fillWidth: true
            Layout.topMargin: -8
            Layout.bottomMargin: -8
            Layout.leftMargin: -23
            Layout.rightMargin: -23
        }
        Block {}
    }
    // qml/modules/rusty/wallpaperSelector/WallpaperSelectorContent.qml:266
    ColumnLayout {
        x: 40; y: 142; width: 300; spacing: 0
        Block {}
        Item {
            Layout.fillWidth: true
            implicitHeight: 12
            Bar {
                anchors {
                    bottom: parent.top
                    left: parent.left
                    right: parent.right
                    leftMargin: 4
                    rightMargin: 4
                }
            }
        }
    }
    // qml/modules/waffle/actionCenter/bluetooth/BluetoothControl.qml:72
    ColumnLayout {
        x: 40; y: 176; width: 300; spacing: 12
        ColumnLayout {
            spacing: 0
            Block {}
            Loader {
                Layout.leftMargin: -4
                Layout.rightMargin: -4
                Layout.fillWidth: true
                sourceComponent: WBar {}
            }
        }
        Block {}
    }
    // qml/modules/waffle/actionCenter/wifi/WifiControl.qml:62
    ColumnLayout {
        x: 40; y: 224; width: 300; spacing: 12
        ColumnLayout {
            spacing: 0
            Block {}
            Loader {
                Layout.leftMargin: -4
                Layout.rightMargin: -4
                Layout.fillWidth: true
                sourceComponent: WBar {}
            }
        }
        Block {}
    }

    // qml/modules/common/widgets/StyledText.qml
    component SText: Text {
        id: styledText
        renderType: Text.NativeRendering
        verticalAlignment: Text.AlignVCenter
        property bool shouldUseNumberFont: /^\d+$/.test(styledText.text)
        property var defaultFont: "Google Sans Flex"
        font {
            hintingPreference: Font.PreferDefaultHinting
            family: defaultFont
            pixelSize: 15
            variableAxes: shouldUseNumberFont ? ({}) : ({ "wght": 450, "wdth": 100 })
        }
        color: "#e6e1e3"
    }
    // qml/modules/common/widgets/MaterialSymbol.qml
    component Symbol: SText {
        id: symbol
        property real iconSize: 15
        property real fill: 0
        property real truncatedFill: fill.toFixed(1)
        renderType: Text.NativeRendering
        font {
            hintingPreference: Font.PreferNoHinting
            family: "Material Symbols Rounded"
            pixelSize: iconSize
            weight: Font.Normal + (Font.DemiBold - Font.Normal) * truncatedFill
            variableAxes: ({ "FILL": truncatedFill, "opsz": iconSize })
        }
    }
    // qml/modules/common/widgets/WavyLine.qml
    component WavyLine: Canvas {
        id: wavyLine
        property real amplitudeMultiplier: 0.5
        property real frequency: 6
        property color color: "#cbc4cb"
        property real lineWidth: 4
        property real fullLength: width

        onPaint: {
            var ctx = getContext("2d");
            ctx.clearRect(0, 0, width, height);

            var amplitude = wavyLine.lineWidth * wavyLine.amplitudeMultiplier;
            var frequency = wavyLine.frequency;
            var phase = Date.now() / 400.0;
            var centerY = height / 2;

            ctx.strokeStyle = wavyLine.color;
            ctx.lineWidth = wavyLine.lineWidth;
            ctx.lineCap = "round";
            ctx.beginPath();
            for (var x = ctx.lineWidth / 2; x <= wavyLine.width - ctx.lineWidth / 2; x += 1) {
                var waveY = centerY + amplitude * Math.sin(frequency * 2 * Math.PI * x / wavyLine.fullLength + phase);
                if (x === 0)
                    ctx.moveTo(x, waveY);
                else
                    ctx.lineTo(x, waveY);
            }
            ctx.stroke();
        }
    }
    // qml/modules/common/widgets/StyledProgressBar.qml
    component SBar: ProgressBar {
        id: sbar
        property real valueBarWidth: 120
        property real valueBarHeight: 4
        property real valueBarGap: 4
        property color highlightColor: "#cbc4cb"
        property color trackColor: "#4a4458"
        property bool wavy: false
        property bool animateWave: true
        property real waveAmplitudeMultiplier: wavy ? 0.5 : 0
        property real waveFrequency: 6
        property real waveFps: 60

        Behavior on waveAmplitudeMultiplier {
            NumberAnimation {
                alwaysRunToEnd: true
                duration: 200
                easing.type: Easing.BezierSpline
                easing.bezierCurve: [0.34, 0.80, 0.34, 1.00, 1, 1]
            }
        }

        Behavior on value {
            NumberAnimation {
                alwaysRunToEnd: true
                duration: 400
                easing.type: Easing.BezierSpline
                easing.bezierCurve: [0.05, 0.7, 0.1, 1, 1, 1]
            }
        }

        background: Item {
            implicitHeight: sbar.valueBarHeight
            implicitWidth: sbar.valueBarWidth
        }

        contentItem: Item {
            id: sbarContent
            anchors.fill: parent

            Loader {
                anchors {
                    left: parent.left
                    verticalCenter: parent.verticalCenter
                }
                active: sbar.wavy
                sourceComponent: WavyLine {
                    id: wavyFill
                    frequency: sbar.waveFrequency
                    color: sbar.highlightColor
                    amplitudeMultiplier: sbar.wavy ? 0.5 : 0
                    height: sbarContent.height * 6
                    width: sbarContent.width * sbar.visualPosition
                    lineWidth: sbarContent.height
                    fullLength: sbar.width
                    Connections {
                        target: sbar
                        function onValueChanged() { wavyFill.requestPaint(); }
                        function onHighlightColorChanged() { wavyFill.requestPaint(); }
                    }
                    FrameAnimation {
                        running: sbar.animateWave
                        onTriggered: {
                            wavyFill.requestPaint()
                        }
                    }
                }
            }

            Loader {
                active: !sbar.wavy
                sourceComponent: Rectangle {
                    anchors.left: parent.left
                    width: sbarContent.width * sbar.visualPosition
                    height: sbarContent.height
                    radius: height / 2
                    color: sbar.highlightColor
                }
            }

            Rectangle { // Right remaining part fill
                anchors.right: parent.right
                width: (1 - sbar.visualPosition) * parent.width - sbar.valueBarGap
                height: parent.height
                radius: height / 2
                color: sbar.trackColor
            }

            Rectangle { // Stop point
                anchors.right: parent.right
                width: sbar.valueBarGap
                height: sbar.valueBarGap
                radius: height / 2
                color: sbar.highlightColor
            }
        }
    }
    // qml/modules/common/widgets/ClippedProgressBar.qml
    component Clipped: ProgressBar {
        id: clipped
        property bool vertical: false
        property real valueBarWidth: 30
        property real valueBarHeight: 18
        property color highlightColor: "#e8def8"
        property color trackColor: Qt.rgba(highlightColor.r, highlightColor.g, highlightColor.b, highlightColor.a * 0.5)
        property alias radius: clippedContent.radius
        property string text
        default property Item textMask: Item {
            width: clipped.valueBarWidth
            height: clipped.valueBarHeight
            SText {
                anchors.centerIn: parent
                font: clipped.font
                text: clipped.text
            }
        }

        text: Math.round(value * 100)
        font {
            pixelSize: 13
            weight: text.length > 2 ? Font.Medium : Font.DemiBold
        }

        background: Item {
            implicitHeight: clipped.valueBarHeight
            implicitWidth: clipped.valueBarWidth
        }

        contentItem: Rectangle {
            id: clippedContent
            anchors.fill: parent
            radius: 9999
            color: clipped.trackColor
            visible: false

            Rectangle {
                id: progressFill
                anchors {
                    top: parent.top
                    bottom: parent.bottom
                    left: parent.left
                    right: undefined
                }
                width: parent.width * clipped.visualPosition
                height: parent.height

                states: State {
                    name: "vertical"
                    when: clipped.vertical
                    AnchorChanges {
                        target: progressFill
                        anchors {
                            top: undefined
                            bottom: parent.bottom
                            left: parent.left
                            right: parent.right
                        }
                    }
                    PropertyChanges {
                        target: progressFill
                        width: parent.width
                        height: parent.height * clipped.visualPosition
                    }
                }

                radius: 2
                color: clipped.highlightColor
            }
        }

        OpacityMask {
            id: roundingMask
            visible: false
            anchors.fill: parent
            source: clippedContent
            maskSource: Rectangle {
                width: clippedContent.width
                height: clippedContent.height
                radius: clippedContent.radius
            }
        }

        OpacityMask {
            anchors.fill: parent
            source: roundingMask
            invert: true
            maskSource: clipped.textMask
        }
    }
    // qml/modules/waffle/looks/WProgressBar.qml
    component WValueBar: ProgressBar {
        id: wvalue

        Behavior on value {
            SmoothedAnimation {
                velocity: 850
            }
        }

        implicitHeight: 4
        background: null

        contentItem: Item {
            id: wvalueContent

            Rectangle {
                id: trackTrough
                anchors {
                    left: parent.left
                    right: parent.right
                    verticalCenter: parent.verticalCenter
                }
                radius: wvalue.implicitHeight / 2
                color: "#9b9b9b"
                implicitHeight: wvalue.implicitHeight
            }

            Rectangle {
                id: trackHighlight
                anchors {
                    left: parent.left
                    verticalCenter: parent.verticalCenter
                }
                radius: wvalue.implicitHeight / 2
                color: "#cbc4cb"
                implicitHeight: wvalue.implicitHeight
                width: wvalueContent.width * wvalue.value
            }
        }
    }

    // qml/modules/rusty/mediaControls/PlayerControl.qml:263, the player not playing
    RowLayout {
        x: 40; y: 280; width: 300
        Rectangle { implicitWidth: 24; implicitHeight: 24; color: "#404040" }
        Item {
            id: progressBarContainer
            Layout.fillWidth: true
            implicitHeight: Math.max(0, progressBarLoader.implicitHeight)

            Loader {
                id: progressBarLoader
                anchors {
                    verticalCenter: parent.verticalCenter
                    left: parent.left
                    right: parent.right
                }
                sourceComponent: SBar {
                    wavy: false
                    highlightColor: "#cbc4cb"
                    trackColor: "#4a4458"
                    value: 83 / 215
                }
            }
        }
        Rectangle { implicitWidth: 24; implicitHeight: 24; color: "#404040" }
    }
    // qml/modules/rusty/onScreenDisplay/OsdValueIndicator.qml:98
    ColumnLayout {
        x: 40; y: 316; width: 300
        spacing: 5

        RowLayout {
            Layout.leftMargin: valueProgressBar.height / 2
            Layout.rightMargin: valueProgressBar.height / 2
            Block {}
        }

        SBar {
            id: valueProgressBar
            Layout.fillWidth: true
            value: 0.37
        }
    }
    // qml/modules/rusty/wallpaperSelector/WallpaperSelectorContent.qml:278
    ColumnLayout {
        x: 40; y: 350; width: 300; spacing: 0
        Block {}
        Item {
            Layout.fillWidth: true
            implicitHeight: 12
            Bar {
                id: indeterminateProgressBar
                visible: false
                anchors {
                    bottom: parent.top
                    left: parent.left
                    right: parent.right
                    leftMargin: 4
                    rightMargin: 4
                }
            }

            SBar {
                value: 0.62
                anchors.fill: indeterminateProgressBar
            }
        }
    }
    // qml/modules/rusty/bar/BatteryIndicator.qml:21, not charging (Appearance.sizes.barHeight 40)
    MouseArea {
        x: 40; y: 384
        width: implicitWidth; height: implicitHeight
        implicitWidth: batteryProgress.implicitWidth
        implicitHeight: 40

        Clipped {
            id: batteryProgress
            anchors.centerIn: parent
            value: 0.75
            highlightColor: "#e8def8"

            Item {
                anchors.centerIn: parent
                width: batteryProgress.valueBarWidth
                height: batteryProgress.valueBarHeight

                RowLayout {
                    anchors {
                        horizontalCenter: parent.horizontalCenter
                        bottom: parent.bottom
                        bottomMargin: (parent.height - height) / 2
                    }
                    spacing: 0

                    Symbol {
                        Layout.alignment: Qt.AlignVCenter
                        Layout.leftMargin: -2
                        Layout.rightMargin: -2
                        fill: 1
                        text: "bolt"
                        iconSize: 12
                        visible: false
                    }
                    SText {
                        Layout.alignment: Qt.AlignVCenter
                        font: batteryProgress.font
                        text: batteryProgress.text
                    }
                }
            }
        }
    }
    // qml/modules/rusty/verticalBar/BatteryIndicator.qml:20, not charging
    MouseArea {
        x: 110; y: 384
        width: 40; height: implicitHeight
        implicitHeight: verticalBatteryProgress.implicitHeight

        Clipped {
            id: verticalBatteryProgress
            anchors.centerIn: parent
            vertical: true
            valueBarWidth: 20
            valueBarHeight: 36
            value: 0.75
            highlightColor: "#e8def8"

            font {
                pixelSize: 13
                weight: Font.DemiBold
            }

            textMask: Item {
                anchors.centerIn: parent
                width: verticalBatteryProgress.valueBarWidth
                height: verticalBatteryProgress.valueBarHeight

                Column {
                    anchors.centerIn: parent
                    spacing: -4

                    Symbol {
                        anchors.horizontalCenter: parent.horizontalCenter
                        fill: 1
                        text: "battery_android_5"
                        iconSize: 16
                    }
                    SText {
                        visible: text.length <= 2
                        anchors.horizontalCenter: parent.horizontalCenter
                        font: verticalBatteryProgress.font
                        text: verticalBatteryProgress.text
                    }
                }
            }
        }
    }
    // qml/modules/waffle/onScreenDisplay/OSDValue.qml:50, without the number
    Item {
        x: 170; y: 384
        width: implicitWidth; height: implicitHeight
        implicitWidth: 170
        implicitHeight: 46

        RowLayout {
            anchors.fill: parent
            anchors.margins: 12

            spacing: 12

            Rectangle {
                Layout.alignment: Qt.AlignVCenter
                implicitWidth: 18; implicitHeight: 18
                color: "#404040"
            }

            WValueBar {
                value: 0.43
                Layout.fillWidth: true
                Layout.alignment: Qt.AlignVCenter
                Layout.rightMargin: 3
            }

            Item {
                visible: false
                implicitWidth: 16
            }
        }
    }
    // qml/modules/common/widgets/LightDarkPreferenceButton.qml:70, toggled
    Item {
        x: 65; y: 446
        width: 250; height: skeletonColumnLayout.implicitHeight + 10 * 2

        ColumnLayout {
            id: skeletonColumnLayout
            anchors.fill: parent
            anchors.margins: 10
            spacing: 10
            Block {}
            SBar {
                Layout.topMargin: 5
                Layout.bottomMargin: 5
                Layout.fillWidth: true
                value: 0.7
                wavy: true
                animateWave: true
                highlightColor: "#cbc4cb"
                trackColor: "#8c8789"
            }
            Block {}
        }
    }
}
