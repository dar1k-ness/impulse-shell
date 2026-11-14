// `includes(x, fromIndex)` starts the search at fromIndex (negative: from the end).
// v4: [false,false,true,true,true,false,true,false,false,true]
var a = [1, 2, 3, NaN];
var r = [a.includes(1, 1), a.includes(3, -1), a.includes(3, -2), a.includes(1, -10), a.includes(NaN, 3),
    a.includes(NaN, 4), a.includes(2, NaN), a.includes(2, Infinity), a.includes(1, "1"), a.includes(2, 1.9)];
var result = JSON.stringify(r);
