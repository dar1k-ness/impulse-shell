// Promise objects, outside of their jobs.
// v4: ["function",1,"Promise","[object Promise]","[object Promise]","TypeError","TypeError","TypeError",true,true,"function",2,1,1,1,"2:function:function:1",true,true]
var out = [];
function name(f) { try { f(); return "none"; } catch (e) { return e.name; } }
out.push(typeof Promise, Promise.length, Promise.name, Object.prototype.toString.call(Promise.resolve(1)), String(Promise.resolve(1)));
out.push(name(function () { new Promise(5); }), name(function () { Promise.prototype.then.call({}); }), name(function () { Promise.resolve.call(undefined, 1); }));
var p = Promise.resolve(42);
out.push(Promise.resolve(p) === p, p instanceof Promise, typeof p.then, Promise.prototype.then.length, Promise.prototype.catch.length, Promise.all.length, Promise.race.length);
var n;
new Promise(function (a, b) { n = arguments.length + ":" + typeof a + ":" + typeof b + ":" + a.length; });
out.push(n, Promise.all([]) instanceof Promise, Promise.reject(1).then() instanceof Promise);
var result = JSON.stringify(out);
