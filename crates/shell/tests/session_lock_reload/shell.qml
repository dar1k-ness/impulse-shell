//@ pragma UseQApplication
//@ pragma Env QS_NO_RELOAD_POPUP=1
//@ pragma Env QT_QUICK_CONTROLS_STYLE=Basic
// Harness of tests/session_lock_reload.rs: the original's own lock screen (`modules/rusty/lock/Lock.qml`) and
// nothing else of the shell, with what the tests read printed as `IMAGE`, `TEXT`, `PAM` and
// `PROBE` lines. The test copies the tree next to this file and runs it under `qs -p` and
// `rusty-impulse -p`.
import "modules/common"
import "services"
import "modules/rusty/lock"
import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.Pam

ShellRoot {
    id: root

    Component.onCompleted: console.log("IMAGE up")

    Lock {
        id: theLock
    }

    Connections {
        target: theLock.context
        function onCurrentTextChanged() {
            console.log("TEXT " + theLock.context.currentText);
        }
    }

    // A conversation the test holds open: it asks for the password and waits for `answer`.
    PamContext {
        id: pam
        configDirectory: Quickshell.env("LOCK_TEST_PAM")
        config: "probe"
        onPamMessage: console.log("PAM asks")
        onCompleted: result => console.log("PAM done " + (result == PamResult.Success))
    }

    IpcHandler {
        target: "probe"
        function state(): void {
            console.log("PROBE locked=" + GlobalStates.screenLocked + " text=" + theLock.context.currentText + " pam=" + pam.active + "/" + pam.responseRequired);
        }
        function reload(): void {
            Quickshell.reload(false);
        }
        function ask(): void {
            pam.start();
        }
        function answer(): void {
            pam.respond(theLock.context.currentText);
        }
    }
}
