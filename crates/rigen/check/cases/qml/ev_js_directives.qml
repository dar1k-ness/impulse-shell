// Script directives are the ones before the first token of the program: `.import` inside a
// comment or a string is not one.
// qt: [1,1,22,"undefined",true]
import QtQuick
import "ev_lib_c.js" as C
import "ev_lib_c.js" as C2
Item {
    property string result: ""
    Component.onCompleted: result = JSON.stringify([C.c, C.f(), C.s.length, typeof C.Z, C === C2])
}
