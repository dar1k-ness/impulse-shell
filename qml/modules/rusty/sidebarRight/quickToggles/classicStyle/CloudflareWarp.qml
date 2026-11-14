import ri.modules.common
import ri.modules.common.widgets
import ri.services
import Impulse
import QtQuick
import Rustshell

QuickToggleButton {
    id: root
    toggled: Warp.connected
    visible: Warp.available
    
    contentItem: CustomIcon {
        id: distroIcon
        source: 'cloudflare-dns-symbolic'

        anchors.centerIn: parent
        width: 16
        height: 16
        colorize: true
        color: root.toggled ? Appearance.m3colors.m3onPrimary : Appearance.colors.colOnLayer1

        Behavior on color {
            animation: Appearance.animation.elementMoveFast.colorAnimation.createObject(this)
        }
    }

    onClicked: {
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
    StyledToolTip {
        text: Translation.tr("Cloudflare WARP (1.1.1.1)")
    }
}
