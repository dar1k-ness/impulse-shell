// Loops over one long string, each of about as many turns as it has lines: the calls cost
// the string's length, not its square (tests/long_strings.rs counts the work of the compiled
// `loops`).

// `n` lines of one width, every fourth a list entry; kind 1 is not ASCII, kind 2 ends in an
// unpaired surrogate as well.
function text(n, kind) {
    var out = [], word = kind ? "запись в этой заметке, №" : "an entry of the note, number";
    for (var i = 0; i < n; i++)
        out.push((i % 4 ? "  " : "- ") + word + " " + String(100000 + i).slice(1));
    return out.join("\n") + (kind === 2 ? "\ud800" : "");
}

// Each takes the text, the width of its lines in code units and their number.
var loops = {
    // The loop of qml/modules/rusty/overlay/notes/NotesContent.qml (updateCopyListEntries).
    notes: function (textValue) {
        const lineRegex = /(.*?)(\r?\n|$)/g;
        let match = null;
        const parsed = [];
        while ((match = lineRegex.exec(textValue)) !== null) {
            const lineText = match[1];
            const newlineText = match[2];
            const lineStart = match.index;
            const lineEnd = lineStart + lineText.length;
            const bulletMatch = lineText.match(/^\s*-\s+(.*\S)\s*$/);
            if (bulletMatch) {
                parsed.push({
                    content: bulletMatch[1].trim(),
                    start: lineStart,
                    end: lineEnd
                });
            }
            if (newlineText === "")
                break;
        }
        return parsed.length;
    },
    exec: function (s) { var re = /\n/g, k = 0; while (re.exec(s)) k++; return k; },
    test: function (s) { var re = /\n/g, k = 0; while (re.test(s)) k++; return k; },
    match: function (s, w, n) { var k = 0; for (var i = 0; i < n; i++) if (s.match(/^-/)) k++; return k; },
    search: function (s, w, n) { var k = 0; for (var i = 0; i < n; i++) k += s.search(/ /); return k; },
    matchGlobal: function (s) { return s.match(/\n/g).length; },
    matchAll: function (s) { var k = 0; for (const m of s.matchAll(/\n/g)) k++; return k; },
    replace: function (s) { return s.replace(/\n/g, "$&$&").length - s.length; },
    replaceCall: function (s) { var k = 0; s.replace(/\n/g, function (m) { k++; return m; }); return k; },
    split: function (s) { return s.split(/\n/).length; },
    length: function (s, w) { var k = 0; for (var i = 0; i < s.length; i += w) k++; return k; },
    index: function (s, w) { var k = 0, l = s.length; for (var i = 0; i < l; i += w) if (s[i] === "-") k++; return k; },
    charAt: function (s, w) { var k = 0, l = s.length; for (var i = 0; i < l; i += w) if (s.charAt(i) === "-") k++; return k; },
    at: function (s, w) { var k = 0, l = s.length; for (var i = 0; i < l; i += w) if (s.at(i) === "-") k++; return k; },
    charCodeAt: function (s, w) { var k = 0, l = s.length; for (var i = 0; i < l; i += w) if (s.charCodeAt(i) === 45) k++; return k; },
    codePointAt: function (s, w) { var k = 0, l = s.length; for (var i = 0; i < l; i += w) if (s.codePointAt(i) === 45) k++; return k; },
    substring: function (s, w) { var k = 0, l = s.length; for (var i = 0; i < l; i += w) if (s.substring(i, i + 1) === "-") k++; return k; },
    substr: function (s, w) { var k = 0, l = s.length; for (var i = 0; i < l; i += w) if (s.substr(i, 1) === "-") k++; return k; },
    slice: function (s, w) { var k = 0, l = s.length; for (var i = 0; i < l; i += w) if (s.slice(i, i + 1) === "-") k++; return k; },
    startsWith: function (s, w) { var k = 0, l = s.length; for (var i = 0; i < l; i += w) if (s.startsWith("-", i)) k++; return k; },
    endsWith: function (s, w) { var k = 0, l = s.length; for (var i = w; i < l; i += w) if (s.endsWith("\n", i)) k++; return k; },
    indexOf: function (s) { var k = 0, p = 0; while ((p = s.indexOf("\n", p) + 1) > 0) k++; return k; },
    lastIndexOf: function (s) { var k = 0, p = s.length; while ((p = s.lastIndexOf("\n", p) - 1) > -2) k++; return k; },
    has: function (s, w) { var k = 0, l = s.length; for (var i = 0; i < l; i += w) if (Object.prototype.hasOwnProperty.call(s, i)) k++; return k; },
    descriptor: function (s, w) { var k = 0, l = s.length; for (var i = 0; i < l; i += w) if (Object.getOwnPropertyDescriptor(s, i).value === "-") k++; return k; },
    includes: function (s, w) { var k = 0, l = s.length; for (var i = 0; i < l; i += w) if (s.includes("-", i)) k++; return k; }
};

var result = [0, 1, 2].map(function (kind) {
    var s = text(200, kind), w = s.indexOf("\n") + 1;
    return Object.keys(loops).map(function (f) {
        return f + " " + loops[f](s, w, 200);
    }).join(", ");
}).join("; ");
