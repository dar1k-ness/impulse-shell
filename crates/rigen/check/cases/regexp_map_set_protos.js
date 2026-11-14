// RegExp, Map and Set have real prototype objects (RegExp.prototype's accessors, Map/Set
// `size` accessors) their instances inherit from, and RegExp has the legacy statics
// (RegExp.$1, lastMatch, ...) the last successful match sets.
// v4: ["|||||||||||||","ab|ab|a|b|||x|x|y|y|b|b|xaby|xaby","b|b||b|||||||b|b|b|b","x|x|||||z|z|w|w|||zxw|zxw","é,a","babc","object","(?:)","","undefined","undefined","/(?:)/","true","true","true","true","b","false","TypeError","TypeError","TypeError","/S/F","a\\/b","giy","i","/a/g","(?:)","/(?:)/","SyntaxError","SyntaxError","SyntaxError","true","false","true","true","false","true","true","0","0","bg","true","ym","(?:)","SyntaxError","(?:)","object","object","TypeError","TypeError","1","false","true","a","true","true","false","true","true","true","TypeError","TypeError","TypeError","true","2","true","5","TypeError","TypeError","3","0","true","2","true","1","0","0","{}","function","undefined","undefined","0"]
function err(f) { try { f(); return 'none'; } catch (e) { return e.name; } }
function statics() {
    return [RegExp.lastMatch, RegExp['$&'], RegExp.$1, RegExp.$2, RegExp.$3, RegExp.$9,
        RegExp.leftContext, RegExp['$`'], RegExp.rightContext, RegExp["$'"],
        RegExp.lastParen, RegExp['$+'], RegExp.input, RegExp.$_].join('|');
}
var r = [statics()];
/(a)(b)?/.exec('xaby');
r.push(statics());
/(a)|(b)/.exec('b');
r.push(statics());
/x(y)?/.test('zxw');
r.push(statics());
'\u00e9xa'.match(/x/);
r.push(RegExp.leftContext + ',' + RegExp.rightContext);
'abc'.match('b');
r.push(RegExp.lastMatch + RegExp.input);
r.push(typeof RegExp.prototype, RegExp.prototype.source, RegExp.prototype.flags,
    RegExp.prototype.global, RegExp.prototype.sticky, String(RegExp.prototype),
    /a/g instanceof RegExp, new RegExp('a') instanceof RegExp, RegExp.prototype.isPrototypeOf(/a/),
    /a/.constructor === RegExp, RegExp.prototype.exec.call(/(b)/, 'abc')[1],
    RegExp.prototype.test.call(/x/, 'abc'), err(function () { RegExp.prototype.exec.call({}, 'a'); }),
    err(function () { RegExp.prototype.test.call(RegExp.prototype, 'a'); }),
    err(function () { RegExp.prototype.compile.call({}, 'a'); }),
    RegExp.prototype.toString.call({source: 'S', flags: 'F'}),
    new RegExp('a/b').source, new RegExp('a', 'yig').flags, new RegExp(/a/g, 'i').flags,
    String(new RegExp(/a/g)), RegExp(undefined).source, String(new RegExp()),
    err(function () { new RegExp('a', 'gg'); }), err(function () { new RegExp('a', 'x'); }),
    err(function () { new RegExp('('); }), 'global' in /a/, /a/.hasOwnProperty('source'),
    /a/.hasOwnProperty('lastIndex'), /a/g.global, /a/.ignoreCase, /a/m.multiline, /a/y.sticky,
    Object.keys(RegExp).length, Object.keys(RegExp.prototype).length);
var re = /a/;
re.compile('b', 'g');
r.push(re.source + re.flags, re.test('abc'));
re.compile(/y/m);
r.push(re.source + re.flags);
re.compile();
r.push(re.source + re.flags, err(function () { re.compile('('); }), re.source);
var m = new Map([[1, 'a']]);
var s = new Set([1, 2, 2]);
var ctx = {n: 0};
new Map([[2, 'x'], [3, 'y']]).forEach(function (v, k) { this.n += k; }, ctx);
r.push(typeof Map.prototype, typeof Set.prototype, err(function () { return Map.prototype.size; }),
    err(function () { return Set.prototype.size; }), m.size, m.hasOwnProperty('size'), 'size' in m,
    Map.prototype.get.call(m, 1), m instanceof Map, s instanceof Set, m instanceof Set,
    Map.prototype.isPrototypeOf(m), m.constructor === Map, s.constructor === Set,
    err(function () { Map.prototype.get.call({}, 1); }), err(function () { Map.prototype.get.call(s, 1); }),
    err(function () { Set.prototype.add.call(m, 1); }), Set.prototype.keys === Set.prototype.values,
    s.size, Set.prototype.has.call(s, 2), ctx.n, err(function () { m.forEach(1); }),
    err(function () { new Map([1]); }), new Set('abca').size, new Map(null).size, m.set(4, 'd') === m,
    m.size, m['delete'](4), m.size, Object.keys(Map.prototype).length, Object.keys(m).length,
    JSON.stringify(m), typeof s.add, typeof m.add, typeof s.get);
s.clear();
r.push(s.size);
var result = JSON.stringify(r.map(String));
