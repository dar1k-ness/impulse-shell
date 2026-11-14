pragma Singleton
import Impulse
import Rustshell
import ri.services
import ri.modules.common

Singleton {
    id: root

    function closeAllWindows() {
        HyprlandData.windowList.map(w => w.pid).forEach(pid => {
            SessionControl.terminate(pid);
        });
    }

    function changePassword() {
        Rustshell.execDetached(["bash", "-c", `${Config.options.apps.changePassword}`]);
    }

    function lock() {
        SessionControl.lock();
    }

    function suspend() {
        SessionControl.suspend();
    }

    function logout() {
        closeAllWindows();
        SessionControl.logout();
    }

    function launchTaskManager() {
        Rustshell.execDetached(["bash", "-c", `${Config.options.apps.taskManager}`]);
    }

    function hibernate() {
        SessionControl.hibernate();
    }

    function poweroff() {
        closeAllWindows();
        SessionControl.poweroff();
    }

    function reboot() {
        closeAllWindows();
        SessionControl.reboot();
    }

    function rebootToFirmware() {
        closeAllWindows();
        SessionControl.rebootToFirmware();
    }
}
