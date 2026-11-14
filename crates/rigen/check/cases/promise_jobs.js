// Promise jobs run in order once the library body returns.
// No v4 pin: Qt 6.12 runs the reactions as later events (and has no finally/allSettled/any).
// `result` is the log array itself: a library exports its top-level bindings once, as it returns.
var result = [];
function push(x) { result.push(x); }
Promise.resolve().then(function () { push(1); }).then(function () { push(4); });
new Promise(function (a) { a(Promise.resolve()); }).then(function () { push("adopt"); });
Promise.resolve().then(function () { push(2); }).then(function () { push(5); }).then(function () { push(6); });
Promise.all([1, Promise.resolve(2), { then: function (f) { f(3); } }]).then(function (v) { push("all" + v.join("")); });
Promise.race([new Promise(function () {}), Promise.resolve(3)]).then(function (v) { push("race" + v); });
Promise.reject(9).catch(function (v) { push("caught" + v); });
Promise.resolve(5).finally(function () { return 0; }).then(function (v) { push("finally" + v); });
Promise.allSettled([1, Promise.reject(2)]).then(function (v) { push(v.map(function (s) { return s.status; }).join("/")); });
Promise.any([Promise.reject(1), Promise.reject(2)]).catch(function (e) { push(e.name + e.errors.join("")); });
new Promise(function () { throw new TypeError("t"); }).then(null, function (e) { push(e.name); });
push("sync");
