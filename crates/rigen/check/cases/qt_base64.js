// Qt.btoa / Qt.atob: the string overloads (UTF-8, lenient decoding) and the ArrayBuffer and
// array overloads (new ArrayBuffer, strict decoding, "Invalid character" with code 5).
// v4: ["aGk=","w6Q=","hi","hi",8,"65","65533,65533,65","65533,65533,65533,65","AB 97,71,107,61","AB ","AB 97,71,108,112","AB 65,65,61,61","AB 65,81,61,61","AB ","AB 104,105","AB 104,105","AB ","Error|Invalid character|5","Error|Invalid character|5","Error|Invalid character|5","Error|Invalid character|5","Error|Invalid character|5","Error|Invalid character|5","Error|Invalid character|5","Error|Invalid character|5","TypeError","Error|Too many arguments|undefined",true,"string","aHR0cDovL2EvYg==","UUZvbnQoeCw5LC0xLDUsNDAwLDAsMCwwLDAsMCwwLDAsMCwwLDAsMSwsMCwwKQ==","TypeError","TypeError","Error|Invalid character|5"]
var r = [];
function b(v) { return v instanceof ArrayBuffer ? "AB " + Array.from(new Uint8Array(v)) : v }
function codes(s) { var c = []; for (var i = 0; i < s.length; i++) c.push(s.charCodeAt(i)); return c.join() }
function u() { return new Uint8Array(Array.prototype.slice.call(arguments)).buffer }
function t(f) { try { r.push(b(f())) } catch (e) { r.push(e.name == "TypeError" ? e.name : e.name + "|" + e.message + "|" + e.code) } }
t(function () { return Qt.btoa("hi") });
t(function () { return Qt.btoa("\u00e4") });
t(function () { return Qt.atob("aGk") });
t(function () { return Qt.atob(" a G k = ") });
t(function () { return Qt.atob("aGVsbG8=junk").length });
t(function () { return codes(Qt.atob("77u/QQ==")) });
t(function () { return codes(Qt.atob("4oJB")) });
t(function () { return codes(Qt.atob("7aCAQQ==")) });
t(function () { return Qt.btoa(u(104, 105)) });
t(function () { return Qt.btoa(u()) });
t(function () { return Qt.btoa([104, "i", 105.9]) });
t(function () { return Qt.btoa([NaN]) });
t(function () { return Qt.btoa([1], undefined) });
t(function () { return Qt.btoa(5) });
t(function () { return Qt.atob(u(97, 71, 107, 61)) });
t(function () { return Qt.atob(["a", "G", "k"]) });
t(function () { return Qt.atob(["a"]) });
t(function () { return Qt.btoa([256]) });
t(function () { return Qt.btoa(["ab"]) });
t(function () { return Qt.btoa([1, , 2]) });
t(function () { return Qt.atob(Symbol("x")) });
t(function () { return Qt.atob([97, 32, 71, 107]) });
t(function () { return Qt.atob(["a", "="]) });
t(function () { return Qt.atob(["a", "G", "=", "x"]) });
t(function () { return Qt.atob(["-", "_"]) });
t(function () { return Qt.btoa(/a/) });
t(function () { return Qt.btoa("a", "b") });
t(function () { var x = u(1); return Qt.atob(Qt.btoa(x)) !== x });
t(function () { return typeof Qt.btoa(new Date(0)) });
t(function () { return Qt.btoa(Qt.url("http://a/b")) });
t(function () { return Qt.btoa(Qt.font({ family: "x" })) });
t(function () { return Qt.btoa(Qt.vector2d(1, 2)) });
t(function () { return Qt.atob(Qt.matrix4x4()) });
t(function () { return Qt.btoa([Qt.url("http://a/b")]) });
var result = JSON.stringify(r);
