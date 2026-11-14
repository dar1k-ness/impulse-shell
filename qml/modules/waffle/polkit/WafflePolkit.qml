import ri
import ri.services
import ri.modules.common
import ri.modules.common.widgets
import ri.modules.common.functions
import QtQuick
import Rustshell
import Rustshell.Wayland

FullscreenPolkitWindow {
    id: root
    contentComponent: Component {
        WPolkitContent {}
    }
}
