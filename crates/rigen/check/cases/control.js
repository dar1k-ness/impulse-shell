// v4: ["one,two","two","5","def","0,1,10,11,20,21,-2,-4,a,b,in|2","ok,f,TypeError,i0,fi0,fi1,fi2","c:boom,f,TypeError,i0,fi0,fi1,fi2","a","caught 1","override",["fin","inner"]]
function sw(x) {
  let r = [];
  switch (x) {
    case 1: r.push("one");
    case 2: r.push("two"); break;
    case "s": { let q = 5; r.push(q); break }
    default: r.push("def");
  }
  return r.join(",");
}
function loops() {
  let out = [];
  outer: for (let i = 0; i < 4; i++) {
    for (let j = 0; j < 4; j++) {
      if (j == 2) continue outer;
      if (i == 3) break outer;
      out.push(i * 10 + j);
    }
  }
  let k = 0;
  while (true) { k++; if (k > 5) break; if (k % 2) continue; out.push(-k) }
  do { k-- } while (k > 2);
  for (var key in { a: 1, b: 2 }) out.push(key);
  blk: { out.push("in"); break blk; }
  return out.join(",") + "|" + k;
}
function tr(n) {
  let log = [];
  try { if (n == 1) throw new Error("boom"); log.push("ok") } catch (e) { log.push("c:" + e.message) } finally { log.push("f") }
  try { null.x } catch (e) { log.push(e.name) }
  for (let i = 0; i < 3; i++) { try { if (i == 1) continue; if (i == 2) break; log.push("i" + i) } finally { log.push("fi" + i) } }
  return log.join(",");
}
function tf() { try { return "a" } finally { trace.push("fin") } }
var trace = [];
function nested() { try { try { throw 1 } finally { trace.push("inner") } } catch (e) { return "caught " + e } }
function finret() { try { throw 2 } finally { return "override" } }
var result = JSON.stringify([sw(1), sw(2), sw("s"), sw(9), loops(), tr(0), tr(1), tf(), nested(), finret(), trace]);
