    // What three writes to field `f` of the value type property `p` of `t` leave, a new value,
    // the same one again and another one: whether the field then reads the value written
    // (`written`) or what it was (`kept`), and how many times the property's signal came.
    function probe(t, p, f) {
        var n = 0, out = []
        var was = t[p][f]
        var sig = t[p + "Changed"]
        if (sig)
            sig.connect(function() { n++ })
        var vs = [was + 3, was + 3, was + 5]
        for (var i = 0; i < vs.length; i++) {
            try {
                var old = t[p][f]
                t[p][f] = vs[i]
                var now = t[p][f]
                out.push((now === vs[i] ? "written" : now === old ? "kept" : String(now)) + " " + n)
            } catch (e) {
                out.push("!" + e)
            }
            n = 0
        }
        return out.join(" | ")
    }
