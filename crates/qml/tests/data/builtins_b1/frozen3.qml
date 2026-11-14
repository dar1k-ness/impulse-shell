import QtQuick
Item {
    function t(name, f) { var s; try { s = String(f()) } catch (e) { s = "THROW " + e.name + ": " + e.message } console.log(name + " => " + s) }
    Component.onCompleted: {
        t("h1", function() { "use strict"; var o = Object.preventExtensions({length: 1, 0: 1}); o[1] = 2; return JSON.stringify(o) })
        t("h2", function() { "use strict"; var o = Object.preventExtensions({length: 1, 0: 1}); o[5] = 2; return JSON.stringify(o) })
        t("h3", function() { "use strict"; var o = Object.preventExtensions({length: 1, 0: 1}); o[100] = 2; return JSON.stringify(o) })
        t("h4", function() { "use strict"; var o = Object.preventExtensions({a: 1}); o[0] = 2; return JSON.stringify(o) })
        t("h5", function() { "use strict"; var o = Object.preventExtensions({}); o[0] = 2; return JSON.stringify(o) })
        t("h6", function() { "use strict"; var a = Object.preventExtensions([1]); a[1] = 2; return a + "|" + a.length })
        t("h7", function() { "use strict"; var a = Object.preventExtensions([1]); a[2] = 2; return a + "|" + a.length })
        t("h8", function() { "use strict"; var a = Object.preventExtensions([1]); a[50] = 2; return a.length })
        t("h9", function() { "use strict"; var a = Object.preventExtensions([]); a[0] = 2; return a + "|" + a.length })
        t("h10", function() { "use strict"; var a = Object.freeze([1, , 3]); a[1] = 2; return a + "|" + (1 in a) + "|" + Object.keys(a) })
        t("h11", function() { "use strict"; var a = Object.freeze(new Array(3)); a[1] = 2; return a })
        t("h12", function() { "use strict"; var a = [1, 2, 3]; a.length = 1; Object.preventExtensions(a); a[1] = 9; return a + "|" + a.length })
        t("h13", function() { "use strict"; var a = [1, 2, 3]; a.pop(); Object.preventExtensions(a); a[2] = 9; return a + "|" + a.length + "|" + (2 in a) })
        t("h14", function() { "use strict"; var o = Object.preventExtensions({length: 1, 0: 1}); Array.prototype.push.call(o, 2, 3, 4, 5, 6, 7, 8, 9); return JSON.stringify(o) })
        t("h15", function() { "use strict"; var o = Object.freeze({length: 3, 0: 3, 1: 2, 2: 1}); Array.prototype.sort.call(o); return JSON.stringify(o) })
        t("h16", function() { "use strict"; var o = Object.freeze({length: 3, 0: 3, 2: 1}); Array.prototype.sort.call(o); return JSON.stringify(o) })
        t("h17", function() { "use strict"; var o = {length: 2, 1: 1}; Object.defineProperty(o, 0, {value: 2, writable: false, enumerable: true, configurable: true}); Array.prototype.sort.call(o); return JSON.stringify(o) })
        t("h18", function() { "use strict"; var o = Object.freeze({length: 2, 0: 2, 1: 1}); Array.prototype.reverse.call(o); return JSON.stringify(o) })
        t("h19", function() { "use strict"; var o = Object.freeze({length: 2, 0: 2, 1: 1}); o[0] = 7; return JSON.stringify(o) })
        t("h20", function() { var o = Object.freeze({length: 2, 0: 2, 1: 1}); o[0] = 7; o.length = 9; return JSON.stringify(o) })
    }
}
