pragma ComponentBehavior: Bound
import ri.services
import ri.modules.common
import ri.modules.common.widgets
import QtQuick
import QtQuick.Layouts
import Rustshell
import Rustshell.Bluetooth
import ri.modules.rusty.overlay.crosshair
import ri.modules.rusty.overlay.volumeMixer
import ri.modules.rusty.overlay.floatingImage
import ri.modules.rusty.overlay.fpsLimiter
import ri.modules.rusty.overlay.recorder
import ri.modules.rusty.overlay.resources
import ri.modules.rusty.overlay.notes

DelegateChooser {
    id: root
    role: "identifier"

    DelegateChoice { roleValue: "crosshair"; Crosshair {} }
    DelegateChoice { roleValue: "floatingImage"; FloatingImage {} }
    DelegateChoice { roleValue: "fpsLimiter"; FpsLimiter {} }
    DelegateChoice { roleValue: "recorder"; Recorder {} }
    DelegateChoice { roleValue: "resources"; Resources {} }
    DelegateChoice { roleValue: "notes"; Notes {} }
    DelegateChoice { roleValue: "volumeMixer"; VolumeMixer {} }
}
