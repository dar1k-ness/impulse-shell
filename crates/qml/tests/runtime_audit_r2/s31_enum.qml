import QtQuick
import "sub"
Item {
    Component.onCompleted: console.log(Enumed.A, Enumed.B, Enumed.C, Enumed.D, Enumed.Kind.B, typeof Enumed.Kind)
}
