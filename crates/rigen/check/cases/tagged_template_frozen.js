// GetTemplateObject (ECMA-262 §13.2.8.4): the strings array and its `raw` are frozen, `raw`
// is a non-writable, non-enumerable, non-configurable own property.
// Qt 6.12 reports both frozen yet still lets their elements be written; the runtime follows
// the spec there (qml builtins tests: `template_objects_are_frozen`).
// v4: [true,true,false,false,false,false,true,["TypeError","TypeError","TypeError","undefined|0,1"]]
function tag(s) { return s; }
function f() { return tag`a${1}b`; }
var s = f();
var d = Object.getOwnPropertyDescriptor(s, "raw");
var w = [];
function t(g) { try { w.push(String(g())); } catch (e) { w.push(e.name); } }
t(function () { "use strict"; s.length = 0; return 1; });
t(function () { "use strict"; s.raw = []; return 1; });
t(function () { "use strict"; return delete s[0]; });
t(function () { s.raw.x = 1; return s.raw.x + "|" + Object.keys(s).join(); });
var result = JSON.stringify([Object.isFrozen(s), Object.isFrozen(s.raw), Object.isExtensible(s),
    d.writable, d.enumerable, d.configurable, f() === s, w]);
