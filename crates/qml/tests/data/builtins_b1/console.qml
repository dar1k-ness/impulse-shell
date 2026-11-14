import QtQuick
Item {
    id: root
    property color c: "#336699"
    property var lst: [1, 2]
    property list<int> il: [1, 2, 3]
    property url u: "http://a/b"
    property date dt: new Date(0)
    property point pt: Qt.point(1, 2)
    function args() { return arguments }
    Component.onCompleted: {
        function L(n, f) { try { f() } catch (e) { console.log(n, "THROW", e.name, e.message) } }
        L(1, function() { console.log("1", new Error("boom")) })
        L(2, function() { try { null.x } catch (e) { console.log("2", e) } })
        L(3, function() { console.log("3", new RangeError("r"), new TypeError(), new SyntaxError("s"), new ReferenceError("q"), new URIError("u"), new EvalError("e")) })
        L(4, function() { var e = new Error("m"); e.name = "Custom"; console.log("4", e) })
        L(5, function() { console.log("5", new Date(86400000 * 365)) })
        L(6, function() { console.log("6", /ab+c/gi) })
        L(7, function() { console.log("7", [1, [2, 3]]) })
        L(8, function() { console.log("8", new Map([[1, 2]]), new Set([1]), new WeakMap()) })
        L(9, function() { console.log("9", { toString: function() { return "custom" } }) })
        L(10, function() { console.log("10", function named(a) { return a }) })
        L(11, function() { console.log("11", function(a) { return a }, (a) => a, Math.max, function() {}.bind(null), root.args) })
        L(12, function() { console.log("12", { toString: function() { throw new Error("inner") } }, "after") })
        L(13, function() { console.log("13", { toString: function() { throw 5 } }, "after") })
        L(14, function() { console.log("14", { toString: function() { return {} }, valueOf: function() { return 7 } }) })
        L(15, function() { console.log("15", { toString: function() { return {} }, valueOf: function() { return {} } }, "after") })
        L(16, function() { var a = [1]; a.push(a); console.log("16", a) })
        L(17, function() { var a = [1]; var b = [a, a]; console.log("17", b) })
        L(18, function() { console.log("18", [undefined, null, , "s", {}, [], [[]], true, -0, 1.5]) })
        L(19, function() { console.log("19", root.args(1, [2], "x")) })
        L(20, function() { console.log("20", new Uint8Array([1, 2]), new ArrayBuffer(2)) })
        L(21, function() { console.log("21", Symbol(), Symbol("d"), Symbol.iterator) })
        L(22, function() { console.log("22", [Symbol("d")]) })
        L(23, function() { console.log("23", new Date(NaN)) })
        L(24, function() { console.log("24", root.c, root.u, root.pt) })
        L(25, function() { console.log("25", root.lst, root.il) })
        L(26, function() { console.log("26", Object.create(null), "after") })
        L(27, function() { console.log("27", undefined, null, true, -0, 0.1, 1e21, NaN, "s") })
        L(28, function() { console.log("28", new String("x"), new Number(2), new Boolean(false)) })
        L(29, function() { console.log("29", {}, { a: 1 }, Math, JSON) })
        L(30, function() { console.log("30", [{ toString: function() { return "el" } }, new Error("in")]) })
        L(31, function() { console.log("31", [{ toString: function() { throw new Error("eth") } }, 2], "after") })
        L(32, function() { console.log("32", function*gen() {}, class K {}, { m() {} }.m) })
        L(33, function() { console.log("33", new Proxy({}, {}), new Proxy([1, 2], {})) })
        L(34, function() { var o = {}; o[Symbol.toPrimitive] = function(h) { return "prim:" + h }; console.log("34", o) })
        L(35, function() { console.log("35", { length: 2, 0: "a", 1: "b" }, "abc".split("")) })
        L(36, function() { console.log("36") ; console.log() ; console.log("") })
        L(37, function() { console.log("37", [1, 2].keys(), new Map().entries()) })
        L(38, function() { console.log("38", Promise.resolve(1), new DataView(new ArrayBuffer(1))) })
        L(39, function() { var e = new Error("x"); e.message = ""; console.log("39", e, new Error()) })
        L(40, function() { var e = new Error("x"); e.toString = function() { return "ovr" }; console.log("40", e) })
        L(41, function() { console.log("41", [[1, [2, [3, [4]]]], [], [[], []]]) })
        L(42, function() { var a = [1, 2]; a.toString = function() { return "arrts" }; console.log("42", a) })
        L(43, function() { console.info("43", [1]); console.warn("43w", [2]); console.error("43e", [3]); console.debug("43d", [4]) })
        L(44, function() { console.log("44", ["a,b", "c"], [1.0, 2.50, 1e21, -0]) })
        L(45, function() { console.log("45", root, [root].length) })
        L(46, function() { console.log("46", root.dt, [new Date(0)]) })
        L(47, function() { console.log("47", String([1, [2, 3]]), "" + new Error("boom"), `${[1, 2]}`) })
        Qt.quit()
    }
}
