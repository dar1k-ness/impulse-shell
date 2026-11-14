import QtQuick
import ri.modules.common
import ri.modules.rusty.sidebarRight.quickToggles

// Sidebar widget "quickToggles": the original's own quick panel, Android or classic as the original's settings
// choose, with the toggles, their order and sizes from those settings (`sidebar.quickToggles`).
// the original's edit mode for it (the sidebar's pencil button: the unused toggles under the divider,
// adding, removing, resizing and swapping) keeps working and keeps writing those settings, the
// one place the quick toggles are set.
Item {
    id: root

    // From the container (WidgetHost.placement): the sidebar's quick-toggle edit mode.
    property bool editMode: false

    signal openAudioOutputDialog()
    signal openAudioInputDialog()
    signal openBluetoothDialog()
    signal openNightLightDialog()
    signal openWifiDialog()

    implicitWidth: panel.item?.implicitWidth ?? 0
    implicitHeight: panel.item?.implicitHeight ?? 0

    Loader {
        id: panel
        sourceComponent: Config.options.sidebar.quickToggles.style === "classic" ? classicPanel : androidPanel
    }

    Component {
        id: androidPanel
        AndroidQuickPanel {
            width: root.width
            editMode: root.editMode
            onOpenAudioOutputDialog: root.openAudioOutputDialog()
            onOpenAudioInputDialog: root.openAudioInputDialog()
            onOpenBluetoothDialog: root.openBluetoothDialog()
            onOpenNightLightDialog: root.openNightLightDialog()
            onOpenWifiDialog: root.openWifiDialog()
        }
    }

    Component {
        id: classicPanel
        ClassicQuickPanel {
            x: (root.width - width) / 2
            onOpenAudioOutputDialog: root.openAudioOutputDialog()
            onOpenAudioInputDialog: root.openAudioInputDialog()
            onOpenBluetoothDialog: root.openBluetoothDialog()
            onOpenNightLightDialog: root.openNightLightDialog()
            onOpenWifiDialog: root.openWifiDialog()
        }
    }
}
