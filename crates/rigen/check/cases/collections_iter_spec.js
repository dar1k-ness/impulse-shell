// Spec semantics where Qt 6.12's V4 deviates (ECMA-262 §24.2.3.4, §24.2.3.1, §24.2.5.2.1): a
// Set entry deleted and re-added while iterated moves to the end and is visited again (V4 skips
// the next entry instead), and an entry added after `clear()` is still ahead of a live iterator
// (V4 ends it).
var readd = [];
var sr = new Set([1, 2, 3]);
for (var v of sr) { readd.push(v); if (v == 1) { sr.delete(1); sr.add(1) } if (readd.length > 6) break }
var sc = new Set([1, 2, 3]), it = sc.values(), cleared = [it.next().value];
sc.clear(); sc.add(9);
cleared.push(it.next().value, it.next().done);
var result = JSON.stringify([readd, cleared]);
