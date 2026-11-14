import QtQuick
Item {
    function d(n, o) { try { console.log(n + " => " + Object.getOwnPropertyNames(o).join(",")) } catch (e) { console.log(n + " => THROW " + e) } }
    Component.onCompleted: {
        var names = ["Object", "Function", "Array", "String", "Number", "Boolean", "Symbol", "Date", "RegExp", "Error", "EvalError", "RangeError", "ReferenceError", "SyntaxError", "TypeError", "URIError", "Map", "Set", "WeakMap", "WeakSet", "Promise", "ArrayBuffer", "SharedArrayBuffer", "DataView", "Proxy", "Int8Array", "Uint8Array", "Uint8ClampedArray", "Int16Array", "Uint16Array", "Int32Array", "Uint32Array", "Float32Array", "Float64Array", "URL", "URLSearchParams"]
        var g = (function() { return this })()
        for (var i = 0; i < names.length; i++) {
            var c = g[names[i]]
            if (c === undefined) { console.log(names[i] + " => MISSING"); continue }
            d(names[i], c)
            if (c.prototype) d(names[i] + ".prototype", c.prototype)
        }
        var ns = ["Math", "JSON", "Reflect", "Atomics"]
        for (var j = 0; j < ns.length; j++) { if (g[ns[j]] === undefined) console.log(ns[j] + " => MISSING"); else d(ns[j], g[ns[j]]) }
        var T = Object.getPrototypeOf(Uint8Array)
        d("%TypedArray%", T)
        d("%TypedArray%.prototype", T.prototype)
        d("%ArrayIteratorPrototype%", Object.getPrototypeOf([][Symbol.iterator]()))
        d("%IteratorPrototype%", Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]())))
        d("%MapIteratorPrototype%", Object.getPrototypeOf(new Map()[Symbol.iterator]()))
        d("%SetIteratorPrototype%", Object.getPrototypeOf(new Set()[Symbol.iterator]()))
        d("%StringIteratorPrototype%", Object.getPrototypeOf(""[Symbol.iterator]()))
        var G = Object.getPrototypeOf(function*() {})
        d("%GeneratorFunction%", G.constructor)
        d("%GeneratorFunction.prototype%", G)
        d("%GeneratorPrototype%", G.prototype)
        d("arguments", (function() { return arguments })(1))
        d("strict arguments", (function() { "use strict"; return arguments })(1))
        d("regexp", /a/g)
        d("exec", /a/.exec("a"))
        d("string", new String("ab"))
        d("date", new Date(0))
        d("bound", (function f(a) {}).bind(null))
        d("array", [1])
        d("template", (function(s) { return s })`a${1}b`)
    }
}
