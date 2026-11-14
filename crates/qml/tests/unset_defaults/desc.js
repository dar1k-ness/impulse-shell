    function isObj(v) { return typeof v === "object" && v !== null && typeof v.objectName === "string" }
    function isItem(v) { return isObj(v) && (v.childrenRect !== undefined || v.contentItem !== undefined) }
    function esc(s) { return s.replace(/\\/g, "\\\\").replace(/\n/g, "\\n").replace(/\t/g, "\\t") }
    function desc(v) {
        if (v === null) return "null"
        if (v === undefined) return "undefined"
        var t = typeof v
        if (t === "number" || t === "boolean") return t + ":" + v
        if (t === "function") return "function"
        if (t === "string") return "\"" + esc(v) + "\""
        if (isObj(v)) return "object"
        if (typeof v.length === "number") return "list:" + v.length
        return "\"" + esc(String(v)) + "\""
    }
    function keys(o) {
        var r = []
        for (var k in o) {
            var v
            try { v = o[k] } catch (e) { continue }
            if (typeof v !== "function") r.push(k)
        }
        return r
    }
    function walk(o, prefix, depth, out) {
        var ks = keys(o)
        for (var i = 0; i < ks.length; i++) {
            var k = ks[i], v = o[k]
            out.push(prefix + k + "\t" + desc(v))
            // Not into a palette (15 colours in each of 3 groups, the same in every item) nor
            // into a colour.
            if (depth < 2 && k !== "parent" && k !== "palette" && typeof v === "object" && v !== null
                    && !isItem(v) && typeof v.length !== "number" && !/^#[0-9a-f]{6,8}$/.test(String(v)))
                walk(v, prefix + k + ".", depth + 1, out)
        }
    }
    function dump(o) { var out = []; walk(o, "", 0, out); return out.join("\n") }
