import QtQuick
Item {
    id: root
    property var self: ({})
    function t(name, f) {
        try { var r = f(); console.log(name, "ok", r); }
        catch (e) { console.log(name, "threw", e.name + ": " + e.message, e instanceof TypeError); }
    }
    Component.onCompleted: {
        console.log([1, 2, 3]);
        console.log([]);
        console.log([[1, 2], [3, [4, 5]]]);
        console.log(["a", "b c", ""]);
        console.log([1.5, -0, NaN, Infinity, 1e21, 1e-7]);
        console.log([null, undefined, true, false]);
        console.log([{ a: 1 }, { b: [1, 2] }]);
        console.log([root].length, typeof [root][0]);
        console.log([Qt.point(1, 2), Qt.rect(1, 2, 3, 4), Qt.size(1, 2)]);
        console.log([Qt.rgba(1, 0, 0, 1)], ["#fff"]);
        console.log(new Array(3));
        console.log([1, , 3]);
        console.log([1, 2, 3], [4, 5], "x", 6);
        console.log({ a: [1, 2], b: { c: [3] } });
        console.log(JSON.stringify([1, [2, 3]]));
        console.log([new Uint8Array([1, 2])], new Uint8Array([1, 2]));
        console.log(["a"].concat([["b"]]), Array.of(1, "2"));
        console.log(String([1, [2, 3]]), "" + [1, [2, 3]]);
        console.debug([1, "a"]); console.info([2, "b"]); console.warn([3, "c"]); console.error([4, "d"]);
        var cyc = [1]; cyc.push(cyc);
        console.log(cyc);
        var lp = { a: 1 }; lp.self = lp;
        console.log([lp].length);
        // circular JSON
        var o = { a: 1 }; o.me = o;
        t("stringify obj", function() { return JSON.stringify(o) });
        var a = [1]; a.push(a);
        t("stringify arr", function() { return JSON.stringify(a) });
        var p = { x: { y: { z: null } } }; p.x.y.z = p.x;
        t("stringify nested", function() { return JSON.stringify(p) });
        var q = { k: [ { m: null } ] }; q.k[0].m = q;
        t("stringify mixed", function() { return JSON.stringify(q) });
        t("stringify indent", function() { return JSON.stringify(o, null, 2) });
        t("stringify toJSON", function() { var w = { toJSON: function() { return w2 } }; var w2 = { w: w }; return JSON.stringify(w) });
        t("stringify replacer", function() { return JSON.stringify({ a: 1 }, function(k, v) { return k === "" ? v : o }) });
        t("stringify shared", function() { var s = { v: 1 }; return JSON.stringify({ a: s, b: s, c: [s, s] }) });
        root.self = o;
        t("stringify prop", function() { return JSON.stringify(root.self) });
        Qt.quit();
    }
}
