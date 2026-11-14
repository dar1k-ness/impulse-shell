// A handler written as a function gets the signal's arguments as its own formals only: the
// signal's parameter names are not in its scope (own signal, and a signal of another type).
// qt: undefined,1;undefined,3;undefined;
import QtQuick
Item {
    property string result: ""
    component Sender: QtObject {
        signal sent(int q)
    }
    signal ev(int a, int b)
    onEv: function (b) { result += typeof a + "," + b + ";" }
    signal ev2(int c)
    onEv2: (x) => { result += typeof c + "," + x + ";" }
    Sender {
        id: snd
        onSent: function () { result += typeof q + ";" }
    }
    Component.onCompleted: { ev(1, 2); ev2(3); snd.sent(5) }
}
