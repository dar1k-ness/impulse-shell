import QtQuick
Item {
    function t(name, f) { var s; try { s = String(f()) } catch (e) { s = "THROW " + e.name + ": " + e.message } console.log(name + " => " + s.replace(/\n/g, "\\n")) }
    function k(a) { return [a.length, Object.keys(a), Object.getOwnPropertyNames(a), JSON.stringify(Object.getOwnPropertyDescriptors(a))].join("|") }
    function w(a, i, v) { "use strict"; try { a[i] = v; return "ok" } catch (e) { return e.name } }
    Component.onCompleted: {
        t("x4", function() { var a = []; Object.defineProperty(a, 0, {value: 1, writable: false}); return w(a, 0, 2) + "|" + a[0] + "|" + k(a) })
        t("x5", function() { var a = []; Object.defineProperty(a, 2, {value: 1, writable: false}); return w(a, 2, 2) + "|" + a[2] + "|" + k(a) })
    }
}
