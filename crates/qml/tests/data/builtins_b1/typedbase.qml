import QtQuick
Item {
    function t(name, f) { var s; try { s = String(f()) } catch (e) { s = "THROW " + e.name + ": " + e.message } console.log(name + " => " + s) }
    Component.onCompleted: {
        var T = Object.getPrototypeOf(Uint8Array)
        t("u1", function() { return T() })
        t("u2", function() { return new T() })
        t("u3", function() { return typeof T })
        t("u4", function() { return String(T) })
        t("u5", function() { return T.name + "|" + T.length })
        t("u6", function() { return T.call(null) })
        t("u7", function() { return Reflect.construct(T, []) })
        t("u8", function() { return Reflect.apply(T, null, []) })
        t("u9", function() { return T instanceof Function })
        t("u10", function() { return Object.prototype.toString.call(T) })
        t("u11", function() { return T.toString() })
        t("u12", function() { return Function.prototype.toString.call(T) })
        t("u13", function() { return [T].map(T) })
        t("u14", function() { class A extends T {}; return new A() })
        t("u15", function() { return T(1, 2) })
        t("u16", function() { return Object.getPrototypeOf(T) === Function.prototype })
        t("m1", function() { return new Map().forEach() })
        t("m2", function() { return new Set().forEach() })
        t("m3", function() { return new Map().forEach(1) })
        t("m4", function() { return new Set().forEach("x") })
        t("m5", function() { return [].forEach() })
        t("m6", function() { return new Map([[1, 2]]).forEach(null) })
        t("m7", function() { return new Set([1]).forEach({}) })
        t("m8", function() { return new Uint8Array(0).forEach() })
    }
}
