import QtQuick
import QtQuick.Layouts
import ri.services
import ri.modules.common
import ri.modules.common.models.quickToggles
import ri.modules.common.functions
import ri.modules.common.widgets

GroupButton {
    id: root
    
    // Info to be passed to by repeater
    required property int buttonIndex
    required property var buttonData
    required property bool expandedSize
    required property real baseCellWidth
    required property real baseCellHeight
    required property real cellSpacing
    required property int cellSize

    // Signals
    signal openMenu()

    // Declared in specific toggles
    property QuickToggleModel toggleModel
    property string name: toggleModel?.name ?? ""
    property string statusText: (toggleModel?.hasStatusText) ? (toggleModel?.statusText || (toggled ? Translation.tr("On") : Translation.tr("Off"))) : ""
    property string tooltipText: toggleModel?.tooltipText ?? ""
    property string buttonIcon: toggleModel?.icon ?? "close"
    property bool available: toggleModel?.available ?? true
    toggled: toggleModel?.toggled ?? false
    property var mainAction: toggleModel?.mainAction ?? null
    altAction: toggleModel?.hasMenu ? (() => root.openMenu()) : (toggleModel?.altAction ?? null)

    // Edit mode state
    property bool editMode: false

    // Sizing shenanigans
    readonly property real cellsWidth: root.baseCellWidth * cellSize + cellSpacing * (cellSize - 1)
    baseWidth: cellsWidth + widthSlide
    baseHeight: root.baseCellHeight
    enableImplicitWidthAnimation: !editMode && root.mouseArea.containsMouse
    enableImplicitHeightAnimation: !editMode && root.mouseArea.containsMouse
    Behavior on baseWidth {
        enabled: !widened.running
        animation: Appearance.animation.elementMove.numberAnimation.createObject(this)
    }
    Behavior on baseHeight {
        animation: Appearance.animation.elementMove.numberAnimation.createObject(this)
    }
    opacity: 0
    Component.onCompleted: {
        opacity = 1
        const from = root.movedFrom();
        if (from) {
            // The toggle was on screen a moment ago, in another row: it starts as wide as it
            // was there.
            root.widthSlide = from.width - root.cellsWidth;
            widened.restart();
            if (from.expanded !== root.expandedSize) root.slideContent(root.cellsWidth);
        }
        root.made = true;
    }
    Behavior on opacity {
        enabled: root.made || !root.movedFrom()
        animation: Appearance.animation.elementMoveFast.numberAnimation.createObject(this)
    }

    // Motion in edit mode (see `movedFrom` of AndroidQuickPanel.qml): a button the change of
    // the toggles puts elsewhere, or makes anew in another row, is drawn where it was and
    // slides to its place. The animations are started by hand: a Behavior of a button made
    // anew does not animate before the button's first layout.
    property Item quickPanel: null
    property bool made: false
    property real widthSlide: 0
    property real slideX: 0
    property real slideY: 0
    property real contentSlide: 0
    // Where the layout has the button, in the panel.
    function placeIn(axis) {
        let at = 0;
        for (let item = root; item && item !== root.quickPanel; item = item.parent)
            at += item[axis];
        return at;
    }
    readonly property real placeX: placeIn("x")
    readonly property real placeY: placeIn("y")
    onPlaceXChanged: slide()
    onPlaceYChanged: slide()
    function movedFrom() {
        return root.quickPanel?.movedFrom?.[root.buttonData.type] ?? null;
    }
    // The layout has put the button somewhere: until it is made it has no place yet.
    function slide() {
        const from = root.movedFrom();
        if (!from || !root.made) return;
        root.slideX = from.x - placeIn("x");
        root.slideY = from.y - placeIn("y");
        slid.restart();
        root.quickPanel.placed();
    }
    // A change of size in edit mode: the icon slides from the middle of the button to its
    // left edge, or back, and the text fades in.
    function slideContent(width) {
        const middle = (width - iconMouseArea.width) / 2 - root.horizontalPadding;
        root.contentSlide = root.expandedSize ? middle : -middle;
        contentSlid.restart();
    }
    onExpandedSizeChanged: {
        if (root.made && root.editMode) root.slideContent(root.width);
    }
    component Eased: NumberAnimation {
        to: 0
        duration: Appearance.animation.elementMove.duration
        easing.type: Appearance.animation.elementMove.type
        easing.bezierCurve: Appearance.animation.elementMove.bezierCurve
    }
    ParallelAnimation {
        id: slid
        Eased { target: root; property: "slideX" }
        Eased { target: root; property: "slideY" }
    }
    Eased { id: widened; target: root; property: "widthSlide" }
    Eased { id: contentSlid; target: root; property: "contentSlide" }
    transform: Translate {
        x: root.slideX
        y: root.slideY
    }
    Connections {
        target: root.quickPanel
        function onAboutToMove() {
            root.quickPanel.movedFrom[root.buttonData.type] = {
                x: root.placeIn("x") + root.slideX,
                y: root.placeIn("y") + root.slideY,
                width: root.width,
                expanded: root.expandedSize
            };
        }
    }

    enabled: available || editMode
    padding: 6
    horizontalPadding: padding
    verticalPadding: padding

    colBackground: Appearance.colors.colLayer2
    colBackgroundToggled: (altAction && expandedSize) ? Appearance.colors.colLayer2 : Appearance.colors.colPrimary
    colBackgroundToggledHover: (altAction && expandedSize) ? Appearance.colors.colLayer2Hover : Appearance.colors.colPrimaryHover
    colBackgroundToggledActive: (altAction && expandedSize) ? Appearance.colors.colLayer2Active : Appearance.colors.colPrimaryActive
    buttonRadius: toggled ? Appearance.rounding.large : height / 2
    buttonRadiusPressed: Appearance.rounding.normal
    property color colText: (toggled && !(altAction && expandedSize) && enabled) ? Appearance.colors.colOnPrimary : ColorUtils.transparentize(Appearance.colors.colOnLayer2, enabled ? 0 : 0.7)
    property color colIcon: expandedSize ? ((root.toggled) ? Appearance.colors.colOnPrimary : Appearance.colors.colOnLayer3) : colText

    onClicked: {
        if (root.expandedSize && root.altAction) root.altAction();
        else root.mainAction();
    }

    contentItem: RowLayout {
        id: contentItem
        spacing: 4
        anchors {
            centerIn: root.expandedSize ? undefined : parent
            fill: root.expandedSize ? parent : undefined
            leftMargin: root.horizontalPadding
            rightMargin: root.horizontalPadding
        }
        transform: Translate {
            x: root.contentSlide
        }

        // Icon
        MouseArea {
            id: iconMouseArea
            hoverEnabled: true
            acceptedButtons: (root.expandedSize && root.altAction) ? Qt.LeftButton : Qt.NoButton
            Layout.alignment: Qt.AlignHCenter
            Layout.fillHeight: true
            Layout.topMargin: root.verticalPadding
            Layout.bottomMargin: root.verticalPadding
            implicitHeight: iconBackground.implicitHeight
            implicitWidth: iconBackground.implicitWidth
            cursorShape: Qt.PointingHandCursor

            onClicked: root.mainAction()

            Rectangle {
                id: iconBackground
                anchors.fill: parent
                implicitWidth: height
                radius: root.radius - root.verticalPadding
                color: {
                    const baseColor = root.toggled ? Appearance.colors.colPrimary : Appearance.colors.colLayer3
                    const transparentizeAmount = (root.altAction && root.expandedSize) ? 0 : 1
                    return ColorUtils.transparentize(baseColor, transparentizeAmount)
                }

                Behavior on radius {
                    animation: Appearance.animation.elementMove.numberAnimation.createObject(this)
                }
                Behavior on color {
                    animation: Appearance.animation.elementMoveFast.colorAnimation.createObject(this)
                }

                MaterialSymbol {
                    anchors.centerIn: parent
                    fill: root.toggled ? 1 : 0
                    iconSize: root.expandedSize ? 22 : 24
                    color: root.colIcon
                    text: root.buttonIcon
                }

                // State layer
                Loader {
                    anchors.fill: parent
                    active: (root.expandedSize && root.altAction)
                    sourceComponent: Rectangle {
                        radius: iconBackground.radius
                        color: ColorUtils.transparentize(root.colIcon, iconMouseArea.containsPress ? 0.88 : iconMouseArea.containsMouse ? 0.95 : 1)
                        Behavior on color {
                            animation: Appearance.animation.elementMoveFast.colorAnimation.createObject(this)
                        }
                    }
                }
            }
        }

        // Text column for expanded size
        Loader {
            Layout.alignment: Qt.AlignVCenter
            Layout.fillWidth: true
            visible: root.expandedSize
            active: visible
            sourceComponent: Column {
                id: label
                spacing: -2
                Component.onCompleted: {
                    // Made by a change of size, with the button or in it.
                    const grown = root.made || root.movedFrom()?.expanded === false;
                    if (!grown || !root.editMode) return;
                    label.opacity = 0;
                    labelShown.start();
                }
                Eased {
                    id: labelShown
                    target: label
                    property: "opacity"
                    to: 1
                }

                StyledText {
                    anchors {
                        left: parent.left
                        right: parent.right
                    }
                    font.pixelSize: Appearance.font.pixelSize.smallie
                    font.weight: 600
                    color: root.colText
                    elide: Text.ElideRight
                    text: root.name
                }

                StyledText {
                    visible: root.statusText
                    anchors {
                        left: parent.left
                        right: parent.right
                    }
                    font {
                        pixelSize: Appearance.font.pixelSize.smaller
                        weight: 100
                    }
                    color: root.colText
                    elide: Text.ElideRight
                    text: root.statusText
                }
            }
        }
    }

    MouseArea { // Blocking MouseArea for edit interactions
        id: editModeInteraction
        visible: root.editMode
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        hoverEnabled: true
        acceptedButtons: Qt.AllButtons

        function toggleEnabled() {
            const index = root.buttonIndex;
            const toggleList = Config.options.sidebar.quickToggles.android.toggles;
            const buttonType = root.buttonData.type;
            root.quickPanel?.beforeMove();
            if (!toggleList.find(toggle => toggle.type === buttonType)) {
                toggleList.push({ type: buttonType, size: 1 });
            } else {
                toggleList.splice(index, 1);
            }
        }

        function toggleSize() {
            const index = root.buttonIndex;
            const toggleList = Config.options.sidebar.quickToggles.android.toggles;
            const buttonType = root.buttonData.type;
            if (!toggleList.find(toggle => toggle.type === buttonType)) return;
            root.quickPanel?.beforeMove();
            toggleList[index].size = 3 - toggleList[index].size; // Alternate between 1 and 2
        }

        function movePositionBy(offset) {
            const index = root.buttonIndex;
            const toggleList = Config.options.sidebar.quickToggles.android.toggles;
            const buttonType = root.buttonData.type;
            const targetIndex = index + offset;
            if (!toggleList.find(toggle => toggle.type === buttonType)) return;
            if (targetIndex < 0 || targetIndex >= toggleList.length) return;
            root.quickPanel?.beforeMove();
            const temp = toggleList[index];
            toggleList[index] = toggleList[targetIndex];
            toggleList[targetIndex] = temp;
        }

        onReleased: (event) => {
            if (event.button === Qt.LeftButton)
                toggleEnabled();
        }
        onPressed: (event) => {
            if (event.button === Qt.RightButton) toggleSize();
        }
        onPressAndHold: (event) => { // Also toggle size
            toggleSize();
        }
        onWheel: (event) => {
            const index = root.buttonIndex;
            const toggleList = Config.options.sidebar.quickToggles.android.toggles;
            const buttonType = root.buttonData.type;
            if (event.angleDelta.y < 0) { // Move to right
                movePositionBy(1);
            } else if (event.angleDelta.y > 0) { // Move to left
                movePositionBy(-1);
            }
            event.accepted = true;
        }
    }

    StyledToolTip {
        extraVisibleCondition: root.tooltipText !== ""
        text: root.tooltipText
    }
}
