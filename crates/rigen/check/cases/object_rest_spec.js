// CopyDataProperties (ECMA-262 §7.3.25) for object rest: an excluded key is never read again,
// the rest copies only own enumerable keys in [[OwnPropertyKeys]] order (integer keys, other
// strings, then symbols), each read through [[GetOwnProperty]] then [[Get]] as it is copied.
var log = [];
var s1 = Symbol("s1"), s2 = Symbol("s2");
function src() {
  var o = {
    get a() { log.push("get a"); return 1 },
    get b() { log.push("get b"); return 2 },
    c: 3,
    [s1]: 4,
    get [s2]() { log.push("get s2"); return 5 },
    7: "seven",
    1: "one"
  };
  Object.defineProperty(o, "hidden", { get() { log.push("get hidden"); return 0 }, enumerable: false });
  return o;
}
function keysOf(r) {
  return Object.keys(r).concat(Object.getOwnPropertySymbols(r).map(function (s) { return s.description }));
}
var out = [];
// Binding pattern: `a` and the symbol `s2` excluded.
var { a, [s2]: x2, ...r1 } = src();
out.push([a, x2, keysOf(r1), r1.b, r1[s1]]);
// Assignment pattern.
var a2, r2;
({ a: a2, ...r2 } = src());
out.push([a2, keysOf(r2)]);
// Function parameter pattern.
function param({ b, ...rest }) { return [b, keysOf(rest)] }
out.push(param(src()));
// Excluded computed keys: ToPropertyKey runs once per key, before its [[Get]].
var kc = 0;
var key = { toString() { log.push("key toString"); kc++; return "a" } };
var { [key]: ka, ["c"]: kcv, ...r3 } = src();
out.push([ka, kcv, kc, keysOf(r3)]);
// A getter deleting a later key, making one non-enumerable and adding one: the key list is
// taken once, each key's descriptor is looked at when it is reached.
var m = {
  get first() { delete this.second; Object.defineProperty(this, "third", { enumerable: false }); this.late = 1; return "f" },
  second: 2, third: 3, fourth: 4
};
var { ...r4 } = m;
out.push(keysOf(r4));
// Proxy source: ownKeys once, then getOwnPropertyDescriptor and get per non-excluded key.
var target = { p: 1, q: 2, [s1]: 3, r: 4 };
Object.defineProperty(target, "ne", { value: 5, enumerable: false, configurable: true });
var px = new Proxy(target, {
  ownKeys(t) { log.push("ownKeys"); return Reflect.ownKeys(t) },
  getOwnPropertyDescriptor(t, k) { log.push("gopd " + String(k)); return Reflect.getOwnPropertyDescriptor(t, k) },
  get(t, k, rcv) { log.push("get " + String(k)); return Reflect.get(t, k, rcv) }
});
var { q, ...r5 } = px;
out.push([q, keysOf(r5)]);
// Spread copies through the same operation: ownKeys once.
var sp = { ...px };
out.push(keysOf(sp));
// Primitive and nullish sources.
var { 0: c0, ...r6 } = "héllo";
out.push([c0, keysOf(r6)]);
var { ...r7 } = 5;
out.push(keysOf(r7));
try { var { ...r8 } = null; out.push("no throw") } catch (e) { out.push(e instanceof TypeError) }
// RequireObjectCoercible comes before any computed key is evaluated.
var nk = 0;
try { var { [(nk++, "k")]: nkv } = undefined; out.push("no throw") } catch (e) { out.push([e instanceof TypeError, nk]) }
// Generator bodies are lowered separately: same rules.
function* gen(o) { var { a, [key]: kk, ...rest } = o; yield [a, kk, keysOf(rest)] }
out.push(gen(src()).next().value);
var got = JSON.stringify([out, log]);
var result = got === "[[[1,5,[\"1\",\"7\",\"b\",\"c\",\"s1\"],2,4],[1,[\"1\",\"7\",\"b\",\"c\",\"s1\",\"s2\"]],[2,[\"1\",\"7\",\"a\",\"c\",\"s1\",\"s2\"]],[1,3,1,[\"1\",\"7\",\"b\",\"s1\",\"s2\"]],[\"first\",\"fourth\"],[2,[\"p\",\"r\",\"s1\"]],[\"p\",\"q\",\"r\",\"s1\"],[\"h\",[\"1\",\"2\",\"3\",\"4\"]],[],true,[true,0],[1,1,[\"1\",\"7\",\"b\",\"c\",\"s1\",\"s2\"]]],[\"get a\",\"get s2\",\"get b\",\"get a\",\"get b\",\"get s2\",\"get b\",\"get a\",\"get s2\",\"key toString\",\"get a\",\"get b\",\"get s2\",\"get q\",\"ownKeys\",\"gopd p\",\"get p\",\"gopd r\",\"get r\",\"gopd ne\",\"gopd Symbol(s1)\",\"get Symbol(s1)\",\"ownKeys\",\"gopd p\",\"get p\",\"gopd q\",\"get q\",\"gopd r\",\"get r\",\"gopd ne\",\"gopd Symbol(s1)\",\"get Symbol(s1)\",\"get a\",\"key toString\",\"get a\",\"get b\",\"get s2\"]]" ? "ok" : got;
