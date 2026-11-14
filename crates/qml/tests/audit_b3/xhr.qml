import QtQuick
Item {
  id: root
  function show(v) { return v === undefined ? "undefined" : v === null ? "null" : typeof v + ":" + v }
  function w(f) { try { return f() } catch (e) { return "THROW " + e } }
  function run(name, setup, done) {
    var x = new XMLHttpRequest(), log = []
    x.onreadystatechange = function() {
      var thisinfo = (this === undefined) + " " + (this === root) + " " + (this === x) + " " + typeof this + " " + String(this).slice(0, 15)
      var info = arguments.length + " " + (this === x) + " " + typeof this
      log.push(x.readyState + ":" + x.status + ":" + x.statusText + ":" + (x.readyState >= 3 ? x.responseText.length : "-"))
      if (x.readyState === 4) console.log(name + "| states=" + log.join(",") + " => " + w(function() { return done(x, info, thisinfo) }))
    }
    try { setup(x) } catch (e) { console.log(name + "| SETUP THROW " + e) }
  }
  Component.onCompleted: {
    try { console.log("s0| " + show(typeof new XMLHttpRequest().getResponseHeader + " " + typeof new XMLHttpRequest().getAllResponseHeaders + " " + typeof new XMLHttpRequest().overrideMimeType)) } catch (e) { console.log("s0| THROW " + e) }
    try { console.log("s1| " + show(w(function(){ var x = new XMLHttpRequest(); x.send(); return "ok" }))) } catch (e) { console.log("s1| THROW " + e) }
    try { console.log("s2| " + show(w(function(){ var x = new XMLHttpRequest(); x.setRequestHeader("a", "b"); return "ok" }))) } catch (e) { console.log("s2| THROW " + e) }
    try { console.log("s3| " + show(w(function(){ var x = new XMLHttpRequest(); x.open("GET"); return "ok" }))) } catch (e) { console.log("s3| THROW " + e) }
    try { console.log("s4| " + show(w(function(){ var x = new XMLHttpRequest(); x.open(); return "ok" }))) } catch (e) { console.log("s4| THROW " + e) }
    try { console.log("s5| " + show(w(function(){ var x = new XMLHttpRequest(); x.open("BREW", "http://127.0.0.1:18765/ok"); return "ok" }))) } catch (e) { console.log("s5| THROW " + e) }
    try { console.log("s6| " + show(w(function(){ var x = new XMLHttpRequest(); x.open("get", "http://127.0.0.1:18765/ok"); return "ok " + x.readyState }))) } catch (e) { console.log("s6| THROW " + e) }
    try { console.log("s7| " + show(w(function(){ var x = new XMLHttpRequest(); x.open("GET", "http://127.0.0.1:18765/ok", true, "u", "p", 6); return "ok " + x.readyState }))) } catch (e) { console.log("s7| THROW " + e) }
    try { console.log("s8| " + show(w(function(){ var x = new XMLHttpRequest(); x.open("GET", "http://127.0.0.1:18765/ok"); x.setRequestHeader("a"); return "ok" }))) } catch (e) { console.log("s8| THROW " + e) }
    try { console.log("s9| " + show(w(function(){ var x = new XMLHttpRequest(); x.open("GET", "http://127.0.0.1:18765/ok"); x.setRequestHeader(); return "ok" }))) } catch (e) { console.log("s9| THROW " + e) }
    try { console.log("s10| " + show(w(function(){ var x = new XMLHttpRequest(); x.open("GET", "http://127.0.0.1:18765/ok"); x.setRequestHeader("a", "b", "c"); return "ok" }))) } catch (e) { console.log("s10| THROW " + e) }
    try { console.log("s11| " + show(w(function(){ var x = new XMLHttpRequest(); return x.getResponseHeader("a") }))) } catch (e) { console.log("s11| THROW " + e) }
    try { console.log("s12| " + show(w(function(){ var x = new XMLHttpRequest(); return x.getAllResponseHeaders() }))) } catch (e) { console.log("s12| THROW " + e) }
    try { console.log("s13| " + show(w(function(){ var x = new XMLHttpRequest(); x.open("GET", "http://127.0.0.1:18765/ok"); return x.getResponseHeader("a") }))) } catch (e) { console.log("s13| THROW " + e) }
    try { console.log("s14| " + show(w(function(){ var x = new XMLHttpRequest(); x.open("GET", "http://127.0.0.1:18765/ok"); return x.getAllResponseHeaders() }))) } catch (e) { console.log("s14| THROW " + e) }
    try { console.log("s15| " + show(w(function(){ var x = new XMLHttpRequest(); x.open("GET", "http://127.0.0.1:18765/ok"); return x.getResponseHeader() }))) } catch (e) { console.log("s15| THROW " + e) }
    try { console.log("s16| " + show(w(function(){ var x = new XMLHttpRequest(); x.open("GET", "http://127.0.0.1:18765/ok"); return x.getAllResponseHeaders(1) }))) } catch (e) { console.log("s16| THROW " + e) }
    try { console.log("s17| " + show(w(function(){ var x = new XMLHttpRequest(); return x.overrideMimeType("text/plain") }))) } catch (e) { console.log("s17| THROW " + e) }
    try { console.log("s18| " + show(w(function(){ var x = new XMLHttpRequest(); return x.overrideMimeType() }))) } catch (e) { console.log("s18| THROW " + e) }
    try { console.log("s19| " + show(w(function(){ var x = new XMLHttpRequest(); return [x.readyState, x.status, JSON.stringify(x.statusText), JSON.stringify(x.responseText), JSON.stringify(x.response), JSON.stringify(x.responseType), String(x.responseXML), JSON.stringify(x.responseURL), String(x.onreadystatechange)].join(" ") }))) } catch (e) { console.log("s19| THROW " + e) }
    try { console.log("s20| " + show(w(function(){ var x = new XMLHttpRequest(); x.open("GET", "http://127.0.0.1:18765/ok"); return [x.readyState, x.status, JSON.stringify(x.statusText), JSON.stringify(x.responseText), JSON.stringify(x.response), String(x.responseXML), JSON.stringify(x.responseURL)].join(" ") }))) } catch (e) { console.log("s20| THROW " + e) }
    try { console.log("s21| " + show(w(function(){ var x = new XMLHttpRequest(); x.open("GET", "http://127.0.0.1:18765/ok"); x.send(); x.send(); return "ok" }))) } catch (e) { console.log("s21| THROW " + e) }
    try { console.log("s22| " + show(w(function(){ var x = new XMLHttpRequest(); x.open("GET", "http://127.0.0.1:18765/ok"); x.send(); x.setRequestHeader("a", "b"); return "ok" }))) } catch (e) { console.log("s22| THROW " + e) }
    try { console.log("s23| " + show(w(function(){ var x = new XMLHttpRequest(); x.open("GET", "http://127.0.0.1:18765/ok"); x.send(); return x.readyState + " " + x.status + JSON.stringify(x.statusText) }))) } catch (e) { console.log("s23| THROW " + e) }
    try { console.log("s24| " + show(w(function(){ var x = new XMLHttpRequest(); var r = []; for (var t of ["json", "arraybuffer", "text", "", "document", "blob", "bogus", 5, null]) { x.responseType = t; r.push(JSON.stringify(x.responseType)) } return r.join(" ") }))) } catch (e) { console.log("s24| THROW " + e) }
    try { console.log("s25| " + show(w(function(){ var x = new XMLHttpRequest(); x.readyState = 3; x.status = 7; x.statusText = "q"; x.responseText = "t"; x.response = "r"; x.responseXML = 1; x.responseURL = "u"; return [x.readyState, x.status, x.statusText, x.responseText, x.response, x.responseXML, x.responseURL].join("|") }))) } catch (e) { console.log("s25| THROW " + e) }
    try { console.log("s26| " + show(w(function(){ var x = new XMLHttpRequest(); var n = 0; x.onreadystatechange = function() { n++ }; x.open("GET", "http://127.0.0.1:18765/ok"); x.open("GET", "http://127.0.0.1:18765/no"); return n + " " + x.readyState }))) } catch (e) { console.log("s26| THROW " + e) }
    try { console.log("s27| " + show(w(function(){ var x = new XMLHttpRequest(); var n = []; x.onreadystatechange = function() { n.push(x.readyState) }; x.open("GET", "http://127.0.0.1:18765/ok"); x.send(); x.abort(); return n.join(",") + " " + x.readyState + " " + x.status }))) } catch (e) { console.log("s27| THROW " + e) }
    try { console.log("s28| " + show(w(function(){ var x = new XMLHttpRequest(); var n = []; x.onreadystatechange = function() { n.push(x.readyState) }; x.abort(); x.open("GET", "http://127.0.0.1:18765/ok"); x.abort(); return n.join(",") + " " + x.readyState }))) } catch (e) { console.log("s28| THROW " + e) }
    try { console.log("s29| " + show(Object.getOwnPropertyNames(new XMLHttpRequest()).sort().join(","))) } catch (e) { console.log("s29| THROW " + e) }
    try { console.log("s30| " + show(Object.getOwnPropertyNames(Object.getPrototypeOf(new XMLHttpRequest())).sort().join(","))) } catch (e) { console.log("s30| THROW " + e) }
    try { console.log("s31| " + show([XMLHttpRequest.UNSENT, XMLHttpRequest.OPENED, XMLHttpRequest.HEADERS_RECEIVED, XMLHttpRequest.LOADING].join(",") + " " + typeof XMLHttpRequest + " " + XMLHttpRequest.DONE + " " + new XMLHttpRequest().DONE + " " + (new XMLHttpRequest() instanceof XMLHttpRequest))) } catch (e) { console.log("s31| THROW " + e) }
    try { console.log("s32| " + show(String(new XMLHttpRequest()) + " " + Object.prototype.toString.call(new XMLHttpRequest()))) } catch (e) { console.log("s32| THROW " + e) }
    try { console.log("s33| " + show(w(function(){ return XMLHttpRequest() }))) } catch (e) { console.log("s33| THROW " + e) }
    try { console.log("s34| " + show(w(function(){ var x = new XMLHttpRequest(); x.open("GET", "http://127.0.0.1:18765/ok"); x.send({}); return "ok" }))) } catch (e) { console.log("s34| THROW " + e) }
    try { console.log("s35| " + show(w(function(){ var x = new XMLHttpRequest(); x.open("GET", "http://127.0.0.1:18765/ok", false); return "ok" }))) } catch (e) { console.log("s35| THROW " + e) }
    try { console.log("s36| " + show(w(function(){ var x = new XMLHttpRequest(); x.open("GET", ""); return "ok " + x.readyState }))) } catch (e) { console.log("s36| THROW " + e) }
    try { console.log("s37| " + show(w(function(){ var x = new XMLHttpRequest(); x.open("GET", "relative/path"); return "ok " + x.readyState }))) } catch (e) { console.log("s37| THROW " + e) }
    try { console.log("s38| " + show(w(function(){ var x = new XMLHttpRequest(); x.open("GET", "http://127.0.0.1:18765/ok"); x.setRequestHeader("Content-Length", "5"); x.setRequestHeader("Host", "h"); x.setRequestHeader("Sec-x", "h"); x.setRequestHeader("Proxy-x", "h"); return "ok" }))) } catch (e) { console.log("s38| THROW " + e) }
    try { console.log("s39| " + show(w(function(){ var r = []; for (var m of ["GET", "PUT", "HEAD", "POST", "DELETE", "OPTIONS", "PROPFIND", "PATCH", "CONNECT", "TRACE", "patch", "Head", ""]) { var x = new XMLHttpRequest(); try { x.open(m, "http://127.0.0.1:18765/ok"); r.push(m + "+") } catch (e) { r.push(m + "-") } } return r.join(" ") }))) } catch (e) { console.log("s39| THROW " + e) }
    run("merge", function(x) { x.open("POST", "http://127.0.0.1:18765/ok"); x.setRequestHeader("X-A", "1"); x.setRequestHeader("X-A", "2"); x.setRequestHeader("x-b", "3"); x.setRequestHeader("X-B", "4"); x.setRequestHeader("X-C", ""); x.setRequestHeader("X-D", 5); x.setRequestHeader("X-E", null); x.setRequestHeader("Content-Type", "a/b"); x.setRequestHeader("content-type", "c/d"); x.send("body") }, function(x, info, thisinfo) { return x.statusText + " " + x.status + " " + x.responseText })
    run("get", function(x) { x.open("GET", "http://127.0.0.1:18765/ok"); x.send() }, function(x, info, thisinfo) { return x.responseText })
    run("bodyget", function(x) { x.open("GET", "http://127.0.0.1:18765/ok"); x.send("gb") }, function(x, info, thisinfo) { return x.responseText })
    run("put", function(x) { x.open("PUT", "http://127.0.0.1:18765/ok"); x.send("pb") }, function(x, info, thisinfo) { return x.responseText })
    run("del", function(x) { x.open("DELETE", "http://127.0.0.1:18765/ok"); x.send("db") }, function(x, info, thisinfo) { return x.responseText })
    run("head", function(x) { x.open("HEAD", "http://127.0.0.1:18765/ok"); x.send() }, function(x, info, thisinfo) { return x.status + " [" + x.responseText + "] " + x.getResponseHeader("content-length") })
    run("options", function(x) { x.open("OPTIONS", "http://127.0.0.1:18765/ok"); x.send("ob") }, function(x, info, thisinfo) { return x.responseText })
    run("patch", function(x) { x.open("PATCH", "http://127.0.0.1:18765/ok"); x.send("xb") }, function(x, info, thisinfo) { return x.responseText })
    run("lower", function(x) { x.open("post", "http://127.0.0.1:18765/ok"); x.send("lb") }, function(x, info, thisinfo) { return x.responseText })
    run("numbody", function(x) { x.open("POST", "http://127.0.0.1:18765/ok"); x.send(5) }, function(x, info, thisinfo) { return x.responseText })
    run("objbody", function(x) { x.open("POST", "http://127.0.0.1:18765/ok"); x.send({a: 1}) }, function(x, info, thisinfo) { return x.responseText })
    run("nullbody", function(x) { x.open("POST", "http://127.0.0.1:18765/ok"); x.send(null) }, function(x, info, thisinfo) { return x.responseText })
    run("abbody", function(x) { x.open("POST", "http://127.0.0.1:18765/ok"); x.send(new Uint8Array([104, 105]).buffer) }, function(x, info, thisinfo) { return x.responseText })
    run("no", function(x) { x.open("GET", "http://127.0.0.1:18765/no"); x.send() }, function(x, info, thisinfo) { return x.status + " " + x.statusText + " [" + x.responseText + "] " + JSON.stringify(x.response) })
    run("err500", function(x) { x.open("GET", "http://127.0.0.1:18765/zzz"); x.send() }, function(x, info, thisinfo) { return x.status + " " + x.statusText + " [" + x.responseText + "]" })
    run("empty", function(x) { x.open("GET", "http://127.0.0.1:18765/empty"); x.send() }, function(x, info, thisinfo) { return x.status + " " + JSON.stringify(x.statusText) + " [" + x.responseText + "] " + JSON.stringify(x.getAllResponseHeaders()) })
    run("noreason", function(x) { x.open("GET", "http://127.0.0.1:18765/noreason"); x.send() }, function(x, info, thisinfo) { return x.status + " " + JSON.stringify(x.statusText) + " [" + x.responseText + "]" })
    run("refused", function(x) { x.open("GET", "http://127.0.0.1:1/x"); x.send() }, function(x, info, thisinfo) { return x.status + " " + JSON.stringify(x.statusText) + " [" + x.responseText + "] " + JSON.stringify(x.getAllResponseHeaders()) + " " + JSON.stringify(x.responseURL) })
    run("json", function(x) { x.responseType = "json"; x.open("GET", "http://127.0.0.1:18765/json"); x.send() }, function(x, info, thisinfo) { return typeof x.response + " " + x.response.a[1] + " " + x.response.b + " " + JSON.stringify(x.responseType) + " " + x.responseText + " " + (x.response === x.response) })
    run("jsonlate", function(x) { x.open("GET", "http://127.0.0.1:18765/json"); x.send(); x.responseType = "json" }, function(x, info, thisinfo) { return typeof x.response + " " + x.response.b })
    run("badjson", function(x) { x.responseType = "json"; x.open("GET", "http://127.0.0.1:18765/badjson"); x.send() }, function(x, info, thisinfo) { return (function() { try { return typeof x.response } catch (e) { return e.name } })() + " " + x.responseText })
    run("ab", function(x) { x.responseType = "arraybuffer"; x.open("GET", "http://127.0.0.1:18765/bin"); x.send() }, function(x, info, thisinfo) { return (x.response instanceof ArrayBuffer) + " " + x.response.byteLength + " " + new Uint8Array(x.response)[255] + " " + x.responseText.length + " " + (x.response === x.response) })
    run("bintext", function(x) { x.open("GET", "http://127.0.0.1:18765/bin"); x.send() }, function(x, info, thisinfo) { return typeof x.response + " " + x.response.length + " " + x.responseText.length + " " + x.responseText.charCodeAt(200) })
    run("text", function(x) { x.responseType = "text"; x.open("GET", "http://127.0.0.1:18765/ok"); x.send() }, function(x, info, thisinfo) { return typeof x.response + " " + (x.response === x.responseText) })
    run("doc", function(x) { x.responseType = "document"; x.open("GET", "http://127.0.0.1:18765/xml"); x.send() }, function(x, info, thisinfo) { return typeof x.response + " " + String(x.response).slice(0, 30) + " " + JSON.stringify(x.responseType) })
    run("bogus", function(x) { x.responseType = "bogus"; x.open("GET", "http://127.0.0.1:18765/ok"); x.send() }, function(x, info, thisinfo) { return typeof x.response + " " + String(x.response).length + " " + JSON.stringify(x.responseType) })
    run("latin1", function(x) { x.open("GET", "http://127.0.0.1:18765/latin1"); x.send() }, function(x, info, thisinfo) { return x.responseText + " " + x.responseText.length })
    run("hdr", function(x) { x.open("GET", "http://127.0.0.1:18765/ok"); x.send() }, function(x, info, thisinfo) { return [x.getResponseHeader("x-custom"), x.getResponseHeader("X-CUSTOM"), x.getResponseHeader("nope"), x.getResponseHeader("x-other"), x.getResponseHeader("set-cookie"), x.getResponseHeader(""), x.getResponseHeader(5), x.getResponseHeader("Content-Type")].map(v => JSON.stringify(v)).join(" ") })
    run("allhdr", function(x) { x.open("GET", "http://127.0.0.1:18765/ok"); x.send() }, function(x, info, thisinfo) { return JSON.stringify(x.getAllResponseHeaders()) })
    run("redir", function(x) { x.open("GET", "http://127.0.0.1:18765/redir"); x.send() }, function(x, info, thisinfo) { return x.status + " " + x.statusText + " " + x.responseURL + " " + x.response.length })
    run("url", function(x) { x.open("GET", "http://127.0.0.1:18765/ok?a=b c#frag"); x.send() }, function(x, info, thisinfo) { return x.responseURL })
    run("xml", function(x) { x.open("GET", "http://127.0.0.1:18765/xml"); x.send() }, function(x, info, thisinfo) { return typeof x.responseXML + " " + (x.responseXML && x.responseXML.documentElement.nodeName) + " " + (x.responseXML && x.responseXML.documentElement.firstChild.nodeName) + " " + (x.responseXML && x.responseXML.documentElement.attributes[0].value) })
    run("xmltext", function(x) { x.open("GET", "http://127.0.0.1:18765/xmlastext"); x.send() }, function(x, info, thisinfo) { return String(x.responseXML) })
    run("xmlover", function(x) { x.open("GET", "http://127.0.0.1:18765/xmlastext"); x.overrideMimeType("text/xml"); x.send() }, function(x, info, thisinfo) { return String(x.responseXML) + " " + x.getResponseHeader("content-type") })
    run("no0", function(x) { x.open("GET", "http://127.0.0.1:18765/no0"); x.send() }, function(x, info, thisinfo) { return x.status + " " + x.statusText + " [" + x.responseText + "]" })
    run("docjson", function(x) { x.responseType = "document"; x.open("GET", "http://127.0.0.1:18765/ok"); x.send() }, function(x, info, thisinfo) { return typeof x.response + " " + String(x.response).slice(0, 20) })
    run("docsame", function(x) { x.responseType = "document"; x.open("GET", "http://127.0.0.1:18765/xml"); x.send() }, function(x, info, thisinfo) { return (x.response === x.responseXML) + " " + (x.responseXML === x.responseXML) + " " + x.response.documentElement.nodeName + " " + Object.keys(x.response).join(",") })
    run("xmlbad", function(x) { x.open("GET", "http://127.0.0.1:18765/xmlbad"); x.send() }, function(x, info, thisinfo) { return String(x.responseXML) + " " + (x.responseXML && String(x.responseXML.documentElement)) })
    run("xmldom", function(x) { x.open("GET", "http://127.0.0.1:18765/xml"); x.send() }, function(x, info, thisinfo) { return (function() { var d = x.responseXML, e = d.documentElement, k = e.firstChild; return [d.nodeName, d.nodeType, e.nodeName, e.tagName, e.nodeType, e.nodeValue, e.childNodes.length, k.nodeName, k.firstChild.nodeName, k.firstChild.nodeValue, k.firstChild.nodeType, e.attributes.length, e.attributes[0].name, e.attributes[0].nodeName, e.attributes[0].nodeType, e.lastChild.nodeName, Object.keys(e).join("+")].join(" ") })() })
    run("abortdone", function(x) { x.open("GET", "http://127.0.0.1:18765/ok"); x.send() }, function(x, info, thisinfo) { return (function() { x.abort(); return x.readyState + " " + w(function() { return x.status }) })() })
    run("thisis", function(x) { x.open("GET", "http://127.0.0.1:18765/ok"); x.send() }, function(x, info, thisinfo) { return thisinfo })
    run("cbthis", function(x) { x.open("GET", "http://127.0.0.1:18765/ok"); x.send() }, function(x, info, thisinfo) { return "args=" + info })
    run("reuse", function(x) { x.open("GET", "http://127.0.0.1:18765/no"); x.send() }, function(x, info, thisinfo) { return (function() { if (!x.again) { x.again = 1; var s = x.status; x.open("GET", "http://127.0.0.1:18765/ok"); x.send(); return "first " + s + " " + x.readyState + " [" + x.responseText + "]" } return "second " + x.status })() })
    run("reopen", function(x) { x.open("GET", "http://127.0.0.1:18765/no"); x.send(); x.open("GET", "http://127.0.0.1:18765/ok"); x.send() }, function(x, info, thisinfo) { return x.status })
    run("throw", function(x) { x.open("GET", "http://127.0.0.1:18765/ok"); x.send() }, function(x, info, thisinfo) { return (function() { throw new Error("boom") })() })
  }
}
