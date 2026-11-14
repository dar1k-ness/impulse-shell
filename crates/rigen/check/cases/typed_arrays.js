// ArrayBuffer, DataView and typed arrays (ECMA-262 §23.2, §25.1, §25.3): element conversion,
// indexed access, shared buffers, methods, and the Array/JSON interplay.
// v4: ["255,0,2,2,0,0,7|-128,-1,127,3,-3,0|4294967295,0|1.100000023841858","1,12,3,4||4|true|false|0,1,2,3","2|4|255,254,0,9,0,0,192,63|65534|1.5|5|true","100,200,44,144|3,4|10|2|false|3,4|0,7,7,7,0|3,4,3,4|1,4464|1,2,3","{\"0\":4,\"1\":5}|{\"a\":{\"0\":1}}|{}|4,5|1|false|4,5|4,5|[object Float64Array]","RangeError,RangeError,RangeError,RangeError"]
function conv() {
    return [String(new Uint8ClampedArray([300, -5, 1.5, 2.5, 0.5, NaN, "7"])),
        String(new Int8Array([128, 255, -129, 3.9, -3.9, Infinity])),
        String(new Uint32Array([-1, 4294967296.5])), String(new Float32Array([1.1]))].join("|")
}
function indexed() {
    var a = new Uint8Array(4);
    a[0] = 257; a[1] = "12"; a[5] = 9; a.length = 7;
    var i = 2;
    a[i] = 3; a[i + 1] += 4;
    return [a.join(), a[5], a.length, 1 in a, 4 in a, Object.keys(a).join()].join("|")
}
function views() {
    var b = new ArrayBuffer(8), u = new Uint8Array(b, 2, 4), d = new DataView(b);
    u.subarray(1)[0] = 9;
    d.setInt16(0, -2);
    d.setFloat32(4, 1.5, true);
    return [u.byteOffset, u.length, new Uint8Array(b).join(), d.getUint16(0), d.getFloat32(4, true),
        b.slice(2, -1).byteLength, ArrayBuffer.isView(d)].join("|")
}
function methods() {
    var a = new Uint8Array([1, 2, 3, 4]);
    return [a.map(function (x) { return x * 100 }).join(), a.filter(function (x) { return x > 2 }).join(),
        a.reduce(function (s, x) { return s + x }), a.indexOf(3), a.includes(5), a.slice(-2).join(),
        new Uint8Array(5).fill(7, 1, -1).join(), a.copyWithin(0, 2).join(),
        Int16Array.of(1, 70000).join(), Uint8Array.from("123").join()].join("|")
}
function interplay() {
    var t = new Uint8Array([4, 5]);
    return [JSON.stringify(t), JSON.stringify({ a: new Int8Array([1]) }), JSON.stringify(new ArrayBuffer(2)),
        Array.from(t).join(), [].concat(t).length, Array.isArray(t), [...t].join(), "" + t,
        Object.prototype.toString.call(new Float64Array(1))].join("|")
}
function errors() {
    var r = [];
    [function () { new Uint8Array(-1) }, function () { new Int32Array(new ArrayBuffer(7)) },
        function () { new DataView(new ArrayBuffer(4)).getInt32(1) }, function () { new Uint8Array(2).set([1, 2, 3]) }]
        .forEach(function (f) { try { f() } catch (e) { r.push(e.name) } });
    return r.join()
}
var result = JSON.stringify([conv(), indexed(), views(), methods(), interplay(), errors()]);
