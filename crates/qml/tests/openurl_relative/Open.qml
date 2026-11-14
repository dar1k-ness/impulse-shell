import QtQuick
// Every address goes to Qt.openUrlExternally as a string, as a url and resolved already.
QtObject {
    readonly property var cases: [
        "rel.html", "sub/a b.html", "sub/a%20b.html", "../up.txt", "./x.txt?q=a%20b&r=c d#f g",
        "sub/../a.txt", "a b/../c d.txt", ".", "./", "..", "/", "foo/", "?q=1", "?q=ü x", "#frag", "#ü x", "",
        "a%2Fb.txt", "ünï.txt", "%C3%BC.txt", "a%E4b.txt", "a+b.txt", "a%25b.txt", "a%zzb.txt",
        "rel with \"quote\".txt", "a|b.txt", "a\\b.txt", "a<b>c.txt", "a{b}c^d`e.txt", "a[b]c.txt", "a'b(c)!*.txt",
        " spaced.txt ", "\ttab.txt", "a\nb.txt", "~/.config/x", "$HOME/x y", "www.example.org", "www.example.org/a b",
        "/abs/a b.txt", "/abs/a%20b.txt", "/abs/ü?q#f", "file:///abs/a b.txt", "file:///abs/a%20b.txt", "FILE:///Abs/X y.txt",
        "file:rel.txt", "file:rel dir/x.txt", "//host/share/f g.txt",
        "https://example.org/a b?q=c d", "https://example.org/a%20b?q=c%20d#x%20y", "https://example.org/%7Euser/ü",
        "HTTPS://Example.ORG/Path", "http://example.org:80/", "http://example.org:99999/", "http://[::1]:8080/x y",
        "https://us er:p w@example.org/x", "https://bücher.example/ü", "https://xn--bcher-kva.example/",
        "https://example.org/?q=a+b&r=%26&s=ü", "https://example.org/a/../b/./c",
        "mailto:a@b.c?subject=x y", "mailto:ü@b.c", "tel:+1 234", "magnet:?xt=urn:btih:abc&dn=a b", "data:text/plain,hello world",
        "about:blank", "javascript:alert(1)", "x:y", "a:b/c", "C:/x y.txt", "localhost:8080/x", "qrc:/x y.qml"
    ]
    readonly property var kinds: ["string", "url", "resolved"]
    function go(at) {
        var c = cases[Math.floor(at / 3)]
        var v = [c, Qt.url(c), Qt.resolvedUrl(c)][at % 3]
        try { return String(Qt.openUrlExternally(v)) } catch (e) { return "throws " + e }
    }
    // Driven from outside, one call at a time: `said` is what the call answered.
    property int at: -1
    property string said
    onAtChanged: said = go(at)
}
