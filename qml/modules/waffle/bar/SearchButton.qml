import QtQuick
import QtQuick.Layouts
import org.kde.kirigami as Kirigami
import ri
import ri.services
import ri.modules.common
import ri.modules.waffle.looks

AppButton {
    id: root

    iconName: checked ? "system-search-checked" : "system-search"
    separateLightDark: true

    checked: GlobalStates.searchOpen && LauncherSearch.query !== ""
    onClicked: {
        GlobalStates.searchOpen = !GlobalStates.searchOpen; // For now...
    }

    BarToolTip {
        id: tooltip
        text: Translation.tr("Search")
        extraVisibleCondition: root.shouldShowTooltip
    }
}
