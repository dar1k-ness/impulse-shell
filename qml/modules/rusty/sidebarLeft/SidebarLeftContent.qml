import ri.services
import ri.modules.common
import ri.modules.common.widgets
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Qt5Compat.GraphicalEffects
import Qt.labs.synchronizer

Item {
    id: root
    required property var scopeRoot
    property int sidebarPadding: 10
    anchors.fill: parent
    property bool translatorEnabled: Config.options.sidebar.translator.enable
    readonly property bool menuConfigured: Config.options.sidebar.assistant.menu.length > 0
    property var tabButtonList: [
        root.menuConfigured ? {"icon": "apps", "name": Translation.tr("Menu")} : {"icon": "forum", "name": Translation.tr("Assistant")},
        ...(root.translatorEnabled ? [{"icon": "translate", "name": Translation.tr("Translator")}] : [])
    ]
    property int tabCount: swipeView.count

    function focusActiveItem() {
        swipeView.currentItem.forceActiveFocus()
    }

    Keys.onPressed: (event) => {
        if (event.modifiers === Qt.ControlModifier) {
            if (event.key === Qt.Key_PageDown) {
                swipeView.incrementCurrentIndex()
                event.accepted = true;
            }
            else if (event.key === Qt.Key_PageUp) {
                swipeView.decrementCurrentIndex()
                event.accepted = true;
            }
        }
    }

    ColumnLayout {
        anchors {
            fill: parent
            margins: sidebarPadding
        }
        spacing: sidebarPadding

        Toolbar {
            visible: tabButtonList.length > 0
            Layout.alignment: Qt.AlignHCenter
            enableShadow: false
            ToolbarTabBar {
                id: tabBar
                Layout.alignment: Qt.AlignHCenter
                tabButtonList: root.tabButtonList
                currentIndex: swipeView.currentIndex
            }
        }

        Rectangle {
            Layout.fillWidth: true
            Layout.fillHeight: true
            implicitWidth: swipeView.implicitWidth
            implicitHeight: swipeView.implicitHeight
            radius: Appearance.rounding.normal
            color: Appearance.colors.colLayer1

            SwipeView { // Content pages
                id: swipeView
                anchors.fill: parent
                spacing: 10
                currentIndex: tabBar.currentIndex

                clip: true
                layer.enabled: true
                layer.effect: OpacityMask {
                    maskSource: Rectangle {
                        width: swipeView.width
                        height: swipeView.height
                        radius: Appearance.rounding.small
                    }
                }

                contentChildren: [
                    (root.menuConfigured ? assistantMenu : Assistant.configured ? assistantChat : assistantGuide).createObject(),
                    ...(root.translatorEnabled ? [translator.createObject()] : []),
                ]
            }
        }

        Component {
            id: assistantMenu
            AssistantMenu {}
        }
        Component {
            id: assistantChat
            AssistantChat {
                session: Assistant.session
            }
        }
        Component {
            id: assistantGuide
            AssistantGuide {}
        }
        Component {
            id: translator
            Translator {}
        }
    }
}