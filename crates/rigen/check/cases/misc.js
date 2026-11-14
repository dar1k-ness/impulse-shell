var o = { a: { b: null, f() { return 1 } }, arr: [1, 2] };
var re = /a(b+)/g;
var s = "xabbbyab".replace(re, "[$1]");
var i = 0; var arr = [10, 20];
arr[i++] += 5;
var u = o?.a?.b?.c, v = o.a?.f(), w = o.zz?.(), x = o.arr?.[1];
o.arr.push(3);
delete o.a.b;
var cnt = 0; cnt ||= 5; let nn = null; nn ??= 3; let aa = 1; aa &&= 7;
var str = "abc"; var chars = []; for (const ch of str) chars.push(ch);
var e1; try { undefinedFn() } catch (e) { e1 = e.message }
var e2; try { o.a.nope() } catch (e) { e2 = e.message }
var e3; try { (1)() } catch (e) { e3 = e.message }
var result = JSON.stringify([s, arr, i, u, v, w, x, o.arr, "b" in o.a, cnt, nn, aa, chars, e1, e2, e3, [..."hi", ...[1]], Math.max(...[1, 5, 3])]);
