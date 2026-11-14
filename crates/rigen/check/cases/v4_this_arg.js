// Callback thisArg (ECMA-262 §23.1.3, §24.1.3.5, §24.2.3.6, §23.1.2.1): the callback's `this`.
// v4: [[2,4],[3,4],[3],true,true,3,1,true,0,"number:5",1,"undefined",true,"outer","bound",["a12",14],[2,4],[2,3],"undefined","undefined"]
var ctx = { k: 2 };
var log = [];
[1, 2].forEach(function (x) { log.push(x * this.k) }, ctx);
var m = [1, 2].map(function (x) { return x + this.k }, ctx);
var fl = [1, 2, 3].filter(function (x) { return x > this.k }, ctx);
var so = [1, 2].some(function (x) { return x === this.k }, ctx);
var ev = [1, 2].every(function (x) { return x <= this.k }, ctx);
var fd = [1, 2, 3].find(function (x) { return x > this.k }, ctx);
var fi = [1, 2, 3].findIndex(function (x) { return x === this.k }, ctx);
var same = [0].map(function () { return this === ctx }, ctx)[0];
var holeThis = [, 1].findIndex(function (x) { return x === undefined && this === ctx }, ctx);
var strictPrim = [0].map(function () { "use strict"; return typeof this + ":" + this }, 5)[0];
var strictStr = [0].filter(function () { "use strict"; return this === "s" }, "s").length;
var strictNone = [0].map(function () { "use strict"; return typeof this })[0];
var strictNull = [0].some(function () { "use strict"; return this === null }, null);
var holder = { tag: "outer", run: function () { return [0].map(() => this.tag, { tag: "arg" })[0] } };
var arrow = holder.run();
var bound = [0].map(function () { return this.tag }.bind({ tag: "bound" }), { tag: "arg" })[0];
var mlog = [];
new Map([["a", 1]]).forEach(function (v, k) { mlog.push(k + v + this.k) }, ctx);
new Set([7]).forEach(function (v) { mlog.push(v * this.k) }, ctx);
var from = Array.from([1, 2], function (x) { return x * this.k }, ctx);
var fromLike = Array.from({ length: 2 }, function (_, i) { return i + this.k }, ctx);
var fromStrict = Array.from([0], function () { "use strict"; return typeof this })[0];
var reduceThis = [1].reduce(function () { "use strict"; return typeof this }, 0);
var result = JSON.stringify([log, m, fl, so, ev, fd, fi, same, holeThis, strictPrim, strictStr,
  strictNone, strictNull, arrow, bound, mlog, from, fromLike, fromStrict, reduceThis]);
