// Name and length of RegExp, Map, Set and their prototype methods (%Set.prototype.keys% is
// %Set.prototype.values%); the legacy RegExp statics are not enumerable.
// v4: Map:0 ; Map.clear=clear/0 ; Map.constructor=Map/0 ; Map.delete=delete/1 ; Map.entries=entries/0 ; Map.forEach=forEach/1 ; Map.get=get/1 ; Map.has=has/1 ; Map.keys=keys/0 ; Map.set=set/2 ; Map.values=values/0 ; Set:0 ; Set.add=add/1 ; Set.clear=clear/0 ; Set.constructor=Set/0 ; Set.delete=delete/1 ; Set.entries=entries/0 ; Set.forEach=forEach/1 ; Set.has=has/1 ; Set.keys=values/0 ; Set.values=values/0 ; RegExp:2 ; RegExp.compile=compile/2 ; RegExp.constructor=RegExp/2 ; RegExp.exec=exec/1 ; RegExp.test=test/1 ; RegExp.toString=toString/0 ; true ; true ; []
var out = [];
var protos = [
    [Map, ["clear", "constructor", "delete", "entries", "forEach", "get", "has", "keys", "set", "values"]],
    [Set, ["add", "clear", "constructor", "delete", "entries", "forEach", "has", "keys", "values"]],
    [RegExp, ["compile", "constructor", "exec", "test", "toString"]]
];
protos.forEach(function (p) {
    var C = p[0];
    out.push(C.name + ":" + C.length);
    p[1].forEach(function (k) {
        var f = C.prototype[k];
        out.push(C.name + "." + k + "=" + f.name + "/" + f.length);
    });
});
out.push(String(Set.prototype.keys === Set.prototype.values));
out.push(String(Map.prototype.constructor === Map && RegExp.prototype.constructor === RegExp));
out.push(JSON.stringify(Object.keys(RegExp)));
var result = out.join(" ; ");
