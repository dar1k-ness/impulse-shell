// The journal of the `creation_order` cases: handlers of every object and document layer
// append to it in the order they run.
pragma Singleton
import QtQuick
QtObject {
    property string s: ""
    function add(x) { s += (s ? " " : "") + x }
}
