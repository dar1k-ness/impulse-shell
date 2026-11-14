// Array elements through ordinary [[Get]], [[GetOwnProperty]], [[Set]] and [[Delete]]
// (ECMA-262 §10.1, §10.4.2): a hole reads the prototype chain, an index accessor is a property
// like any other, and Array.prototype.concat spreads by IsConcatSpreadable (§23.1.3.1). Where
// QV4 departs from it (concat of a non-spreadable array, an inherited element at a hole) see
// `concat_follows_ecma262_where_qv4_deviates` (crates/qml/src/builtins.rs).
// v4: ["hole proto => P|P|P,1|P,1|P,1|true|false","hole object proto => O||7,O,9,|1|7!,O!,9!,","hole past end => ,1,false","accessor new index => 5,5,1,0,[5]","accessor over element => [\"g\",\"g\",0,[\"g\"],[\"0\",\"length\"]]","accessor descriptor => function,false,true,true,9,2","accessor past end => 4|x|false|0,3|1---x","accessor setter => 42,43|got|3","accessor no setter => g,2","accessor no setter strict THROW TypeError","accessor delete => true|back|1,back,3|0,1,2","accessor to data => v,1,v,3,v,true,true,false","accessor truncate => 1,,false|undefined,false","accessor array methods => 2,40,6|24|3,20,1|3,20,1|true","accessor push => |3|g","inherited setter => s,t|0|false","inherited readonly => 0,ro,false","non-enumerable element => w|0,2|0,2|0,1,2,length|false|1,w,3","concat spreadable object => [\"a\",\"b\"],3","concat proxy => [0,1,2]","concat holes => 8|0,2,3,5,7|false|false","concat accessor => [0,\"g\",2]","concat generic receiver => 2,object,2,2","concat array-like not spread => 1"]
var r = [];
function t(name, f) {
    try {
        r.push(name + " => " + f());
    } catch (e) {
        r.push(name + " THROW " + e.name);
    }
}
t("hole proto", function() {
    Array.prototype[0] = "P";
    try {
        var a = [, 1];
        var o = [];
        for (var x of a) o.push(x);
        var i = 0;
        return [a[0], a[i], o, [...a], Array.from(a), 0 in a, a.hasOwnProperty(0)].join("|");
    } finally {
        delete Array.prototype[0];
    }
})
t("hole object proto", function() {
    Object.prototype[1] = "O";
    try {
        var a = [7, , 9];
        a.length = 4;
        return [a[1], a[3], a.join(), a.indexOf("O"), a.map(function(v) { return v + "!" })].join("|");
    } finally {
        delete Object.prototype[1];
    }
})
t("hole past end", function() { var a = [1]; return [a[5], a.length, 5 in a].join() })
t("accessor new index", function() {
    var a = [];
    Object.defineProperty(a, "0", {get: function() { return 5 }, configurable: true, enumerable: true});
    var i = 0;
    return a[0] + "," + a[i] + "," + a.length + "," + Object.keys(a) + "," + JSON.stringify(a)
})
t("accessor over element", function() {
    var g = [0];
    Object.defineProperty(g, 0, {get: function() { return "g" }});
    return JSON.stringify([g[0], g.join(), g.indexOf("g"), g.slice(), Object.getOwnPropertyNames(g)])
})
t("accessor descriptor", function() {
    var a = [1, 2];
    Object.defineProperty(a, 1, {get: function() { return 9 }});
    var d = Object.getOwnPropertyDescriptor(a, 1);
    return [typeof d.get, "value" in d, d.enumerable, d.configurable, a[1], a.length].join()
})
t("accessor past end", function() {
    var a = [1];
    Object.defineProperty(a, 3, {get: function() { return "x" }, enumerable: true, configurable: true});
    return [a.length, a[3], 1 in a, Object.keys(a), a.join("-")].join("|")
})
t("accessor setter", function() {
    var log = [];
    var a = [1, 2, 3];
    Object.defineProperty(a, 1, {get: function() { return "got" }, set: function(v) { log.push(v) }, configurable: true});
    a[1] = 42;
    var i = 1;
    a[i] = 43;
    return log + "|" + a[1] + "|" + a.length
})
t("accessor no setter", function() {
    var a = [1, 2];
    Object.defineProperty(a, 0, {get: function() { return "g" }, configurable: true});
    a[0] = 3;
    return a[0] + "," + a.length
})
t("accessor no setter strict", function() {
    "use strict";
    var a = [1, 2];
    Object.defineProperty(a, 0, {get: function() { return "g" }, configurable: true});
    a[0] = 3;
})
t("accessor delete", function() {
    var a = [1, 2, 3];
    Object.defineProperty(a, 1, {get: function() { return "g" }, configurable: true});
    var d = delete a[1];
    a[1] = "back";
    return [d, a[1], a.join(), Object.keys(a)].join("|")
})
t("accessor to data", function() {
    var a = [1, 2, 3];
    Object.defineProperty(a, 1, {get: function() { return "g" }, configurable: true, enumerable: true});
    Object.defineProperty(a, 1, {value: "v", writable: true});
    var d = Object.getOwnPropertyDescriptor(a, 1);
    return [a[1], a.join(), d.value, d.writable, d.enumerable, "get" in d].join()
})
t("accessor truncate", function() {
    var a = [1, 2, 3];
    Object.defineProperty(a, 2, {get: function() { return "g" }, configurable: true});
    a.length = 1;
    var r1 = [a.length, a[2], 2 in a].join();
    a.length = 3;
    return r1 + "|" + a[2] + "," + (2 in a)
})
t("accessor array methods", function() {
    var a = [1, 2, 3];
    var n = 0;
    Object.defineProperty(a, 1, {get: function() { n++; return 20 }, configurable: true, enumerable: true});
    var s = a.map(function(v) { return v * 2 }) + "|" + a.reduce(function(x, y) { return x + y }) + "|" + a.reverse() + "|" + a.join();
    return s + "|" + (n > 0)
})
t("accessor push", function() {
    var log = [];
    var a = [1];
    Object.defineProperty(a, 1, {get: function() { return "g" }, set: function(v) { log.push("set" + v) }, configurable: true});
    a.push("p");
    return log + "|" + a.length + "|" + a[1]
})
t("inherited setter", function() {
    var log = [];
    Object.defineProperty(Array.prototype, "5", {set: function(v) { log.push(v) }, configurable: true});
    try {
        var a = [];
        a[5] = "s";
        var i = 5;
        a[i] = "t";
        return log + "|" + a.length + "|" + a.hasOwnProperty(5)
    } finally {
        delete Array.prototype[5];
    }
})
t("inherited readonly", function() {
    Object.defineProperty(Array.prototype, "2", {value: "ro", writable: false, configurable: true});
    try {
        var a = [];
        a[2] = "w";
        return a.length + "," + a[2] + "," + a.hasOwnProperty(2)
    } finally {
        delete Array.prototype[2];
    }
})
t("non-enumerable element", function() {
    var a = [1, 2, 3];
    Object.defineProperty(a, 1, {enumerable: false});
    var ks = [];
    for (var k in a) ks.push(k);
    a[1] = "w";
    return [a[1], ks, Object.keys(a), Object.getOwnPropertyNames(a), a.propertyIsEnumerable(1), a.join()].join("|")
})
t("concat spreadable object", function() {
    var o = {length: 2, 0: "a", 1: "b"};
    o[Symbol.isConcatSpreadable] = true;
    return JSON.stringify([].concat(o)) + "," + [0].concat(o).length
})
t("concat proxy", function() { return JSON.stringify([0].concat(new Proxy([1, 2], {}))) })
t("concat holes", function() {
    var o = {length: 3, 0: "x", 2: "z"};
    o[Symbol.isConcatSpreadable] = true;
    var r1 = [1, , 2].concat(o, [, 4]);
    return r1.length + "|" + Object.keys(r1) + "|" + (1 in r1) + "|" + (4 in r1)
})
t("concat accessor", function() {
    var a = [1, 2];
    Object.defineProperty(a, 0, {get: function() { return "g" }, configurable: true, enumerable: true});
    return JSON.stringify([0].concat(a))
})
t("concat generic receiver", function() {
    var r1 = Array.prototype.concat.call({a: 1}, [2]);
    return r1.length + "," + typeof r1[0] + "," + r1[1] + "," + Array.prototype.concat.call("s", 1).length
})
t("concat array-like not spread", function() { return [].concat({length: 2, 0: "a", 1: "b"}).length })
var result = JSON.stringify(r);
