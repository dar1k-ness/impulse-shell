import QtQuick
import QtQuick.Layouts
import Rustshell
import Rustshell.Widgets
import ri.services
import ri.modules.common
import ri.modules.common.widgets

SettingsPage {
    id: root

    // impulse-shell repository: the only place to change it.
    readonly property string repoUrl: "https://github.com/dar1k-ness/impulse-shell"

    component CardButton: RippleButtonWithIcon {
        buttonRadius: Appearance.rounding.full
        colBackground: Appearance.colors.colLayer3
        colBackgroundHover: Appearance.colors.colLayer3Hover
    }

    component LinkButton: CardButton {
        property string url: ""
        onClicked: Qt.openUrlExternally(url)
    }

    // Icon, title and subtitle of an info card. No default property here: the card's children
    // are placed by the caller, next to it.
    component CardHeader: RowLayout {
        id: header
        property string title
        property string subtitle
        property string icon: ""
        property string iconSource: ""
        Layout.fillWidth: true
        spacing: 16
        Rectangle {
            implicitWidth: 56
            implicitHeight: 56
            radius: Appearance.rounding.small
            color: Appearance.colors.colPrimaryContainer
            MaterialSymbol {
                visible: header.iconSource.length === 0
                anchors.centerIn: parent
                text: header.icon
                iconSize: 30
                fill: 1
                color: Appearance.colors.colOnPrimaryContainer
            }
            IconImage {
                visible: header.iconSource.length > 0
                anchors.centerIn: parent
                implicitSize: 36
                source: header.iconSource
            }
        }
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 1
            StyledText {
                Layout.fillWidth: true
                text: header.title
                font.pixelSize: Appearance.font.pixelSize.large
                font.weight: Font.Medium
                color: Appearance.colors.colOnLayer2
                elide: Text.ElideRight
            }
            StyledText {
                Layout.fillWidth: true
                text: header.subtitle
                font.pixelSize: Appearance.font.pixelSize.small
                color: Appearance.colors.colSubtext
                elide: Text.ElideRight
            }
        }
    }

    SettingsGroup {
        title: Translation.tr("Shell")
        SettingsCard {
            RowLayout {
                Layout.fillWidth: true
                spacing: 8
                CardHeader {
                    title: "Impulse"
                    subtitle: Translation.tr("Based on illogical-impulse by end-4")
                    icon: "rocket_launch"
                }
                LinkButton {
                    materialIcon: "code"
                    mainText: Translation.tr("Source")
                    url: root.repoUrl
                }
                LinkButton {
                    materialIcon: "bug_report"
                    mainText: Translation.tr("Report issue")
                    url: `${root.repoUrl}/issues`
                }
            }
        }
    }

    SettingsGroup {
        title: Translation.tr("Dotfiles")
        SettingsCard {
            CardHeader {
                title: "Impulse"
                subtitle: Translation.tr("Based on end-4/dots-hyprland")
                icon: "folder_special"
            }
            Flow {
                Layout.fillWidth: true
                spacing: 6
                LinkButton {
                    materialIcon: "auto_stories"
                    mainText: Translation.tr("Docs")
                    url: "https://end-4.github.io/dots-hyprland-wiki/en/ii-qs/02usage/"
                }
                LinkButton {
                    materialIcon: "favorite"
                    mainText: Translation.tr("Donate")
                    url: "https://github.com/sponsors/end-4"
                }
            }
        }
    }

    SettingsGroup {
        title: Translation.tr("System")
        SettingsCard {
            id: systemCard
            property bool justCopied: false
            CardHeader {
                title: SystemInfo.distroName
                subtitle: [SystemInfo.desktopEnvironment, SystemInfo.windowingSystem].filter(s => s.length > 0).join(" · ")
                iconSource: SystemInfo.logo.length > 0 ? Rustshell.iconPath(SystemInfo.logo) : ""
                icon: "computer"
            }
            Flow {
                Layout.fillWidth: true
                spacing: 6
                CardButton {
                    materialIcon: systemCard.justCopied ? "check" : "content_copy"
                    mainText: systemCard.justCopied ? Translation.tr("Copied") : Translation.tr("Copy info")
                    onClicked: {
                        Rustshell.clipboardText = `${SystemInfo.distroName} | ${SystemInfo.desktopEnvironment} | ${SystemInfo.windowingSystem} | Impulse`;
                        systemCard.justCopied = true;
                        copiedTimer.restart();
                    }
                    Timer {
                        id: copiedTimer
                        interval: 1500
                        onTriggered: systemCard.justCopied = false
                    }
                }
                LinkButton {
                    visible: SystemInfo.documentationUrl.length > 0
                    materialIcon: "auto_stories"
                    mainText: Translation.tr("Documentation")
                    url: SystemInfo.documentationUrl
                }
                LinkButton {
                    visible: SystemInfo.supportUrl.length > 0
                    materialIcon: "support"
                    mainText: Translation.tr("Help & Support")
                    url: SystemInfo.supportUrl
                }
                LinkButton {
                    visible: SystemInfo.bugReportUrl.length > 0
                    materialIcon: "bug_report"
                    mainText: Translation.tr("Report a Bug")
                    url: SystemInfo.bugReportUrl
                }
                LinkButton {
                    visible: SystemInfo.privacyPolicyUrl.length > 0
                    materialIcon: "policy"
                    materialIconFill: false
                    mainText: Translation.tr("Privacy Policy")
                    url: SystemInfo.privacyPolicyUrl
                }
            }
        }
    }
}
