import QtQuick
Item {
    id: root
    property date dt: new Date(0)
    function t(name, f) { var s; try { s = String(f()) } catch (e) { s = "THROW " + e.name + ": " + e.message } console.log(name + " => " + s.replace(/\n/g, "\\n")) }
    function all(d) { return [String(d), d.toString(), d + "", `${d}`, d.toUTCString(), d.toGMTString(), d.toDateString(), d.toTimeString(), isNaN(d) ? "-" : d.toISOString(), JSON.stringify(d), "%1".arg(d)].join(" | ") }
    Component.onCompleted: {
        t("D1", function() { return all(new Date(0)) })
        t("D2", function() { return all(new Date(Date.UTC(2024, 0, 2, 3, 4, 5, 6))) })
        t("D3", function() { return all(new Date(Date.UTC(2024, 6, 14, 13, 4, 5, 6))) })
        t("D4", function() { return all(new Date(NaN)) })
        t("D5", function() { return all(new Date(Date.UTC(-5, 0, 1))) })
        t("D6", function() { return all(new Date(Date.UTC(12345, 0, 1))) })
        t("D7", function() { return all(new Date(Date.UTC(5, 0, 1))).replace(/1905/g, "Y") + " // " + (function(){ var d = new Date(0); d.setUTCFullYear(5); return all(d) })() })
        t("D8", function() { return all(new Date(8.64e15)) })
        t("D9", function() { return all(new Date(-8.64e15)) })
        t("D10", function() { return all(new Date(Date.UTC(2024, 11, 25, 23, 59, 59, 999))) })
        t("D11", function() { return all(new Date(Date.UTC(999, 1, 9, 9, 9, 9))) })
        t("D12", function() { return all(new Date(Date.UTC(0, 0, 1))) + " // " + (function(){ var d = new Date(0); d.setUTCFullYear(0); return all(d) })() })
        t("D13", function() { return all(root.dt) })
        t("D14", function() { console.log(new Date(0), [new Date(0)], {d: new Date(0)}, root.dt, new Date(NaN)); return "" })
        t("D15", function() { return [new Date(0)].toString() + "|" + [new Date(0)].join() + "|" + Object.prototype.toString.call(new Date(0)) })
        t("D16", function() { return new Date(2024, 0, 2, 3, 4, 5).toString() + "|" + new Date(2024, 6, 2, 3, 4, 5).toTimeString() })
        t("D17", function() { return Date.prototype.toString.call(5) })
        t("D18", function() { return Date.prototype.toUTCString.call({}) })
        t("D19", function() { return /^(Mon|Tue|Wed|Thu|Fri|Sat|Sun) [A-Z][a-z][a-z] \d\d? \d\d:\d\d:\d\d \d{4} GMT([+-]\d{4})?$/.test(Date()) })
        t("D20", function() { return [typeof Date(), Date().length > 20, Date.prototype.toGMTString === Date.prototype.toUTCString].join() })
        t("D21", function() { return [new Date(NaN).toDateString(), new Date(NaN).toTimeString(), new Date(NaN).toUTCString(), new Date(NaN).toLocaleString(), new Date(NaN).toLocaleDateString(), new Date(NaN).toLocaleTimeString()].join("|") })
        t("D22", function() { return [Date.parse(new Date(0).toString()), Date.parse(new Date(0).toUTCString()), Date.parse("Thu Jan 1 00:00:00 1970 GMT"), Date.parse("Thu Jan 01 1970 00:00:00 GMT+0000"), Date.parse("Thu, 01 Jan 1970 00:00:00 GMT")].join("|") })
        t("D23", function() { var d = new Date(Date.UTC(2024, 0, 2, 3, 4, 5, 6)); return [Date.parse(d.toString()), Date.parse(d.toUTCString()), Date.parse(d.toDateString()), Date.parse(d.toISOString())].join("|") })
    }
}
