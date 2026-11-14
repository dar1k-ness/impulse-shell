import ri
import ri.services
import ri.modules.common
import ri.modules.common.widgets
import ri.modules.common.functions
import ri.modules.rusty.sidebarLeft.assistant
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Qt5Compat.GraphicalEffects
import Rustshell

Item {
    id: root
    required property var session
    readonly property string agentName: root.session.agentName || root.session.program

    property real padding: 4
    property var inputField: messageInputField
    property string pickerKind: "" // "", "model" or "mode"
    readonly property var pickerItems: {
        const items = root.pickerKind === "model" ? root.session.models : root.pickerKind === "mode" ? root.session.modes : [];
        const query = pickerSearch.text.trim().toLowerCase();
        return query.length === 0 ? items : items.filter(i => `${i.name} ${i.value} ${i.description}`.toLowerCase().includes(query));
    }
    readonly property var modeIcons: ({ "code": "code", "accept-edits": "code", "plan": "checklist", "architect": "architecture", "bypass": "gpp_maybe", "bypassPermissions": "gpp_maybe", "ask": "help", "default": "shield" })

    function openPicker(kind) {
        root.pickerKind = root.pickerKind === kind ? "" : kind;
        pickerSearch.text = "";
        if (root.pickerKind === "model") pickerSearch.forceActiveFocus();
    }

    onFocusChanged: focus => {
        if (focus) root.inputField.forceActiveFocus();
    }

    Component.onCompleted: root.session.start()

    Keys.onPressed: event => {
        messageInputField.forceActiveFocus();
        if (event.modifiers === Qt.NoModifier) {
            if (event.key === Qt.Key_PageUp) {
                messageListView.contentY = Math.max(0, messageListView.contentY - messageListView.height / 2);
                event.accepted = true;
            } else if (event.key === Qt.Key_PageDown) {
                messageListView.contentY = Math.min(messageListView.contentHeight - messageListView.height / 2, messageListView.contentY + messageListView.height / 2);
                event.accepted = true;
            }
        }
        if ((event.modifiers & Qt.ControlModifier) && (event.modifiers & Qt.ShiftModifier) && event.key === Qt.Key_O) {
            root.session.clear();
        }
    }

    function handleInput(inputText) {
        const text = inputText.trim();
        if (text.length === 0) return;
        root.pickerKind = "";
        root.session.sendPrompt(text);
        messageListView.positionViewAtEnd();
    }

    component StatusItem: MouseArea {
        id: statusItem
        property string icon
        property string statusText
        property string description
        hoverEnabled: true
        implicitHeight: statusItemRowLayout.implicitHeight
        implicitWidth: statusItemRowLayout.implicitWidth

        RowLayout {
            id: statusItemRowLayout
            spacing: 0
            MaterialSymbol {
                text: statusItem.icon
                iconSize: Appearance.font.pixelSize.huge
                color: Appearance.colors.colSubtext
            }
            StyledText {
                font.pixelSize: Appearance.font.pixelSize.small
                text: statusItem.statusText
                color: Appearance.colors.colSubtext
                animateChange: true
            }
        }

        StyledToolTip {
            text: statusItem.description
            extraVisibleCondition: false
            alternativeVisibleCondition: statusItem.containsMouse
        }
    }

    component StatusSeparator: Rectangle {
        implicitWidth: 4
        implicitHeight: 4
        radius: implicitWidth / 2
        color: Appearance.colors.colOutlineVariant
    }

    ColumnLayout {
        anchors {
            fill: parent
            margins: root.padding
        }
        spacing: root.padding

        Item { // Messages
            Layout.fillWidth: true
            Layout.fillHeight: true
            layer.enabled: true
            layer.effect: OpacityMask {
                maskSource: Rectangle {
                    width: messageListView.width
                    height: messageListView.height
                    radius: Appearance.rounding.small
                }
            }

            StyledRectangularShadow {
                z: 1
                target: statusBg
                opacity: messageListView.atYBeginning ? 0 : 1
                visible: opacity > 0
            }
            Rectangle {
                id: statusBg
                z: 2
                anchors {
                    horizontalCenter: parent.horizontalCenter
                    top: parent.top
                    topMargin: 4
                }
                implicitWidth: statusRowLayout.implicitWidth + 10 * 2
                implicitHeight: Math.max(statusRowLayout.implicitHeight, 38)
                radius: Appearance.rounding.normal - root.padding
                color: messageListView.atYBeginning ? Appearance.colors.colLayer2 : Appearance.colors.colLayer2Base

                RowLayout {
                    id: statusRowLayout
                    anchors.centerIn: parent
                    spacing: 10

                    StatusItem {
                        icon: root.session.waitingForExit ? "hourglass_top" : root.session.authRequired ? "lock" : root.session.sessionId.length > 0 ? "link" : root.session.running ? "sync" : "link_off"
                        statusText: root.agentName
                        description: root.session.waitingForExit ? Translation.tr("Waiting for %1 to exit").arg(root.session.startedProgram) :
                            root.session.authRequired ? Translation.tr("The agent needs you to sign in") :
                            root.session.sessionId.length > 0 ? root.session.command.join(" ") :
                            root.session.running ? Translation.tr("Connecting...") : Translation.tr("Not running")
                    }
                    StatusSeparator {
                        visible: root.session.modes.length > 0
                    }
                    StatusItem {
                        visible: root.session.modes.length > 0
                        icon: "shield"
                        statusText: root.session.currentModeName
                        description: Translation.tr("Session mode")
                    }
                    StatusSeparator {
                        visible: root.session.busy
                    }
                    StatusItem {
                        visible: root.session.busy
                        icon: "stop_circle"
                        statusText: Translation.tr("Stop")
                        description: Translation.tr("Stop the response")
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.session.cancel()
                    }
                }
            }

            ScrollEdgeFade {
                z: 1
                target: messageListView
                vertical: true
            }

            StyledListView {
                id: messageListView
                z: 0
                anchors.fill: parent
                spacing: 10
                popin: false
                topMargin: statusBg.implicitHeight + statusBg.anchors.topMargin * 2
                touchpadScrollFactor: Config.options.interactions.scrolling.touchpadScrollFactor * 1.4
                mouseScrollFactor: Config.options.interactions.scrolling.mouseScrollFactor * 1.4
                add: null

                onCountChanged: if (atYEnd || root.session.busy) Qt.callLater(positionViewAtEnd)

                model: ScriptModel {
                    values: root.session.messages
                }
                delegate: AssistantMessage {
                    required property var modelData
                    messageData: modelData
                    session: root.session
                }
            }

            PagePlaceholder {
                z: 2
                shown: root.session.messages.length === 0
                icon: "forum"
                title: root.agentName
                description: root.session.authRequired ? Translation.tr("Sign in with a button below to start") : Translation.tr("Talking to %1 over the Agent Client Protocol").arg(root.session.command.join(" "))
                descriptionHorizontalAlignment: Text.AlignHCenter
                shape: MaterialShape.Shape.PixelCircle
            }

            ScrollToBottomButton {
                z: 3
                target: messageListView
            }
        }

        Rectangle { // Model / mode picker
            id: picker
            visible: root.pickerKind.length > 0
            Layout.fillWidth: true
            Layout.preferredHeight: Math.min(root.height * 0.55, pickerColumn.implicitHeight + 8 * 2)
            radius: Appearance.rounding.normal - root.padding
            color: Appearance.colors.colLayer2

            ColumnLayout {
                id: pickerColumn
                anchors.fill: parent
                anchors.margins: 8
                spacing: 6

                RowLayout {
                    Layout.fillWidth: true
                    StyledText {
                        Layout.fillWidth: true
                        Layout.leftMargin: 4
                        font.pixelSize: Appearance.font.pixelSize.normal
                        color: Appearance.colors.colOnLayer2
                        text: root.pickerKind === "model" ? Translation.tr("Model") : Translation.tr("Mode")
                    }
                    RippleButton {
                        implicitWidth: 28
                        implicitHeight: 28
                        buttonRadius: Appearance.rounding.full
                        onClicked: root.pickerKind = ""
                        contentItem: MaterialSymbol {
                            horizontalAlignment: Text.AlignHCenter
                            iconSize: Appearance.font.pixelSize.larger
                            text: "close"
                        }
                    }
                }

                Rectangle {
                    visible: root.pickerKind === "model"
                    Layout.fillWidth: true
                    implicitHeight: 34
                    radius: Appearance.rounding.small
                    color: Appearance.colors.colLayer1
                    StyledTextInput {
                        id: pickerSearch
                        anchors.fill: parent
                        anchors.leftMargin: 10
                        anchors.rightMargin: 10
                        verticalAlignment: TextInput.AlignVCenter
                        clip: true
                        color: Appearance.colors.colOnLayer1
                        font.pixelSize: Appearance.font.pixelSize.small
                        Keys.onEscapePressed: root.pickerKind = ""
                        Keys.onReturnPressed: if (root.pickerItems.length > 0) {
                            root.session.setModel(root.pickerItems[0].value);
                            root.pickerKind = "";
                        }
                        StyledText {
                            anchors.verticalCenter: parent.verticalCenter
                            visible: pickerSearch.text.length === 0
                            font.pixelSize: Appearance.font.pixelSize.small
                            color: Appearance.colors.colSubtext
                            text: Translation.tr("Search models...")
                        }
                    }
                }

                StyledText {
                    visible: root.pickerItems.length === 0
                    Layout.leftMargin: 4
                    font.pixelSize: Appearance.font.pixelSize.small
                    color: Appearance.colors.colSubtext
                    text: Translation.tr("Nothing found")
                }

                StyledListView {
                    id: pickerList
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    implicitHeight: contentHeight
                    clip: true
                    spacing: 2
                    popin: false
                    model: ScriptModel {
                        values: root.pickerItems
                    }
                    delegate: RippleButton {
                        id: pickerItem
                        required property var modelData
                        readonly property bool selected: root.pickerKind === "model" ? modelData.value === root.session.currentModel : modelData.value === root.session.currentMode
                        width: pickerList.width
                        implicitHeight: pickerItemRow.implicitHeight + 8 * 2
                        buttonRadius: Appearance.rounding.small
                        toggled: selected
                        colBackground: Appearance.colors.colLayer2
                        colBackgroundToggled: Appearance.colors.colSecondaryContainer
                        colBackgroundToggledHover: Appearance.colors.colSecondaryContainerHover
                        onClicked: {
                            if (root.pickerKind === "model") root.session.setModel(modelData.value);
                            else root.session.setMode(modelData.value);
                            root.pickerKind = "";
                        }
                        contentItem: RowLayout {
                            id: pickerItemRow
                            spacing: 10
                            MaterialSymbol {
                                Layout.leftMargin: 6
                                iconSize: Appearance.font.pixelSize.larger
                                color: pickerItem.selected ? Appearance.m3colors.m3onSecondaryContainer : Appearance.colors.colSubtext
                                text: root.pickerKind === "mode" ? (root.modeIcons[pickerItem.modelData.value] ?? "tune") : (pickerItem.selected ? "check" : "neurology")
                            }
                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 0
                                StyledText {
                                    Layout.fillWidth: true
                                    elide: Text.ElideRight
                                    font.pixelSize: Appearance.font.pixelSize.small
                                    color: pickerItem.selected ? Appearance.m3colors.m3onSecondaryContainer : Appearance.colors.colOnLayer2
                                    text: pickerItem.modelData.name
                                }
                                StyledText {
                                    visible: text.length > 0 && text !== pickerItem.modelData.name
                                    Layout.fillWidth: true
                                    elide: Text.ElideRight
                                    font.pixelSize: Appearance.font.pixelSize.smaller
                                    color: Appearance.colors.colSubtext
                                    text: pickerItem.modelData.description
                                }
                            }
                        }
                    }
                }
            }
        }

        Rectangle { // Sign-in banner: the agent's own auth methods (ACP `authenticate`)
            visible: root.session.authRequired
            Layout.fillWidth: true
            implicitHeight: authColumn.implicitHeight + 12 * 2
            radius: Appearance.rounding.normal - root.padding
            color: Appearance.colors.colSecondaryContainer

            ColumnLayout {
                id: authColumn
                anchors {
                    left: parent.left
                    right: parent.right
                    verticalCenter: parent.verticalCenter
                    margins: 12
                }
                spacing: 8
                StyledText {
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    color: Appearance.m3colors.m3onSecondaryContainer
                    font.pixelSize: Appearance.font.pixelSize.small
                    text: root.session.authMethods.length > 0 ? Translation.tr("%1 needs you to sign in").arg(root.agentName)
                        : Translation.tr("%1 needs you to sign in. Run it once in a terminal to do that, then start a new chat.").arg(root.agentName)
                }
                Flow {
                    Layout.fillWidth: true
                    spacing: 6
                    Repeater {
                        model: root.session.authMethods
                        delegate: RippleButton {
                            id: authButton
                            required property var modelData
                            implicitHeight: 34
                            implicitWidth: authText.implicitWidth + 20 * 2
                            buttonRadius: Appearance.rounding.full
                            colBackground: Appearance.colors.colPrimary
                            colBackgroundHover: Appearance.colors.colPrimaryHover
                            onClicked: root.session.authenticate(authButton.modelData.id)
                            contentItem: StyledText {
                                id: authText
                                horizontalAlignment: Text.AlignHCenter
                                color: Appearance.m3colors.m3onPrimary
                                text: authButton.modelData.name ?? authButton.modelData.id
                            }
                            StyledToolTip {
                                text: authButton.modelData.description ?? ""
                                extraVisibleCondition: (authButton.modelData.description ?? "").length > 0
                            }
                        }
                    }
                }
            }
        }

        Rectangle { // Input area
            id: inputWrapper
            Layout.fillWidth: true
            radius: Appearance.rounding.normal - root.padding
            color: Appearance.colors.colLayer2
            implicitHeight: Math.max(inputFieldRowLayout.implicitHeight + commandButtonsRow.implicitHeight + 5 * 3, 45)
            clip: true

            Behavior on implicitHeight {
                animation: Appearance.animation.elementMove.numberAnimation.createObject(this)
            }

            RowLayout { // Input field and send button
                id: inputFieldRowLayout
                anchors {
                    bottom: commandButtonsRow.top
                    left: parent.left
                    right: parent.right
                    bottomMargin: 5
                }
                spacing: 0

                ScrollView {
                    id: inputScrollView
                    Layout.fillWidth: true
                    // A hint of several lines is read whole: the field is as high as they are.
                    Layout.preferredHeight: Math.min(root.height * 3 / 5, Math.max(messageInputField.height,
                        inputHint.visible && inputHint.lineCount > 1
                            ? inputHint.height + messageInputField.topPadding + messageInputField.bottomPadding : 0))
                    clip: true
                    ScrollBar.vertical.policy: ScrollBar.AsNeeded

                    StyledTextArea {
                        id: messageInputField
                        anchors.fill: parent
                        // As wide as its own placeholder would make it (TextArea.qml), the hint
                        // in its place: a line no wider than the hint is not broken.
                        implicitWidth: Math.max(contentWidth, Math.min(inputHint.implicitWidth,
                            inputScrollView.availableWidth - leftPadding - rightPadding)) + leftPadding + rightPadding
                        wrapMode: TextArea.Wrap
                        padding: 10
                        color: activeFocus ? Appearance.m3colors.m3onSurface : Appearance.m3colors.m3onSurfaceVariant
                        background: null

                        // The field's own placeholder is cut at the field's height; this one wraps.
                        // It is as wide as the view; the field is as wide as the hint is long.
                        StyledText {
                            id: inputHint
                            visible: messageInputField.text.length === 0 && !messageInputField.preeditText
                            x: messageInputField.leftPadding
                            y: messageInputField.topPadding
                            width: inputScrollView.availableWidth - messageInputField.leftPadding - messageInputField.rightPadding
                            wrapMode: Text.Wrap
                            font: messageInputField.font
                            color: messageInputField.placeholderTextColor
                            text: Translation.tr("Message %1...").arg(root.agentName)
                        }

                        Keys.onPressed: event => {
                            if (event.key === Qt.Key_Enter || event.key === Qt.Key_Return) {
                                if (event.modifiers & Qt.ShiftModifier) {
                                    messageInputField.insert(messageInputField.cursorPosition, "\n");
                                } else {
                                    const inputText = messageInputField.text;
                                    messageInputField.clear();
                                    root.handleInput(inputText);
                                }
                                event.accepted = true;
                            } else if (event.key === Qt.Key_Escape && root.session.busy) {
                                root.session.cancel();
                                event.accepted = true;
                            }
                        }
                    }
                }

                RippleButton { // Send / stop button
                    id: sendButton
                    Layout.alignment: Qt.AlignBottom
                    Layout.rightMargin: 5
                    implicitWidth: 40
                    implicitHeight: 40
                    buttonRadius: Appearance.rounding.small
                    enabled: messageInputField.text.length > 0 || root.session.busy
                    toggled: enabled

                    onClicked: {
                        if (root.session.busy && messageInputField.text.length === 0) {
                            root.session.cancel();
                            return;
                        }
                        const inputText = messageInputField.text;
                        messageInputField.clear();
                        root.handleInput(inputText);
                    }

                    contentItem: MaterialSymbol {
                        anchors.centerIn: parent
                        horizontalAlignment: Text.AlignHCenter
                        iconSize: 22
                        color: sendButton.enabled ? Appearance.m3colors.m3onPrimary : Appearance.colors.colOnLayer2Disabled
                        text: (root.session.busy && messageInputField.text.length === 0) ? "stop" : "arrow_upward"
                    }
                }
            }

            RowLayout { // Controls
                id: commandButtonsRow
                anchors {
                    left: parent.left
                    right: parent.right
                    bottom: parent.bottom
                    bottomMargin: 5
                    leftMargin: 10
                    rightMargin: 5
                }
                spacing: 4

                AssistantChip {
                    visible: root.session.models.length > 0
                    Layout.maximumWidth: 150
                    chipIcon: "neurology"
                    chipLabel: root.session.currentModelName || Translation.tr("Model")
                    toggled: root.pickerKind === "model"
                    tooltip: Translation.tr("Choose a model")
                    onClicked: root.openPicker("model")
                }

                AssistantChip {
                    visible: root.session.modes.length > 0
                    chipIcon: root.modeIcons[root.session.currentMode] ?? "shield"
                    chipLabel: root.session.currentModeName || Translation.tr("Mode")
                    toggled: root.pickerKind === "mode"
                    tooltip: Translation.tr("Session mode")
                    onClicked: root.openPicker("mode")
                }

                Item {
                    Layout.fillWidth: true
                }

                AssistantChip {
                    chipIcon: "add_comment"
                    tooltip: Translation.tr("New chat")
                    onClicked: root.session.clear()
                }
            }
        }
    }
}
