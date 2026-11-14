// URLSearchParams iterators (WebIDL pair iterator, URL Standard §6.2): entries/keys/values
// return %URLSearchParamsIteratorPrototype% objects that read the list at each step, also
// after a URL's query is replaced; a sequence init takes any object with an @@iterator. No
// `// v4:` pin: Qt 6.12's V4 deviates from WebIDL (entries/keys/values return Array Iterators
// over a snapshot yielding `[index, pair]`, URLSearchParams has no @@iterator, a Map is no
// sequence init); crates/qml/src/url.rs tests the standard's values.
function err(f) { try { return f() } catch (e) { return e.name } }
var p = new URLSearchParams("a=1&b=2");
var ei = p.entries(), ki = p.keys(), vi = p.values();
var IP = Object.getPrototypeOf(ei);
var steps = [ki.next(), ki.next(), ki.next()];
p.append("c", "3");
steps.push(ki.next(), ki.next());
var live = [];
var q = new URLSearchParams("x=1");
for (var e of q) { live.push(e.join("=")); if (live.length < 3) q.append("y" + live.length, "2") }
var del = [];
var d = new URLSearchParams("a=1&b=2&c=3");
for (var k of d.keys()) { del.push(k); if (k == "a") d.delete("a") }
var u = new URL("http://h/?m=1&n=2"), ui = u.searchParams.values();
var uv = [ui.next().value];
u.search = "?z=9&w=8";
uv.push(ui.next().value);
var result = JSON.stringify([
  Array.isArray(ei), Object.prototype.toString.call(ei), Object.prototype.toString.call(IP),
  IP === Object.getPrototypeOf(ki), IP === Object.getPrototypeOf(vi),
  Object.getPrototypeOf(IP) === Object.getPrototypeOf(Object.getPrototypeOf([].keys())),
  ei[Symbol.iterator]() === ei,
  URLSearchParams.prototype[Symbol.iterator] === URLSearchParams.prototype.entries,
  URLSearchParams.prototype.entries.name, URLSearchParams.prototype.keys.length,
  typeof IP.next, IP.next.length, Object.getOwnPropertyNames(IP),
  [ei.next().value, vi.next().value, vi.next().value],
  steps, live, del, uv,
  Array.from(new URLSearchParams("a=1&b=2").keys()), Array.from(new URLSearchParams("a=1").values()),
  Array.from(new URLSearchParams("a=1")), Array.from(new URLSearchParams("a=1&b=2").entries()),
  new URLSearchParams(new URLSearchParams("a=1&b=2").entries()).toString(),
  new URLSearchParams(new Map([["m", "1"]])).toString(),
  [err(function () { return IP.next.call({}) }), err(function () { return IP.next.call([].keys()) }),
   err(function () { return [].keys().next.call(ki) }), err(function () { return URLSearchParams.prototype.keys.call({}) }),
   err(function () { return new URLSearchParams(["ab"]) }), err(function () { return new URLSearchParams({ [Symbol.iterator]: 5 }) }),
   err(function () { return new URLSearchParams([["a"]]) })]
]);
