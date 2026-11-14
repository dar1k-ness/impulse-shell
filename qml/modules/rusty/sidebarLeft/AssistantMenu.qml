import ri
import ri.services
import ri.modules.common
import ri.modules.common.widgets
import QtQuick
import QtQuick.Layouts
import Rustshell
import Rustshell.Io

/**
 * A menu built by the user's script (`sidebar.assistant.menu`): the script runs each time the
 * sidebar opens and prints a JSON array of {icon, label, description?, exec}; a click runs
 * `exec` through `sh -c` and closes the sidebar.
 */
Item {
    id: root
    property list<var> items: []
    property string error: ""

    function reload() {
        menuProc.running = false;
        menuProc.running = true;
    }

    Component.onCompleted: root.reload()
    Connections {
        target: GlobalStates
        function onSidebarLeftOpenChanged() {
            if (GlobalStates.sidebarLeftOpen) root.reload();
        }
    }

    Process {
        id: menuProc
        command: ["sh", "-c", Config.options.sidebar.assistant.menu]
        stdout: StdioCollector {
            onStreamFinished: {
                try {
                    const parsed = JSON.parse(text);
                    root.items = Array.isArray(parsed) ? parsed : [];
                    root.error = "";
                } catch (e) {
                    root.items = [];
                    root.error = e.message;
                }
            }
        }
    }

    StyledFlickable {
        anchors.fill: parent
        anchors.margins: 10
        contentHeight: column.implicitHeight
        clip: true

        ColumnLayout {
            id: column
            width: parent.width
            spacing: 4

            RowLayout {
                Layout.fillWidth: true
                Layout.bottomMargin: 6
                StyledText {
                    Layout.fillWidth: true
                    elide: Text.ElideRight
                    text: Config.options.sidebar.assistant.menu
                    font.family: Appearance.font.family.monospace
                    font.pixelSize: Appearance.font.pixelSize.smaller
                    color: Appearance.colors.colSubtext
                }
                RippleButton {
                    implicitWidth: 32
                    implicitHeight: 32
                    buttonRadius: Appearance.rounding.full
                    onClicked: root.reload()
                    contentItem: MaterialSymbol {
                        anchors.centerIn: parent
                        text: "refresh"
                        iconSize: 18
                        color: Appearance.colors.colSubtext
                    }
                    StyledToolTip {
                        text: Translation.tr("Run the script again")
                    }
                }
            }

            StyledText {
                visible: root.error.length > 0
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                text: Translation.tr("The script did not print a JSON array: %1").arg(root.error)
                font.pixelSize: Appearance.font.pixelSize.small
                color: Appearance.colors.colError ?? Appearance.m3colors.m3error
            }

            Repeater {
                model: root.items
                delegate: RippleButton {
                    id: entry
                    required property var modelData
                    Layout.fillWidth: true
                    implicitHeight: (entry.modelData.description?.length > 0) ? 56 : 44
                    buttonRadius: Appearance.rounding.small
                    colBackground: Appearance.colors.colLayer2
                    colBackgroundHover: Appearance.colors.colLayer2Hover
                    colRipple: Appearance.colors.colLayer2Active
                    onClicked: {
                        if (entry.modelData.exec?.length > 0)
                            Rustshell.execDetached(["sh", "-c", entry.modelData.exec]);
                        GlobalStates.sidebarLeftOpen = false;
                    }
                    contentItem: RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 12
                        anchors.rightMargin: 12
                        spacing: 12
                        MaterialSymbol {
                            text: entry.modelData.icon ?? "terminal"
                            iconSize: 22
                            color: Appearance.colors.colPrimary
                        }
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 0
                            StyledText {
                                Layout.fillWidth: true
                                elide: Text.ElideRight
                                text: entry.modelData.label ?? entry.modelData.exec ?? ""
                                font.pixelSize: Appearance.font.pixelSize.normal
                                color: Appearance.colors.colOnLayer2
                            }
                            StyledText {
                                Layout.fillWidth: true
                                visible: text.length > 0
                                elide: Text.ElideRight
                                text: entry.modelData.description ?? ""
                                font.pixelSize: Appearance.font.pixelSize.smaller
                                color: Appearance.colors.colSubtext
                            }
                        }
                    }
                }
            }
        }
    }
}
