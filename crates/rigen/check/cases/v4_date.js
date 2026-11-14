// Date (ECMA-262 §21.4, Annex B.2.3) and Qt's QML Date extensions: the statics and
// prototype methods QV4 6.12 has, MakeDay/MakeTime/TimeClip on setters, and
// fromLocale*String parsing. Results avoid the host time zone (UTC methods, explicit
// zones, or local fields read back in local time).
// v4: ["UTC fromLocaleDateString fromLocaleString fromLocaleTimeString timeZoneUpdated","getUTCDate getUTCDay getUTCFullYear getUTCHours getUTCMilliseconds getUTCMinutes getUTCMonth getUTCSeconds getYear setUTCDate setUTCFullYear setUTCHours setUTCMilliseconds setUTCMinutes setUTCMonth setUTCSeconds setYear toGMTString toTimeString toUTCString",true,1704164645006,1704067200000,null,946598400000,-59011459200000,-2208988800000,-62198755200000,915148800000,1738368000000,1701388800000,1703980800000,1704160922001,null,null,1704070800000,8640000000000000,null,-8640000000000000,null,1706745600000,"2024,0,2,2,3,4,5,6","1969,11,31,3,23,59,59,999","-5,0,1,0,0,0,0,0","NaN,NaN,NaN,NaN,NaN,NaN,NaN,NaN","1709380800000|2024-03-02T12:00:00.000Z","1704024000000|2023-12-31T12:00:00.000Z","1677672000000|2023-03-01T12:00:00.000Z","-62006731200000|0005-01-31T12:00:00.000Z","1706749261000|2024-02-01T01:01:01.000Z","1706702340000|2024-01-31T11:59:00.000Z","1706702401002|2024-01-31T12:00:01.002Z","1706702400000|2024-01-31T12:00:00.000Z","NaN|NaN","NaN|NaN","NaN|NaN","NaN|NaN","NaN|NaN","NaN|NaN","NaN|NaN","-1|1969-12-31T23:59:59.999Z","7|1970-01-01T00:00:00.007Z","+275760-09-13T00:00:00.000Z","NaN","1970-01-01T00:00:00.001Z","1969-12-31T23:59:59.999Z","NaN","1999,0,1,0,0,0,0","2024,1,1,1,0,0,0","2024,1,29,0,0,0,0","NaN","2024,2,2,12,0,0,0","5,0,31,12,0,0,0","2024,1,1,12,0,0,0","2024,0,31,1,2,3,4","NaN|NaN","2020,0,1,0,0,0,0",null,"124|true|1999,5,15,10,0,0,0","2001,5,15,10,0,0,0","-1,5,15,10,0,0,0","NaN|NaN","1950,0,1,0,0,0,0",null,true,"Invalid Date","03:04:05","TypeError","TypeError",null,"Locale: Date.timeZoneUpdated(): Invalid arguments","1924-01-02T00:00:00.000Z","NaN","2024-01-02T00:00:00.000Z","NaN","2024-01-02T00:00:00.000Z","NaN","2024-01-02T00:00:00.000Z","2024-01-02T00:00:00.000Z","2024-01-02T00:00:00.000Z","NaN","2024-01-02T00:00:00.000Z","NaN","NaN","NaN","2024-01-01T00:00:00.000Z","1924-01-01T00:00:00.000Z","NaN","NaN","-000004-01-02T00:00:00.000Z","2024-01-01T00:00:00.000Z","NaN","NaN","NaN","NaN","2024-01-02T00:00:00.000Z","2024-02-01T00:00:00.000Z","2024-03-02T00:00:00.000Z","2024-03-07T00:00:00.000Z","NaN","2024-01-02T00:00:00.000Z","NaN","2024-01-01T00:00:00.000Z","NaN","2024-01-01T00:00:00.000Z","2024-01-01T00:00:00.000Z","NaN","NaN","NaN","NaN","2024-01-02T00:00:00.000Z","NaN","Locale: Date.fromLocaleDateString(): Invalid arguments","Locale: Date.fromLocaleDateString(): Invalid arguments","Locale: Date.fromLocaleDateString(): Invalid arguments","Locale: Date.fromLocaleDateString(): Invalid datetime format","Locale: Date.fromLocaleTimeString(): Invalid datetime format","Locale: Date.fromLocaleString(): Invalid arguments","13,2,0,0","0,30,0,0","13,30,0,0","1,2,3,678","1,2,3,600","13,2,0,0","13,2,0,0","NaN","NaN","NaN","NaN","NaN","NaN","NaN","NaN","2024,0,2,13,4,5,0","2024,0,2,15,0,0,0","1900,0,1,13,4,0,0","NaN","2024-01-02T13:04:05.000Z","NaN","13:04 13:04 13:04 13:04|13:04   |13:04   13:04|11:04 11:04  |11:04  11:04 |11:04 11:04 11:04 |11:04   11:04|11:04  11:04 11:04|11:04 11:04  11:04|12:04   |16:34  16:34 16:34|   ","2023-12-31T22:00:00.000Z","2023-12-31T21:00:00.000Z","NaN","7,7,0,4,3,1,0,0"]
function err(f) { try { return f() } catch (e) { return e.name === "Error" ? e.message : e.name } }
function iso(d) { return isNaN(d.getTime()) ? "NaN" : d.toISOString() }
function loc(d) { return isNaN(d.getTime()) ? "NaN" : [d.getFullYear(), d.getMonth(), d.getDate(), d.getHours(), d.getMinutes(), d.getSeconds(), d.getMilliseconds()].join(",") }
var en = Qt.locale("en_US"), de = Qt.locale("de_DE"), ru = Qt.locale("ru_RU"), c = Qt.locale("C");
var u = new Date(Date.UTC(2024, 0, 2, 3, 4, 5, 6));
var UTC_GET = ["getUTCFullYear", "getUTCMonth", "getUTCDate", "getUTCDay", "getUTCHours", "getUTCMinutes", "getUTCSeconds", "getUTCMilliseconds"];
function utc(d) { return UTC_GET.map(function (k) { return d[k]() }).join(",") }
function set(t, k, args) { var d = new Date(t); var r = d[k].apply(d, args); return r + "|" + iso(d) }
var t0 = Date.UTC(2024, 0, 31, 12);
var STATICS = ["UTC", "fromLocaleDateString", "fromLocaleString", "fromLocaleTimeString", "timeZoneUpdated"];
var PROTO = ["getUTCDate", "getUTCDay", "getUTCFullYear", "getUTCHours", "getUTCMilliseconds", "getUTCMinutes",
  "getUTCMonth", "getUTCSeconds", "getYear", "setUTCDate", "setUTCFullYear", "setUTCHours", "setUTCMilliseconds",
  "setUTCMinutes", "setUTCMonth", "setUTCSeconds", "setYear", "toGMTString", "toTimeString", "toUTCString"];
var result = JSON.stringify([
  STATICS.filter(function (k) { return typeof Date[k] === "function" && Date.hasOwnProperty(k) }).join(" "),
  PROTO.filter(function (k) { return typeof u[k] === "function" && Date.prototype.hasOwnProperty(k) }).join(" "),
  Date.prototype.toGMTString === Date.prototype.toUTCString,
  Date.UTC(2024, 0, 2, 3, 4, 5, 6), Date.UTC(2024), Date.UTC(), Date.UTC(99, 11, 31), Date.UTC(100, 0),
  Date.UTC(0), Date.UTC(-1, 0), Date.UTC(99.9), Date.UTC(2024, 13, 1), Date.UTC(2024, -1, 1),
  Date.UTC(2024, 0, 0), Date.UTC(2024, 0, 1, 25, 61, 61, 1001), Date.UTC(NaN), Date.UTC(2024, Infinity),
  Date.UTC(2024, 0, 1.9, 1.9), Date.UTC(275760, 8, 13), Date.UTC(275760, 8, 13, 0, 0, 0, 1),
  Date.UTC(-271821, 3, 20), Date.UTC(-271821, 3, 19, 23, 59, 59, 999), Date.UTC("2024", "1"),
  utc(u), utc(new Date(-1)), utc(new Date(Date.UTC(-5, 0, 1))), utc(new Date(NaN)),
  set(t0, "setUTCMonth", [1]), set(t0, "setUTCDate", [0]), set(t0, "setUTCFullYear", [2023, 1, 29]),
  set(t0, "setUTCFullYear", [5]), set(t0, "setUTCHours", [24, 60, 60, 1000]), set(t0, "setUTCMinutes", [-1]),
  set(t0, "setUTCSeconds", [1.7, 2.9]), set(t0, "setUTCMilliseconds", [-0.5]), set(t0, "setUTCHours", [NaN]),
  set(t0, "setUTCMonth", [Infinity]), set(t0, "setUTCDate", []), set(t0, "setUTCFullYear", [275761]),
  set(NaN, "setUTCMonth", [1]), set(NaN, "setUTCHours", [1]), set(t0, "setTime", [8.64e15 + 1]),
  set(t0, "setTime", [-1.5]), set(t0, "setTime", ["7"]), iso(new Date(8.64e15)), iso(new Date(8.64e15 + 1)),
  iso(new Date(1.9)), iso(new Date(-1.9)), iso(new Date(Infinity)),
  loc(new Date(99, 0)), loc(new Date(2024, 0, 31, 24, 60)), loc(new Date(2024, 1, 29.5)), loc(new Date(NaN, 0)),
  (function () { var d = new Date(2024, 0, 31, 12); d.setMonth(1); return loc(d) })(),
  (function () { var d = new Date(2024, 0, 31, 12); d.setFullYear(5); return loc(d) })(),
  (function () { var d = new Date(2024, 0, 31, 12); d.setFullYear(2023, 13, 1); return loc(d) })(),
  (function () { var d = new Date(2024, 0, 31, 12); d.setHours(1, 2, 3, 4); return loc(d) })(),
  (function () { var d = new Date(2024, 0, 31, 12); return d.setMinutes(NaN) + "|" + d.getTime() })(),
  (function () { var d = new Date(NaN); d.setFullYear(2020); return loc(d) })(),
  (function () { var d = new Date(NaN); return d.setDate(1) })(),
  (function () { var d = new Date(2024, 5, 15, 10); return [d.getYear(), d.setYear(99) === d.getTime(), loc(d)].join("|") })(),
  (function () { var d = new Date(2024, 5, 15, 10); d.setYear(2001); return loc(d) })(),
  (function () { var d = new Date(2024, 5, 15, 10); d.setYear(-1); return loc(d) })(),
  (function () { var d = new Date(2024, 5, 15, 10); return d.setYear(NaN) + "|" + d.getTime() })(),
  (function () { var d = new Date(NaN); d.setYear(50); return loc(d) })(),
  new Date(NaN).getYear(), u.toGMTString() === u.toUTCString(), new Date(NaN).toUTCString(),
  new Date(2024, 0, 2, 3, 4, 5).toTimeString().slice(0, 8),
  err(function () { return Date.prototype.getUTCDate.call({}) }),
  err(function () { return Date.prototype.toUTCString.call(1) }),
  Date.timeZoneUpdated(), err(function () { return Date.timeZoneUpdated(1) }),
  iso(Date.fromLocaleDateString(en, "1/2/24", 1)), iso(Date.fromLocaleDateString(en, "1/2/2024", 1)),
  iso(Date.fromLocaleDateString(en, "Tuesday, January 2, 2024")), iso(Date.fromLocaleDateString(en, "Wednesday, January 2, 2024")),
  iso(Date.fromLocaleDateString(de, "02.01.2024", "dd.MM.yyyy")), iso(Date.fromLocaleDateString(de, "2.1.2024", "dd.MM.yyyy")),
  iso(Date.fromLocaleDateString(de, "2.1.2024", "d.M.yyyy")), iso(Date.fromLocaleDateString(de, "02.01.2024", "d.M.yyyy")),
  iso(Date.fromLocaleDateString(ru, "2 января 2024", "d MMMM yyyy")), iso(Date.fromLocaleDateString(ru, "2 январь 2024", "d MMMM yyyy")),
  iso(Date.fromLocaleDateString(en, "2 JAN 2024", "d MMM yyyy")), iso(Date.fromLocaleDateString(en, "2 Jan 2024", "d MMMM yyyy")),
  iso(Date.fromLocaleDateString(en, " 2024", "yyyy")), iso(Date.fromLocaleDateString(en, "2024x", "yyyy")),
  iso(Date.fromLocaleDateString(en, "2024", "yyyy")), iso(Date.fromLocaleDateString(en, "24", "yy")),
  iso(Date.fromLocaleDateString(en, "024", "yyyy")), iso(Date.fromLocaleDateString(en, "02024", "yyyy")),
  iso(Date.fromLocaleDateString(en, "-0005-01-02", "yyyy-MM-dd")), iso(Date.fromLocaleDateString(en, "+2024", "yyyy")),
  iso(Date.fromLocaleDateString(en, "0000", "yyyy")), iso(Date.fromLocaleDateString(en, "2024-02-30", "yyyy-MM-dd")),
  iso(Date.fromLocaleDateString(en, "1900-02-29", "yyyy-MM-dd")), iso(Date.fromLocaleDateString(en, "2024-13-01", "yyyy-MM-dd")),
  iso(Date.fromLocaleDateString(en, "Tue, 2024", "ddd, yyyy")), iso(Date.fromLocaleDateString(en, "2024-02 Thu", "yyyy-MM ddd")),
  iso(Date.fromLocaleDateString(en, "2024-03 Sat", "yyyy-MM ddd")), iso(Date.fromLocaleDateString(en, "2024-03 Thu", "yyyy-MM ddd")),
  iso(Date.fromLocaleDateString(en, "2024-01-31 Tue", "yyyy-MM-dd ddd")), iso(Date.fromLocaleDateString(en, "2024 2 2", "yyyy d d")),
  iso(Date.fromLocaleDateString(en, "2024 2 3", "yyyy d d")), iso(Date.fromLocaleDateString(en, "24 2024", "yy yyyy")),
  iso(Date.fromLocaleDateString(en, "25 2024", "yy yyyy")), iso(Date.fromLocaleDateString(en, "it's 2024", "'it''s' yyyy")),
  iso(Date.fromLocaleDateString(en, "2024 x 01", "yyyy 'x' MM")), iso(Date.fromLocaleDateString(en, "2024 X 01", "yyyy 'x' MM")),
  iso(Date.fromLocaleDateString(en, "2024  01", "yyyy MM")), iso(Date.fromLocaleDateString(en, "2024-01-02 13:04", "yyyy-MM-dd hh:mm")),
  iso(Date.fromLocaleDateString(en, "5", 5)), iso(Date.fromLocaleDateString(c, "Tuesday, 2 January 2024")),
  iso(Date.fromLocaleDateString(en)),
  err(function () { return Date.fromLocaleDateString() }), err(function () { return Date.fromLocaleDateString(1, "x") }),
  err(function () { return Date.fromLocaleDateString(en, "x", "y", 1) }), err(function () { return Date.fromLocaleDateString(en, "x", {}) }),
  err(function () { return Date.fromLocaleTimeString(en, "x", true) }), err(function () { return Date.fromLocaleString() }),
  loc(Date.fromLocaleTimeString(en, "1:02 PM", "h:mm AP")).split(",").slice(3).join(","),
  loc(Date.fromLocaleTimeString(en, "12:30 am", "h:mm AP")).split(",").slice(3).join(","),
  loc(Date.fromLocaleTimeString(en, "13:30", "h:mm")).split(",").slice(3).join(","),
  loc(Date.fromLocaleTimeString(en, "1:02:03.678", "h:mm:ss.zzz")).split(",").slice(3).join(","),
  loc(Date.fromLocaleTimeString(en, "1:02:03.6", "h:mm:ss.z")).split(",").slice(3).join(","),
  loc(Date.fromLocaleTimeString(en, "1:02\u202fPM", 1)).split(",").slice(3).join(","),
  loc(Date.fromLocaleTimeString(en, "1:02 PM", 1)).split(",").slice(3).join(","),
  iso(Date.fromLocaleTimeString(en, "13:30 PM", "h:mm AP")), iso(Date.fromLocaleTimeString(en, "0 PM", "h AP")),
  iso(Date.fromLocaleTimeString(en, "3 PM", "H AP")), iso(Date.fromLocaleTimeString(en, "1:02:03.67", "h:mm:ss.zzz")),
  iso(Date.fromLocaleTimeString(en, "24:00", "HH:mm")), iso(Date.fromLocaleTimeString(en, "13:02 UTC", "HH:mm t")),
  iso(Date.fromLocaleTimeString(en, "13:04 2024", "hh:mm yyyy")), iso(Date.fromLocaleTimeString(en, "1 P", "h A")),
  loc(Date.fromLocaleString(en, "2024-01-02 13:04:05", "yyyy-MM-dd hh:mm:ss")),
  loc(Date.fromLocaleString(en, "PM 3 2024-01-02", "A h yyyy-MM-dd")),
  loc(Date.fromLocaleString(en, "13:04", "hh:mm")), loc(Date.fromLocaleString(c, "Tuesday, 2 January 2024 13:04:05")),
  iso(Date.fromLocaleString(en, "Tuesday, January 2, 2024 1:04:05 PM UTC")),
  iso(Date.fromLocaleString(en, "Tuesday, January 2, 2024 1:04:05 PM")),
  ["UTC", "Z", "GMT", "+0200", "+02:00", "+02", "UTC+2", "UTC+02:00", "UTC+0200", "GMT+1", "UTC-03:30", "utc"].map(function (z) {
    return ["t", "tt", "ttt", "tttt"].map(function (t) {
      return iso(Date.fromLocaleString(en, "2024-01-02 13:04:05 " + z, "yyyy-MM-dd hh:mm:ss " + t)).slice(11, 16)
    }).join(" ")
  }).join("|"),
  iso(Date.fromLocaleString(en, "2024 +02:00 +02:00", "yyyy ttt ttt")), iso(Date.fromLocaleString(en, "2024 +02:00 +03:00", "yyyy ttt ttt")),
  iso(Date.fromLocaleString(en, "2024 1 PM", "yyyy h")),
  [Date.length, Date.UTC.length, Date.timeZoneUpdated.length, Date.prototype.setUTCHours.length,
   Date.prototype.setUTCFullYear.length, Date.prototype.setYear.length, Date.prototype.getUTCDay.length,
   Date.prototype.toUTCString.length].join(",")
]);
