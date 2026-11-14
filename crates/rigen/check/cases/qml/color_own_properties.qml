// The properties of a color are its own (Qt 6.12), on one Qt.rgba makes and on one read from
// a property alike: listed, described, copied and frozen as an object's, and a name script
// writes to it is not kept.
// qt: 12,157,,true,true/12,157,,true,true|12,12,12/12,12,12|12,12,1/12,12,1|1,12/1,12|82,,1/82,,1|true,true,true,1/true,true,true,1|object,0/object,0|true,1,12/true,1,12|true,,12/true,,12|true,0.5000076293945312,12/true,0.5000076293945312,12|true,12/true,12|3,13,0/3,13,0|0.5000076293945312,#800000,12,193/0.5000076293945312,#800000,12,193|true,false,true/true,false,true|,true,12/,true,12|12,/12,|9,1,12/9,0.5000076293945312,12
import QtQuick
Item {
    id: root
    property string result
    property color col: "red"
    function t(f) { try { return String(f()) } catch (e) { return "E:" + e } }
    function both(f) { return t(() => f(Qt.rgba(1, 0, 0, 1))) + "/" + t(() => f(root.col)) }
    Component.onCompleted: {
        var l = []
        l.push(both(x => { x.foo = 1; return [Object.keys(x).length, JSON.stringify(x).length, x.foo, "r" in x, x.hasOwnProperty("r")] }))
        l.push(both(x => { x.foo = 1; var n = 0; for (var k in x) n++; return [n, Object.getOwnPropertyNames(x).length, Reflect.ownKeys(x).length] }))
        l.push(both(x => { var n = 0; for (var k in x) n++; return [n, Object.entries(x).length, Object.values(x)[0]] }))
        l.push(both(x => [Object.assign({}, x).r, Object.keys(Object.assign({}, x)).length]))
        l.push(both(x => { x["zz"] = 2; return [Object.keys(x).join(",").length, x.zz, x.r] }))
        l.push(both(x => { var d = Object.getOwnPropertyDescriptor(x, "r"); return [d.writable, d.enumerable, d.configurable, d.value] }))
        l.push(both(x => { x.foo = 1; var d = Object.getOwnPropertyDescriptor(x, "g"); return [typeof d, d && d.value] }))
        l.push(both(x => [delete x.r, x.r, Object.keys(x).length]))
        l.push(both(x => { x.foo = 1; return [delete x.foo, x.foo, Object.keys(x).length] }))
        l.push(both(x => { Object.freeze(x); x.r = 0.5; return [Object.isFrozen(x), x.r, Object.keys(x).length] }))
        l.push(both(x => { x.foo = 1; Object.seal(x); return [Object.isSealed(x), Object.keys(x).length] }))
        l.push(both(x => { Object.defineProperty(x, "bar", {value: 3, enumerable: true}); return [x.bar, Object.keys(x).length, x.g] }))
        l.push(both(x => { x.foo = 1; x.r = 0.5; return [x.r, String(x), Object.keys(x).length, JSON.stringify(x).length] }))
        l.push(both(x => { x.foo = 1; return [x.propertyIsEnumerable("r"), x.propertyIsEnumerable("foo"), Object.prototype.hasOwnProperty.call(x, "hsvHue")] }))
        l.push(both(x => { var y = x; y.foo = 1; return [x.foo, x === y, Object.keys(y).length] }))
        l.push(both(x => { x.foo = 1; return [Object.keys(root.col).length, root.col.foo] }))
        l.push(both(x => { Object.setPrototypeOf(x, {q: 9}); return [x.q, x.r, Object.keys(x).length] }))
        result = l.join("|")
    }
}
