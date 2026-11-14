// qt: readByBinding(2) readByHandler(21) trigger(21) plain(31) | root 3,2,11,21,31 ran 1,1,1,1,1 | readByBinding(3) trigger(21) readByHandler(22) plain(32) | 4,3,12,22,32 ran 2,2,2,2,2| child 4,3,12,22,32 ran 2,2,2,2,2 
import QtQuick
Item {
    id: root
    property string result: ""
    // How many times each binding ran.
    property var ran: ({ reader: 0, readByBinding: 0, trigger: 0, readByHandler: 0, plain: 0 })
    function log(s) { result += s }
    function counted(name, v) { ran[name]++; return v }
    function counts() {
        return ran.reader + "," + ran.readByBinding + "," + ran.trigger + "," + ran.readByHandler + "," + ran.plain
    }
    function values() {
        return reader + "," + readByBinding + "," + trigger + "," + readByHandler + "," + plain
    }

    property int src: 1

    // A binding reads a property whose binding has not run yet.
    property int reader: counted("reader", readByBinding + 1)
    property int readByBinding: counted("readByBinding", src + 1)
    onReadByBindingChanged: log("readByBinding(" + readByBinding + ") ")

    // A change handler does.
    property int trigger: counted("trigger", src + 10)
    onTriggerChanged: log("trigger(" + readByHandler + ") ")
    property int readByHandler: counted("readByHandler", src + 20)
    onReadByHandlerChanged: log("readByHandler(" + readByHandler + ") ")

    // Nothing reads this one before Component.onCompleted.
    property int plain: counted("plain", src + 30)
    onPlainChanged: log("plain(" + plain + ") ")

    QtObject {
        Component.onCompleted: root.log("| child " + root.values() + " ran " + root.counts() + " ")
    }

    Component.onCompleted: {
        log("| root " + values() + " ran " + counts() + " | ")
        src = 2
        log("| " + values() + " ran " + counts())
    }
}
