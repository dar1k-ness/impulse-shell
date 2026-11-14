import QtQuick
import QtQuick.Layouts
import ri.modules.common
import ri.modules.common.widgets
import ri.modules.common.functions

/**
 * One settings row: title and optional subtitle on the left, the control on the right.
 * `shown` hides the row (dependent rows bind it to their parent option). A row with `parentRow`
 * is a sub-option: indented one step per level on its own shade, with a guide line per level,
 * sliding open and shut with its parent, which gets an arrow to fold its shown sub-options away.
 */
Rectangle {
    id: root
    property string title: ""
    property string subtitle: ""
    property string keywords: ""
    property bool shown: true
    property bool searchable: true
    property bool clickable: false
    property Item parentRow: null
    property real controlSpacing: 8
    property bool flash: false
    property bool showChildToggle: true
    property bool collapsed: false
    property var childRows: []
    readonly property bool isChild: parentRow !== null
    readonly property int depth: parentRow ? (parentRow.depth ?? 0) + 1 : 0
    // Its own `shown` and, in a SettingsPair, the pair's
    readonly property bool ownShown: shown && (!(parent && parent.settingsPair === true) || parent.shown)
    readonly property bool hasShownChildren: childRows.some(c => c.ownShown)
    readonly property bool effShown: ownShown && (!parentRow || ((parentRow.effShown ?? true) && !parentRow.collapsed))
    property real reveal: effShown ? 1 : 0
    Behavior on reveal {
        NumberAnimation {
            duration: 220
            easing.type: Easing.OutCubic
        }
    }
    default property alias controlData: controlSlot.data
    signal clicked

    readonly property real bigRadius: Appearance.rounding.normal
    readonly property real smallRadius: 4
    readonly property Item edgeItem: (parent && parent.settingsPair === true) ? parent : root
    readonly property int pairSide: (parent && parent.settingsPair === true) ? (parent.children[0] === root ? 1 : 2) : 0
    readonly property bool groupFirst: {
        const p = edgeItem.parent;
        if (!p)
            return true;
        const c = p.children;
        for (let i = 0; i < c.length; i++) {
            const s = c[i].effShown !== undefined ? c[i].effShown : c[i].shown;
            if (s === undefined)
                continue;
            if (s)
                return c[i] === edgeItem;
        }
        return true;
    }
    readonly property bool groupLast: {
        const p = edgeItem.parent;
        if (!p)
            return true;
        const c = p.children;
        for (let i = c.length - 1; i >= 0; i--) {
            const s = c[i].effShown !== undefined ? c[i].effShown : c[i].shown;
            if (s === undefined)
                continue;
            if (s)
                return c[i] === edgeItem;
        }
        return true;
    }

    visible: reveal > 0
    clip: reveal < 1
    Layout.fillWidth: true
    Layout.preferredHeight: implicitHeight * reveal
    implicitHeight: Math.max(root.subtitle.length > 0 ? 60 : 52, rowLayout.implicitHeight + 16)
    topLeftRadius: groupFirst && pairSide !== 2 ? bigRadius : smallRadius
    topRightRadius: groupFirst && pairSide !== 1 ? bigRadius : smallRadius
    bottomLeftRadius: groupLast && pairSide !== 2 ? bigRadius : smallRadius
    bottomRightRadius: groupLast && pairSide !== 1 ? bigRadius : smallRadius
    // No Behavior on color: a palette change would animate every row of every preloaded page
    color: root.isChild ? ColorUtils.mix(Appearance.colors.colLayer2, Appearance.colors.colLayer3, 0.35) : Appearance.colors.colLayer2

    Repeater {
        model: root.depth
        Rectangle {
            required property int index
            z: 1
            x: 18 + index * 18
            width: 2
            radius: 1
            anchors {
                top: parent.top
                bottom: parent.bottom
                margins: 10
            }
            color: Appearance.colors.colOutlineVariant
        }
    }

    Rectangle {
        anchors.fill: parent
        topLeftRadius: root.topLeftRadius
        topRightRadius: root.topRightRadius
        bottomLeftRadius: root.bottomLeftRadius
        bottomRightRadius: root.bottomRightRadius
        color: root.flash ? Appearance.colors.colSecondaryContainer : Appearance.colors.colLayer2Hover
        opacity: (root.flash || (root.clickable && root.enabled && rowMouse.containsMouse)) ? 1 : 0
        visible: opacity > 0
        Behavior on opacity {
            animation: Appearance.animation.elementMoveFast.numberAnimation.createObject(this)
        }
    }

    MouseArea {
        id: rowMouse
        anchors.fill: parent
        enabled: root.clickable && root.enabled
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.clicked()
    }

    RowLayout {
        id: rowLayout
        anchors {
            left: parent.left
            right: parent.right
            // Pinned to the top, not centred: while the row folds, it slides under the clip instead of squashing
            top: parent.top
            topMargin: (root.implicitHeight - rowLayout.implicitHeight) / 2
            leftMargin: 16 + root.depth * 18
            rightMargin: 14
        }
        spacing: 12
        opacity: (root.enabled ? 1 : 0.4) * (root.isChild ? root.reveal * root.reveal : 1)

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 1
            StyledText {
                Layout.fillWidth: true
                text: root.title
                color: Appearance.colors.colOnLayer2
                font.pixelSize: Appearance.font.pixelSize.small
                font.weight: Font.Medium
                elide: Text.ElideRight
            }
            StyledText {
                visible: root.subtitle.length > 0
                Layout.fillWidth: true
                text: root.subtitle
                color: Appearance.colors.colSubtext
                font.pixelSize: Appearance.font.pixelSize.smallie
                elide: Text.ElideRight
            }
        }

        Item {
            id: childToggle
            visible: root.showChildToggle && root.hasShownChildren
            implicitWidth: 32
            implicitHeight: 32
            Rectangle {
                anchors.fill: parent
                radius: width / 2
                color: Appearance.colors.colLayer2Hover
                opacity: childToggleMouse.containsMouse ? 1 : 0
            }
            MaterialSymbol {
                anchors.centerIn: parent
                text: "expand_more"
                iconSize: 22
                rotation: root.collapsed ? 0 : 180
                color: Appearance.colors.colSubtext
                Behavior on rotation {
                    animation: Appearance.animation.elementMoveFast.numberAnimation.createObject(this)
                }
            }
            MouseArea {
                id: childToggleMouse
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: root.collapsed = !root.collapsed
            }
        }

        RowLayout {
            id: controlSlot
            Layout.fillWidth: false
            spacing: root.controlSpacing
        }
    }

    Timer {
        id: flashTimer
        interval: 900
        onTriggered: root.flash = false
    }

    // Not `reveal`: that name is the fold progress property above
    function highlight() {
        root.flash = true;
        flashTimer.restart();
    }

    Component.onCompleted: {
        if (root.parentRow && root.parentRow.childRows !== undefined)
            root.parentRow.childRows = root.parentRow.childRows.concat([root]);
        if (root.searchable && root.title.length > 0)
            SettingsSearch.register(root);
    }
    Component.onDestruction: SettingsSearch.unregister(root)
}
