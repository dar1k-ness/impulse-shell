// Qt.binding outside a property assignment: a function object Qt hands back unchanged by
// arrays/objects, which throws when called and checks its argument.
// v4: ["function",true,true,true,true,0,0,true,"TypeError: Bindings must not be called directly.","Error: binding(): argument (binding expression) must be a function","Error: Insufficient arguments","Error: Too many arguments","Error: binding(): argument (binding expression) must be a function","ok","ok",true,true,"{}","[null]"]
function msg(f) { try { f(); return "ok" } catch (e) { return e.name + ": " + e.message } }
function area() { return 6 }
var b = Qt.binding(area);
var r = [];
r.push(typeof b, b !== area, b instanceof Function, b.call === Function.prototype.call);
r.push(b.name === area.name, b.length, Object.keys(b).length, String(b) === "function " + area.name + "() { [native code] }");
r.push(msg(function () { b() }));
r.push(msg(function () { Qt.binding(5) }), msg(function () { Qt.binding() }));
r.push(msg(function () { Qt.binding(area, 2) }), msg(function () { Qt.binding(Math.max) }));
r.push(msg(function () { Qt.binding(area.bind(null)) }), msg(function () { Qt.binding(b) }));
var arr = [b], obj = {x: b};
r.push(arr[0] === b, obj.x === b, JSON.stringify(obj), JSON.stringify(arr));
var result = JSON.stringify(r);
