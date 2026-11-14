import ri.services
import ri.modules.common
import ri.modules.common.widgets
import ri.modules.common.functions
import QtQuick
import QtQuick.Layouts
import Rustshell

/**
 * Shown while neither `sidebar.assistant.command` nor `sidebar.assistant.menu` is set:
 * how the panel talks to an agent and how to replace it with a script-built menu.
 */
Item {
    id: root
    readonly property string configPath: Directories.shellConfigPath

    StyledFlickable {
        anchors.fill: parent
        anchors.margins: 10
        contentHeight: column.implicitHeight
        clip: true

        ColumnLayout {
            id: column
            width: parent.width
            spacing: 10

            StyledText {
                Layout.fillWidth: true
                Layout.topMargin: 6
                Layout.leftMargin: 4
                wrapMode: Text.Wrap
                text: Translation.tr("This panel is set in %1, under sidebar.assistant. Set one of:").arg(root.configPath)
                font.pixelSize: Appearance.font.pixelSize.small
                color: Appearance.colors.colSubtext
            }

            GuideCard {
                icon: "forum"
                title: "command"
                text: Translation.tr("Runs an agent and chats with it over the Agent Client Protocol (JSON-RPC on stdin/stdout). The command is a list of arguments, run without a shell.")
                code: '"sidebar": {\n  "assistant": {\n    "command": ["gemini", "--experimental-acp"]\n  }\n}'
                note: Translation.tr("Gemini CLI: gemini --experimental-acp\nClaude Code: claude-code-acp (Zed adapter)")
            }

            GuideCard {
                icon: "list"
                title: "menu"
                text: Translation.tr("Runs with sh -c each time the sidebar opens and prints a JSON array. Each entry has icon (a Material Symbols name), label, an optional description, and exec, which runs with sh -c on click.")
                code: '"sidebar": {\n  "assistant": {\n    "menu": "~/.config/rusty-impulse/menu.sh"\n  }\n}'
                code2: '#!/bin/sh\necho \'[\n  {"icon": "terminal", "label": "Terminal",\n   "exec": "kitty"},\n  {"icon": "update", "label": "Update",\n   "description": "yay -Syu",\n   "exec": "kitty -e yay -Syu"}\n]\''
                note: Translation.tr("If both are set, the menu is shown.")
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.topMargin: 4
                spacing: 8
                RippleButtonWithIcon {
                    Layout.fillWidth: true
                    materialIcon: "edit"
                    mainText: "config.json"
                    onClicked: Qt.openUrlExternally(`file://${root.configPath}`)
                }
                RippleButtonWithIcon {
                    Layout.fillWidth: true
                    materialIcon: "description"
                    mainText: Translation.tr("ACP specification")
                    onClicked: Qt.openUrlExternally("https://agentclientprotocol.com/")
                }
            }
        }
    }

    component GuideCard: Rectangle {
        id: card
        property string icon
        property string title
        property string text
        property string code
        property string code2: ""
        property string note: ""
        Layout.fillWidth: true
        implicitHeight: cardColumn.implicitHeight + 28
        radius: Appearance.rounding.normal
        color: Appearance.colors.colLayer2

        ColumnLayout {
            id: cardColumn
            anchors {
                fill: parent
                margins: 14
            }
            spacing: 8
            RowLayout {
                spacing: 8
                MaterialSymbol {
                    text: card.icon
                    iconSize: 20
                    color: Appearance.colors.colPrimary
                }
                StyledText {
                    text: card.title
                    font.pixelSize: Appearance.font.pixelSize.normal
                    color: Appearance.colors.colOnLayer2
                }
            }
            StyledText {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                text: card.text
                font.pixelSize: Appearance.font.pixelSize.small
                color: Appearance.colors.colSubtext
            }
            CodeBlock { code: card.code }
            CodeBlock {
                visible: card.code2.length > 0
                code: card.code2
            }
            StyledText {
                visible: card.note.length > 0
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                text: card.note
                font.pixelSize: Appearance.font.pixelSize.smaller
                color: Appearance.colors.colSubtext
            }
        }
    }

    component CodeBlock: Rectangle {
        property string code
        Layout.fillWidth: true
        implicitHeight: codeText.implicitHeight + 20
        radius: Appearance.rounding.small
        color: Appearance.colors.colLayer1
        StyledText {
            id: codeText
            anchors {
                fill: parent
                margins: 10
            }
            text: parent.code
            wrapMode: Text.WrapAnywhere
            font.family: Appearance.font.family.monospace
            font.pixelSize: Appearance.font.pixelSize.smaller
            color: Appearance.colors.colOnLayer1
        }
    }
}
