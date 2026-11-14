import QtQml

// What is asked of each locale of answers.qt: its numeric symbols, numbers, dates and times
// written in its digits, and the same read back. `show(name)` answers one line a question,
// its label and the answer after a tab; an empty name is the system locale.
QtObject {
    // A string's UTF-16 units: a digit outside the BMP is two.
    function units(s) {
        var r = []
        for (var i = 0; i < s.length; i++)
            r.push(s.charCodeAt(i).toString(16))
        return r.join(" ")
    }
    function text(v) {
        if (v instanceof Date)
            return isNaN(v) ? "invalid date" : "date " + v.getTime()
        return typeof v + " " + v
    }
    // The time of a parsed time alone: its date is today's.
    function time(v) {
        return isNaN(v) ? "invalid date"
                        : v.getHours() + ":" + v.getMinutes() + ":" + v.getSeconds() + "." + v.getMilliseconds()
    }
    function show(name) {
        var l = name === "" ? Qt.locale() : Qt.locale(name), r = []
        function q(label, f) {
            var answer
            try { answer = f() } catch (e) { answer = "throws " + e.message }
            r.push(label + "\t" + answer)
        }
        var d = new Date(2024, 0, 10, 9, 5, 7, 45), far = new Date(12345, 0, 1), old = new Date(-5, 0, 1)
        var symbols = ["zeroDigit", "decimalPoint", "groupSeparator", "negativeSign", "positiveSign", "exponential", "percent"]
        q("name", () => l.name)
        symbols.forEach(p => q(p, () => l[p] + " length " + l[p].length + " units " + units(l[p])))
        q("zeroDigit.charAt", () => units(l.zeroDigit.charAt(0)) + "|" + units(l.zeroDigit.charAt(1)))

        q("number", () => (1234567.891).toLocaleString(l))
        q("number f 0", () => (-9876543210).toLocaleString(l, 'f', 0))
        q("number e 3", () => (-1234.5).toLocaleString(l, 'e', 3))
        q("number E 2", () => (0.000012345).toLocaleString(l, 'E', 2))
        q("number g 4", () => (1234567.891).toLocaleString(l, 'g', 4))
        q("number length", () => {
            var s = (1234567890).toLocaleString(l, 'f', 0)
            return s.length + " charAt(0) " + units(s.charAt(0)) + " charAt(last) " + units(s.charAt(s.length - 1))
        })
        q("currency", () => (1234.5).toLocaleCurrencyString(l) + "|" + (-0.5).toLocaleCurrencyString(l))
        q("default locale", () => (1234567.891).toLocaleString() + "|" + "%L1".arg(-1234567) + "|" + "%L1".arg(1.5))

        q("Number.fromLocaleString local", () => text(Number.fromLocaleString(l, (1234567.891).toLocaleString(l))))
        q("Number.fromLocaleString local e", () => text(Number.fromLocaleString(l, (-1234.5).toLocaleString(l, 'e', 3))))
        q("Number.fromLocaleString ASCII", () => text(Number.fromLocaleString(l, "1234.5")))
        q("Number.fromLocaleString ASCII grouped", () => text(Number.fromLocaleString(l, "-1,234.5")))
        q("Number.fromLocaleString ASCII digits", () => text(Number.fromLocaleString(l, "1234" + l.decimalPoint + "5")))
        q("Number.fromLocaleString mixed digits", () => text(Number.fromLocaleString(l, "1" + l.zeroDigit + "2")))

        q("toLocaleString", () => d.toLocaleString(l))
        q("toLocaleString short", () => d.toLocaleString(l, Locale.ShortFormat))
        q("toLocaleDateString", () => d.toLocaleDateString(l) + "|" + d.toLocaleDateString(l, Locale.ShortFormat))
        q("toLocaleTimeString", () => d.toLocaleTimeString(l) + "|" + d.toLocaleTimeString(l, Locale.ShortFormat))
        q("default formats", () => d.toLocaleString() + "|" + d.toLocaleDateString() + "|" + d.toLocaleTimeString())
        q("fields", () => d.toLocaleString(l, 'd dd M MM yy yyyy h hh H HH m mm s ss'))
        q("milliseconds", () => [45, 500, 40, 0].map(ms => new Date(2024, 0, 10, 9, 5, 7, ms).toLocaleString(l, 'z|zz|zzz')).join(" "))
        q("zone", () => d.toLocaleString(l, 't|tt|ttt|tttt'))
        q("year 12345", () => far.toLocaleString(l, 'yyyy|yy'))
        q("year -5", () => old.toLocaleString(l, 'yyyy|yy'))
        q("date length", () => {
            var s = d.toLocaleString(l, 'yyyy')
            return s.length + " charAt(0) " + units(s.charAt(0)) + " charAt(last) " + units(s.charAt(s.length - 1))
        })
        q("Qt.formatDateTime", () => Qt.formatDateTime(d, l, Locale.ShortFormat) + "|" + Qt.formatDateTime(d, l, Locale.LongFormat))
        q("Qt.formatDate", () => Qt.formatDate(d, l, Locale.ShortFormat) + "|" + Qt.formatDate(d, l, Locale.LongFormat))
        q("Qt.formatTime", () => Qt.formatTime(d, l, Locale.ShortFormat) + "|" + Qt.formatTime(d, l, Locale.LongFormat))
        q("Qt.format default", () => Qt.formatDateTime(d, 'd/M/yy h:m:s.z t|tt|ttt') + "|" + Qt.formatDate(d) + "|" + Qt.formatTime(d))

        var fields = 'yyyy M d H m s z'
        q("Date.fromLocaleString local", () => text(Date.fromLocaleString(l, d.toLocaleString(l, fields), fields)))
        q("Date.fromLocaleString ASCII", () => text(Date.fromLocaleString(l, "2024 1 10 9 5 7 045", fields)))
        q("Date.fromLocaleString padded", () => text(Date.fromLocaleString(l, d.toLocaleString(l, 'yyyyMMddHHmmsszzz'), 'yyyyMMddHHmmsszzz')))
        q("Date.fromLocaleString short", () => text(Date.fromLocaleString(l, d.toLocaleString(l, Locale.ShortFormat), Locale.ShortFormat)))
        q("Date.fromLocaleString year 12345", () => text(Date.fromLocaleString(l, far.toLocaleString(l, 'yyyy M d'), 'yyyy M d')))
        q("Date.fromLocaleString year -5", () => text(Date.fromLocaleString(l, old.toLocaleString(l, 'yyyy M d'), 'yyyy M d')))
        q("Date.fromLocaleString ASCII signs", () => ["+12345 1 1", "-0006 1 1", "12345 1 1", "\u22120006 1 1"].map(s => text(Date.fromLocaleString(l, s, 'yyyy M d'))).join("|"))
        q("Date.fromLocaleDateString local", () => text(Date.fromLocaleDateString(l, d.toLocaleDateString(l, Locale.ShortFormat), Locale.ShortFormat)))
        q("Date.fromLocaleDateString ASCII", () => text(Date.fromLocaleDateString(l, "10.01.2024", "dd.MM.yyyy")))
        q("Date.fromLocaleTimeString local", () => time(Date.fromLocaleTimeString(l, d.toLocaleTimeString(l, Locale.ShortFormat), Locale.ShortFormat)))
        q("Date.fromLocaleTimeString ASCII", () => time(Date.fromLocaleTimeString(l, "09:05:07.045", "HH:mm:ss.zzz")))
        return r.join("\n")
    }
}
