import ri.services
import ri.modules.common
import ri.modules.common.models.quickToggles
import ri.modules.common.functions
import ri.modules.common.widgets
import QtQuick
import Rustshell
import Rustshell.Bluetooth

AndroidQuickToggleButton {
    id: root
    
    toggleModel: BluetoothToggle {}
}
