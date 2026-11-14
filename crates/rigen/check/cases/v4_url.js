// URL and URLSearchParams (WHATWG URL Standard, as Qt 6.12's V4 global object has them); only
// programs whose Qt 6.12 result is the standard's (crates/qml/src/url.rs tests the rest).
// v4: [["https://user:pw@example.com:8080/a/b?x=1&y=2#frag","https://example.com:8080","https:","user","pw","example.com:8080","example.com","8080","/a/b","?x=1&y=2","#frag"],"object","x=1&y=2","https://user:pw@example.com:8080/a/b?x=1&y=2#frag","https://user:pw@example.com:8080/a/b?x=1&y=2#frag","\"https://user:pw@example.com:8080/a/b?x=1&y=2#frag\"","https://user:pw@example.com:8080/a/b?x=1&y=2#frag","[object URL]","TypeError: Invalid URL: nope","http://x.org/a/b/c","file:///tmp/x%20y","1.2.0.3","http://example.com/","https://u:p@h.com:99/b#zz","http://q.com:7/b","http://q.com:7","https://z/","TypeError: Invalid URL: bad","https://z/",["1",["1","3"],true,false,"a=1&b=2&a=3",null],"a=1&a=0&b=x&z=b%26c%3D%C3%A9","x=1&y=2","x=1&y=2","a=b%3Dc&=x","","",[],[],[],[],"TypeError",[],"function,function,function,function,function,function,function,function,function,function,function,function,function",true,true,true,"function","function"]
function err(f) { try { return f() } catch (e) { return e.name + ": " + e.message } }
var u = new URL("https://user:pw@example.com:8080/a/b?x=1&y=2#frag");
var parts = ["href", "origin", "protocol", "username", "password", "host", "hostname", "port", "pathname",
  "search", "hash"].map(function (k) { return u[k] });
var s = new URL("http://a/b");
s.hash = "zz"; s.port = "99"; s.hostname = "h.com"; s.protocol = "https"; s.username = "u"; s.password = "p";
var h = new URL("http://a/b");
h.host = "q.com:7";
h.origin = "x";
var r = new URL("http://a/b");
r.href = "https://z/";
var p = new URLSearchParams("a=1&b=2&a=3");
var q = new URLSearchParams("b=2&a=1&c=3&a=0");
q.sort(); q.append("z", "b&c=\u00e9"); q.set("b", "x"); q.delete("c");
var keys = []; for (var k in new URL("http://a/")) keys.push(k);
var result = JSON.stringify([parts, typeof u.searchParams, u.searchParams.toString(), u.toString(), u.toJSON(),
  JSON.stringify(u), String(u), Object.prototype.toString.call(u), err(function () { return new URL("nope") }),
  new URL("b/c", "http://x.org/a/d").href,
  new URL("file:///tmp/x y").href,
  new URL("http://1.2.3/").hostname, new URL("HTTP://EXAMPLE.com/").href,
  s.href, h.href, h.origin, r.href, err(function () { r.href = "bad"; return r.href }), r.href,
  [p.get("a"), p.getAll("a"), p.has("b"), p.has("z"), p.toString(), p.get("z")], q.toString(),
  new URLSearchParams({ x: "1", y: 2 }).toString(), new URLSearchParams([["x", "1"], ["y", "2"]]).toString(),
  new URLSearchParams("a=b=c&&=x").toString(), new URLSearchParams().toString(), new URLSearchParams("").toString(),
  Object.keys(URL.prototype), Object.keys(URLSearchParams.prototype),
  Object.keys(new URL("http://a/")), keys, err(function () { return URL.prototype.href }).split(":")[0],
  ["constructor", "hash", "host", "hostname", "href", "origin", "password", "pathname", "port", "protocol", "search",
    "searchParams", "toJSON", "toString", "username"].filter(function (k) { return !(k in URL.prototype) }),
  ["append", "constructor", "delete", "entries", "forEach", "get", "getAll", "has", "keys", "set", "sort", "toString",
    "values"].map(function (k) { return typeof URLSearchParams.prototype[k] }).join(),
  URL.prototype.constructor === URL, new URL("http://a/") instanceof URL,
  new URLSearchParams("a=1") instanceof URLSearchParams, typeof URL, typeof URLSearchParams]);
