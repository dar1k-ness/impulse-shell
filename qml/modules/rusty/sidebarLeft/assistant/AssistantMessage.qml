import ri.services
import ri.modules.common
import ri.modules.common.widgets
import ri.modules.common.functions
import ri.modules.rusty.sidebarLeft.aiChat
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Rustshell

Rectangle {
    id: root
    property var messageData
    property var session

    readonly property string role: messageData?.role ?? ""
    readonly property bool isChat: role === "user" || role === "assistant" || role === "interface" || role === "plan"
    property bool renderMarkdown: true
    property bool expanded: role === "permission"

    property real messagePadding: 7
    property list<var> messageBlocks: isChat ? StringUtils.splitMarkdownBlocks(messageData?.content ?? "") : []

    readonly property var kindIcons: ({
        "read": "description",
        "edit": "edit_note",
        "delete": "delete",
        "move": "drive_file_move",
        "search": "search",
        "execute": "terminal",
        "think": "psychology",
        "fetch": "language",
        "switch_mode": "swap_horiz",
        "other": "build"
    })
    readonly property var statusIcons: ({
        "pending": "schedule",
        "in_progress": "progress_activity",
        "completed": "check_circle",
        "failed": "error",
        "cancelled": "cancel"
    })

    anchors.left: parent?.left
    anchors.right: parent?.right
    implicitHeight: columnLayout.implicitHeight + messagePadding * 2
    radius: Appearance.rounding.normal
    color: role === "permission" ? Appearance.colors.colSecondaryContainer : Appearance.colors.colLayer1

    ColumnLayout {
        id: columnLayout
        anchors {
            left: parent.left
            right: parent.right
            top: parent.top
            margins: root.messagePadding
        }
        spacing: 3

        // ---- Header for chat-like messages ----
        Rectangle {
            visible: root.isChat
            Layout.fillWidth: true
            implicitHeight: headerRow.implicitHeight + 4 * 2
            color: Appearance.colors.colSecondaryContainer
            radius: Appearance.rounding.small

            RowLayout {
                id: headerRow
                anchors {
                    fill: parent
                    margins: 4
                    leftMargin: 10
                }
                spacing: 12

                MaterialSymbol {
                    iconSize: Appearance.font.pixelSize.larger
                    color: Appearance.m3colors.m3onSecondaryContainer
                    text: root.role === "user" ? "person" : root.role === "assistant" ? "neurology" : root.role === "plan" ? "checklist" : "settings"
                }
                StyledText {
                    Layout.fillWidth: true
                    elide: Text.ElideRight
                    color: Appearance.m3colors.m3onSecondaryContainer
                    // A prompt sent while an agent asked to end is waited for says where it is.
                    text: root.role === "user" ? [SystemInfo.username || "You",
                            root.messageData?.queued && root.session?.waitingForExit ? Translation.tr("In the queue") : ""].filter(t => t.length > 0).join(" · ") :
                        root.role === "assistant" ? [root.messageData?.agent, root.messageData?.model].filter(t => t?.length > 0).join(" · ") :
                        root.role === "plan" ? Translation.tr("Plan") : Translation.tr("Interface")
                }
                ButtonGroup {
                    spacing: 5
                    AiMessageControlButton {
                        id: copyButton
                        implicitHeight: 30
                        buttonIcon: activated ? "inventory" : "content_copy"
                        onClicked: {
                            Rustshell.clipboardText = root.messageData?.content ?? "";
                            copyButton.activated = true;
                            copyTimer.restart();
                        }
                        Timer {
                            id: copyTimer
                            interval: 1500
                            onTriggered: copyButton.activated = false
                        }
                        StyledToolTip {
                            text: Translation.tr("Copy")
                        }
                    }
                    AiMessageControlButton {
                        implicitHeight: 30
                        activated: !root.renderMarkdown
                        buttonIcon: "code"
                        onClicked: root.renderMarkdown = !root.renderMarkdown
                        StyledToolTip {
                            text: Translation.tr("View Markdown source")
                        }
                    }
                }
            }
        }

        // ---- Chat content ----
        ColumnLayout {
            visible: root.isChat
            Layout.fillWidth: true
            spacing: 0

            FadeLoader {
                Layout.alignment: Qt.AlignHCenter
                shown: root.isChat && root.messageBlocks.length < 1 && !(root.messageData?.done ?? true)
                sourceComponent: MaterialLoadingIndicator {
                    loading: true
                }
            }
            Repeater {
                model: ScriptModel {
                    values: root.messageBlocks
                }
                delegate: DelegateChooser {
                    role: "type"
                    DelegateChoice { roleValue: "code"; MessageCodeBlock {
                        renderMarkdown: root.renderMarkdown
                        segmentContent: modelData.content
                        segmentLang: modelData.lang
                        messageData: root.messageData
                    } }
                    DelegateChoice { roleValue: "think"; MessageThinkBlock {
                        renderMarkdown: root.renderMarkdown
                        segmentContent: modelData.content
                        messageData: root.messageData
                        done: root.messageData?.done ?? false
                        completed: modelData.completed ?? false
                    } }
                    DelegateChoice { roleValue: "text"; MessageTextBlock {
                        renderMarkdown: root.renderMarkdown
                        segmentContent: modelData.content
                        messageData: root.messageData
                        done: root.messageData?.done ?? false
                        forceDisableChunkSplitting: root.messageData?.content.includes("```") ?? true
                    } }
                }
            }
        }

        // ---- Thinking ----
        Loader {
            active: root.role === "thought"
            visible: active
            Layout.fillWidth: true
            sourceComponent: MessageThinkBlock {
                segmentContent: root.messageData?.content ?? ""
                messageData: root.messageData
                done: root.messageData?.done ?? false
                completed: root.messageData?.done ?? false
            }
        }

        // ---- The agent asked to end that has not exited, and the button that kills it ----
        RowLayout {
            visible: root.role === "ending"
            Layout.fillWidth: true
            Layout.leftMargin: 4
            spacing: 8

            MaterialSymbol {
                iconSize: Appearance.font.pixelSize.larger
                color: Appearance.colors.colSubtext
                text: "hourglass_top"
            }
            StyledText {
                Layout.fillWidth: true
                // The program's name is read whole, however long.
                wrapMode: Text.Wrap
                font.pixelSize: Appearance.font.pixelSize.small
                color: Appearance.colors.colOnLayer1
                text: Translation.tr("Waiting for %1 to exit").arg(root.messageData?.content ?? "")
            }
            RippleButton {
                implicitHeight: 32
                implicitWidth: endText.implicitWidth + 16 * 2
                buttonRadius: Appearance.rounding.small
                colBackground: Appearance.colors.colLayer2
                colBackgroundHover: Appearance.colors.colLayer2Hover
                onClicked: root.session.kill()
                contentItem: StyledText {
                    id: endText
                    horizontalAlignment: Text.AlignHCenter
                    font.pixelSize: Appearance.font.pixelSize.small
                    color: Appearance.colors.colOnLayer2
                    text: Translation.tr("End now")
                }
            }
        }

        // ---- Tool call / permission header ----
        MouseArea {
            visible: root.role === "tool" || root.role === "permission"
            Layout.fillWidth: true
            implicitHeight: toolRow.implicitHeight + 6
            cursorShape: Qt.PointingHandCursor
            onClicked: root.expanded = !root.expanded

            RowLayout {
                id: toolRow
                anchors {
                    left: parent.left
                    right: parent.right
                    verticalCenter: parent.verticalCenter
                    leftMargin: 4
                    rightMargin: 4
                }
                spacing: 8

                MaterialSymbol {
                    iconSize: Appearance.font.pixelSize.larger
                    color: root.role === "permission" ? Appearance.m3colors.m3onSecondaryContainer : Appearance.colors.colSubtext
                    text: root.role === "permission" ? "shield_question" : (root.kindIcons[root.messageData?.kind] ?? "build")
                }
                StyledText {
                    Layout.fillWidth: true
                    elide: Text.ElideRight
                    font.pixelSize: Appearance.font.pixelSize.small
                    color: root.role === "permission" ? Appearance.m3colors.m3onSecondaryContainer : Appearance.colors.colOnLayer1
                    text: root.messageData?.title || root.messageData?.kind || ""
                }
                MaterialSymbol {
                    id: statusIcon
                    visible: root.role === "tool"
                    iconSize: Appearance.font.pixelSize.normal
                    color: root.messageData?.status === "failed" ? Appearance.m3colors.m3error : Appearance.colors.colSubtext
                    text: root.statusIcons[root.messageData?.status] ?? "schedule"
                    RotationAnimation on rotation {
                        running: root.messageData?.status === "in_progress"
                        from: 0
                        to: 360
                        duration: 1200
                        loops: Animation.Infinite
                        onStopped: statusIcon.rotation = 0
                    }
                }
                MaterialSymbol {
                    visible: (root.messageData?.detail?.length ?? 0) > 0 || (root.messageData?.content?.length ?? 0) > 0
                    iconSize: Appearance.font.pixelSize.normal
                    color: Appearance.colors.colSubtext
                    text: root.expanded ? "expand_less" : "expand_more"
                }
            }
        }

        // ---- Tool details ----
        Rectangle {
            visible: (root.role === "tool" || root.role === "permission") && root.expanded && detailText.text.length > 0
            Layout.fillWidth: true
            implicitHeight: detailText.implicitHeight + 8 * 2
            radius: Appearance.rounding.small
            color: Appearance.colors.colLayer2

            StyledText {
                id: detailText
                anchors {
                    left: parent.left
                    right: parent.right
                    top: parent.top
                    margins: 8
                }
                wrapMode: Text.WrapAnywhere
                textFormat: Text.PlainText
                font.family: Appearance.font.family.monospace
                font.pixelSize: Appearance.font.pixelSize.smaller
                color: Appearance.colors.colOnLayer2
                text: {
                    const parts = [root.messageData?.detail ?? "", root.messageData?.content ?? ""].filter(p => p.length > 0);
                    const joined = parts.join("\n\n");
                    return joined.length > 4000 ? joined.slice(0, 4000) + "\n…" : joined;
                }
            }
        }

        // ---- Permission answer buttons ----
        Flow {
            visible: root.role === "permission" && !(root.messageData?.done ?? true)
            Layout.fillWidth: true
            spacing: 5
            Repeater {
                model: root.role === "permission" ? (root.messageData?.options ?? []) : []
                delegate: RippleButton {
                    required property var modelData
                    implicitHeight: 32
                    implicitWidth: optionText.implicitWidth + 16 * 2
                    buttonRadius: Appearance.rounding.small
                    colBackground: modelData.kind?.startsWith("allow") ? Appearance.colors.colPrimary : Appearance.colors.colLayer2
                    colBackgroundHover: modelData.kind?.startsWith("allow") ? Appearance.colors.colPrimaryHover : Appearance.colors.colLayer2Hover
                    onClicked: root.session.answerPermission(root.messageData, modelData.optionId)
                    contentItem: StyledText {
                        id: optionText
                        horizontalAlignment: Text.AlignHCenter
                        font.pixelSize: Appearance.font.pixelSize.small
                        color: modelData.kind?.startsWith("allow") ? Appearance.m3colors.m3onPrimary : Appearance.colors.colOnLayer2
                        text: modelData.name
                    }
                }
            }
        }
        StyledText {
            visible: root.role === "permission" && (root.messageData?.done ?? false)
            Layout.leftMargin: 4
            font.pixelSize: Appearance.font.pixelSize.small
            color: Appearance.m3colors.m3onSecondaryContainer
            text: Translation.tr("Answer: %1").arg(root.messageData?.resolved ?? "")
        }
    }
}
