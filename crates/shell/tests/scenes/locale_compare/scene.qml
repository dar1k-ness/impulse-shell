// What String.prototype.localeCompare gives for every pair of `strings`, in the locale the
// process runs in. dump.sh runs this in Qt 6.12 once per locale and keeps the answers in scene.qt.
import QtQml

QtObject {
    property var strings: [
        // Empty, prefixes, white space.
        "", "a", "ab", "abc", "ab ", "a b", " a", "a ", " ", "  ", "\t", "\n", "\u00A0", "\u3000",
        "\u200B", "a\u200Bb", "\u00ADab", "a\u0000b", "\u0001",
        // Case.
        "A", "B", "b", "Ab", "aB", "AB", "ABC", "Abc", "z", "Z",
        // Latin letters with marks, composed and not.
        "\u00E4", "\u00C4", "a\u0308", "\u00E5", "\u00C5", "\u00F6", "\u00D6", "o", "oe", "\u00F8",
        "\u0153", "\u00E6", "ae", "\u00FC", "u", "ue", "y", "\u00FD", "v", "w", "\u00E9", "e",
        "e\u0301", "\u00E8", "\u00EA", "cote", "c\u00F4te", "cot\u00E9", "c\u00F4t\u00E9",
        "\u00F1", "n", "nz", "\u00DF", "ss", "sz", "\u0131", "i", "I", "\u0130", "\u00E7", "c",
        "cz", "\u015F", "s", "\u011F", "g", "h", "\u0301",
        // Cyrillic.
        "\u0430", "\u0410", "\u0431", "\u044F", "\u042F", "\u0435", "\u0451", "\u0401", "\u0436",
        "\u0435\u0436", "\u0451\u0436", "\u0401\u0436", "\u0435\u0436\u0430", "\u0438", "\u0439",
        "\u044A", "\u044C", "\u0456", "\u0457",
        "\u041F\u0440\u0438\u0432\u0435\u0442", "\u043F\u0440\u0438\u0432\u0435\u0442",
        // Digits.
        "0", "1", "2", "9", "10", "01", "a1", "a2", "a10", "\uFF11", "\u0663", "\u00B2", "1a",
        // Punctuation and symbols.
        "!", "?", ".", ",", "-", "_", "(", ")", "'", "\"", "#", "@", "/", "\\", "$", "\u20AC",
        "\u2026", "\u00AB", "\u2014", "\u2010", "a-b", "a_b", "a.b", "ab-", "-ab", "a'b",
        // Other scripts.
        "\u03B1", "\u03A9", "\u05D0", "\u0628", "\u0E01",
        // CJK.
        "\u4E00", "\u4E8C", "\u4E09", "\u4E2D", "\u56FD", "\u65E5\u672C", "\u65E5\u672C\u8A9E",
        "\u6F22\u5B57", "\u6771\u4EAC", "\u4EAC\u90FD", "\uD840\uDC00", "\u3042", "\u30A2",
        "\uFF71", "\u304B", "\u304C", "\u30AB", "\u30AC", "\u30FC", "\u3042\u30FC",
        "\u3042\u3042", "\u30A2\u30FC", "\uD55C", "\uAC00",
        // Emoji.
        "\uD83D\uDE00", "\uD83D\uDE01", "\uD83D\uDC4D", "\uD83D\uDC4D\uD83C\uDFFD", "\u2764",
        "\u2764\uFE0F", "\uD83D\uDC68\u200D\uD83D\uDC69\u200D\uD83D\uDC67",
        "\uD83C\uDDF7\uD83C\uDDFA", "\uD83C\uDDE9\uD83C\uDDEA", "\u263A",
        // Not characters.
        "\uFFFD", "\uFFFE", "\uFFFF", "\uD800", "a\uDC00",
    ]

    function sign(n) { return n < 0 ? "<" : n > 0 ? ">" : "=" }

    // The strings in the order localeCompare puts them, each with how it compares with the one
    // before it; then every pair whose answer that order does not give (none, when the
    // comparison is an order); then the answers that are not -1, 0 or 1.
    function rows() {
        let n = strings.length
        let order = []
        for (let i = 0; i < n; i++)
            order.push(i)
        order.sort((i, j) => strings[i].localeCompare(strings[j]) || i - j)
        let line = "" + order[0]
        for (let k = 1; k < n; k++)
            line += " " + sign(strings[order[k - 1]].localeCompare(strings[order[k]])) + " " + order[k]
        let place = []
        let rank = 0
        for (let k = 0; k < n; k++) {
            if (k > 0 && strings[order[k - 1]].localeCompare(strings[order[k]]) !== 0)
                rank++
            place[order[k]] = rank
        }
        let odd = []
        let values = {}
        for (let i = 0; i < n; i++) {
            for (let j = 0; j < n; j++) {
                let c = strings[i].localeCompare(strings[j])
                let want = place[i] < place[j] ? "<" : place[i] > place[j] ? ">" : "="
                if (sign(c) !== want)
                    odd.push(i + sign(c) + j)
                if (c < -1 || c > 1)
                    values[i + ":" + j] = c
            }
        }
        let keys = Object.keys(values)
        return "order " + line
            + "\npairs " + n * n + " odd " + odd.length + " " + odd.slice(0, 40).join(" ")
            + "\nvalues " + keys.length + " " + keys.slice(0, 40).map(k => k + "=" + values[k]).join(" ")
    }
}
