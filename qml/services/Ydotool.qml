pragma Singleton

import ri.modules.common
import Impulse
import Rustshell

Singleton {
    id: root
    property int shiftMode: 0 // 0: off, 1: on, 2: lock
    property list<int> shiftKeys: [42, 54] // Keycodes for Shift keys (left and right)
    property list<int> altKeys: [56, 100] // Keycodes for Alt keys (left and right) 
    property list<int> ctrlKeys: [29, 97] // Keycodes for Ctrl keys (left and right)

    function releaseAllKeys() {
        KeySender.releaseAll()
        root.shiftMode = 0; // Reset shift mode
    }

    function releaseShiftKeys() {
        root.shiftKeys.forEach(keycode => KeySender.release(keycode))
        root.shiftMode = 0; // Reset shift mode
    }

    function press(keycode) {
        KeySender.press(keycode)
    }

    function release(keycode) {
        KeySender.release(keycode)
    }
}
