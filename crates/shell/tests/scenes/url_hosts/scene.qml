// Hosts outside ASCII and `xn--` hosts in url values: a literal of a url property, a string
// written to one, Qt.url, and two urls compared. `rows()` is one line per check.
import QtQuick
Item {
    id: root
    property url lit0: "http://\u041F\u0420\u0418\u041C\u0415\u0420.\u0420\u0424/a"
    property url lit1: "http://xn--e1afmkfd.xn--p1ai/a"
    property url lit2: "http://b\u00FCcher.com/"
    property url lit3: "http://xn--bcher-kva.de/"
    property url lit4: "http://fa\u00DF.de/"
    property url lit5: "http://xn--a.com/"
    property url lit6: "file://\u0445\u043E\u0441\u0442/a"
    property url lit7: "//\u043F\u0440\u0438\u043C\u0435\u0440.\u0440\u0444/a"
    property string host: "ПРИМЕР.РФ"
    property url bound: "http://" + host + "/a"
    property url made: Qt.url("http://" + host + "/a")
    property var held: Qt.url("http://XN--E1AFMKFD.XN--P1AI/a")
    property url u
    property url v
    property var hosts: [
        "http://\u041F\u0420\u0418\u041C\u0415\u0420.\u0420\u0424/a",
        "http://\u043F\u0440\u0438\u043C\u0435\u0440.\u0440\u0444/a",
        "http://xn--e1afmkfd.xn--p1ai/a",
        "http://XN--E1AFMKFD.XN--P1AI/a",
        "http://\u043F\u0440\u0438\u043C\u0435\u0440.com/a",
        "http://xn--e1afmkfd.com/a",
        "http://xn--e1afmkfd.\u0440\u0444/",
        "http://b\u00FCcher.com/",
        "http://B\u00DCCHER.com/",
        "http://bu\u0308cher.com/",
        "http://xn--bcher-kva.com/",
        "http://b\u00FCcher.de/",
        "http://xn--bcher-kva.de/",
        "http://Xn--Bcher-Kva.DE/",
        "http://fa\u00DF.de/",
        "http://xn--fa-hia.de/",
        "http://FASS.de/",
        "http://stra\u00DFe.example/",
        "http://\u03C3\u03C2.gr/",
        "http://\u03A3\u03A3.gr/",
        "http://xn--4xaa.gr/",
        "http://\u65E5\u672C\u8A9E.jp/",
        "http://xn--wgv71a119e.jp/",
        "http://\u4F8B\u3048.\u30C6\u30B9\u30C8/",
        "http://\u65E5\u672C\u8A9E\u3002jp/",
        "http://\uFF45\uFF58\uFF41\uFF4D\uFF50\uFF4C\uFF45.com/",
        "http://a\u00ADb.com/",
        "http://a\u200Db.com/",
        "http://a\u200Cb.com/",
        "http://\u0628\u200C\u0628.com/",
        "http://\u0915\u094D\u200D\u0937.com/",
        "http://\u05D0.com/",
        "http://\u05D0a.com/",
        "http://a\u05D0.com/",
        "http://\u05D01.com/",
        "http://1\u05D0.com/",
        "http://\u05D0\u1B3A\u05D0.com/",
        "http://\u0558.com/",
        "http://\u0301a.com/",
        "http://xn--a.com/",
        "http://xn--.com/",
        "http://xn--a-ecp.ru/",
        "http://xn--80ak6aa92e.com/",
        "http://xn--bcher-kva.xn--p1ai/",
        "http://xn--ls8h.la/",
        "http://\uD83D\uDCA9.la/",
        "http://xn--B\u00FCcher.com/",
        "http://\u0130stanbul.com/",
        "http://\u01C5.com/",
        "http://\u24D0.com/",
        "http://a\u2488com/",
        "http://\u338F.com/",
        "http://-\u00FC.com/",
        "http://\u00FC-.com/",
        "http://ab--\u00FC.com/",
        "http://\u00FC..com/",
        "http://.\u0440\u0444/",
        "http://\u0440\u0444./",
        "http://\u043F\u0440\u0438\u043C\u0435\u0440.\u0440\u0444./a",
        "http://%D0%BF%D1%80%D0%B8%D0%BC%D0%B5%D1%80.%D1%80%D1%84/",
        "http://%d0%bf.com/",
        "http://xn--%62cher-kva.com/",
        "http://user:\u043F\u0430\u0440\u043E\u043B\u044C@\u043F\u0440\u0438\u043C\u0435\u0440.\u0440\u0444:8080/\u043F \u0443\u0442\u044C?\u043A=\u0437#\u044F",
        "http://m\u00FCnchen.example:080/\u00FC?\u00FC=\u00FC#\u00FC",
        "https://\u041F\u0420\u0418\u041C\u0415\u0420.\u0420\u0424",
        "ftp://b\u00FCcher.de/x",
        "ws://\u043F\u0440\u0438\u043C\u0435\u0440.\u0440\u0444:1/",
        "file://\u0445\u043E\u0441\u0442/a",
        "file://xn--n1agdj/a",
        "file://\u0425\u041E\u0421\u0422/a b",
        "//\u043F\u0440\u0438\u043C\u0435\u0440.\u0440\u0444/a",
        "//xn--e1afmkfd.xn--p1ai/a",
        "qs://\u041F\u0420\u0418\u041C\u0415\u0420.\u0420\u0424/a",
        "x-y://\u041F\u0420\u0418\u041C\u0415\u0420.\u0420\u0424/a",
        "mailto:\u043A\u0442\u043E@\u043F\u0440\u0438\u043C\u0435\u0440.\u0440\u0444",
        "http://EXAMPLE.com/",
        "http://ex ample.com/",
        "http://exa\u3000mple.com/",
        "http://a_b.com/",
        "http://\u00FC_b.com/",
        "http://1.2.3.\uFF14/",
        "http://\uFF10x7f.1/",
        "http://[::ffff:1.2.3.4]/",
        "http://\u00FC:x/",
        "\u041F\u0420\u0418\u041C\u0415\u0420",
        "\u043F\u0440\u0438\u043C\u0435\u0440/\u0444\u0430\u0439\u043B.png",
        "/\u0430\u0431\u0441/\u043F\u0443\u0442\u044C \u0441 \u043F\u0440\u043E\u0431\u0435\u043B\u043E\u043C.png",
        "",
    ]
    property var pairs: [
        ["http://\u041F\u0420\u0418\u041C\u0415\u0420.\u0420\u0424/a", "http://\u043F\u0440\u0438\u043C\u0435\u0440.\u0440\u0444/a"],
        ["http://\u043F\u0440\u0438\u043C\u0435\u0440.\u0440\u0444/a", "http://xn--e1afmkfd.xn--p1ai/a"],
        ["http://XN--E1AFMKFD.XN--P1AI/a", "http://\u043F\u0440\u0438\u043C\u0435\u0440.\u0440\u0444/a"],
        ["http://\u043F\u0440\u0438\u043C\u0435\u0440.com/a", "http://xn--e1afmkfd.com/a"],
        ["http://b\u00FCcher.com/", "http://xn--bcher-kva.com/"],
        ["http://bu\u0308cher.com/", "http://b\u00FCcher.com/"],
        ["http://b\u00FCcher.de/", "http://xn--bcher-kva.de/"],
        ["http://fa\u00DF.de/", "http://fass.de/"],
        ["http://fa\u00DF.de/", "http://xn--fa-hia.de/"],
        ["http://\u03C3\u03C2.gr/", "http://\u03A3\u03A3.gr/"],
        ["http://\uFF45\uFF58\uFF41\uFF4D\uFF50\uFF4C\uFF45.com/", "http://example.com/"],
        ["http://a\u00ADb.com/", "http://ab.com/"],
        ["http://\u65E5\u672C\u8A9E\u3002jp/", "http://\u65E5\u672C\u8A9E.jp/"],
        ["http://%D0%BF%D1%80%D0%B8%D0%BC%D0%B5%D1%80.%D1%80%D1%84/", "http://\u043F\u0440\u0438\u043C\u0435\u0440.\u0440\u0444/"],
        ["http://\u043F\u0440\u0438\u043C\u0435\u0440.\u0440\u0444./a", "http://\u043F\u0440\u0438\u043C\u0435\u0440.\u0440\u0444/a"],
        ["http://EXAMPLE.com/", "http://example.com/"],
        ["http://\u043F\u0440\u0438\u043C\u0435\u0440.\u0440\u0444/a", "http://\u043F\u0440\u0438\u043C\u0435\u0440.\u0440\u0444/b"],
        ["http://\u043F\u0440\u0438\u043C\u0435\u0440.\u0440\u0444/%7Ea", "http://\u043F\u0440\u0438\u043C\u0435\u0440.\u0440\u0444/~a"],
        ["http://\u043F\u0440\u0438\u043C\u0435\u0440.\u0440\u0444/\u043F", "http://\u043F\u0440\u0438\u043C\u0435\u0440.\u0440\u0444/%D0%BF"],
        ["http://xn--a.com/", ""],
        ["http://\u0558.com/", ""],
        ["http://xn--a.com/", "http://xn--.com/"],
        ["http://\u0558.com/", "http://\u0558.com/"],
        ["http://xn--.com/", "http://xn--.com/"],
        ["http://xn--.com/", "http://-\u00FC.com/"],
        ["http://xn--.com/a", "http://xn--.com/b"],
        ["http://xn--.com/", "https://xn--.com/"],
        ["http://xn--.com/", "http:///"],
        ["http://xn--.com/", "http:/"],
        ["http://u@xn--.com/", "http://xn--.com/"],
        ["http://xn--.com:8/", "http://xn--.com/"],
        ["http://\u00FC:x/", "http://\u00FC/"],
        ["http://\u00FC:x/", "http://\u00FC:y/"],
        ["http://\u00FC:x/", "http://xn--tda/"],
        ["http://xn--.com/?a", "http://xn--.com/?b"],
        ["http://xn--.com/#a", "http://xn--.com/#a"],
        ["//xn--.com/a", "//xn--.com/a"],
        ["file://xn--.com/a", "file:///a"],
        ["file://\u0445\u043E\u0441\u0442/a", "file://xn--n1agdj/a"],
        ["//\u043F\u0440\u0438\u043C\u0435\u0440.\u0440\u0444/a", "//\u041F\u0420\u0418\u041C\u0415\u0420.\u0420\u0424/a"],
        ["\u041F\u0420\u0418\u041C\u0415\u0420", "\u043F\u0440\u0438\u043C\u0435\u0440"],
        ["", ""],
    ]
    // Printable ASCII as it is, everything else as \uXXXX.
    function esc(s) {
        var out = ""
        for (var i = 0; i < s.length; i++) {
            var c = s.charCodeAt(i)
            if (c >= 32 && c < 127 && c != 92)
                out += s.charAt(i)
            else
                out += "\\u" + ("000" + c.toString(16)).slice(-4)
        }
        return out
    }
    function rows() {
        var out = []
        var lits = [lit0, lit1, lit2, lit3, lit4, lit5, lit6, lit7, bound, made, held]
        for (var i = 0; i < lits.length; i++)
            out.push("literal " + i + ": " + typeof lits[i] + " [" + esc(String(lits[i])) + "]")
        out.push("literal equal: " + (lit0 == lit1) + " " + (lit0 === lit1) + " " + (bound == made) + " "
            + (bound == held) + " " + (made === held) + " " + (lit3 == lit2) + " " + (lit0 != lit1))
        for (var i = 0; i < hosts.length; i++) {
            var s = hosts[i]
            u = s
            var q = Qt.url(s)
            v = q
            out.push("host [" + esc(s) + "]: property [" + esc(String(u)) + "] toString [" + esc(u.toString())
                + "] Qt.url [" + esc(String(q)) + "] Qt.url in property [" + esc(String(v)) + "] "
                + (u == q) + " " + (u === q) + " " + (u == v) + " " + (u == s) + " " + (q == s) + " "
                + (String(u) == s))
        }
        host = "bücher.COM"
        out.push("rebound: [" + esc(String(bound)) + "] [" + esc(String(made)) + "] " + (bound == made))
        for (var i = 0; i < pairs.length; i++) {
            var a = pairs[i][0], b = pairs[i][1]
            u = a
            v = b
            var x = Qt.url(a), y = Qt.url(b)
            out.push("pair [" + esc(a) + "] [" + esc(b) + "]: properties " + (u == v) + " " + (u === v) + " "
                + (u != v) + " " + (u !== v) + " Qt.url " + (x == y) + " " + (x === y) + " " + (x != y)
                + " mixed " + (u == y) + " " + (x == v) + " " + (u == b) + " " + (x == b) + " " + (a == y)
                + " strings " + (String(u) == String(v)) + " " + (x.toString() === y.toString()))
        }
        return out.join("\n")
    }
}
