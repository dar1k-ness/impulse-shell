import QtQuick
Item {
    function t(n, f) { try { console.log(n + " => " + f()) } catch (e) { console.log(n + " => THROW " + e) } }
    Component.onCompleted: {
        t("S1", function(){ class A extends Array {} var a = new A(1, 2, 3); var m = a.map(function(x){ return x }); return (m instanceof A) + "|" + (a.filter(function(){ return true }) instanceof A) + "|" + (a.slice(0) instanceof A) + "|" + (a.concat([1]) instanceof A) + "|" + (a.splice(0, 1) instanceof A) })
        t("S2", function(){ var n = 0; var a = [1, 2]; a.constructor = { [Symbol.species]: function(l) { n++; return [] } }; var m = a.map(function(x){ return x }); return n + "|" + m.length })
        t("S3", function(){ var a = [1]; a.constructor = 5; return [a.filter(function(){ return true }), a.slice(0), a.concat([2]), a.splice(0, 1)].join("|") })
        t("S4", function(){ var a = [1]; a.constructor = { [Symbol.species]: 5 }; return [a.filter(function(){ return true }), a.slice(0), a.concat([2]), a.splice(0, 1)].join("|") })
        t("S5", function(){ var a = [1]; a.constructor = function() { throw new Error("called") }; return a.map(function(x){ return x }) })
        t("S6", function(){ return typeof Array[Symbol.species] + "|" + typeof Symbol.species + "|" + typeof Promise[Symbol.species] + "|" + typeof Map[Symbol.species] + "|" + typeof RegExp[Symbol.species] + "|" + typeof ArrayBuffer[Symbol.species] })
        t("S7", function(){ return [String.prototype.replace.call(undefined, "a", "b"), String.prototype.replace.call(null, "u", "b"), String.prototype.replace.call(undefined, /u/, "b")].join("|") })
        t("S8", function(){ return String.prototype.search.call(undefined, "a") })
        t("S9", function(){ return String.prototype.match.call(undefined, "u") })
        t("S10", function(){ return String.prototype.split.call(undefined, "e") })
        t("S11", function(){ return String.prototype.concat.call(undefined, "e") })
        t("S12", function(){ return String.prototype.trim.call(undefined) })
        t("S13", function(){ class A {} class B extends A { constructor() { this.x = 1 } } try { new B() } catch (e) { return e.name + "|" + e.message + "|" + (e instanceof TypeError) } })
        t("S14", function(){ class A {} class B extends A { constructor() { } } try { new B() } catch (e) { return e.name + "|" + e.message } })
        t("S15", function(){ class A {} class B extends A { constructor() { super(); super() } } try { new B() } catch (e) { return e.name + "|" + e.message } })
        t("S16", function(){ class A {} class B extends A { constructor() { var f = () => this; f() } } try { new B() } catch (e) { return e.name + "|" + e.message } })
        t("S17", function(){ class A {} class B extends A { constructor() { super.x } } try { new B() } catch (e) { return e.name + "|" + e.message } })
    }
}
