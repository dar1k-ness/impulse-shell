import QtQuick
Item {
    function t(name, f) { var s; try { s = String(f()) } catch (e) { s = "THROW " + e.name + ": " + e.message } console.log(name + " => " + s.replace(/\n/g, "\\n")) }
    function k(a) { return [a.length, Object.keys(a), Object.getOwnPropertyNames(a), JSON.stringify(Object.getOwnPropertyDescriptors(a)), Object.isFrozen(a), Object.isSealed(a), Object.isExtensible(a)].join("|") }
    function w(a, i, v) { "use strict"; try { a[i] = v; return "ok" } catch (e) { return e.name } }
    function del(a, i) { "use strict"; try { return delete a[i] } catch (e) { return e.name } }
    function c(f) { "use strict"; try { return String(f()) } catch (e) { return e.name + ": " + e.message } }
    Component.onCompleted: {
        t("t4", function() { var a = [1]; a.length = 100000; Object.freeze(a); return c(function() { return a.push(2) }) + "|" + a.length + "|" + a[100000] + "|" + (100000 in a) })
        t("t7", function() { var a = [1, 2, 3]; a.length = 100000; Object.preventExtensions(a); return [w(a, 3, 9), w(a, 99999, 9), c(function() { return a.push(4) }), a[100000], k(a)].join("|") })
        t("t18", function() { var a = [3, 1, 2]; a.length = 100000; Object.freeze(a); return c(function() { a.sort(); return "ok" }) + "|" + [a[0], a[1], a[2], a[3]] + "|" + a.length })
        t("t24", function() { var a = [1, 2, 3]; Object.preventExtensions(a); a.length = 100000; return [w(a, 3, 1), w(a, 99999, 1), c(function() { return a.push(4) }), a.length, a[3], a[100000]].join("|") })
        t("t31", function() { var a = new Array(100000); for (var i = 0; i < 10; i++) a[i] = i; Object.freeze(a); return [w(a, 3, 9), w(a, 10, 9), w(a, 15, 9), w(a, 16, 9), c(function() { return a.push(1) }), a[3], a[10], a[15], (10 in a), Object.keys(a)].join("|") })
    }
}
