import ri.modules.common
import ri.modules.common.models.quickToggles
import ri.modules.common.widgets
import ri.services
import QtQuick
import Rustshell
import Rustshell.Io

AndroidQuickToggleButton {
    id: root

    toggleModel: CloudflareWarpToggle {}
}
