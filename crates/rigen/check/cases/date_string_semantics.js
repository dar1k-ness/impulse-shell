// Date called as a function returns a string (ECMA-262 §21.4.2.1), Date.prototype[@@toPrimitive]
// (§21.4.4.45), Date.prototype.toJSON on any receiver (§21.4.4.37), Date.parse's 24:00 and
// ±8.64e15 edges (§21.4.1.32) and String.prototype.normalize's RangeError (§22.1.3.15).
// v4: string|string|string|true|object|"function"|1|"[Symbol.toPrimitive]"|0|true|true|throw TypeError|throw TypeError|throw TypeError|7|"s"|false|true|-1|null|null|"x"|throw TypeError|throw TypeError|null|"1970-01-01T00:00:00.000Z"|8640000000000000|-8640000000000000|NaN|1583020800000|1583020800000|1583017200000|-62198755200000|throw RangeError|throw RangeError|throw RangeError|1|2|1|1|"fi"|"fi"
function t(f) {
    try {
        var v = f();
        return typeof v === "string" ? JSON.stringify(v) : String(v);
    } catch (e) {
        return "throw " + e.name;
    }
}
var d0 = new Date(0);
var tp = Date.prototype[Symbol.toPrimitive];
var result = [
    typeof Date(),
    typeof Date(0),
    typeof Date(2020, 1, 1),
    Date(0) === Date(),
    typeof new Date(),
    t(function () { return typeof tp; }),
    t(function () { return tp.length; }),
    t(function () { return tp.name; }),
    t(function () { return d0[Symbol.toPrimitive]("number"); }),
    t(function () { return d0[Symbol.toPrimitive]("default") === d0.toString(); }),
    t(function () { return d0[Symbol.toPrimitive]("string") === d0.toString(); }),
    t(function () { return d0[Symbol.toPrimitive]("other"); }),
    t(function () { return d0[Symbol.toPrimitive](); }),
    t(function () { return tp.call(1, "number"); }),
    t(function () { return tp.call({valueOf: function () { return 7; }}, "number"); }),
    t(function () { return tp.call({toString: function () { return "s"; }}, "default"); }),
    t(function () { return Object.getOwnPropertyDescriptor(Date.prototype, Symbol.toPrimitive).writable; }),
    t(function () { return d0 + 1 === d0.toString() + "1"; }),
    t(function () { return d0 - 1; }),
    t(function () {
        return Date.prototype.toJSON.call({valueOf: function () { return NaN; }, toISOString: function () { return "x"; }});
    }),
    t(function () {
        return Date.prototype.toJSON.call({valueOf: function () { return Infinity; }, toISOString: function () { return "x"; }});
    }),
    t(function () {
        return Date.prototype.toJSON.call({valueOf: function () { return 5; }, toISOString: function () { return "x"; }});
    }),
    t(function () { return Date.prototype.toJSON.call({valueOf: function () { return 5; }}); }),
    t(function () { return Date.prototype.toJSON.call(null); }),
    t(function () { return new Date(NaN).toJSON(); }),
    t(function () { return d0.toJSON(); }),
    t(function () { return Date.parse("+275760-09-13T00:00:00.000Z"); }),
    t(function () { return Date.parse("-271821-04-20T00:00:00.000Z"); }),
    t(function () { return Date.parse("+275760-09-13T00:00:00.001Z"); }),
    t(function () { return Date.parse("2020-02-29T24:00Z"); }),
    t(function () { return Date.parse("2020-02-29T24:00:00.000Z"); }),
    t(function () { return Date.parse("2020-02-29T24:00:00+01:00"); }),
    t(function () { return Date.parse("-000001-01-01T00:00:00Z"); }),
    t(function () { return "x".normalize("b"); }),
    t(function () { return "x".normalize("nfc"); }),
    t(function () { return "x".normalize(null); }),
    t(function () { return "\u00e9".normalize("NFC").length; }),
    t(function () { return "\u00e9".normalize("NFD").length; }),
    t(function () { return "\u00e9".normalize().length; }),
    t(function () { return "\u00e9".normalize(undefined).length; }),
    t(function () { return "\ufb01".normalize("NFKC"); }),
    t(function () { return "\ufb01".normalize("NFKD"); })
].join("|");
