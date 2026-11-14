// Array, String, Number, Boolean and Function have real prototype objects whose methods are
// the ones method calls dispatch to, so they can be read, compared and called generically.
// v4: ["object","object","function","object","function","function","function","function","3,2+3","true","x","a-b","aa,bb","true","true","true","7","true","0","1.2","true","true","true","abc3","2,1,false","2","undefined","0","true","undefined"]
function args() { return [].slice.call(arguments).length + ',' + Array.prototype.slice.call(arguments, 1).join('+'); }
var o = {length: 3, 0: 'c', 1: 'a', 2: 'b'};
Array.prototype.sort.call(o);
var p = {length: 2, 0: 1, 1: 2};
var popped = Array.prototype.pop.call(p);
Array.prototype.myLen = function () { return this.length; };
var mine = [1, 2].myLen();
delete Array.prototype.myLen;
var r = [
    typeof Array.prototype, typeof String.prototype, typeof Function.prototype, typeof Number.prototype,
    typeof [].slice, typeof ''.trim, typeof args.call, typeof Function,
    args(1, 2, 3), Array.prototype.slice === [].slice, String.prototype.trim.call('  x  '),
    Array.prototype.join.call({length: 2, 0: 'a', 1: 'b'}, '-'),
    Array.prototype.map.call('ab', function (c) { return c + c; }).join(),
    [] instanceof Array, args instanceof Function, Array.prototype.isPrototypeOf([]),
    Function.prototype.call.call(function () { return this.v; }, {v: 7}),
    'slice' in [], Object.keys(Array.prototype).length, Number.prototype.toFixed.call(1.234, 1),
    [].constructor === Array, ''.constructor === String, args.constructor === Function,
    o[0] + o[1] + o[2] + o.length, popped + ',' + p.length + ',' + (1 in p), mine, typeof [].myLen,
    Array.prototype.length, Array.isArray(Array.prototype), Function.prototype(),
];
var result = JSON.stringify(r.map(String));
