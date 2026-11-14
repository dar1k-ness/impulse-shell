// On the qualifier of `import "dir" as Here` every name is an object (Qt takes it for a
// type of the directory), the same one each time, that converts to its name; on the qualifier
// of a module import the module's types are what their bare names are and no other name is
// defined. Neither object has keys, and a write to a name changes nothing.
// qt: object Nope lower true false undefined true false false undefined 0 false objectobject object true 0 4 undefined undefined undefined true false 0 object object object object object object undefined
import QtQuick
import QtQuick as Q
import QtQuick.Controls as C
import "." as Here
Item {
    id: root
    function w() { Here.Nope = 1; Q.Item = 1; return typeof Here.Nope + typeof Q.Item }
    property string result: [typeof Here.Nope, String(Here.Nope), "" + Here.lower, "Nope" in Here, 0 in Here,
        typeof Here[0], Here.Nope === Here.Nope, Here.Nope == null, !Here.Nope, typeof Here.Nope.x,
        Object.keys(Here).length, Here === Here, w(),
        typeof Q.Item, Q.Item === Item, Q.Item.TopLeft, Q.Text.AlignHCenter, typeof Q.Nope, typeof Q.lower,
        typeof Q.Button, "Item" in Q, "Nope" in Q, Object.keys(Q).length, typeof Q.Component,
        typeof Q.Screen, typeof Q.Keys, typeof Q.Qt, typeof Q.ListModel, typeof C.Button, typeof C.Item].join(" ")
}
