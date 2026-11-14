// A signal declared in QML converts its arguments to the declared parameter types: a number
// or string is what JavaScript makes of the value, a value with no conversion arrives as the
// type's default.
// qt: 7:number,8:string;1:number,null:string;0:number,undefined:string;1410065408:number,1,2:string;12:number,true:string;1.5:number,x:string;NaN:number,undefined:undefined;0:number,1:number;true,#ff0000,a/b:object;false,#000000,:object;true,#102030,http://x/:object;true,false,[1,2,3],QPointF(1, 2);false,true,[],QPointF(3, 4);false,true,[],QPointF(0, 0);Insufficient arguments
import QtQuick
Item {
    id: root
    property string result: ""
    signal withArgs(int a, string b)
    signal typed(x: real, y: var)
    signal flags(bool b, color c, url u)
    signal objs(Item i, list<int> l, point p)
    onWithArgs: (a, b) => result += a + ":" + typeof a + "," + b + ":" + typeof b + ";"
    onTyped: (x, y) => result += x + ":" + typeof x + "," + y + ":" + typeof y + ";"
    onFlags: (b, c, u) => result += b + "," + c + "," + u + ":" + typeof u + ";"
    onObjs: (i, l, p) => result += (i === root) + "," + (i === null) + "," + JSON.stringify(l) + "," + p + ";"
    Component.onCompleted: {
        withArgs("7", 8)
        withArgs(1.6, null)
        withArgs("abc", undefined)
        withArgs(1e10, [1, 2])
        withArgs(" 12 ", true)
        typed("1.5", "x")
        typed(undefined, undefined)
        typed(null, 1)
        flags("x", "red", "a/b")
        flags(0, "nope", "")
        flags({}, "#102030", Qt.url("http://x/"))
        objs(root, [1, "2", 3.7], Qt.point(1, 2))
        objs(5, 5, { x: 3, y: 4 })
        objs(null, "x", 5)
        try {
            withArgs(1)
        } catch (e) {
            result += e.message
        }
    }
}
