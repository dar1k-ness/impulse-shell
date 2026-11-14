// URL Standard parts Qt 6.12's V4 lacks (no `// v4:` pin): the `protocol` setter's scheme
// state override, `URLSearchParams.prototype.size`, and the static `URL.parse`/`URL.canParse`.
function err(f) { try { return f() } catch (e) { return e.name + ": " + e.message } }
function proto(href, p) { var u = new URL(href); u.protocol = p; return u.href }
var sp = new URLSearchParams("a=1&b=2&a=3");
var before = sp.size;
sp.append("c", "4");
var u = new URL("http://h/?x=1&y=2");
var sizes = [before, sp.size, u.searchParams.size];
u.search = "?z=1";
sizes.push(u.searchParams.size, new URLSearchParams().size);
var parsed = URL.parse("/a?q#f", "http://h/b");
var result = JSON.stringify([
  proto("http://a/", "file"), proto("http://localhost/x", "file"), proto("http://a/C:/x", "file"),
  proto("http://u@a/", "file"), proto("http://a:81/", "file"), proto("file:///x", "http"), proto("file://h/x", "http"),
  proto("http://a/", "file:junk"), proto("ws://a:80/", "http"), proto("https://a/p?q#f", "FILE"), proto("http://a/", "x"),
  sizes, err(function () { return Object.getOwnPropertyDescriptor(URLSearchParams.prototype, "size").get.call({}) }),
  URL.canParse("x:"), URL.canParse("/a"), URL.canParse("/a", "http://h"), URL.canParse("/a", "bad"), URL.canParse("http://[::1"),
  URL.parse("bad"), URL.parse("/a", "bad"), String(parsed), parsed instanceof URL, parsed.searchParams.get("q"),
  URL.parse.length, URL.canParse.length, err(function () { return URL.canParse() }), err(function () { return URL.parse() }),
  typeof URL.prototype.parse
]);
