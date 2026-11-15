pragma Singleton
pragma ComponentBehavior: Bound
import QtQuick
import Rustshell
import Rustshell.Hyprland

/**
 * Manages a HyprlandFocusGrab that's to be shared by all windows.
 * "Persistent" is for windows that should always be included but not closed on dismiss, like bar and onscreen keyboard.
 * "Dismissable" is for stuff like sidebars
 */ 
Singleton {
    id: root

    signal dismissed()

    property list<var> persistent: []
    property list<var> dismissable: []
    // Always in the grab while the grab is active (the edit overlay above a sidebar).
    property list<var> pinned: []

    function dismiss() {
        root.dismissable = [];
        root.dismissed();
    }

    Component.onCompleted: {
        console.log("[GlobalFocusGrab] Initialized");
    }

    function addPersistent(window) {
        if (root.persistent.indexOf(window) === -1) {
            root.persistent.push(window);
        }
    }

    function removePersistent(window) {
        var index = root.persistent.indexOf(window);
        if (index !== -1) {
            root.persistent.splice(index, 1);
        }
    }

    function addPinned(window) {
        if (root.pinned.indexOf(window) === -1) {
            root.pinned.push(window);
        }
    }
    function removePinned(window) {
        var index = root.pinned.indexOf(window);
        if (index !== -1) {
            root.pinned.splice(index, 1);
        }
    }
    function addDismissable(window) {
        if (root.dismissable.indexOf(window) === -1) {
            root.dismissable.push(window);
        }
    }

    function removeDismissable(window) {
        var index = root.dismissable.indexOf(window);
        if (index !== -1) {
            root.dismissable.splice(index, 1);
        }
    }

    // Whether an item of `window` has active focus: the window's root item has it then. Asked of
    // that one item, not of every item of the window: a sidebar that builds its content while
    // open changes a list of children per item it makes, and each change would ask again.
    function hasActive(window) {
        const content = window?.contentItem;
        return ((content?.parent ?? content)?.activeFocus) ?? false;
    }

    HyprlandFocusGrab {
        id: grab
        windows: root.dismissable.every(w => !w?.focusable) || root.dismissable.some(w => hasActive(w)) ? [...root.dismissable, ...root.persistent, ...root.pinned] : [...root.dismissable, ...root.pinned]
        active: root.dismissable.length > 0
        onCleared: () => {
            root.dismiss();
        }
    }

}
