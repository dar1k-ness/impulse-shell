// `delete a?.b` deletes the property; a nullish base ends the chain and gives true without
// evaluating the rest of it.
// v4: [true,false,true,true,false,true,true,0,true,false]
var a = { b: 1, c: { d: 2 } }, n = null, k = 0;
var r = [delete a?.b, "b" in a, delete n?.b, delete a?.c.d, "d" in a.c, delete n?.c.d];
r.push(delete n?.[k++], k, delete a?.["c"], "c" in a);
var result = JSON.stringify(r);
