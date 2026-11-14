import ri.modules.common
import ri.modules.common.widgets
import ri.services
import Rustshell
import Rustshell.Io
import Impulse
import QtQuick

QuickToggleButton {
    id: root
    buttonIcon: "gamepad"

    onClicked: {
        root.toggled = !root.toggled
        applyRequest.send()
    }
    HyprlandRequest {
        id: applyRequest
        // Reloading the config brings back what game mode turned off
        command: root.toggled ? "eval hl.config({ animations = { enabled = false }, decoration = { shadow = { enabled = false }, blur = { enabled = false }, rounding = 0 }, general = { gaps_in = 0, gaps_out = 0, border_size = 1, allow_tearing = true } })" : "reload"
    }
    Component.onCompleted: fetchActiveState.send()
    HyprlandRequest {
        id: fetchActiveState
        command: "getoption animations:enabled"
        json: true
        onReplied: text => {
            // The value comes as "int" or, since Hyprland 0.57, as "bool"
            let animations;
            try {
                const option = JSON.parse(text);
                animations = option.int ?? option.bool;
            } catch (e) {}
            // On while animations are off, and when Hyprland gave no value
            root.toggled = !(animations === true || (Number.isInteger(animations) && animations !== 0));
        }
    }
    StyledToolTip {
        text: Translation.tr("Game mode")
    }
}