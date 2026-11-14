import Impulse
import QtQuick
import ri.services
import ri.modules.common
import ri.modules.common.functions
import ri.modules.common.widgets
import Rustshell

QuickToggleModel {
    id: root
    name: Translation.tr("Cloudflare WARP")

    toggled: Warp.connected
    // False until `warp-cli status` answers: without warp-cli there is nothing to toggle.
    available: Warp.available
    icon: "cloud_lock"
    
    mainAction: () => {
        if (toggled) {
            Warp.disconnect()
        } else {
            root.connectRequest = Warp.connect()
        }
    }

    // What Warp.refresh() and Warp.connect() returned for this toggle's own requests.
    property int statusRequest: -1
    property int connectRequest: -1
    Component.onCompleted: root.statusRequest = Warp.refresh()

    Connections {
        target: Warp
        function onConnectExited(request, exitCode) {
            if (request !== root.statusRequest && request !== root.connectRequest) return
            if (exitCode !== 0) {
                Notify.send(
                    Translation.tr("Cloudflare WARP"),
                    Translation.tr("Connection failed. Please inspect manually with the <tt>warp-cli</tt> command")
                )
            }
        }
        function onRegistrationExited(request, exitCode) {
            if (request !== root.statusRequest) return
            console.log("Warp registration exited with code:", exitCode)
            if (exitCode !== 0) {
                Notify.send(
                    Translation.tr("Cloudflare WARP"),
                    Translation.tr("Registration failed. Please inspect manually with the <tt>warp-cli</tt> command")
                )
            }
        }
    }
    tooltipText: Translation.tr("Cloudflare WARP (1.1.1.1)")
}
