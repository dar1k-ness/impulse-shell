//! QML `XMLHttpRequest` against a local server: the request target the native runtime sends
//! must be the one Qt's QQmlXMLHttpRequest sends (QUrl TolerantMode, `toEncoded()`); the
//! expected targets were recorded from `qs` (Qt 6) on the same URLs. And the states a request
//! goes through, with what it reads in each, must be the ones Qt 6.12 itself (QQmlEngine
//! through the PySide6 of ri-testenv) reports against the same server.
mod event_loop;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

/// Answers every request with `200` and its request target as the body.
fn server() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            std::thread::spawn(move || {
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                loop {
                    let mut h = String::new();
                    if r.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                        break;
                    }
                }
                let target = line.split(' ').nth(1).unwrap_or("").to_string();
                let mut s = s;
                let _ = write!(
                    s,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{target}",
                    target.len()
                );
            });
        }
    });
    port
}

#[test]
fn request_target_matches_qt() {
    let cases: &[(&str, &str)] = &[
        // A space in the query, a non-ASCII tag in the path.
        (
            "/?json&c=blue sky&l=20&s=fav&t=1&p=1",
            "/?json&c=blue%20sky&l=20&s=fav&t=1&p=1",
        ),
        (
            "/日本 x?json&quantity=20",
            "/%E6%97%A5%E6%9C%AC%20x?json&quantity=20",
        ),
        // Tag-search templates (danbooru brackets, gelbooru trailing `%`).
        (
            "/tags.json?limit=10&search[name_matches]=ab*",
            "/tags.json?limit=10&search[name_matches]=ab*",
        ),
        ("/t?name_pattern=ab%", "/t?name_pattern=ab%25"),
        ("/t?x=%zz%41", "/t?x=%25zz%2541"),
        ("/p a/b?q=a%41b", "/p%20a/b?q=aAb"),
        ("/p?q=%7e%7E%2f", "/p?q=~~%2F"),
        ("/p?q=!$&'()*+,;=:@/?[]", "/p?q=!$&'()*+,;=:@/?[]"),
        ("/p?q=a%4", "/p?q=a%254"),
        ("/p?q=a\tb", "/p?q=a%09b"),
        ("/p?q=é%20x", "/p?q=%C3%A9%20x"),
        ("/ü/x", "/%C3%BC/x"),
        ("/p?q=%E6%97", "/p?q=%E6%97"),
        ("/x?q={a}|b^`\"<>\\", "/x?q=%7Ba%7D%7Cb%5E%60%22%3C%3E%5C"),
        ("/x?q=a#frag", "/x?q=a"),
    ];
    let port = server();
    let urls: Vec<String> = cases
        .iter()
        .map(|(u, _)| format!("http://127.0.0.1:{port}{u}"))
        .collect();
    let src = format!(
        r#"import QtQuick
Item {{
    property var got: ({{}})
    property int pending: {n}
    function remaining() {{ return pending }}
    function result(i) {{ return got[i] }}
    Component.onCompleted: {{
        const urls = {urls}
        urls.forEach((u, i) => {{
            const x = new XMLHttpRequest()
            x.onreadystatechange = () => {{
                if (x.readyState === XMLHttpRequest.DONE) {{
                    got[i] = x.status + " " + x.responseText
                    pending--
                }}
            }}
            x.open("GET", u)
            x.send()
        }})
    }}
}}"#,
        n = cases.len(),
        urls = serde_json::to_string(&urls).unwrap()
    );
    let mut rt = qml::runtime::Runtime::new(None);
    let mut lp = event_loop::Loop::new(&mut rt);
    let root = rt.load_str(&src).unwrap();
    rt.settle();
    let left = |rt: &mut qml::runtime::Runtime| {
        rt.call_method(root, "remaining", vec![])
            .unwrap()
            .unwrap()
            .to_js_string()
    };
    lp.block_until(&mut rt, |rt| left(rt) == "0");
    for (i, (url, want)) in cases.iter().enumerate() {
        let got = rt
            .call_method(root, "result", vec![qml::value::Value::Num(i as f64)])
            .unwrap()
            .unwrap()
            .to_js_string();
        assert_eq!(got, format!("200 {want}"), "{url}");
    }
}

/// The Python with PySide6 in ri-testenv (tools/testenv/Containerfile).
const PYTHON: &str = "/opt/qv4-venv/bin/python";

/// Loads the document argv[1], runs until its `finished` signal and prints its `out` property.
const QT_RUN: &str = r#"
import sys
from PySide6.QtCore import QCoreApplication, QObject, QUrl, SIGNAL, qInstallMessageHandler
from PySide6.QtQml import QQmlEngine, QQmlComponent
qInstallMessageHandler(lambda mode, context, message: None)
app = QCoreApplication(sys.argv[:1])
engine = QQmlEngine()
component = QQmlComponent(engine, QUrl.fromLocalFile(sys.argv[1]))
root = component.create()
if root is None:
    sys.exit("; ".join(e.toString() for e in component.errors()))
if not root.property("done"):
    QObject.connect(root, SIGNAL("finished()"), app.quit)
    app.exec()
sys.stdout.write(root.property("out"))
"#;

/// What the state server answers on each path; `/hang` never answers and `/cut` closes after
/// 3 of the 10 bytes it announced.
const ANSWERS: [(&str, &str); 12] = [
    (
        "/ok",
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nX-Probe: a b\r\nSet-Cookie: c=1\r\nX-Twice: 1\r\nx-twice: 2\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
    ),
    (
        "/empty",
        "HTTP/1.1 200 OK\r\nX-Probe: e\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    ),
    (
        "/nocontent",
        "HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n",
    ),
    (
        "/missing",
        "HTTP/1.1 404 Not Found\r\nX-Probe: m\r\nContent-Length: 4\r\nConnection: close\r\n\r\ngone",
    ),
    (
        "/missing0",
        "HTTP/1.1 404 Not Found\r\nX-Probe: m0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    ),
    (
        "/fail",
        "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 4\r\nConnection: close\r\n\r\nboom",
    ),
    (
        "/auth",
        "HTTP/1.1 401 Unauthorized\r\nContent-Length: 3\r\nConnection: close\r\n\r\nath",
    ),
    (
        "/proxy",
        "HTTP/1.1 407 Proxy Authentication Required\r\nX-Probe: p\r\nContent-Length: 3\r\nConnection: close\r\n\r\npxy",
    ),
    (
        "/nope",
        "HTTP/1.1 404 Nope\r\nX-Probe: n\r\nContent-Length: 4\r\nConnection: close\r\n\r\ngone",
    ),
    (
        "/fine",
        "HTTP/1.1 200 Quite  Fine\r\nX-Probe: f\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
    ),
    (
        "/bare",
        "HTTP/1.1 200\r\nX-Probe: b\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
    ),
    (
        "/redirect",
        "HTTP/1.1 302 Found\r\nLocation: /ok\r\nX-Probe: r\r\nContent-Length: 2\r\nConnection: close\r\n\r\nrd",
    ),
];

fn state_server() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            std::thread::spawn(move || {
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                loop {
                    let mut h = String::new();
                    if r.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                        break;
                    }
                }
                let path = line.split(' ').nth(1).unwrap_or("");
                let mut s = s;
                match path {
                    // Held open until the client closes it.
                    "/hang" => while r.read_line(&mut line).is_ok_and(|n| n > 0) {},
                    "/cut" => {
                        let _ = s.write_all(
                            b"HTTP/1.1 200 OK\r\nX-Probe: c\r\nContent-Length: 10\r\nConnection: close\r\n\r\nabc",
                        );
                    }
                    _ => {
                        let answer = ANSWERS.iter().find(|a| a.0 == path).unwrap();
                        let _ = s.write_all(answer.1.as_bytes());
                    }
                }
            });
        }
    });
    port
}

/// Runs each case on a fresh request and writes down every handler call with the state and
/// what `status`, `statusText`, `responseText`, `getAllResponseHeaders()` and
/// `getResponseHeader("X-Probe")` give in it (or what they throw).
const STATES_QML: &str = r#"import QtQml
QtObject {
    id: root
    property string out
    property bool done: false
    property var queue: []
    signal finished()
    function isDone() { return done }
    function log() { return out }
    function get(f) { try { return JSON.stringify(f()) } catch (e) { return "!" + e.name + ":" + e.message + ":" + e.code } }
    function snap(x) {
        return [x.readyState, get(() => x.status), get(() => x.statusText), get(() => x.responseText),
                get(() => x.getAllResponseHeaders()), get(() => x.getResponseHeader("X-Probe"))].join(" ")
    }
    function next() {
        if (queue.length === 0) { done = true; finished(); return }
        const c = queue.shift()
        out += "== " + c.name + "\n"
        const x = new XMLHttpRequest()
        let ended = false
        const end = () => { if (!ended) { ended = true; Qt.callLater(root.next) } }
        x.onreadystatechange = function() {
            out += "  state " + snap(x) + (this === x ? " this is the request" : "") + "\n"
            if (c.inHandler) c.inHandler(x)
        }
        x.onload = () => { out += "  load " + x.readyState + "\n" }
        x.onerror = () => { out += "  error " + x.readyState + "\n" }
        x.onloadend = () => { out += "  loadend " + x.readyState + "\n"; if (!c.again || c.again(x)) end() }
        try {
            out += "  new " + snap(x) + "\n"
            c.run(x)
            out += "  ran " + snap(x) + "\n"
        } catch (e) {
            out += "  threw " + e.name + ":" + e.message + " " + snap(x) + "\n"
        }
        if (c.silent) end()
    }
    function simple(name, url) { return { name: name, run: x => { x.open("GET", url); x.send() } } }
    Component.onCompleted: {
        const B = "@BASE@"
        const note = (what, x) => { root.out += "  " + what + " " + root.snap(x) + "\n" }
        queue = [
            simple("answer with a body", B + "/ok"),
            simple("answer without a body", B + "/empty"),
            simple("204", B + "/nocontent"),
            simple("404 with a body", B + "/missing"),
            simple("404 without a body", B + "/missing0"),
            simple("500", B + "/fail"),
            simple("401", B + "/auth"),
            simple("407", B + "/proxy"),
            simple("redirect", B + "/redirect"),
            simple("404 in the server's words", B + "/nope"),
            simple("200 in the server's words", B + "/fine"),
            simple("200 without words", B + "/bare"),
            simple("answer cut short", B + "/cut"),
            simple("nothing listens", "@CLOSED@/x"),
            { name: "abort after send", silent: true, run: x => { x.open("GET", B + "/hang"); x.send(); note("sent", x); x.abort() } },
            { name: "abort before send", silent: true, run: x => { x.open("GET", B + "/ok"); x.abort() } },
            { name: "abort of a new request", silent: true, run: x => { x.abort() } },
            { name: "abort in the handler", run: x => { x.open("GET", B + "/ok"); x.send() },
              inHandler: x => { if (x.readyState === 3) { x.abort(); note("aborted", x) } } },
            { name: "request used again after abort", run: x => { x.open("GET", B + "/hang"); x.send(); x.abort(); x.open("GET", B + "/ok"); x.send() },
              again: x => x.status === 200 },
            { name: "request used again when done", run: x => { x.open("GET", B + "/missing"); x.send() },
              again: x => { if (x.status === 200) return true; x.open("GET", B + "/ok"); x.send(); return false } },
            { name: "synchronous answer", silent: true, run: x => { x.open("GET", B + "/ok", false); note("opened", x); x.send() } },
            { name: "synchronous 404", silent: true, run: x => { x.open("GET", B + "/missing", false); x.send() } },
            { name: "synchronous 404 in the server's words", silent: true, run: x => { x.open("GET", B + "/nope", false); x.send() } },
            { name: "synchronous, nothing listens", silent: true, run: x => { x.open("GET", "@CLOSED@/x", false); x.send() } },
            { name: "send twice", run: x => { x.open("GET", B + "/ok"); x.send(); x.send() } },
            { name: "send before open", silent: true, run: x => { x.send() } },
            { name: "header after send", run: x => { x.open("GET", B + "/ok"); x.send(); x.setRequestHeader("A", "b") } },
            { name: "open twice", run: x => { x.open("GET", B + "/missing"); x.open("GET", B + "/ok"); x.send() } },
            { name: "handler throws", run: x => { x.open("GET", B + "/ok"); x.send() },
              inHandler: x => { if (x.readyState === 3) throw new Error("oops") } },
        ]
        next()
    }
}
"#;

#[test]
fn ready_states_match_qt() {
    if !std::path::Path::new(PYTHON).exists() {
        return testenv::unavailable(format_args!("{PYTHON} (PySide6) is not installed"));
    }
    let port = state_server();
    let closed = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let src = STATES_QML
        .replace("@BASE@", &format!("http://127.0.0.1:{port}"))
        .replace("@CLOSED@", &format!("http://127.0.0.1:{closed}"));

    let dir = tempfile::tempdir().unwrap();
    let doc = dir.path().join("states.qml");
    std::fs::write(&doc, &src).unwrap();
    let qt = std::process::Command::new(PYTHON)
        .args(["-c", QT_RUN])
        .arg(&doc)
        .env("QT_QPA_PLATFORM", "offscreen")
        .output()
        .unwrap();
    assert!(
        qt.status.success(),
        "Qt: {}",
        String::from_utf8_lossy(&qt.stderr)
    );
    let want = String::from_utf8(qt.stdout).unwrap();
    assert!(want.contains("== handler throws\n"), "Qt:\n{want}");

    let mut rt = qml::runtime::Runtime::new(None);
    let mut lp = event_loop::Loop::new(&mut rt);
    let root = rt.load_str(&src).unwrap();
    rt.settle();
    let call = |rt: &mut qml::runtime::Runtime, f: &str| {
        rt.call_method(root, f, vec![])
            .unwrap()
            .unwrap()
            .to_js_string()
    };
    // Stops early when a case never ends; the logs then differ at that case.
    lp.until(&mut rt, |rt| call(rt, "isDone") == "true");
    let got = call(&mut rt, "log");
    // Case by case, so a difference names its case.
    let cases = |log: &str| -> Vec<String> { log.split("== ").map(str::to_string).collect() };
    for (got, want) in cases(&got).iter().zip(cases(&want)) {
        assert_eq!(*got, want, "\nours:\n{got}\nQt:\n{want}");
    }
    assert_eq!(got, want);
}

/// One answer of the response server: the path, its header lines after the status line and
/// the body.
type Answer = (&'static str, &'static str, &'static [u8]);

const RESPONSES: &[Answer] = &[
    ("/latin1", "Content-Type: text/plain; charset=ISO-8859-1", b"caf\xe9 \xe8"),
    ("/cp1251", "Content-Type: text/plain;charset=windows-1251", b"\xcf\xf0\xe8\xe2\xe5\xf2"),
    ("/cscase", "Content-Type: text/plain; Charset=ISO-8859-1", b"caf\xe9"),
    ("/csparam", "Content-Type: text/plain; charset=ISO-8859-1; x=y", b"caf\xe9"),
    ("/quoted", "Content-Type: text/plain; charset=\"ISO-8859-1\"", b"caf\xe9"),
    ("/unknowncs", "Content-Type: text/plain; charset=foo-bar", b"caf\xc3\xa9 \xe9"),
    ("/utf16cs", "Content-Type: text/plain; charset=UTF-16", b"\xff\xfeh\x00\xe9\x00"),
    ("/utf16le", "Content-Type: text/plain; charset=UTF-16LE", b"h\x00\xe9\x00"),
    ("/bom16", "Content-Type: text/plain", b"\xff\xfeh\x00\xe9\x00"),
    ("/bom16be", "Content-Type: text/plain", b"\xfe\xff\x00h\x00\xe9"),
    ("/bom8", "Content-Type: text/plain", b"\xef\xbb\xbfh\xc3\xa9"),
    ("/bom8cs", "Content-Type: text/plain; charset=utf-8", b"\xef\xbb\xbfh\xc3\xa9"),
    ("/badutf", "Content-Type: text/plain", b"a\xe9b\xc3"),
    ("/notype", "X-N: 1", b"a\xe9b"),
    ("/xmlenc", "Content-Type: text/xml", b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><a>\xe9</a>"),
    ("/xmlnoenc", "Content-Type: application/xml", b"<a>\xc3\xa9 \xe9</a>"),
    ("/svg", "Content-Type: image/svg+xml", b"<?xml version=\"1.0\" encoding=\"windows-1251\"?><a>\xcf</a>"),
    ("/htmlmeta", "Content-Type: text/html", b"<html><head><meta charset=\"windows-1251\"></head><body>\xcf</body></html>"),
    ("/htmlplain", "Content-Type: text/html", b"<html><body>\xc3\xa9 \xe9</body></html>"),
    ("/json", "Content-Type: application/json", b"{\"a\":[1,\"\xc3\xa9\"],\"b\":null}"),
    ("/nobody", "Content-Type: text/plain", b""),
    ("/badjson", "Content-Type: application/json", b"{\"a\":"),
    ("/bytes", "Content-Type: application/octet-stream", b"\x00\x01\xff\x80\n"),
    ("/xml", "Content-Type: text/xml", b"<?xml version=\"1.0\" standalone=\"yes\"?>\n<!-- c --><root a=\"1\" b='x&amp;y'>t1<kid>in &lt;b&gt;</kid><![CDATA[cd<]]>  <e/>\n</root>"),
    ("/xmlbad", "Content-Type: text/xml", b"<root><a></root>"),
    ("/xmlastext", "Content-Type: text/plain", b"<root/>"),
    // "hello gzip", "hello deflate" (zlib), "hello raw" (bare deflate), "hello br",
    // "hello zstd" and "hello xgzip" (gzip under the old name).
    ("/gzip", "Content-Type: text/plain\r\nContent-Encoding: gzip", b"\x1f\x8b\x08\x00\x00\x00\x00\x00\x02\x03\xcb\x48\xcd\xc9\xc9\x57\x48\xaf\xca\x2c\x00\x00\x19\x6a\xd2\xdf\x0a\x00\x00\x00"),
    ("/deflate", "Content-Type: text/plain\r\nContent-Encoding: deflate", b"\x78\x9c\xcb\x48\xcd\xc9\xc9\x57\x48\x49\x4d\xcb\x49\x2c\x49\x05\x00\x23\x0c\x05\x0a"),
    ("/rawdeflate", "Content-Type: text/plain\r\nContent-Encoding: deflate", b"\xcb\x48\xcd\xc9\xc9\x57\x28\x4a\x2c\x07\x00"),
    ("/badgzip", "Content-Type: text/plain\r\nContent-Encoding: gzip", b"not gzip at all"),
    ("/br", "Content-Type: text/plain\r\nContent-Encoding: br", b"\x8b\x03\x80\x68\x65\x6c\x6c\x6f\x20\x62\x72\x03"),
    ("/zstd", "Content-Type: text/plain\r\nContent-Encoding: zstd", b"\x28\xb5\x2f\xfd\x00\x58\x51\x00\x00\x68\x65\x6c\x6c\x6f\x20\x7a\x73\x74\x64"),
    ("/xgzip", "Content-Type: text/plain\r\nContent-Encoding: x-gzip", b"\x1f\x8b\x08\x00\x00\x00\x00\x00\x02\x03\xcb\x48\xcd\xc9\xc9\x57\xa8\x48\xaf\xca\x2c\x00\x00\x3b\xdf\x10\x23\x0b\x00\x00\x00"),
    ("/identity", "Content-Type: text/plain\r\nContent-Encoding: identity", b"plain"),
    ("/unkenc", "Content-Type: text/plain\r\nContent-Encoding: foo", b"plain foo"),
    ("/gzipcaps", "Content-Type: text/plain\r\nContent-Encoding: GZIP", b"\x1f\x8b\x08\x00\x00\x00\x00\x00\x02\x03\xcb\x48\xcd\xc9\xc9\x57\x48\xaf\xca\x2c\x00\x00\x19\x6a\xd2\xdf\x0a\x00\x00\x00"),
    ("/brcaps", "Content-Type: text/plain\r\nContent-Encoding: Br", b"\x8b\x03\x80\x68\x65\x6c\x6c\x6f\x20\x62\x72\x03"),
    ("/emptydeflate", "Content-Type: text/plain\r\nContent-Encoding: deflate", b""),
    ("/emptygzip", "Content-Type: text/plain\r\nContent-Encoding: gzip", b""),
    // The first 20 of the 30 bytes of "hello gzip".
    ("/cutgzip", "Content-Type: text/plain\r\nContent-Encoding: gzip", b"\x1f\x8b\x08\x00\x00\x00\x00\x00\x02\x03\xcb\x48\xcd\xc9\xc9\x57\x48\xaf\xca\x2c"),
    // The first 12 of the 20 bytes of "hello deflate", 8 of the 12 of "hello br", and of two
    // zstd blocks "hello " and "zstd" the first and a byte of the second.
    ("/cutdeflate", "Content-Type: text/plain\r\nContent-Encoding: deflate", b"\x78\x9c\xcb\x48\xcd\xc9\xc9\x57\x48\x49\x4d\xcb"),
    ("/cutbr", "Content-Type: text/plain\r\nContent-Encoding: br", b"\x8b\x03\x80\x68\x65\x6c\x6c\x6f"),
    ("/cutzstd", "Content-Type: text/plain\r\nContent-Encoding: zstd", b"\x28\xb5\x2f\xfd\x00\x58\x30\x00\x00\x68\x65\x6c\x6c\x6f\x20\x21"),
    ("/zstd2", "Content-Type: text/plain\r\nContent-Encoding: zstd", b"\x28\xb5\x2f\xfd\x00\x58\x30\x00\x00\x68\x65\x6c\x6c\x6f\x20\x21\x00\x00zstd"),
    // zstd that ends inside a block: four bytes of a stored block of ten after the block
    // "hello ", the same without the block before it and as the frame's last block, a block
    // of one byte repeated without the byte, and two of the three bytes of a block's header.
    ("/cutzstdraw", "Content-Type: text/plain\r\nContent-Encoding: zstd", b"\x28\xb5\x2f\xfd\x00\x58\x30\x00\x00hello \x50\x00\x00zstd"),
    ("/cutzstdraw1", "Content-Type: text/plain\r\nContent-Encoding: zstd", b"\x28\xb5\x2f\xfd\x00\x58\x50\x00\x00zstd"),
    ("/cutzstdrawlast", "Content-Type: text/plain\r\nContent-Encoding: zstd", b"\x28\xb5\x2f\xfd\x00\x58\x51\x00\x00zstd"),
    ("/cutzstdrle", "Content-Type: text/plain\r\nContent-Encoding: zstd", b"\x28\xb5\x2f\xfd\x00\x58\x30\x00\x00hello \x52\x00\x00"),
    ("/cutzstdhead", "Content-Type: text/plain\r\nContent-Encoding: zstd", b"\x28\xb5\x2f\xfd\x00\x58\x30\x00\x00hello \x50\x00"),
    // gzip that ends early: 5 of the 10 bytes of its head, "hello gzip" without the last
    // of its 8 bytes of sum and length, and 20 and 27 of the 39 bytes of
    // "hello, hello, hello, hello and hello gzip" (before and inside what it repeats).
    ("/cutgziphead", "Content-Type: text/plain\r\nContent-Encoding: gzip", b"\x1f\x8b\x08\x00\x00"),
    ("/cutgzipsum", "Content-Type: text/plain\r\nContent-Encoding: gzip", b"\x1f\x8b\x08\x00\x00\x00\x00\x00\x02\x03\xcb\x48\xcd\xc9\xc9\x57\x48\xaf\xca\x2c\x00\x00\x19\x6a\xd2\xdf\x0a\x00\x00"),
    ("/cutgzip2", "Content-Type: text/plain\r\nContent-Encoding: gzip", b"\x1f\x8b\x08\x00\x00\x00\x00\x00\x02\x03\xcb\x48\xcd\xc9\xc9\xd7\x51\xc8\xc0\xa4"),
    ("/cutgzip3", "Content-Type: text/plain\r\nContent-Encoding: gzip", b"\x1f\x8b\x08\x00\x00\x00\x00\x00\x02\x03\xcb\x48\xcd\xc9\xc9\xd7\x51\xc8\xc0\xa4\x14\x12\xf3\x52\xa0\xac\xf4"),
    // "hello" in frames that ask for the last 128 MiB and the last 144 MiB to be kept.
    ("/zstdwindow", "Content-Type: text/plain\r\nContent-Encoding: zstd", b"\x28\xb5\x2f\xfd\x00\x88\x29\x00\x00hello"),
    ("/zstdwindow2", "Content-Type: text/plain\r\nContent-Encoding: zstd", b"\x28\xb5\x2f\xfd\x00\x89\x29\x00\x00hello"),
    // Frames of the formats 0.4 to 0.7, from before the format was fixed: "hello" in one raw block.
    ("/zstd04", "Content-Type: text/plain\r\nContent-Encoding: zstd", b"\x24\xb5\x2f\xfd\x00\x40\x00\x05hello\xc0\x00\x00"),
    ("/zstd05", "Content-Type: text/plain\r\nContent-Encoding: zstd", b"\x25\xb5\x2f\xfd\x00\x40\x00\x05hello\xc0\x00\x00"),
    ("/zstd06", "Content-Type: text/plain\r\nContent-Encoding: zstd", b"\x26\xb5\x2f\xfd\x00\x40\x00\x05hello\xc0\x00\x00"),
    ("/zstd07", "Content-Type: text/plain\r\nContent-Encoding: zstd", b"\x27\xb5\x2f\xfd\x20\x05\x40\x00\x05hello\xc0\x00\x00"),
    // Frames one after another: of the format in use ("first"), of the old ones ("hello") and
    // one that is to be skipped.
    ("/zstd01", "Content-Type: text/plain\r\nContent-Encoding: zstd", b"\x1e\xb5\x2f\xfd\x40\x00\x05hello\xc0\x00\x00"),
    ("/zstd1x04", "Content-Type: text/plain\r\nContent-Encoding: zstd", b"\x28\xb5\x2f\xfd\x00\x58\x29\x00\x00first\x24\xb5\x2f\xfd\x00\x40\x00\x05hello\xc0\x00\x00"),
    ("/zstd1x01", "Content-Type: text/plain\r\nContent-Encoding: zstd", b"\x28\xb5\x2f\xfd\x00\x58\x29\x00\x00first\x1e\xb5\x2f\xfd\x40\x00\x05hello\xc0\x00\x00"),
    ("/zstd1x05", "Content-Type: text/plain\r\nContent-Encoding: zstd", b"\x28\xb5\x2f\xfd\x00\x58\x29\x00\x00first\x25\xb5\x2f\xfd\x00\x40\x00\x05hello\xc0\x00\x00"),
    ("/zstd1x07", "Content-Type: text/plain\r\nContent-Encoding: zstd", b"\x28\xb5\x2f\xfd\x00\x58\x29\x00\x00first\x27\xb5\x2f\xfd\x20\x05\x40\x00\x05hello\xc0\x00\x00"),
    ("/zstd05x1", "Content-Type: text/plain\r\nContent-Encoding: zstd", b"\x25\xb5\x2f\xfd\x00\x40\x00\x05hello\xc0\x00\x00\x28\xb5\x2f\xfd\x00\x58\x29\x00\x00first"),
    ("/zstd05x04", "Content-Type: text/plain\r\nContent-Encoding: zstd", b"\x25\xb5\x2f\xfd\x00\x40\x00\x05hello\xc0\x00\x00\x24\xb5\x2f\xfd\x00\x40\x00\x05hello\xc0\x00\x00"),
    ("/zstd05x07", "Content-Type: text/plain\r\nContent-Encoding: zstd", b"\x25\xb5\x2f\xfd\x00\x40\x00\x05hello\xc0\x00\x00\x27\xb5\x2f\xfd\x20\x05\x40\x00\x05hello\xc0\x00\x00"),
    ("/zstdskip05", "Content-Type: text/plain\r\nContent-Encoding: zstd", b"\x50\x2a\x4d\x18\x03\x00\x00\x00abc\x25\xb5\x2f\xfd\x00\x40\x00\x05hello\xc0\x00\x00"),
    ("/zstdskip04", "Content-Type: text/plain\r\nContent-Encoding: zstd", b"\x50\x2a\x4d\x18\x03\x00\x00\x00abc\x24\xb5\x2f\xfd\x00\x40\x00\x05hello\xc0\x00\x00"),
    ("/zstdskip1", "Content-Type: text/plain\r\nContent-Encoding: zstd", b"\x50\x2a\x4d\x18\x03\x00\x00\x00abc\x28\xb5\x2f\xfd\x00\x58\x29\x00\x00first"),
    ("/zstd1xskip05", "Content-Type: text/plain\r\nContent-Encoding: zstd", b"\x28\xb5\x2f\xfd\x00\x58\x29\x00\x00first\x50\x2a\x4d\x18\x03\x00\x00\x00abc\x25\xb5\x2f\xfd\x00\x40\x00\x05hello\xc0\x00\x00"),
    ("/badbr", "Content-Type: text/plain\r\nContent-Encoding: br", b"not brotli at all, not at all"),
    ("/badzstd", "Content-Type: text/plain\r\nContent-Encoding: zstd", b"not zstd at all"),
    ("/baddeflate", "Content-Type: text/plain\r\nContent-Encoding: deflate", b"\x78\x9c\xff\xff\xff\xff\xff\xff"),
    ("/ascii", "Content-Type: text/plain; charset=us-ascii", b"a\xe9b"),
    ("/cp1252", "Content-Type: text/plain; charset=windows-1252", b"\x80\xe9"),
    ("/latin1c1", "Content-Type: text/plain; charset=latin1", b"\x80\xe9"),
    ("/koi8", "Content-Type: text/plain; charset=KOI8-R", b"\xf0\xd2\xc9"),
    ("/iso5", "Content-Type: text/plain; charset=ISO_8859-5", b"\xbf\xe0\xd8"),
    ("/utf32", "Content-Type: text/plain", b"\xff\xfe\x00\x00h\x00\x00\x00\xe9\x00\x00\x00"),
    ("/utf16odd", "Content-Type: text/plain; charset=utf-16be", b"\x00h\x00\xe9\x00"),
    ("/utf8odd", "Content-Type: text/plain", b"\xc0\xafa\xed\xa0\x80b\xf4\x90\x80\x80c\x80d\xe2\x82e\xf0\x9f\x98\x80f\xe2\x82"),
    ("/emptycs", "Content-Type: text/plain; charset=", b"a\xe9b"),
    ("/htmlequiv", "Content-Type: text/html", b"<html><head><META http-equiv=\"Content-Type\" content=\"text/html; charset=KOI8-R\"></head><body>\xf0</body></html>"),
    ("/htmlunicode", "Content-Type: text/html", b"<html><head><meta charset=unicode></head><body>\xc3\xa9</body></html>"),
    ("/htmlbom", "Content-Type: text/html", b"\xff\xfe<\x00p\x00>\x00"),
    ("/htmlcs", "Content-Type: text/html; charset=ISO-8859-1", b"<meta charset=\"windows-1251\">\xcf"),
    ("/xml2roots", "Content-Type: text/xml", b"<a/><b/>"),
    ("/xmltext", "Content-Type: text/xml", b"x<a/>"),
    ("/xmlopen", "Content-Type: text/xml", b"<a><b></b>"),
    ("/xmlentity", "Content-Type: text/xml", b"<a>&nbsp;</a>"),
    ("/xmlns", "Content-Type: text/xml", b"<p:a xmlns:p=\"urn:p\" xmlns=\"urn:d\" p:k=\"v\" k2=\"w\" xml:lang=\"en\"><b><c xmlns=\"\"/></b></p:a>"),
    ("/xmldoctype", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY e \"x\">]><a>&e;</a>"),
    ("/xmldoctype2", "Content-Type: text/xml", b"<?xml version=\"1.0\"?><!DOCTYPE a SYSTEM \"a.dtd\"><a>t</a>"),
    ("/xmlcomment", "Content-Type: text/xml", b"<a>x<!-- c -->y<?pi z?>w</a><!-- after --> "),
    ("/xmlws", "Content-Type: text/xml", b" \n<a/>"),
    ("/xmldeclws", "Content-Type: text/xml", b" <?xml version=\"1.0\"?><a/>"),
    ("/xml11", "Content-Type: text/xml", b"<?xml version=\"1.1\"?><a/>"),
    ("/xmlempty", "Content-Type: text/xml", b""),
    ("/xmlcr", "Content-Type: text/xml", b"<a>l1\r\nl2\rl3</a>"),
    ("/xmlattr", "Content-Type: text/xml", b"<a k=\"a\tb\nc &#65; &lt;\" k2='q\"q'/>"),
    ("/xmldup", "Content-Type: text/xml", b"<a k=\"1\" k=\"2\"/>"),
    ("/xmlprefix", "Content-Type: text/xml", b"<p:a/>"),
    ("/xmlcdata", "Content-Type: text/xml", b"<a>x<![CDATA[]]>y<![CDATA[z]]><![CDATA[w]]></a>"),
    ("/xmlutf16", "Content-Type: text/xml", b"\xff\xfe<\x00a\x00>\x00\xe9\x00<\x00/\x00a\x00>\x00"),
    ("/xmlunknownenc", "Content-Type: text/xml", b"<?xml version=\"1.0\" encoding=\"foo-bar\"?><a/>"),
    ("/xmlstandalone", "Content-Type: text/xml", b"<?xml version='1.0' encoding='UTF-8' standalone='no' ?><a/>"),
    ("/xmlnoversion", "Content-Type: text/xml", b"<?xml encoding=\"UTF-8\"?><a/>"),
    ("/xmlcharref", "Content-Type: text/xml", b"<a>&#x41;&#66;&apos;&quot;</a>"),
    ("/xmlmismatch", "Content-Type: text/xml", b"<a></b>"),
    ("/xmlafter", "Content-Type: text/xml", b"<a/>x"),
    ("/xmllt", "Content-Type: text/xml", b"<a k=\"<\"/>"),
    ("/xmlamp", "Content-Type: text/xml", b"<a>a & b</a>"),
    ("/xmlnospace", "Content-Type: text/xml", b"<a k=\"1\"j=\"2\"/>"),
    ("/xmlendspace", "Content-Type: text/xml", b"<a ></a >"),
    // Document types, as QXmlStreamReader reads them: external, parameter and internal
    // entities and where they are referred to, default attributes, what a subset may hold.
    // `/dtdlimit` has entities of 8, 64, 512 and 4096 characters under one of 4097.
    ("/dtdext", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY e SYSTEM \"e.txt\">]><a>x&e;y</a>"),
    ("/dtdextpub", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY e PUBLIC \"-//p\" \"e.txt\">]><a>x&e;y</a>"),
    ("/dtdextws", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY e SYSTEM  'e.txt' >\n<!ENTITY  %  q  \"w\" > ]><a>x&e;y</a>"),
    ("/dtdextfirst", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY e SYSTEM \"e.txt\"><!ENTITY e \"v\">]><a>x&e;y</a>"),
    ("/dtdpeunused", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY % p \"<!ENTITY e 'v'>\">]><a>x&e;y</a>"),
    ("/dtdpeundeclared", "Content-Type: text/xml", b"<!DOCTYPE a [%p;]><a>x</a>"),
    ("/dtdpeext", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY % p SYSTEM \"p.dtd\">%p;]><a>x&e;y</a>"),
    ("/dtdpeextthen", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY % p SYSTEM \"p.dtd\">%p;<!ENTITY e \"v\">]><a>x&e;y</a>"),
    ("/dtdpethen", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY % p \"\">%p;]><a>x&u;y</a>"),
    ("/dtdpethenattr", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY % p \"\">%p;]><a k=\"x&u;y\"/>"),
    ("/dtdpethenvalue", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY % p \"\">%p;<!ENTITY e \"<b>x&u;y</b>\">]><a>&e;</a>"),
    ("/dtdpeinvalue", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY % p \"v\"><!ENTITY e \"x%p;y\">]><a>&e;</a>"),
    ("/dtdpequote", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY % p \"'\"><!ENTITY e \"a%p;b\">]><a>&e;</a>"),
    ("/dtdpercent", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY e \"100%\">]><a>&e;</a>"),
    ("/dtdpeself", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY % p \"&#37;p;\">%p;]><a>x</a>"),
    ("/dtdpendata", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY % p SYSTEM \"p\" NDATA n>]><a>x</a>"),
    ("/dtdpenospace", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY %p \"w\">]><a>x</a>"),
    ("/dtdpesplit", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY % p \"<!ENTITY e 'ab\">%p;cd'>]><a>&e;</a>"),
    ("/dtdpesplitdecl", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY % p \"<!ENTITY e 'ab'\">%p;>]><a>&e;</a>"),
    ("/dtdpecharref", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY % p \"&#60;!ENTITY e 'v'>\">%p;]><a>&e;</a>"),
    ("/dtdpeinpe", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY % p \"<!ENTITY e 'v'>\"><!ENTITY % q \"%p;\">%q;]><a>&e;</a>"),
    ("/dtdextsubset", "Content-Type: text/xml", b"<!DOCTYPE a SYSTEM \"a.dtd\"><a>x&u;y</a>"),
    ("/dtdextsubsetattr", "Content-Type: text/xml", b"<!DOCTYPE a SYSTEM \"a.dtd\"><a k=\"x&u;y\"/>"),
    ("/dtdextsubsetpub", "Content-Type: text/xml", b"<!DOCTYPE a PUBLIC \"-//p\" \"a.dtd\" [<!ENTITY e \"v\">]><a>x&u;y&e;</a>"),
    ("/dtdpublicid", "Content-Type: text/xml", b"<!DOCTYPE a PUBLIC \"p{\" \"a.dtd\"><a>x</a>"),
    ("/dtdmarkupopen", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY e \"<b>\">]><a>&e;</b></a>"),
    ("/dtdcharref", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY e \"&#60;b/>\"><!ENTITY f \"&#38;#60;\">]><a>&e;|&f;</a>"),
    ("/dtdinattrlt", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY e \"<b/>\">]><a k=\"x&e;y\"/>"),
    ("/dtdinattrdoctype", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY e \"<!DOCTYPE x>\">]><a k=\"&e;\"/>"),
    ("/dtdinattramp", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY f \"&#38;#60;\">]><a k=\"&f;\"/>"),
    ("/dtdinattrtab", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY e \"a&#9;b&#10;c\">]><a k=\"&e;\"/>"),
    ("/dtdentityprefix", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY e \"<p:b/>\">]><a xmlns:p=\"urn:p\">&e;</a>"),
    ("/dtdentitycomment", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY e \"<!-- a\nb -->\">]><a>x&e;y</a>"),
    ("/dtdentitycr", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY e \"a&#13;b\">]><a>&e;</a>"),
    ("/dtdentityname", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY % p \"\"><!ENTITY e \"&am\">%p;]><a>x&e;p;y</a>"),
    ("/dtdpredefined", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY lt \"&#38;#60;\"><!ENTITY amp \"zz\">]><a>&amp;</a>"),
    ("/dtdlimit", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY a \"vvvvvvvv\"><!ENTITY b \"&a;&a;&a;&a;&a;&a;&a;&a;\"><!ENTITY c \"&b;&b;&b;&b;&b;&b;&b;&b;\"><!ENTITY d \"&c;&c;&c;&c;&c;&c;&c;&c;\"><!ENTITY e \"&d;v\">]><a>&e;</a>"),
    ("/dtdlimitattr", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY a \"vvvvvvvv\"><!ENTITY b \"&a;&a;&a;&a;&a;&a;&a;&a;\"><!ENTITY c \"&b;&b;&b;&b;&b;&b;&b;&b;\"><!ENTITY d \"&c;&c;&c;&c;&c;&c;&c;&c;\"><!ENTITY e \"&d;v\">]><a k=\"&e;\"/>"),
    ("/dtdattlist", "Content-Type: text/xml", b"<!DOCTYPE a [<!ATTLIST a d CDATA \"dv\" k CDATA #IMPLIED>]><a k=\"1\"/>"),
    ("/dtdattlistfixed", "Content-Type: text/xml", b"<!DOCTYPE a [<!ATTLIST a d NMTOKEN #FIXED \" dv  w \"><!ATTLIST b q (x|y) \"x\">]><a><b/><b q=\"y\"/></a>"),
    ("/dtdattlistns", "Content-Type: text/xml", b"<!DOCTYPE a [<!ATTLIST a xmlns CDATA \"urn:d\" xmlns:p CDATA \"urn:p\">]><a><p:b/></a>"),
    ("/dtdattlistnswritten", "Content-Type: text/xml", b"<!DOCTYPE a [<!ATTLIST a xmlns CDATA \"urn:d\">]><a xmlns=\"urn:w\"/>"),
    ("/dtdattlistprefix", "Content-Type: text/xml", b"<!DOCTYPE a [<!ATTLIST a p:d CDATA \"dv\">]><a xmlns:p=\"urn:p\"/>"),
    ("/dtdattlistnoprefix", "Content-Type: text/xml", b"<!DOCTYPE a [<!ATTLIST a p:d CDATA \"dv\">]><a/>"),
    ("/dtdattlistsame", "Content-Type: text/xml", b"<!DOCTYPE a [<!ATTLIST a q:d CDATA \"dv\">]><a xmlns:p=\"urn:p\" xmlns:q=\"urn:p\" p:d=\"w\"/>"),
    ("/dtdattlisttwice", "Content-Type: text/xml", b"<!DOCTYPE a [<!ATTLIST a d CDATA \"1\" d CDATA \"2\"><!ATTLIST a d CDATA \"3\" k NMTOKEN \"4\">]><a/>"),
    ("/dtdattlistentity", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY e \"\"><!ENTITY f \"v\"><!ATTLIST a d CDATA \"&e;\" k CDATA \"x&f;y\">]><a/>"),
    ("/dtdattlistundeclared", "Content-Type: text/xml", b"<!DOCTYPE a [<!ATTLIST a d CDATA \"x&u;y\">]><a/>"),
    ("/dtdattlistlines", "Content-Type: text/xml", b"<!DOCTYPE a [<!ATTLIST a d CDATA \"x\n\ty\" n NMTOKENS \" x\n\ty \">]><a/>"),
    ("/dtdattlistcr", "Content-Type: text/xml", b"<!DOCTYPE a [<!ATTLIST a d CDATA \"x\ry\r\nz\">]><a/>"),
    ("/dtdattlistnospace", "Content-Type: text/xml", b"<!DOCTYPE a [<!ATTLIST a d CDATA#IMPLIED>]><a/>"),
    ("/dtdattnorm", "Content-Type: text/xml", b"<!DOCTYPE a [<!ATTLIST a k NMTOKENS #IMPLIED>]><a k=\"  x   y  \" j=\"  x   y  \"/>"),
    ("/dtdbaddecl", "Content-Type: text/xml", b"<!DOCTYPE a [<!FOO a>]><a>x</a>"),
    ("/dtdbadentity", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY e>]><a>x</a>"),
    ("/dtdentitynoquote", "Content-Type: text/xml", b"<!DOCTYPE a [<!ENTITY e v>]><a>x</a>"),
    ("/dtdtextinsubset", "Content-Type: text/xml", b"<!DOCTYPE a [ junk ]><a>x</a>"),
    ("/dtdcondsect", "Content-Type: text/xml", b"<!DOCTYPE a [<![INCLUDE[<!ENTITY e \"v\">]]>]><a>x</a>"),
    ("/dtdbadelement", "Content-Type: text/xml", b"<!DOCTYPE a [<!ELEMENT a (b,c|d)>]><a>x</a>"),
    ("/dtdbadnotation", "Content-Type: text/xml", b"<!DOCTYPE a [<!NOTATION n>]><a>x</a>"),
    ("/dtddashes", "Content-Type: text/xml", b"<a><!-- a -- b --></a>"),
    ("/dtdbrackets", "Content-Type: text/xml", b"<a>x]]>y</a>"),
    ("/dtdcontrol", "Content-Type: text/xml", b"<a>x\x01y</a>"),
    ("/dtdnsempty", "Content-Type: text/xml", b"<a xmlns:p=\"\"/>"),
    ("/dtdnssame", "Content-Type: text/xml", b"<a xmlns:p=\"urn:p\" xmlns:q=\"urn:p\" p:d=\"w\" q:d=\"v\"/>"),
    ("/dtdhex0x", "Content-Type: text/xml", b"<a>&#x0x41;</a>"),
    ("/dtdnameat", "Content-Type: text/xml", b"<a k@=\"1\"/>"),
    ("/dtdpiplus", "Content-Type: text/xml", b"<a><?pi+x?></a>"),
    // Names as QXmlUtils::isNCName and the tokenizer take them, the XML declaration with
    // its attributes read as a tag's, the encoding the first bytes show and the one the
    // declaration names, and entity values that each declare the next (`/dtddeep`: 17).
    ("/dtdnamedash", "Content-Type: text/xml", b"<a\xe2\x80\x94/>"),
    ("/dtdnamemark", "Content-Type: text/xml", b"<\xcc\x81a/>"),
    ("/dtdnameastral", "Content-Type: text/xml", b"<a\xf0\x9f\x98\x80/>"),
    ("/dtdnametarget", "Content-Type: text/xml", b"<a><?\xe2\x80\x94 x?></a>"),
    ("/dtdnameprefix", "Content-Type: text/xml", b"<p\xe2\x80\x94:a xmlns:p\xe2\x80\x94=\"urn:p\"/>"),
    ("/dtdtokenmark", "Content-Type: text/xml", b"<!DOCTYPE a [<!ATTLIST a d (x\xcc\x81) #IMPLIED>]><a/>"),
    ("/dtdtokencircled", "Content-Type: text/xml", b"<!DOCTYPE a [<!ATTLIST a d (x\xe2\x92\xb6) #IMPLIED>]><a/>"),
    ("/dtddecltwice", "Content-Type: text/xml", b"<?xml version=\"1.0\" encoding=\"utf-8\" encoding=\"UTF-8\"?><a/>"),
    ("/dtddeclyesno", "Content-Type: text/xml", b"<?xml version=\"1.0\" standalone=\"yes\" standalone=\"no\"?><!DOCTYPE a SYSTEM \"a.dtd\"><a>x&u;y</a>"),
    ("/dtddeclref", "Content-Type: text/xml", b"<?xml version=\"1.0\" standalone=\"y&#101;s\"?><a/>"),
    ("/dtddeclencref", "Content-Type: text/xml", b"<?xml version=\"1.0\" encoding=\"UTF&#45;8\"?><a/>"),
    ("/dtddeclns", "Content-Type: text/xml", b"<?xml version=\"1.0\" xmlns=\"urn:d\"?><a k=\"1\"><b/></a>"),
    ("/dtddeclprefix", "Content-Type: text/xml", b"<?xml version=\"1.0\" xmlns:p=\"urn:p\"?><a><p:b p:k=\"1\"/></a>"),
    ("/dtddeclall", "Content-Type: text/xml", b"<?xml version=\"1.0\" encoding=\"UTF-8\" xmlns:p=\"urn:p\" standalone=\"yes\" xmlns=\"urn:d\"?><a><p:b/></a>"),
    ("/dtddeclattlist", "Content-Type: text/xml", b"<?xml version=\"1.0\" xmlns:p=\"urn:p\"?><!DOCTYPE a [<!ATTLIST a p:d CDATA \"dv\">]><a/>"),
    ("/dtdencascii", "Content-Type: text/xml", b"<?xml version=\"1.0\" encoding=\"US-ASCII\"?><a>\xe9</a>"),
    ("/dtdencutflatin", "Content-Type: text/xml", b"<?xml version=\"1.0\" encoding=\"UTF-8\" encoding=\"latin1\"?><a>\xe9</a>"),
    ("/dtdutf16nomark", "Content-Type: text/xml", b"<\x00a\x00>\x00\xe9\x00<\x00/\x00a\x00>\x00"),
    ("/dtdutf16benomark", "Content-Type: text/xml", b"\x00<\x00?\x00x\x00m\x00l\x00 \x00v\x00e\x00r\x00s\x00i\x00o\x00n\x00=\x00\"\x001\x00.\x000\x00\"\x00?\x00>\x00<\x00a\x00>\x00\xe9\x00<\x00/\x00a\x00>"),
    ("/dtdutf16utf8", "Content-Type: text/xml", b"\xff\xfe<\x00?\x00x\x00m\x00l\x00 \x00v\x00e\x00r\x00s\x00i\x00o\x00n\x00=\x00\"\x001\x00.\x000\x00\"\x00 \x00e\x00n\x00c\x00o\x00d\x00i\x00n\x00g\x00=\x00\"\x00U\x00T\x00F\x00-\x008\x00\"\x00?\x00>\x00<\x00a\x00>\x00\xe9\x00<\x00/\x00a\x00>\x00"),
    ("/dtdutf32", "Content-Type: text/xml", b"\xff\xfe\x00\x00<\x00\x00\x00a\x00\x00\x00>\x00\x00\x00\xe9\x00\x00\x00<\x00\x00\x00/\x00\x00\x00a\x00\x00\x00>\x00\x00\x00"),
    ("/dtdutf32be", "Content-Type: text/xml", b"\x00\x00\x00<\x00\x00\x00a\x00\x00\x00>\x00\x00\x00\xe9\x00\x00\x00<\x00\x00\x00/\x00\x00\x00a\x00\x00\x00>"),
    ("/dtdutf32named", "Content-Type: text/xml", b"<\x00\x00\x00?\x00\x00\x00x\x00\x00\x00m\x00\x00\x00l\x00\x00\x00 \x00\x00\x00v\x00\x00\x00e\x00\x00\x00r\x00\x00\x00s\x00\x00\x00i\x00\x00\x00o\x00\x00\x00n\x00\x00\x00=\x00\x00\x00\"\x00\x00\x001\x00\x00\x00.\x00\x00\x000\x00\x00\x00\"\x00\x00\x00 \x00\x00\x00e\x00\x00\x00n\x00\x00\x00c\x00\x00\x00o\x00\x00\x00d\x00\x00\x00i\x00\x00\x00n\x00\x00\x00g\x00\x00\x00=\x00\x00\x00\"\x00\x00\x00U\x00\x00\x00T\x00\x00\x00F\x00\x00\x00-\x00\x00\x003\x00\x00\x002\x00\x00\x00\"\x00\x00\x00?\x00\x00\x00>\x00\x00\x00<\x00\x00\x00a\x00\x00\x00>\x00\x00\x00\xe9\x00\x00\x00<\x00\x00\x00/\x00\x00\x00a\x00\x00\x00>\x00\x00\x00"),
    (
        "/dtddeep",
        "Content-Type: text/xml",
        b"<!DOCTYPE a [<!ENTITY % p \"\">%p;<!ENTITY e1 \"<!DOCTYPE x [<!ENTITY e2 '<!DOCTYPE x [<!ENTITY e3 &#34;<!DOCTYPE x [<!ENTITY e4 &#38\
          ;#39;<!DOCTYPE x [<!ENTITY e5 &#38;#38;#34;<!DOCTYPE x [<!ENTITY e6 &#38;#38;#38;#39;<!DOCTYPE x [<!ENTITY e7 &#38;#38;#38;#38;#34\
          ;<!DOCTYPE x [<!ENTITY e8 &#38;#38;#38;#38;#38;#39;<!DOCTYPE x [<!ENTITY e9 &#38;#38;#38;#38;#38;#38;#34;<!DOCTYPE x [<!ENTITY e10 \
          &#38;#38;#38;#38;#38;#38;#38;#39;<!DOCTYPE x [<!ENTITY e11 &#38;#38;#38;#38;#38;#38;#38;#38;#34;<!DOCTYPE x [<!ENTITY e12 &#38;#38\
          ;#38;#38;#38;#38;#38;#38;#38;#39;<!DOCTYPE x [<!ENTITY e13 &#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#34;<!DOCTYPE x [<!ENTITY e14 \
          &#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#39;<!DOCTYPE x [<!ENTITY e15 &#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#34;<!\
          DOCTYPE x [<!ENTITY e16 &#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#39;<!DOCTYPE x [<!ENTITY e17 &#38;#38;#38;#38;#38;#3\
          8;#38;#38;#38;#38;#38;#38;#38;#38;#34;v&#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#34;>]><x k=&#38;#38;#38;#38;#38;#\
          38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#39;&#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;e17;&#38;#38;#38;#38;#\
          38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#39;/>&#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#39;>]><x k=&#38;#38;#38;#38\
          ;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#34;&#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;e16;&#38;#38;#38;#38;#38\
          ;#38;#38;#38;#38;#38;#38;#38;#38;#38;#34;/>&#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#34;>]><x k=&#38;#38;#38;#38;#38;#38;#\
          38;#38;#38;#38;#38;#38;#38;#39;&#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;e15;&#38;#38;#38;#38;#38;#38;#38;#38;#38;#\
          38;#38;#38;#38;#39;/>&#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#39;>]><x k=&#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#34\
          ;&#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;e14;&#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#34;/>&#38;#38;#38;#38;\
          #38;#38;#38;#38;#38;#38;#34;>]><x k=&#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#39;&#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;\
          #38;e13;&#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#39;/>&#38;#38;#38;#38;#38;#38;#38;#38;#38;#39;>]><x k=&#38;#38;#38;#38;#38;#\
          38;#38;#38;#38;#38;#34;&#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;e12;&#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;#34;/>&#38;#38;#3\
          8;#38;#38;#38;#38;#38;#34;>]><x k=&#38;#38;#38;#38;#38;#38;#38;#38;#38;#39;&#38;#38;#38;#38;#38;#38;#38;#38;#38;#38;e11;&#38;#38;#\
          38;#38;#38;#38;#38;#38;#38;#39;/>&#38;#38;#38;#38;#38;#38;#38;#39;>]><x k=&#38;#38;#38;#38;#38;#38;#38;#38;#34;&#38;#38;#38;#38;#3\
          8;#38;#38;#38;#38;e10;&#38;#38;#38;#38;#38;#38;#38;#38;#34;/>&#38;#38;#38;#38;#38;#38;#34;>]><x k=&#38;#38;#38;#38;#38;#38;#38;#39\
          ;&#38;#38;#38;#38;#38;#38;#38;#38;e9;&#38;#38;#38;#38;#38;#38;#38;#39;/>&#38;#38;#38;#38;#38;#39;>]><x k=&#38;#38;#38;#38;#38;#38;\
          #34;&#38;#38;#38;#38;#38;#38;#38;e8;&#38;#38;#38;#38;#38;#38;#34;/>&#38;#38;#38;#38;#34;>]><x k=&#38;#38;#38;#38;#38;#39;&#38;#38;\
          #38;#38;#38;#38;e7;&#38;#38;#38;#38;#38;#39;/>&#38;#38;#38;#39;>]><x k=&#38;#38;#38;#38;#34;&#38;#38;#38;#38;#38;e6;&#38;#38;#38;#\
          38;#34;/>&#38;#38;#34;>]><x k=&#38;#38;#38;#39;&#38;#38;#38;#38;e5;&#38;#38;#38;#39;/>&#38;#39;>]><x k=&#38;#38;#34;&#38;#38;#38;e\
          4;&#38;#38;#34;/>&#34;>]><x k=&#38;#39;&#38;#38;e3;&#38;#39;/>'>]><x k=&#34;&#38;e2;&#34;/>\">]><a k='&e1;'/>",
    ),
    // The names ICU has for US-ASCII and ISO-8859-1.
    ("/csascii0", "Content-Type: text/plain; charset=ANSI_X3.4-1986", b"a\xc3\xa9b"),
    ("/csascii1", "Content-Type: text/plain; charset=ISO_646.irv:1991", b"a\xc3\xa9b"),
    ("/csascii2", "Content-Type: text/plain; charset=iso_646.irv:1983", b"a\xc3\xa9b"),
    ("/csascii3", "Content-Type: text/plain; charset=us", b"a\xc3\xa9b"),
    ("/csascii4", "Content-Type: text/plain; charset=csASCII", b"a\xc3\xa9b"),
    ("/csascii5", "Content-Type: text/plain; charset=iso-ir-6", b"a\xc3\xa9b"),
    ("/csascii6", "Content-Type: text/plain; charset=cp367", b"a\xc3\xa9b"),
    ("/csascii7", "Content-Type: text/plain; charset=ascii7", b"a\xc3\xa9b"),
    ("/csascii8", "Content-Type: text/plain; charset=646", b"a\xc3\xa9b"),
    ("/csascii9", "Content-Type: text/plain; charset=windows-20127", b"a\xc3\xa9b"),
    ("/csascii10", "Content-Type: text/plain; charset=ibm-367", b"a\xc3\xa9b"),
    ("/csascii11", "Content-Type: text/plain; charset=IBM367", b"a\xc3\xa9b"),
    ("/cslatin12", "Content-Type: text/plain; charset=8859_1", b"\x80\xe9"),
    ("/cslatin13", "Content-Type: text/plain; charset=iso-ir-100", b"\x80\xe9"),
    ("/cslatin14", "Content-Type: text/plain; charset=ISO_8859-1:1987", b"\x80\xe9"),
    ("/cslatin15", "Content-Type: text/plain; charset=819", b"\x80\xe9"),
    ("/cslatin16", "Content-Type: text/plain; charset=csISOLatin1", b"\x80\xe9"),
    ("/xmlencus", "Content-Type: text/xml", b"<?xml version=\"1.0\" encoding=\"US\"?><a/>"),
    ("/xmlencusnot", "Content-Type: text/xml", b"<?xml version=\"1.0\" encoding=\"US\"?><a>\xe9</a>"),
];

/// The body of `/big/<encoding>/<what>` and its encoding's name: 11 MiB of zeros and, before
/// them, noise that makes the body decode to `<what>` times its size (`30`, `70`), or after
/// them (`70z`, `150z`); `9` and `11` are that many MiB of zeros alone. QNetworkReply gives
/// up on a body once it has decoded to over 10 MiB and to over 40 times (`gzip`, `deflate`)
/// or 100 times (`br`, `zstd`) what has come of it, so the order tells what it has seen by
/// then: all of the noise, or none.
fn big(path: &str) -> Option<(&'static str, Vec<u8>)> {
    use flate2::Compression;
    const MIB: usize = 1 << 20;
    let (encoding, what) = path.strip_prefix("/big/")?.split_once('/')?;
    let (zeros, times, zeros_first) = match what {
        "9" => (9 * MIB, 0, false),
        "11" => (11 * MIB, 0, false),
        "30" => (11 * MIB, 30, false),
        "70" => (11 * MIB, 70, false),
        "70z" => (11 * MIB, 70, true),
        "150z" => (11 * MIB, 150, true),
        _ => return None,
    };
    let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
    let noise: Vec<u8> = (0..zeros.checked_div(times).unwrap_or(0))
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 32) as u8
        })
        .collect();
    let plain = || {
        let mut all = vec![0u8; zeros];
        if zeros_first {
            all.extend_from_slice(&noise);
        } else {
            all.splice(0..0, noise.iter().copied());
        }
        all
    };
    Some(match encoding {
        "gzip" => {
            let mut to = flate2::write::GzEncoder::new(vec![], Compression::best());
            to.write_all(&plain()).unwrap();
            ("gzip", to.finish().unwrap())
        }
        "deflate" => {
            let mut to = flate2::write::ZlibEncoder::new(vec![], Compression::best());
            to.write_all(&plain()).unwrap();
            ("deflate", to.finish().unwrap())
        }
        // A frame of blocks of at most 128 KiB: the noise as it is, the zeros as a byte and
        // how many times it repeats; the last block says so in its first bit.
        "zstd" => {
            const BLOCK: usize = 128 << 10;
            let mut blocks: Vec<(u32, &[u8])> = vec![];
            let stored = noise.chunks(BLOCK).map(|c| (c.len() as u32 * 8, c));
            let repeated = (0..zeros / BLOCK).map(|_| (BLOCK as u32 * 8 + 2, &[0u8][..]));
            if zeros_first {
                blocks.extend(repeated.chain(stored));
            } else {
                blocks.extend(stored.chain(repeated));
            }
            let mut all = b"\x28\xb5\x2f\xfd\x00\x58".to_vec();
            let last = blocks.len() - 1;
            for (i, (head, data)) in blocks.into_iter().enumerate() {
                let head = head + u32::from(i == last);
                all.extend_from_slice(&head.to_le_bytes()[..3]);
                all.extend_from_slice(data);
            }
            ("zstd", all)
        }
        // The stream of the zeros is what brotli 1.2.1 makes of them. Noise goes before it
        // in blocks stored as they are, each after its header (not the last, a length of
        // four digits, the length less one, stored) filled up to a whole byte; the stream of
        // the zeros follows from its fifth bit, after its window size.
        "br" if !zeros_first => {
            let stream: &[u8] = match zeros / MIB {
                9 => b"\xcb\xff\xff\x3f\x00\x24\x00\xe2\xb1\x40\x72\xef\xff\xea\xff\xff\x01\x48\x00\xc4\x61\x01\xe8\xde\x3f\x00",
                _ => b"\xcb\xff\xff\x3f\x00\x24\x00\xe2\xb1\x40\x72\xef\xff\xf2\xff\xff\x05\x80\x04\x40\x1c\x16\x80\xee\xfd\x0b",
            };
            let mut all: Vec<u8> = vec![];
            let mut bits = 0usize;
            let put = |all: &mut Vec<u8>, bits: &mut usize, value: u32, count: usize| {
                for i in 0..count {
                    if (*bits).is_multiple_of(8) {
                        all.push(0);
                    }
                    *all.last_mut().unwrap() |= ((value >> i & 1) as u8) << (*bits % 8);
                    *bits += 1;
                }
            };
            put(&mut all, &mut bits, u32::from(stream[0]) & 0xF, 4);
            for c in noise.chunks(1 << 16) {
                put(&mut all, &mut bits, 0, 3);
                put(&mut all, &mut bits, c.len() as u32 - 1, 16);
                put(&mut all, &mut bits, 1, 1);
                all.extend_from_slice(c);
                bits = all.len() * 8;
            }
            for i in 4..stream.len() * 8 {
                put(
                    &mut all,
                    &mut bits,
                    u32::from(stream[i / 8] >> (i % 8) & 1),
                    1,
                );
            }
            ("br", all)
        }
        _ => return None,
    })
}

/// Serves [`RESPONSES`]; `/echo…` answers with the request as it arrived (its first line, its
/// headers but `Host` in order of name, `--`, its body), `/wire` with its head as it arrived,
/// `/redir` points to `/echo2#frag`, `/see` answers 303 to `/echo` and `/gone` answers 404.
fn response_server() -> u16 {
    use std::io::Read;
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            std::thread::spawn(move || {
                let mut got = Vec::new();
                let mut buf = [0u8; 4096];
                let end = loop {
                    if let Some(i) = got.windows(4).position(|w| w == b"\r\n\r\n") {
                        break i;
                    }
                    match s.read(&mut buf) {
                        Ok(n) if n > 0 => got.extend_from_slice(&buf[..n]),
                        _ => return,
                    }
                };
                let mut lines: Vec<Vec<u8>> = got[..end]
                    .split(|b| *b == b'\n')
                    .map(|l| l.strip_suffix(b"\r").unwrap_or(l).to_vec())
                    .collect();
                let first = lines.remove(0);
                let path = String::from_utf8_lossy(&first)
                    .split(' ')
                    .nth(1)
                    .unwrap_or("")
                    .to_string();
                let length = lines
                    .iter()
                    .find_map(|l| {
                        let l = String::from_utf8_lossy(l).to_ascii_lowercase();
                        l.strip_prefix("content-length:")?.trim().parse().ok()
                    })
                    .unwrap_or(0usize);
                let mut body = got[end + 4..].to_vec();
                while body.len() < length {
                    match s.read(&mut buf) {
                        Ok(n) if n > 0 => body.extend_from_slice(&buf[..n]),
                        _ => return,
                    }
                }
                if path == "/redir" {
                    let _ = s.write_all(b"HTTP/1.1 302 Found\r\nLocation: /echo2#frag\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    return;
                }
                if path == "/see" {
                    let _ = s.write_all(b"HTTP/1.1 303 See Other\r\nLocation: /echo\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    return;
                }
                if path == "/gone" {
                    let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Type: text/xml\r\nContent-Length: 4\r\nConnection: close\r\n\r\n<a/>");
                    return;
                }
                // `/plain/<n>`: `<n>` MiB sent as they are.
                if let Some(mib) = path
                    .strip_prefix("/plain/")
                    .and_then(|n| n.parse::<usize>().ok())
                {
                    let mut all = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        mib << 20
                    )
                    .into_bytes();
                    all.resize(all.len() + (mib << 20), b'a');
                    let _ = s.write_all(&all);
                    return;
                }
                if let Some((encoding, body)) = big(&path) {
                    // In one piece, so that the reader has the body's start with its head.
                    let mut all = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Encoding: {encoding}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .into_bytes();
                    all.extend_from_slice(&body);
                    let _ = s.write_all(&all);
                    return;
                }
                // `/split1<path>`: the body of `<path>` in two chunks, its first byte and the rest.
                let (path, split) = match path.strip_prefix("/split1") {
                    Some(rest) => (rest.to_string(), true),
                    None => (path, false),
                };
                let echo: Vec<u8>;
                let (toclose, path) = match path.strip_prefix("/toclose") {
                    Some(p) => (true, p.to_string()),
                    None => (false, path),
                };
                let (head, body): (&str, &[u8]) = match RESPONSES.iter().find(|a| a.0 == path) {
                    Some(a) => (a.1, a.2),
                    None if path == "/wire" => {
                        echo = got[..end].iter().copied().filter(|b| *b != b'\r').collect();
                        ("Content-Type: text/plain", &echo)
                    }
                    None => {
                        lines.retain(|l| !l.to_ascii_lowercase().starts_with(b"host:"));
                        lines.sort();
                        let mut e = first.clone();
                        for l in &lines {
                            e.push(b'\n');
                            e.extend_from_slice(l);
                        }
                        e.extend_from_slice(b"\n--\n");
                        e.extend_from_slice(&body);
                        echo = e;
                        ("Content-Type: text/plain", &echo)
                    }
                };
                if split && !body.is_empty() {
                    // Chunks of the transfer coding reach the decoder apart, in Qt and here,
                    // however the network brings them.
                    let mut all = format!(
                        "HTTP/1.1 200 OK\r\n{head}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n1\r\n"
                    )
                    .into_bytes();
                    all.extend_from_slice(&body[..1]);
                    all.extend_from_slice(format!("\r\n{:x}\r\n", body.len() - 1).as_bytes());
                    all.extend_from_slice(&body[1..]);
                    all.extend_from_slice(b"\r\n0\r\n\r\n");
                    let _ = s.write_all(&all);
                    return;
                }
                if toclose {
                    // No length: the body ends where the connection does. In one piece, so
                    // that the reader has all of it at once.
                    let mut all = format!("HTTP/1.1 200 OK\r\n{head}\r\nConnection: close\r\n\r\n")
                        .into_bytes();
                    all.extend_from_slice(body);
                    let _ = s.write_all(&all);
                    return;
                }
                let _ = write!(
                    s,
                    "HTTP/1.1 200 OK\r\n{head}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(body);
            });
        }
    });
    port
}

/// The bytes of `s` where each char is one byte (Latin-1), `?` for a char that is none
/// (QString::toLatin1).
fn latin1(s: &str) -> Vec<u8> {
    s.chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?'))
        .collect()
}

/// `s` in UTF-16, the low byte first.
fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// RFC 2104's HMAC with MD5.
fn hmac_md5(key: &[u8], message: &[u8]) -> Vec<u8> {
    use md5::Digest;
    let mut block = [0u8; 64];
    match key.len() > 64 {
        true => block[..16].copy_from_slice(&md5::Md5::digest(key)),
        false => block[..key.len()].copy_from_slice(key),
    }
    let pad = |with: u8| block.iter().map(|b| b ^ with).collect::<Vec<u8>>();
    let inner = md5::Md5::digest([pad(0x36), message.to_vec()].concat());
    md5::Md5::digest([pad(0x5c), inner.to_vec()].concat()).to_vec()
}

/// The login [`login_server`] expects: `u`, `d` and `pw` of the address, `u`, `DOM` and `p`
/// without them.
struct Who {
    user: String,
    domain: String,
    password: String,
}

/// The challenge of every NTLM type 2 message of [`login_server`], and the time its target
/// information gives.
const NTLM_CHALLENGE: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
const NTLM_TIME: [u8; 8] = [0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17];

/// An NTLM type 2 message in base64: `flags`, the target name `target` in the charset the
/// flags name, the parts of `info` (`name`, `time`, `end`) as target information, and only
/// its first `cut` bytes when that is given.
fn ntlm_type2(flags: u32, target: &str, info: &str, cut: Option<usize>) -> String {
    use base64::Engine;
    let name = match flags & 1 {
        0 => latin1(target),
        _ => utf16(target),
    };
    let mut pairs = vec![];
    for part in info.split(',') {
        let (id, value): (u16, Vec<u8>) = match part {
            "name" => (2, utf16("DOM")),
            "time" => (7, NTLM_TIME.to_vec()),
            "end" => (0, vec![]),
            _ => continue,
        };
        pairs.extend(id.to_le_bytes());
        pairs.extend((value.len() as u16).to_le_bytes());
        pairs.extend(value);
    }
    let buffer = |len: usize, at: usize| {
        let len = (len as u16).to_le_bytes();
        [&len[..], &len[..], &(at as u32).to_le_bytes()[..]].concat()
    };
    let mut m = b"NTLMSSP\0".to_vec();
    m.extend(2u32.to_le_bytes());
    m.extend(buffer(name.len(), 48));
    m.extend(flags.to_le_bytes());
    m.extend(NTLM_CHALLENGE);
    m.extend([0; 8]);
    m.extend(buffer(pairs.len(), 48 + name.len()));
    m.extend(name);
    m.extend(pairs);
    m.truncate(cut.unwrap_or(m.len()));
    base64::engine::general_purpose::STANDARD.encode(m)
}

/// What the NTLM type 3 message `m` holds: its bytes up to the two answers in hex, then
/// each answer. An answer's hash is written as `<right>` when it is NTLMv2's for the login
/// `who` and [`NTLM_CHALLENGE`], the client's random challenge as `<8 hex>` and a time that
/// is whole seconds of this century as `<now>`: Qt takes the clock where the target
/// information gives no time.
fn ntlm_type3(m: &[u8], who: &Who) -> String {
    use md4::Digest;
    let hex = |b: &[u8]| b.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let len = |at: usize| {
        m.get(at..at + 2)
            .map_or(0, |b| usize::from(u16::from_le_bytes([b[0], b[1]])))
    };
    let (lm, nt) = (len(12), len(20));
    if m.len() < 64 + lm + nt {
        return hex(m);
    }
    let (head, answers) = m.split_at(m.len() - lm - nt);
    let (lm, nt) = answers.split_at(lm);
    let named = utf16(&(who.user.to_uppercase() + &who.domain));
    let key = hmac_md5(&md4::Md4::digest(utf16(&who.password)), &named);
    let hash = |answer: &[u8]| match answer.split_at_checked(16) {
        Some((hash, rest)) if hmac_md5(&key, &[&NTLM_CHALLENGE, rest].concat()) == hash => {
            "<right>".to_string()
        }
        _ => hex(&answer[..answer.len().min(16)]),
    };
    let random = |b: &[u8]| match b.iter().all(|b| b"0123456789abcdef".contains(b)) {
        true => "<8 hex>".to_string(),
        false => hex(b),
    };
    let time = |b: &[u8]| {
        let t = u64::from_le_bytes(b.try_into().unwrap_or_default());
        // 2001-01-01 in the hundreds of nanoseconds since 1601 that NTLM counts.
        match t % 10_000_000 == 0 && t > 126_227_808_000_000_000 {
            true => "<now>".to_string(),
            false => hex(b),
        }
    };
    let mut out = format!("type 3 {}", hex(head));
    if let Some((_, challenge)) = lm.split_at_checked(16) {
        out += &format!(" lm {}{}", hash(lm), random(challenge));
    }
    match nt.len() >= 44 {
        true => {
            out += &format!(
                " nt {}{}{}{}{}",
                hash(nt),
                hex(&nt[16..24]),
                time(&nt[24..32]),
                random(&nt[32..40]),
                hex(&nt[40..])
            );
        }
        false => out += &format!(" nt {}", hex(nt)),
    }
    out
}

/// What a request to [`login_server`] said: its method, its `Authorization` (`none` without
/// one), the names of its headers but `Host` as they came, and its body; a byte of the head
/// that is not ASCII is written as `\x` and its two hex digits. Of a Digest `Authorization`
/// the client's random `cnonce` is written as `<32 hex>`, the `uri` that is the request's as
/// `<this>`, and the `response` as `<right>` when it is the RFC 2617 hash for the password
/// of `who` in Latin-1. An NTLM type 3 message is written as [`ntlm_type3`] reads it.
fn said(first: &str, lines: &[String], body: &[u8], who: &Who) -> String {
    let md5 = |s: String| qml::builtins::md5_hex(&latin1(&s));
    let (method, target) = first.split_once(' ').unwrap_or((first, ""));
    let target = target.split(' ').next().unwrap_or("");
    let header = |name: &str| {
        lines.iter().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.eq_ignore_ascii_case(name).then(|| v.trim().to_string())
        })
    };
    let mut auth = header("authorization").unwrap_or("none".into());
    if let Some(fields) = auth.strip_prefix("Digest ").map(str::to_string) {
        let field = |name: &str| {
            fields
                .split(", ")
                .find_map(|f| f.strip_prefix(name)?.strip_prefix('='))
                .map(|v| v.trim_matches('"').to_string())
        };
        let get = |name: &str| field(name).unwrap_or_default();
        let (cnonce, uri) = (get("cnonce"), get("uri"));
        let password: String = latin1(&who.password).into_iter().map(char::from).collect();
        let mut ha1 = md5(format!("{}:{}:{password}", get("username"), get("realm")));
        if get("algorithm").eq_ignore_ascii_case("md5-sess") {
            ha1 = md5(format!("{ha1}:{}:{cnonce}", get("nonce")));
        }
        let ha2 = md5(format!("{method}:{uri}"));
        let right = md5(match field("qop") {
            Some(qop) => format!("{ha1}:{}:{}:{cnonce}:{qop}:{ha2}", get("nonce"), get("nc")),
            None => format!("{ha1}:{}:{ha2}", get("nonce")),
        });
        let hex = cnonce.len() == 32
            && cnonce
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if hex {
            auth = auth.replace(&format!("cnonce=\"{cnonce}\""), "cnonce=<32 hex>");
        }
        if uri == target {
            auth = auth.replace(&format!("uri=\"{uri}\""), "uri=<this>");
        }
        if get("response") == right {
            auth = auth.replace(&format!("response=\"{right}\""), "response=<right>");
        }
    }
    if let Some(m) = ntlm_token(&auth).filter(|m| m.get(8) == Some(&3)) {
        auth = format!("NTLM {}", ntlm_type3(&m, who));
    }
    let names: Vec<&str> = lines
        .iter()
        .filter_map(|l| Some(l.split_once(':')?.0))
        .filter(|k| !k.eq_ignore_ascii_case("host"))
        .collect();
    let body = match body {
        [] => String::new(),
        body => format!(" + {}", String::from_utf8_lossy(body)),
    };
    let head: String = format!("{method} {auth} [{}]", names.join(","))
        .chars()
        .map(|c| match c {
            ' '..='~' => c.to_string(),
            _ => format!("\\x{:02x}", u32::from(c)),
        })
        .collect();
    head + &body
}

/// The message of an NTLM `Authorization`.
fn ntlm_token(auth: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    let token = auth.strip_prefix("NTLM")?.trim();
    base64::engine::general_purpose::STANDARD.decode(token).ok()
}

/// The requests the [`login_server`]s hold back, by the name of their group: how many of a
/// group wait, whether it lets requests through, and how often it was let go.
static HELD: std::sync::Mutex<Vec<(String, usize, bool, u64)>> = std::sync::Mutex::new(vec![]);
static HELD_MOVED: std::sync::Condvar = std::sync::Condvar::new();

/// Runs `f` on the group `name` of [`HELD`] until it gives something, waiting for the group
/// to change in between.
fn held<T>(name: &str, mut f: impl FnMut(&mut (String, usize, bool, u64)) -> Option<T>) -> T {
    let mut all = HELD.lock().unwrap();
    loop {
        let at = match all.iter().position(|g| g.0 == name) {
            Some(at) => at,
            None => {
                all.push((name.to_string(), 0, false, 0));
                all.len() - 1
            }
        };
        // `f` may have changed the group, whatever it gives.
        let was = all[at].clone();
        let out = f(&mut all[at]);
        if was != all[at] {
            HELD_MOVED.notify_all();
        }
        if let Some(out) = out {
            return out;
        }
        all = HELD_MOVED.wait(all).unwrap();
    }
}

/// A server that asks for a login. `/q?h=…&ok=…&pw=…` answers 401 with a `WWW-Authenticate`
/// line for each part of `h` between `|`, until the request has an `Authorization` (one that
/// contains `ok`, when `ok` is given; none does when it is `never`), then 200; `/said` answers
/// 200 at once and `/to?…` sends on to the address after the `?`. The body of a 200 is what
/// the request [`said`] (`u`, `d` and `pw` are the login's user, domain and password), the
/// body of a 401 is `need: ` and the same. A connection is closed after its answer unless
/// the address has `keep` in it.
///
/// `h` may be several challenges between `||`: a request without an `Authorization` gets
/// the first, one whose `Authorization` has `nonce="n3"` or `realm="r3"` the fifth, and any
/// other the last; `{n}` in a challenge is that number plus one, 0 for the first. So a
/// challenge can change with each answer while the server keeps nothing of a request.
///
/// `/ntlm` asks for NTLM: it answers `NTLM` alone (with the lines of `more` after it, before
/// it with `first`), a type 1 message with the type 2 message [`ntlm_type2`] makes of
/// `flags`, `target`, `info` and `cut` (with a Basic challenge instead when the address has
/// `down`), and a type 3 message with 200, or as it answers no message with `refuse`.
///
/// `v` is the HTTP version of the answer and `conn` its `Connection` (none when empty),
/// which then is all that says whether the connection stays.
///
/// A request with `hold=k&of=3` is answered once three such requests came to any of these
/// servers; from then on the group `k` holds nothing back. `/held?k=…&of=…` answers once
/// that many wait (and with `go` lets them go), `/gone?k=…` says whether the group still
/// holds, and `/free?k=a,b` lets those of the groups `a` and `b` go and makes the groups
/// hold again. Else the servers keep nothing between requests, so Qt and the runtime get
/// the same.
fn login_server() -> u16 {
    use std::io::Read;
    fn bytes(s: &str) -> Vec<u8> {
        let b = s.as_bytes();
        let mut out = vec![];
        let mut i = 0;
        while i < b.len() {
            let hex = b
                .get(i + 1..i + 3)
                .and_then(|h| std::str::from_utf8(h).ok());
            match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                Some(v) if b[i] == b'%' => {
                    out.push(v);
                    i += 3;
                }
                _ => {
                    out.push(b[i]);
                    i += 1;
                }
            }
        }
        out
    }
    fn decoded(s: &str) -> String {
        String::from_utf8_lossy(&bytes(s)).into_owned()
    }
    // Each byte as one char: what goes into the head of an answer as it is.
    fn raw(s: &str) -> String {
        bytes(s).into_iter().map(char::from).collect()
    }
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            std::thread::spawn(move || {
                let mut got = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    let end = loop {
                        if let Some(i) = got.windows(4).position(|w| w == b"\r\n\r\n") {
                            break i;
                        }
                        match s.read(&mut buf) {
                            Ok(n) if n > 0 => got.extend_from_slice(&buf[..n]),
                            _ => return,
                        }
                    };
                    let head: String = got[..end].iter().map(|&b| char::from(b)).collect();
                    let mut lines: Vec<String> = head.split("\r\n").map(str::to_string).collect();
                    let first = lines.remove(0);
                    let length = lines
                        .iter()
                        .find_map(|l| {
                            let l = l.to_ascii_lowercase();
                            l.strip_prefix("content-length:")?.trim().parse().ok()
                        })
                        .unwrap_or(0usize);
                    got.drain(..end + 4);
                    while got.len() < length {
                        match s.read(&mut buf) {
                            Ok(n) if n > 0 => got.extend_from_slice(&buf[..n]),
                            _ => return,
                        }
                    }
                    let body: Vec<u8> = got.drain(..length).collect();
                    let target = first.split(' ').nth(1).unwrap_or("").to_string();
                    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
                    let part = |name: &str| {
                        query
                            .split('&')
                            .find_map(|a| a.strip_prefix(name)?.strip_prefix('='))
                    };
                    let arg = |name: &str| part(name).map(decoded);
                    let has = |name: &str| query.split('&').any(|a| a == name);
                    let number = |name: &str| arg(name).and_then(|n| n.parse::<usize>().ok());
                    let who = Who {
                        user: arg("u").unwrap_or("u".into()),
                        domain: arg("d").unwrap_or("DOM".into()),
                        password: arg("pw").unwrap_or("p".into()),
                    };
                    let said = said(&first, &lines, &body, &who);
                    let auth = lines.iter().find_map(|l| {
                        let (k, v) = l.split_once(':')?;
                        k.eq_ignore_ascii_case("authorization").then(|| v.trim())
                    });
                    if let Some(group) = arg("hold") {
                        let of = number("of").unwrap_or(usize::MAX);
                        let mut round = None;
                        held(&group, |g| {
                            if round.is_none() {
                                g.1 += 1;
                                g.2 |= g.1 >= of;
                                round = Some(g.3);
                            }
                            (g.2 || round != Some(g.3)).then_some(())
                        });
                    }
                    let groups = arg("k").unwrap_or_default();
                    let ok = arg("ok").unwrap_or_default();
                    let challenges = |lines: &[&str]| -> String {
                        lines
                            .iter()
                            .filter(|h| !h.is_empty())
                            .map(|h| format!("WWW-Authenticate: {h}\r\n"))
                            .collect()
                    };
                    let (status, head, body) = match path {
                        "/to" => (
                            "302 Found",
                            format!("Location: {}\r\n", decoded(query)),
                            String::new(),
                        ),
                        "/q" if !auth.is_some_and(|a| ok != "never" && a.contains(&ok)) => {
                            let h = part("h").map(raw).unwrap_or_default();
                            let all: Vec<&str> = h.split("||").collect();
                            let n = auth.and_then(|a| {
                                ["nonce=\"n", "realm=\"r"].iter().find_map(|p| {
                                    let rest = &a[a.find(p)? + p.len()..];
                                    rest[..rest.find('"')?].parse::<usize>().ok()
                                })
                            });
                            let at = match (auth, n) {
                                (None, _) => 0,
                                (_, Some(n)) => n + 1,
                                _ => all.len(),
                            };
                            let h = all[at.min(all.len() - 1)]
                                .replace("{n}", &n.map_or(0, |n| n + 1).to_string());
                            (
                                "401 Unauthorized",
                                challenges(&h.split('|').collect::<Vec<_>>()),
                                format!("need: {said}"),
                            )
                        }
                        "/ntlm" => {
                            let kind = auth.and_then(ntlm_token).and_then(|m| m.get(8).copied());
                            let more = part("more").map(raw).unwrap_or_default();
                            let mut lines: Vec<&str> = more.split('|').collect();
                            lines.insert(if has("first") { lines.len() } else { 0 }, "NTLM");
                            let type2 = format!(
                                "NTLM {}",
                                ntlm_type2(
                                    arg("flags")
                                        .and_then(|f| u32::from_str_radix(&f, 16).ok())
                                        .unwrap_or(0x0088_8205),
                                    &part("target").map(raw).unwrap_or("DOM".into()),
                                    &arg("info").unwrap_or_default(),
                                    number("cut"),
                                )
                            );
                            let head = match kind {
                                Some(3) if !has("refuse") => None,
                                Some(1) if has("down") => Some(challenges(&["Basic realm=\"r\""])),
                                Some(1) => Some(challenges(&[&type2])),
                                _ => Some(challenges(&lines)),
                            };
                            match head {
                                Some(head) => ("401 Unauthorized", head, format!("need: {said}")),
                                None => ("200 OK", String::new(), said),
                            }
                        }
                        "/held" => {
                            let of = number("of").unwrap_or(1);
                            held(&groups, |g| {
                                let all = g.1 >= of || g.2;
                                g.2 |= all && has("go");
                                all.then_some(())
                            });
                            ("200 OK", String::new(), said)
                        }
                        "/gone" => {
                            let gone = held(&groups, |g| Some(g.2));
                            let body = if gone { "they went" } else { "they wait" };
                            ("200 OK", String::new(), body.to_string())
                        }
                        "/free" => {
                            for group in groups.split(',') {
                                held(group, |g| {
                                    (g.1, g.2, g.3) = (0, false, g.3 + 1);
                                    Some(())
                                });
                            }
                            ("200 OK", String::new(), said)
                        }
                        _ => ("200 OK", String::new(), said),
                    };
                    let keep = target.contains("keep");
                    let close = match part("conn").map(raw) {
                        Some(c) if c.is_empty() => String::new(),
                        Some(c) => format!("Connection: {c}\r\n"),
                        None if keep => String::new(),
                        None => "Connection: close\r\n".into(),
                    };
                    let version = arg("v").unwrap_or("1.1".into());
                    // In one piece, so that the reader has the body with its head.
                    let head = format!(
                        "HTTP/{version} {status}\r\n{head}Content-Type: text/plain\r\nContent-Length: {}\r\n{close}\r\n",
                        body.len()
                    );
                    let all = [latin1(&head), body.into_bytes()].concat();
                    if s.write_all(&all).is_err() || !keep {
                        return;
                    }
                }
            });
        }
    });
    port
}

/// Runs each case on a fresh request against [`response_server`] and writes down what the
/// case reads: the text in the charset the answer names, compressed answers, `response` for
/// each `responseType`, the `responseXML` tree, `responseURL`, what a request sends for each
/// method, body and header, and what `open`, `setRequestHeader` and `overrideMimeType` refuse.
const RESPONSES_QML: &str = r#"import QtQml
QtObject {
    id: root
    property string out
    property bool done: false
    property var queue: []
    signal finished()
    function isDone() { return done }
    function log() { return out }
    function show(v) {
        if (v === undefined) return "undefined"
        if (v === null) return "null"
        if (v instanceof ArrayBuffer) return "ArrayBuffer[" + Array.from(new Uint8Array(v)).join(",") + "]"
        if (typeof v === "string") return JSON.stringify(v)
        if (typeof v === "object") { try { return "object " + JSON.stringify(v) } catch (e) { return "object !" + e.message } }
        return typeof v + " " + v
    }
    function get(f) { try { return show(f()) } catch (e) { return "!" + e.name + ":" + e.message + ":" + e.code } }
    function node(n, depth) {
        if (n === null || n === undefined) return String(n)
        let s = "(" + n.nodeType + " " + n.nodeName + " " + JSON.stringify(n.nodeValue) + (n.namespaceUri ? " in " + n.namespaceUri : "")
        if (n.attributes) for (let i = 0; i < n.attributes.length; i++) { const a = n.attributes[i]; s += " @" + a.name + "=" + JSON.stringify(a.value) + (a.namespaceUri ? " in " + a.namespaceUri : "") }
        if (n.childNodes && depth < 6) for (let i = 0; i < n.childNodes.length; i++) s += " " + node(n.childNodes[i], depth + 1)
        return s + ")"
    }
    function next() {
        if (queue.length === 0) { done = true; finished(); return }
        const c = queue.shift()
        out += "== " + c.name + "\n"
        const x = new XMLHttpRequest()
        let ended = false
        const end = () => { if (!ended) { ended = true; Qt.callLater(root.next) } }
        const say = (what, f) => { const v = get(f); out += "  " + what + " " + v + "\n" }
        let first = !!c.then, last = !!c.last
        // Asks the server for something else until the request is where `c.until` wants it
        // (DONE if it does not say): a request that runs no handler tells nobody it ended.
        const settled = rounds => {
            if (rounds === 0 || (c.until ? c.until(x) : x.readyState === 4)) { say(rounds ? "late" : "never", () => c.late(x)); end(); return }
            const y = new XMLHttpRequest()
            y.onreadystatechange = () => { if (y.readyState === 4) settled(rounds - 1) }
            y.open("GET", "@BASE@/echo"); y.send()
        }
        x.onreadystatechange = function() {
            if (c.each) say("state " + x.readyState, () => c.each(x))
            if (c.note) c.note(x)
            if (x.readyState !== 4) return
            if (c.last) { if (last) { last = false; say("last", () => c.last(x)); settled(500) }; return }
            // Qt runs no handler of a request sent from its own last handler.
            if (first) { first = false; Qt.callLater(() => say("then", () => c.then(x))); return }
            if (c.done) say("done", () => c.done(x))
            end()
        }
        try { say("ran", () => c.run(x)) } catch (e) { out += "  threw " + e + "\n" }
        if (c.silent) end()
    }
    // A long body: the states it went through (a state it stays in is written once), its
    // length, its first, middle and last byte and the reply's headers.
    function big(name, path) {
        const seen = []
        return { name: name, run: x => { x.open("GET", "@BASE@/big/" + path); x.responseType = "arraybuffer"; x.send() },
                 note: x => { if (seen[seen.length - 1] !== x.readyState) seen.push(x.readyState) },
                 done: x => { const n = x.response.byteLength, b = n ? new Uint8Array(x.response) : []
                              return ["states " + seen.join(","), x.status, n, b[0], b[n >> 1], b[n - 1], show(x.getAllResponseHeaders())].join(" | ") } }
    }
    // What a reply came to: its states, status, text and headers; `onerror` writes a line.
    function whole(name, path) {
        const seen = []
        return { name: name, run: x => { x.onerror = () => { root.out += "  onerror\n" }; x.open("GET", "@BASE@" + path); x.send() },
                 note: x => { if (seen[seen.length - 1] !== x.readyState) seen.push(x.readyState) },
                 done: x => ["states " + seen.join(","), x.status, show(x.statusText), show(x.responseText), show(x.getAllResponseHeaders())].join(" | ") }
    }
    function text(name, path) {
        return { name: name, run: x => { x.open("GET", "@BASE@" + path); x.send() }, done: x => x.responseText }
    }
    function heard(name, path) {
        const seen = []
        const on = (x, what) => x["on" + what] = () => seen.push(what)
        return { name: name, run: x => { for (const w of ["load", "error", "abort", "timeout", "loadend"]) on(x, w); x.open("GET", "@BASE@" + path); x.send() },
                 note: x => { if (seen[seen.length - 1] !== x.readyState) seen.push(x.readyState) },
                 last: x => [x.status, show(x.responseText), show(x.getAllResponseHeaders())].join(" | "),
                 until: x => seen.includes("loadend"),
                 late: x => seen.join(",") }
    }
    function typed(name, path, type, when) {
        return { name: name, run: x => { x.open("GET", "@BASE@" + path); if (when !== "late") x.responseType = type; x.send(); return x.responseType },
                 each: x => x.response,
                 done: x => { if (when === "late") x.responseType = type; return [show(x.response), show(x.responseType), show(x.responseText)].join(" | ") } }
    }
    function xml(name, path) {
        return { name: name, run: x => { x.open("GET", "@BASE@" + path); x.send() },
                 each: x => x.responseXML === null ? "null" : "doc",
                 done: x => { const d = x.responseXML; if (d === null) return "null " + show(x.responseType)
                     return [show(x.responseType), node(d.documentElement, 0), show(d.xmlVersion), show(d.xmlEncoding), show(d.xmlStandalone), typeof x.response].join(" | ") } }
    }
    function echo(name, prep, method, body) {
        return { name: name, run: x => { x.open(method || "GET", "@BASE@/echo"); prep(x); x.send(body) }, done: x => x.responseText }
    }
    function wire(name, prep, method, body) {
        return { name: name, run: x => { x.open(method || "GET", "@BASE@/wire"); prep(x); x.send(body) }, done: x => x.responseText.replace(/127\.0\.0\.1:\d+/, "HOST") }
    }
    function code(name, path, at) {
        return { name: name, run: x => { x.open("GET", "@BASE@" + path); x.send() }, done: x => { const t = x.responseText, r = []; for (let i = 0; i < t.length; i++) r.push(t.charCodeAt(i).toString(16)); return r.join(" ") } }
    }
    function tree(name, path) {
        return { name: name, run: x => { x.open("GET", "@BASE@" + path); x.send() },
                 done: x => { const d = x.responseXML; return d === null ? "null" : [node(d.documentElement, 0), show(d.xmlVersion), show(d.xmlEncoding), show(d.xmlStandalone)].join(" | ") } }
    }
    function seen(x) { return [get(() => x.status), show(x.responseText.split("\n")[0]), show(x.getAllResponseHeaders().split("\r\n")[0])].join(" ") }
    // A request that its last handler (the one of DONE) sends again with `send`: the states
    // its handlers see, then what it reads once it is DONE again (or where `until` wants it).
    function again(name, path, send, until, async) {
        return { name: name, run: x => { x.open("GET", "@BASE@" + path, async !== false); x.send() }, each: seen, last: send, until: until, late: x => x.readyState + " " + seen(x) }
    }
    // What an `arraybuffer` response is and what can be made of it.
    function buffer(name, url) {
        const made = x => { const r = x.response
            return [typeof r, r instanceof ArrayBuffer, Object.getPrototypeOf(r) === ArrayBuffer.prototype, get(() => r.byteLength), get(() => new Uint8Array(r).length),
                    get(() => new Uint8Array(r, 0).length), get(() => new Int16Array(r).length), get(() => new DataView(r).byteLength), get(() => r.slice(0).byteLength),
                    get(() => ArrayBuffer.isView(r)), get(() => String(r)), get(() => JSON.stringify(r)), get(() => Qt.btoa(r)), x.response === x.response].join(" | ") }
        return { name: name, run: x => { x.open("GET", url); x.responseType = "arraybuffer"; x.send() }, each: made, done: made }
    }
    function opened(name, f) { return { name: name, silent: true, run: x => { f(x); return x.readyState } } }
    Component.onCompleted: {
        const B = "@BASE@"
        queue = [
            text("charset latin1", "/latin1"), text("charset windows-1251", "/cp1251"), text("Charset in capitals", "/cscase"),
            text("charset then another parameter", "/csparam"), text("charset in quotes", "/quoted"), text("charset nobody knows", "/unknowncs"),
            text("charset UTF-16 with a mark", "/utf16cs"), text("charset UTF-16LE", "/utf16le"), text("UTF-16 mark, no charset", "/bom16"),
            text("UTF-16BE mark, no charset", "/bom16be"), text("UTF-8 mark, no charset", "/bom8"), text("UTF-8 mark, charset utf-8", "/bom8cs"),
            text("bytes that are not UTF-8", "/badutf"), text("no content type", "/notype"), text("xml with an encoding", "/xmlenc"),
            text("xml without an encoding", "/xmlnoenc"), text("svg with an encoding", "/svg"), text("html with a meta charset", "/htmlmeta"),
            text("html without a charset", "/htmlplain"),
            text("gzip", "/gzip"), text("deflate (zlib)", "/deflate"), text("deflate (raw)", "/rawdeflate"), text("gzip that is not", "/badgzip"),
            text("br", "/br"), text("zstd", "/zstd"), text("x-gzip", "/xgzip"), text("identity", "/identity"), text("encoding nobody knows", "/unkenc"),
            { name: "gzip headers", run: x => { x.open("GET", B + "/gzip"); x.send() }, done: x => x.getAllResponseHeaders() + " # " + x.status },
            { name: "bad gzip headers", run: x => { x.open("GET", B + "/badgzip"); x.send() }, done: x => x.getAllResponseHeaders() + " # " + x.status },
            { name: "synchronous gzip", silent: true, run: x => { x.open("GET", B + "/gzip", false); x.send(); return x.responseText + " # " + x.getAllResponseHeaders() } },
            typed("json", "/json", "json"), typed("JSON in capitals", "/json", "JSON"), typed("json that is cut", "/badjson", "json"),
            typed("json of a latin1 text", "/latin1", "json"), typed("arraybuffer", "/bytes", "arraybuffer"), typed("text", "/json", "text"),
            typed("document", "/xml", "document"), typed("document of a text", "/xmlastext", "document"), typed("type nobody knows", "/json", "blob"),
            typed("type set when done", "/json", "json", "late"), typed("arraybuffer of gzip", "/gzip", "arraybuffer"),
            { name: "type is any value", silent: true, run: x => { const r = [show(x.responseType)]; x.responseType = 5; r.push(show(x.responseType)); x.responseType = null; r.push(show(x.responseType)); x.responseType = undefined; r.push(show(x.responseType)); return r.join(" ") } },
            { name: "response before the answer", silent: true, run: x => { const r = [show(x.response), show(x.responseXML), show(x.responseURL)]; x.open("GET", B + "/json"); x.responseType = "json"; r.push(show(x.response), show(x.responseXML), show(x.responseURL)); return r.join(" ") } },
            { name: "json twice is one object", run: x => { x.open("GET", B + "/json"); x.responseType = "json"; x.send() }, done: x => x.response === x.response },
            xml("xml", "/xml"), xml("xml that is broken", "/xmlbad"), xml("xml sent as text", "/xmlastext"), xml("no content type", "/notype"),
            xml("svg", "/svg"), xml("xml with an encoding", "/xmlenc"), xml("json", "/json"),
            { name: "responseXML twice is one object", run: x => { x.open("GET", B + "/xml"); x.send() }, done: x => x.responseXML === x.responseXML },
            { name: "xml nodes", run: x => { x.open("GET", B + "/xml"); x.send() }, done: x => { const d = x.responseXML, e = d.documentElement
                return [show(d.nodeType), show(d.nodeName), show(d.childNodes.length), show(e.tagName), show(e.firstChild.nodeValue), show(e.lastChild.nodeName),
                        show(e.firstChild.nextSibling.nodeName), show(e.firstChild.nextSibling.previousSibling.data), show(e.childNodes[1].parentNode === e), show(e.childNodes[1].firstChild.length),
                        show(e.childNodes[2].nodeName), show(e.childNodes[2].isElementContentWhitespace), show(e.attributes.a === undefined ? "no" : e.attributes.a.value), show(e.attributes.length), show(e.namespaceUri), show(e.localName), show(e.prefix), show(e.baseUri), show(e.ownerElement), show(String(d)), show(String(e)), show(String(e.firstChild)), show(Object.keys(e).join(","))].join(" ") } },
            echo("plain GET", x => {}), echo("headers the page may not set", x => { for (const h of ["Accept-Charset", "Accept-Encoding", "Connection", "Content-Length", "Cookie", "Cookie2", "Content-Transfer-Encoding", "Date", "Expect", "Host", "Keep-Alive", "Referer", "TE", "Trailer", "Transfer-Encoding", "Upgrade", "Via", "Proxy-X", "Sec-X", "accept-encoding", "X-Mine", "User-Agent", "Origin", "Accept", "Accept-Language", "Authorization"]) x.setRequestHeader(h, "v-" + h) }),
            echo("a header set twice", x => { x.setRequestHeader("X-A", "1"); x.setRequestHeader("x-a", "2"); x.setRequestHeader("X-B", "b"); x.setRequestHeader("X-A", "3") }),
            echo("header values", x => { x.setRequestHeader("X-E", ""); x.setRequestHeader("X-N", 5); x.setRequestHeader("X-U", "é"); x.setRequestHeader("X-S", "  a  b  ") }),
            echo("POST of a text", x => {}, "POST", "héllo"), echo("POST without a body", x => {}, "POST"),
            echo("POST with a type", x => x.setRequestHeader("Content-Type", "application/json"), "POST", "{}"),
            echo("POST with a charset", x => x.setRequestHeader("Content-Type", "text/x; charset=latin1; a=b"), "POST", "é"),
            echo("POST of bytes", x => {}, "POST", new Uint8Array([104, 255, 0, 105]).buffer), echo("POST of a number", x => {}, "POST", 5), echo("POST of null", x => {}, "POST", null),
            echo("PUT", x => {}, "PUT", "p"), echo("DELETE with a body", x => {}, "DELETE", "d"), echo("PATCH", x => {}, "PATCH", "pa"), echo("OPTIONS", x => {}, "OPTIONS", "o"),
            echo("PROPFIND", x => {}, "PROPFIND", "pf"), echo("GET with a body", x => {}, "GET", "g"), echo("get in small letters", x => {}, "get"), echo("Post in mixed letters", x => {}, "Post", "m"),
            { name: "HEAD", run: x => { x.open("HEAD", B + "/echo"); x.send() }, done: x => show(x.responseText) + " " + x.status + " " + x.getResponseHeader("content-length") },
            opened("method nobody knows", x => x.open("TRACE", B + "/echo")), opened("method CONNECT", x => x.open("CONNECT", B + "/echo")), opened("open with one argument", x => x.open("GET")),
            opened("open with six arguments", x => x.open("GET", B + "/echo", true, "u", "p", 1)), opened("open with no arguments", x => x.open()),
            opened("header with one argument", x => { x.open("GET", B + "/echo"); x.setRequestHeader("A") }), opened("header with three arguments", x => { x.open("GET", B + "/echo"); x.setRequestHeader("A", "b", "c") }),
            opened("responseType with no setter argument", x => { x.responseType = "json"; return x.responseType }),
            { name: "user and password", run: x => { x.open("GET", B + "/echo", true, "us er", "p@ss") ; x.send() }, done: x => x.responseText + " # " + x.responseURL.replace(/127\.0\.0\.1:\d+/, "HOST") },
            { name: "a fragment in the address", run: x => { x.open("GET", B + "/echo?q=1#frag"); x.send() }, each: x => x.responseURL.replace(B, "B"), done: x => x.responseText.split("\n")[0] },
            { name: "address after a redirect", run: x => { x.open("GET", B + "/redir"); x.send() }, each: x => x.responseURL.replace(B, "B"), done: x => x.responseText.split("\n")[0] },
            { name: "overrideMimeType to xml", run: x => { x.open("GET", B + "/xmlastext"); x.overrideMimeType("text/xml"); x.send() }, done: x => node(x.responseXML && x.responseXML.documentElement, 0) },
            { name: "overrideMimeType with a charset", run: x => { x.open("GET", B + "/badutf"); x.overrideMimeType("text/plain; charset=ISO-8859-1"); x.send() }, done: x => x.responseText },
            { name: "overrideMimeType away from xml", run: x => { x.open("GET", B + "/xml"); x.overrideMimeType("text/plain"); x.send() }, done: x => show(x.responseXML) },
            { name: "overrideMimeType that is not a type", run: x => { x.open("GET", B + "/xml"); x.overrideMimeType("bogus"); x.send() }, done: x => show(x.responseXML) + " " + show(x.responseText.length) },
            { name: "overrideMimeType when done", run: x => { x.open("GET", B + "/xml"); x.send() }, done: x => x.overrideMimeType("text/plain") },
            opened("overrideMimeType with no argument", x => x.overrideMimeType()), opened("overrideMimeType before open", x => x.overrideMimeType("text/xml")),
            text("GZIP in capitals", "/gzipcaps"), text("Br in capitals", "/brcaps"), text("deflate of nothing", "/emptydeflate"), text("gzip of nothing", "/emptygzip"),
            text("gzip that is cut", "/cutgzip"), text("br that is not", "/badbr"), text("zstd that is not", "/badzstd"), text("deflate that is not", "/baddeflate"),
            text("deflate that is cut", "/cutdeflate"), text("br that is cut", "/cutbr"), text("zstd that is cut", "/cutzstd"), text("zstd in two blocks", "/zstd2"),
            big("gzip of 9 MiB of zeros", "gzip/9"), big("gzip of 11 MiB of zeros", "gzip/11"), big("gzip to 30 times its size", "gzip/30"),
            big("gzip to 70 times its size", "gzip/70z"), big("deflate of 11 MiB of zeros", "deflate/11"), big("deflate to 30 times its size", "deflate/30"),
            big("br of 9 MiB of zeros", "br/9"), big("br of 11 MiB of zeros", "br/11"), big("br to 70 times its size", "br/70"),
            text("zstd cut in a stored block", "/cutzstdraw"), text("zstd cut in its first block, a stored one", "/cutzstdraw1"),
            text("zstd cut in its last block, a stored one", "/cutzstdrawlast"), text("zstd cut in a block of one byte repeated", "/cutzstdrle"),
            text("zstd cut in the head of a block", "/cutzstdhead"), heard("zstd cut in a stored block, to the close", "/toclose/cutzstdraw"),
            heard("zstd that is cut, to the close", "/toclose/cutzstd"), heard("zstd that is not, to the close", "/toclose/badzstd"),
            heard("gzip to the close", "/toclose/gzip"), heard("gzip that is cut, to the close", "/toclose/cutgzip"),
            heard("gzip cut in its head, to the close", "/toclose/cutgziphead"), heard("gzip cut in its sum, to the close", "/toclose/cutgzipsum"),
            heard("gzip cut before what it repeats, to the close", "/toclose/cutgzip2"), heard("gzip cut in what it repeats, to the close", "/toclose/cutgzip3"),
            heard("gzip that is not, to the close", "/toclose/badgzip"), heard("gzip of nothing, to the close", "/toclose/emptygzip"),
            heard("gzip cut in its head", "/cutgziphead"), heard("gzip cut in its sum", "/cutgzipsum"), heard("gzip cut in what it repeats", "/cutgzip3"),
            heard("deflate that is cut, to the close", "/toclose/cutdeflate"), heard("br that is cut, to the close", "/toclose/cutbr"),
            heard("text to the close", "/toclose/json"),
            big("zstd of 11 MiB of zeros", "zstd/11"), big("zstd to 70 times its size", "zstd/70"), big("zstd to 150 times its size", "zstd/150z"),
            text("zstd with a window of 128 MiB", "/zstdwindow"), text("zstd with a window of 144 MiB", "/zstdwindow2"), text("zstd in the format 0.4", "/zstd04"), text("zstd in the format 0.5", "/zstd05"), text("zstd in the format 0.6", "/zstd06"), text("zstd in the format 0.7", "/zstd07"),
            whole("zstd in the format 0.1, whole", "/zstd01"), whole("zstd, then the format 0.4, whole", "/zstd1x04"), whole("zstd, then the format 0.1, whole", "/zstd1x01"), whole("zstd, then the format 0.5, whole", "/zstd1x05"), whole("zstd, then the format 0.7, whole", "/zstd1x07"), whole("zstd in the format 0.5, then zstd, whole", "/zstd05x1"), whole("zstd in the format 0.5, then 0.4, whole", "/zstd05x04"), whole("zstd in the format 0.5, then 0.7, whole", "/zstd05x07"), whole("zstd to skip, then the format 0.5, whole", "/zstdskip05"), whole("zstd to skip, then the format 0.4, whole", "/zstdskip04"), whole("zstd to skip, then zstd, whole", "/zstdskip1"), whole("zstd, one to skip, then the format 0.5, whole", "/zstd1xskip05"), whole("zstd, in two chunks, whole", "/split1/zstd"), whole("zstd, then the format 0.5, in two chunks, whole", "/split1/zstd1x05"), whole("zstd in the format 0.4, whole", "/zstd04"), whole("zstd in the format 0.5, whole", "/zstd05"),
            { name: "cut gzip headers", run: x => { x.open("GET", B + "/cutgzip"); x.send() }, done: x => x.getAllResponseHeaders() + " # " + x.status },
            { name: "deflate headers", run: x => { x.open("GET", B + "/deflate"); x.send() }, done: x => x.getAllResponseHeaders() + " # " + x.status },
            { name: "deflate of nothing headers", run: x => { x.open("GET", B + "/emptydeflate"); x.send() }, done: x => x.getAllResponseHeaders() + " # " + x.status },
            { name: "bad br states", run: x => { x.open("GET", B + "/badbr"); x.send() }, each: x => x.status + " " + show(x.getAllResponseHeaders()) + " " + show(x.responseText), done: x => "done" },
            code("charset us-ascii", "/ascii"), code("charset windows-1252", "/cp1252"), code("charset latin1 below a0", "/latin1c1"), text("charset KOI8-R", "/koi8"),
            text("charset ISO_8859-5", "/iso5"), text("UTF-32 mark, no charset", "/utf32"), text("UTF-16BE with a byte left over", "/utf16odd"),
            code("UTF-8 that is not", "/utf8odd"), text("charset that is empty", "/emptycs"), text("html with an http-equiv charset", "/htmlequiv"),
            text("html with charset unicode", "/htmlunicode"), text("html with a mark", "/htmlbom"), text("html with a charset in the header too", "/htmlcs"),
            tree("xml that is not UTF-8", "/xmlnoenc"), tree("xml with two roots", "/xml2roots"), tree("xml with text before the root", "/xmltext"),
            tree("xml left open", "/xmlopen"), tree("xml with an entity nobody declared", "/xmlentity"), tree("xml with namespaces", "/xmlns"),
            tree("xml with a declared entity", "/xmldoctype"), tree("xml with a document type", "/xmldoctype2"), tree("xml with comments", "/xmlcomment"),
            tree("xml after white space", "/xmlws"), tree("xml declaration after white space", "/xmldeclws"), tree("xml 1.1", "/xml11"), tree("xml of nothing", "/xmlempty"),
            tree("xml with carriage returns", "/xmlcr"), tree("xml attribute values", "/xmlattr"), tree("xml attribute twice", "/xmldup"),
            tree("xml prefix nobody declared", "/xmlprefix"), tree("xml character data sections", "/xmlcdata"), tree("xml in UTF-16", "/xmlutf16"),
            tree("xml encoding nobody knows", "/xmlunknownenc"), tree("xml standalone no", "/xmlstandalone"), tree("xml declaration without a version", "/xmlnoversion"),
            tree("xml character references", "/xmlcharref"), tree("xml end tag of another name", "/xmlmismatch"), tree("xml with text after the root", "/xmlafter"),
            tree("xml attribute with a less-than", "/xmllt"), tree("xml with a bare ampersand", "/xmlamp"), tree("xml attributes not spaced", "/xmlnospace"),
            tree("xml tags with white space", "/xmlendspace"),
            wire("a request as it goes out", x => {}),
            wire("header names as written", x => { x.setRequestHeader("x-lower", "1"); x.setRequestHeader("X-UPPER", "2"); x.setRequestHeader("x-twice", "a"); x.setRequestHeader("X-TWICE", "b"); x.setRequestHeader("user-agent", "ua"); x.setRequestHeader("accept-language", "xx") }),
            wire("a POST as it goes out", x => { x.setRequestHeader("content-type", "text/x"); x.setRequestHeader("X-After", "1") }, "POST", "body"),
            wire("a PATCH as it goes out", x => {}, "PATCH", "body"), wire("a POST of nothing as it goes out", x => {}, "POST"),
            { name: "a POST with no argument as it goes out", run: x => { x.open("POST", B + "/wire"); x.send() }, done: x => x.responseText.replace(/127\.0\.0\.1:\d+/, "HOST") },
            wire("a GET with a type as it goes out", x => { x.setRequestHeader("content-type", "text/x"); x.setRequestHeader("X-After", "1") }),
            { name: "headers when opened again", run: x => { x.open("GET", B + "/echo"); x.setRequestHeader("X-A", "1"); x.setRequestHeader("X-B", "1"); x.open("GET", B + "/echo"); x.setRequestHeader("X-A", "2"); x.setRequestHeader("X-A", "3"); x.send() }, done: x => x.responseText },
            { name: "headers after an answer", run: x => { x.open("GET", B + "/echo"); x.setRequestHeader("X-A", "1"); x.send() }, then: x => { x.open("POST", B + "/echo"); x.send("b") }, done: x => x.responseText },
            { name: "headers after an error answer", run: x => { x.open("GET", B + "/gone"); x.setRequestHeader("X-A", "1"); x.send() }, then: x => { const was = x.status + " " + show(x.responseText) + " " + show(x.responseXML); x.open("GET", B + "/echo"); x.send(); return was }, done: x => x.responseText },
            { name: "headers after a failure", run: x => { x.open("GET", "http://127.0.0.1:1/"); x.setRequestHeader("X-A", "1"); x.send() }, then: x => { x.open("GET", B + "/echo"); x.send() }, done: x => x.responseText },
            { name: "a document read, then another answer", run: x => { x.open("GET", B + "/xml"); x.send() }, then: x => { const d = x.responseXML; x.open("GET", B + "/json"); x.send(); return node(d.documentElement, 9) },
              done: x => [show(x.responseType), node(x.responseXML && x.responseXML.documentElement, 9), typeof x.response, show(x.responseText)].join(" | ") },
            { name: "a charset, then an answer without one", run: x => { x.open("GET", B + "/latin1"); x.send() }, then: x => { x.open("GET", B + "/badutf"); x.send() }, done: x => x.responseText },
            { name: "a type, then an answer without one", run: x => { x.open("GET", B + "/xml"); x.send() }, then: x => { x.open("GET", B + "/notype"); x.send() }, done: x => show(x.responseXML) },
            { name: "overrideMimeType, then another answer", run: x => { x.open("GET", B + "/xmlastext"); x.overrideMimeType("text/xml; charset=ISO-8859-1"); x.send() }, then: x => { x.open("GET", B + "/badutf"); x.send() }, done: x => show(x.responseXML) + " " + x.responseText },
            { name: "POST sent on to a GET", run: x => { x.open("POST", B + "/see"); x.setRequestHeader("X-A", "1"); x.send("body") }, done: x => x.responseText },
            { name: "PUT with a type sent on to a GET", run: x => { x.open("PUT", B + "/see"); x.setRequestHeader("Content-Type", "text/x"); x.send("body") }, done: x => x.responseText },
            { name: "a getter on something else", silent: true, run: x => Object.getOwnPropertyDescriptor(Object.getPrototypeOf(x), "readyState").get.call({}) },
            { name: "a method on something else", silent: true, run: x => x.open.call({}, "GET", B + "/echo") },
            { name: "the prototype", silent: true, run: x => [Object.getPrototypeOf(x) === XMLHttpRequest.prototype, XMLHttpRequest.DONE, x.DONE, typeof XMLHttpRequest.prototype.open, Object.keys(XMLHttpRequest).join(","), x instanceof XMLHttpRequest, typeof Object.getOwnPropertyDescriptor(XMLHttpRequest.prototype, "responseType").set, Object.getOwnPropertyDescriptor(XMLHttpRequest.prototype, "status").set].join(" ") },
            { name: "a node getter on something else", run: x => { x.open("GET", B + "/xml"); x.send() }, done: x => { const e = x.responseXML.documentElement, np = Object.getPrototypeOf(Object.getPrototypeOf(e))
                return [get(() => Object.getOwnPropertyDescriptor(np, "nodeName").get.call({})), get(() => Object.getOwnPropertyDescriptor(Object.getPrototypeOf(e), "tagName").get.call({})), get(() => Object.getOwnPropertyDescriptor(Object.getPrototypeOf(x.responseXML), "xmlVersion").get.call(e)),
                        Object.getOwnPropertyNames(np).sort().join(","), Object.getOwnPropertyNames(Object.getPrototypeOf(e.firstChild)).sort().join(","), Object.getOwnPropertyNames(Object.getPrototypeOf(e.attributes[0])).sort().join(","),
                        show(e.attributes[0].nodeName), show(e.attributes[0].nodeValue), show(e.attributes[0].nodeType), show(e.attributes[0].parentNode === null), show(e.attributes[0].ownerElement.nodeName), show(e.attributes.b.value), show(e.attributes.zz), show(e.childNodes[99]),
                        show(e.parentNode), show(x.responseXML.parentNode), show(x.responseXML.firstChild), show(x.responseXML.attributes), show(e.firstChild.attributes), show(e.firstChild.firstChild), show(e.firstChild.previousSibling), show(e.lastChild.nextSibling),
                        show(e.firstChild.wholeText), show(e.firstChild.length), show(e.documentElement), show(Object.keys(e.childNodes).join(",")), show(e.childNodes.length), show(JSON.stringify(e.attributes))].join(" ") } },
            again("sent again from its last handler", "/json", x => { x.open("GET", B + "/echo"); x.send() }),
            again("sent again to a page that is gone", "/json", x => { x.open("GET", B + "/gone"); x.send() }),
            again("sent again after a page that is gone", "/gone", x => { x.open("GET", B + "/echo"); x.send() }),
            again("sent again after an answer that is not gzip", "/badgzip", x => { x.open("GET", B + "/echo"); x.send() }),
            again("sent again after an answer without a body", "/nobody", x => { x.open("GET", B + "/echo"); x.send() }),
            again("opened in its last handler, sent later", "/json", x => { x.open("GET", B + "/echo"); Qt.callLater(() => x.send()) }),
            again("sent again and aborted in its last handler", "/json", x => { x.open("GET", B + "/echo"); x.send(); x.abort() }, x => x.readyState === 0),
            again("sent again twice from its last handler", "/json", x => { x.open("GET", B + "/echo"); x.send(); x.open("GET", B + "/gzip"); x.send() }),
            again("sent again from the last handler of a synchronous request", "/json", x => { x.open("GET", B + "/echo"); x.send() }, null, false),
            (() => { let sent = false
                     return { name: "sent again from onloadend", run: x => { x.onloadend = () => { if (sent) return; sent = true; x.open("GET", B + "/echo"); x.send() }; x.open("GET", B + "/json"); x.send() },
                              each: seen, last: x => "waits", until: x => sent && x.readyState === 4, late: seen } })(),
            // From the handler of DONE, Qt 6.12.0 ends with a segmentation fault.
            (() => { let sent = false
                     return { name: "sent again synchronously from onloadend", run: x => { x.onloadend = () => { if (sent) return; sent = true; x.open("GET", B + "/echo", false); x.send() }; x.open("GET", B + "/json"); x.send() },
                              each: seen, last: x => "waits", until: x => sent && x.readyState === 4, late: seen } })(),
            (() => { let sent = false
                     return { name: "sent again from onload", run: x => { x.onload = () => { if (sent) return; sent = true; x.open("GET", B + "/echo"); x.send() }; x.onloadend = () => { root.out += "  loadend " + seen(x) + "\n" }; x.open("GET", B + "/json"); x.send() },
                              each: seen, last: x => "waits", until: x => sent && x.readyState === 4, late: seen } })(),
            buffer("arraybuffer of an answer without a body", B + "/nobody"), buffer("arraybuffer of a gzip given up on", B + "/big/gzip/11"),
            buffer("arraybuffer of an answer that is not gzip", B + "/badgzip"), buffer("arraybuffer of a failure", "http://127.0.0.1:1/"),
            buffer("arraybuffer of a page that is gone", B + "/gone"), buffer("arraybuffer of an answer with a body", B + "/bytes"),
            { name: "arraybuffer without a body sent on", run: x => { x.open("GET", B + "/nobody"); x.responseType = "arraybuffer"; x.send() },
              then: x => { const r = x.response; x.open("POST", B + "/echo"); x.responseType = ""; x.send(r) }, done: x => x.responseText },
            tree("xml with an external entity", "/dtdext"), tree("xml with a public external entity", "/dtdextpub"), tree("xml with white space in entity declarations", "/dtdextws"),
            tree("xml with an entity declared external, then internal", "/dtdextfirst"), tree("xml with a parameter entity nobody refers to", "/dtdpeunused"),
            tree("xml with a parameter entity nobody declared", "/dtdpeundeclared"), tree("xml with an external parameter entity", "/dtdpeext"),
            tree("xml with an entity declared after an external parameter entity", "/dtdpeextthen"), tree("xml with an entity nobody declared after a parameter entity", "/dtdpethen"),
            tree("xml attribute with an entity nobody declared after a parameter entity", "/dtdpethenattr"),
            tree("xml with an entity nobody declared in a value after a parameter entity", "/dtdpethenvalue"), tree("xml with a parameter entity in an entity value", "/dtdpeinvalue"),
            tree("xml with a parameter entity that is a quote in an entity value", "/dtdpequote"), tree("xml with a percent sign in an entity value", "/dtdpercent"),
            tree("xml with a parameter entity that refers to itself", "/dtdpeself"), tree("xml with a parameter entity that has a notation", "/dtdpendata"),
            tree("xml with a parameter entity declared without a space", "/dtdpenospace"), tree("xml with a parameter entity that ends inside an entity value", "/dtdpesplit"),
            tree("xml with a parameter entity that ends inside a declaration", "/dtdpesplitdecl"),
            tree("xml with a parameter entity whose declaration starts with a character reference", "/dtdpecharref"),
            tree("xml with a parameter entity in the value of another", "/dtdpeinpe"), tree("xml with an entity nobody declared and an external subset", "/dtdextsubset"),
            tree("xml attribute with an entity nobody declared and an external subset", "/dtdextsubsetattr"),
            tree("xml with a public external subset and an internal one", "/dtdextsubsetpub"), tree("xml with a public identifier that is not one", "/dtdpublicid"),
            tree("xml with an entity that leaves an element open", "/dtdmarkupopen"), tree("xml with character references in entity values", "/dtdcharref"),
            tree("xml attribute with an entity that holds a tag", "/dtdinattrlt"), tree("xml attribute with an entity that holds a document type", "/dtdinattrdoctype"),
            tree("xml attribute with an entity that holds a character reference", "/dtdinattramp"), tree("xml attribute with an entity that holds a tab and a line feed", "/dtdinattrtab"),
            tree("xml with an entity that uses a prefix of the document", "/dtdentityprefix"), tree("xml with an entity that holds a comment of two lines", "/dtdentitycomment"),
            tree("xml with an entity that holds a carriage return", "/dtdentitycr"), tree("xml with an entity that ends inside a reference", "/dtdentityname"),
            tree("xml that declares amp and lt again", "/dtdpredefined"), tree("xml with an entity of 4097 characters", "/dtdlimit"),
            tree("xml attribute with an entity of 4097 characters", "/dtdlimitattr"), tree("xml with a default attribute", "/dtdattlist"),
            tree("xml with fixed and enumerated default attributes", "/dtdattlistfixed"), tree("xml with default namespace declarations", "/dtdattlistns"),
            tree("xml with a default namespace declaration and a written one", "/dtdattlistnswritten"), tree("xml with a default attribute that has a prefix", "/dtdattlistprefix"),
            tree("xml with a default attribute whose prefix nobody declared", "/dtdattlistnoprefix"), tree("xml with a default attribute written under another prefix", "/dtdattlistsame"),
            tree("xml with an attribute declared twice", "/dtdattlisttwice"), tree("xml with an entity in a default attribute", "/dtdattlistentity"),
            tree("xml with an entity nobody declared in a default attribute", "/dtdattlistundeclared"), tree("xml with line ends in default attributes", "/dtdattlistlines"),
            tree("xml with carriage returns in a default attribute", "/dtdattlistcr"), tree("xml with an attribute type and default not spaced", "/dtdattlistnospace"),
            tree("xml with an attribute declared as tokens", "/dtdattnorm"), tree("xml with a declaration nobody knows", "/dtdbaddecl"),
            tree("xml with an entity declared without a value", "/dtdbadentity"), tree("xml with an entity value without quotes", "/dtdentitynoquote"),
            tree("xml with text in the internal subset", "/dtdtextinsubset"), tree("xml with a conditional section", "/dtdcondsect"),
            tree("xml with a content model of both , and |", "/dtdbadelement"), tree("xml with a notation declared without an identifier", "/dtdbadnotation"),
            tree("xml with two dashes inside a comment", "/dtddashes"), tree("xml with the end of a character data section in a text", "/dtdbrackets"),
            tree("xml with a control character", "/dtdcontrol"), tree("xml with a prefix declared as nothing", "/dtdnsempty"),
            tree("xml attribute twice under two prefixes of one namespace", "/dtdnssame"), tree("xml character reference with 0x after the x", "/dtdhex0x"),
            tree("xml attribute name with an at sign", "/dtdnameat"), tree("xml processing instruction with a plus after its target", "/dtdpiplus"),
            tree("xml element name with a dash that is no letter", "/dtdnamedash"), tree("xml element name that starts with a combining mark", "/dtdnamemark"),
            tree("xml element name with a character past U+FFFF", "/dtdnameastral"), tree("xml processing instruction whose target is no name", "/dtdnametarget"),
            tree("xml prefix that is no name", "/dtdnameprefix"), tree("xml name token with a combining mark", "/dtdtokenmark"),
            tree("xml name token with a circled letter", "/dtdtokencircled"), tree("xml declaration with the encoding twice", "/dtddecltwice"),
            tree("xml declaration with standalone yes, then no", "/dtddeclyesno"), tree("xml declaration with a character reference in standalone", "/dtddeclref"),
            tree("xml declaration with a character reference in the encoding", "/dtddeclencref"), tree("xml declaration that declares a namespace", "/dtddeclns"),
            tree("xml declaration that declares a prefix", "/dtddeclprefix"), tree("xml declaration with namespaces, an encoding and standalone", "/dtddeclall"),
            tree("xml declaration that declares the prefix of a default attribute", "/dtddeclattlist"), tree("xml that names US-ASCII and is not", "/dtdencascii"),
            tree("xml in latin1 that names UTF-8, then latin1", "/dtdencutflatin"), tree("xml in UTF-16 without a mark", "/dtdutf16nomark"), tree("xml in UTF-16 from the big end without a mark", "/dtdutf16benomark"),
            tree("xml in UTF-16 with a mark that names UTF-8", "/dtdutf16utf8"), tree("xml in UTF-32 with a mark", "/dtdutf32"),
            tree("xml in UTF-32 from the big end without a mark", "/dtdutf32be"), tree("xml in UTF-32 without a mark that names UTF-32", "/dtdutf32named"),
            { name: "xml with entity values seventeen deep", run: x => { x.open("GET", B + "/dtddeep"); x.send() },
              done: x => { const d = x.responseXML; return d === null ? "null" : d.documentElement.attributes[0].value.length } },
            // Every name of US-ASCII and of ISO-8859-1 that ICU, which Qt asks, knows.
            code("charset ANSI_X3.4-1986", "/csascii0"), code("charset ISO_646.irv:1991", "/csascii1"), code("charset iso_646.irv:1983", "/csascii2"), code("charset us", "/csascii3"),
            code("charset csASCII", "/csascii4"), code("charset iso-ir-6", "/csascii5"), code("charset cp367", "/csascii6"), code("charset ascii7", "/csascii7"),
            code("charset 646", "/csascii8"), code("charset windows-20127", "/csascii9"), code("charset ibm-367", "/csascii10"), code("charset IBM367", "/csascii11"),
            code("charset 8859_1", "/cslatin12"), code("charset iso-ir-100", "/cslatin13"), code("charset ISO_8859-1:1987", "/cslatin14"), code("charset 819", "/cslatin15"), code("charset csISOLatin1", "/cslatin16"),
            tree("xml that names US", "/xmlencus"), tree("xml that names US and is not", "/xmlencusnot"),
            { name: "what a request has", silent: true, run: x => { const names = []; for (const k in x) names.push(k); return names.sort().join(",") + " # " + Object.getOwnPropertyNames(Object.getPrototypeOf(x)).sort().join(",") + " # " + show(x.withCredentials) + show(x.timeout) + show(x.upload) + show(String(x)) } },
        ]
        // Logins. Each of @LOGIN0@… is another [`login_server`]: Qt remembers a login for a
        // host and port, so each group of cases starts with nothing remembered.
        {
            const BASIC = 'Basic realm="r"', DIGEST = 'Digest realm="r", nonce="abc", qop="auth", opaque="op"'
            const q = (host, h, ok, more) => host + "/q?h=" + encodeURIComponent(h) + (ok ? "&ok=" + encodeURIComponent(ok) : "") + (more || "")
            const read = x => x.status + " " + x.responseText + " # " + x.responseURL.replace(/127\.0\.0\.1:\d+/, "HOST").split("?")[0]
            const openAs = (x, method, url, user, password) => { if (user === undefined) x.open(method, url); else if (password === undefined) x.open(method, url, true, user); else x.open(method, url, true, user, password) }
            // A request with this login: the states it went through and what it read.
            const login = (name, url, user, password, own) => {
                const seen = []
                return { name: name, run: x => { openAs(x, "GET", url, user, password); if (own) x.setRequestHeader("Authorization", own); x.send() }, note: x => { seen.push(x.readyState) }, done: x => seen.join("") + " " + read(x) }
            }
            // The same and, once it ended, another on the same object.
            const two = (name, url, user, password, own, then) => { const c = login(name, url, user, password, own); c.then = x => { const was = read(x); then(x); return was }; return c }
            const sync = (name, url, user, password) => ({ name: name, silent: true, run: x => { if (user === undefined) x.open("GET", url, false); else x.open("GET", url, false, user, password); x.send(); return read(x) } })
            const A = "@LOGIN0@", D = "@LOGIN1@", S = "@LOGIN2@", U = "@LOGIN3@", K = "@LOGIN4@"
            queue = queue.concat([
                login("a login answers a Basic challenge", q(A, BASIC), "us er", "p@s"),
                login("the login goes with the next request", A + "/said"),
                login("another login goes at once", q(A, BASIC, "", "&n=1"), "other", "pw"),
                sync("a synchronous request starts without the login", q(A, BASIC, "", "&n=2")),
                two("a refused login is forgotten", q(A, BASIC, "never"), "u", "p", "", x => { x.open("GET", A + "/said"); x.send() }),
                login("the refused login answers the next challenge", q(A, BASIC, "", "&n=3")),
                login("a challenge that cannot be answered", q(A, 'Bearer realm="r"', "never")),
                login("an Authorization of the page after a 401", A + "/said", undefined, undefined, "Own 1"),
                two("an Authorization of the page, then a challenge", A + "/said", undefined, undefined, "Own 2", x => { x.open("GET", q(A, BASIC, "Basic ")); x.send() }),
                login("no challenge", q(A, "", "never")),
                two("a challenge for another realm", q(A, 'Basic realm="s"', "never"), undefined, undefined, "", x => { x.open("GET", A + "/said"); x.send() }),
                login("a login answers a Digest challenge", q(D, DIGEST), "u", "p"),
                login("Digest goes with the next request", D + "/said"),
                two("another nonce refuses the login", q(D, 'Digest realm="r", nonce="def", qop="auth"', 'nonce="def"'), undefined, undefined, "", x => { x.open("GET", q(D, 'Digest realm="r", nonce="def", qop="auth"', 'nonce="def"')); x.send() }),
                login("a stale nonce", q(D, 'Digest realm="r", nonce="ghi", qop="auth", stale=true', 'nonce="ghi"')),
                login("Digest, then a Basic challenge for another realm", q(D, 'Basic realm="s"', "Basic ")),
                login("Basic goes on after Digest", D + "/said"),
                sync("a synchronous request answers the challenge", q(S, BASIC), "u", "p"),
                sync("a synchronous request finds the login", q(S, 'basic realm="r"', "", "&n=1")),
                sync("Digest without qop", q(S, 'Digest realm="r", nonce="n1"')),
                sync("Digest MD5-sess", q(S, 'Digest realm="r", nonce="n2", algorithm=MD5-sess, qop="auth-int,auth"')),
                sync("Digest that wants auth-int", q(S, 'Digest realm="r", nonce="n3", qop="auth-int"'), "u", "p"),
                sync("Digest SHA-256 and Basic", q(S, 'Digest realm="r", nonce="n4", algorithm=SHA-256, qop="auth"|' + BASIC)),
                sync("Basic and Digest", q(S, BASIC + "|" + DIGEST)),
                sync("Basic and Digest in one line", q(S, BASIC + ", " + DIGEST)),
                sync("Negotiate and Basic", q(S, "Negotiate|" + BASIC)),
                sync("a synchronous login refused", q(S, BASIC, "never"), "a", "b"),
                sync("the refused login is found", q(S, BASIC, "", "&n=2")),
                { name: "a POST answers the challenge with its body", run: x => { x.open("POST", q(U, BASIC), true, "u", "p"); x.send("body") }, done: x => read(x) },
                login("a login in the address", q(U, BASIC, "", "&n=1").replace("//", "//a:b@")),
                login("a user without a password", q(U, BASIC, "", "&n=2"), "u"),
                login("a user in the address", q(U, BASIC, "", "&n=3").replace("//", "//v@")),
                login("an empty user", q(U, BASIC, "", "&n=4"), "", "pw"),
                login("a login Latin-1 cannot write", q(U, BASIC, "", "&n=5"), "п:é", "ÿ€"),
                login("a login with escapes", q(U, BASIC, "", "&n=6"), "a%41@/b", "c%d"),
                login("a redirect in the host keeps the login", U + "/to?" + encodeURIComponent("/q?h=" + encodeURIComponent(BASIC)), "r1", "r2"),
                login("a redirect to a full address drops the login", U + "/to?" + encodeURIComponent(U + "/said"), "v", "w"),
                login("a redirect to an address with a login", U + "/to?" + encodeURIComponent(U.replace("//", "//x:y@") + "/said")),
                login("a challenge without a realm", q(U, "Basic", "Basic bjpt"), "n", "m", "Own"),
                login("another login for one request", U + "/said", "o", "q"),
                login("the login without a realm comes back", U + "/said"),
                two("an Authorization of the page on a kept connection", q(K, BASIC, "Basic ", "&keep"), "u", "p", "Own", x => { x.open("GET", q(K, BASIC, "Basic ", "&keep&n=1")); x.send() }),
            ])
            // More logins, each group on a server of its own again.
            const O = ["@LOGIN5@", "@LOGIN6@", "@LOGIN7@", "@LOGIN8@", "@LOGIN9@", "@LOGIN10@", "@LOGIN11@", "@LOGIN12@", "@LOGIN13@", "@LOGIN14@", "@LOGIN15@", "@LOGIN16@", "@LOGIN17@", "@LOGIN18@", "@LOGIN19@", "@LOGIN20@", "@LOGIN21@", "@LOGIN22@", "@LOGIN23@", "@LOGIN24@", "@LOGIN25@", "@LOGIN26@", "@LOGIN27@", "@LOGIN28@", "@LOGIN29@", "@LOGIN30@", "@LOGIN31@", "@LOGIN32@", "@LOGIN33@", "@LOGIN34@", "@LOGIN35@"]
            const get = (url, user, password) => y => { openAs(y, "GET", url, user, password); y.send() }
            // Requests in steps: those of a step go out at once, the next step when they all
            // ended, and `last` after them. What each read is written with a step's sorted:
            // which of the sockets a request gets is a race in Qt.
            const steps = (name, last, list) => {
                const out = [], rest = list.slice()
                let over = null
                const step = () => {
                    if (rest.length === 0) { over(); return }
                    const got = []
                    let left = 0
                    rest.shift().forEach(f => {
                        const y = new XMLHttpRequest()
                        left++
                        y.onreadystatechange = () => {
                            if (y.readyState !== 4) return
                            got.push(y.status + " " + y.responseText)
                            if (--left === 0) { out.push(got.sort().join(" ; ")); step() }
                        }
                        f(y)
                    })
                }
                return { name: name, run: x => { over = () => { x.open("GET", last); x.send() }; step() }, done: x => out.join(" | ") + " | " + read(x) }
            }
            // A login and the page's own Authorization answer a challenge whose reply says
            // `more` of its connection; then the page's own alone, which is replaced on the
            // first try only where the socket was closed.
            const own = (name, more) => two(name, q(O[8], BASIC, "Basic ", "&keep" + more), "u", "p", "Own", x => { x.open("GET", q(O[8], BASIC, "Basic ", "&keep&n=1" + more)); x.setRequestHeader("Authorization", "Own"); x.send() })
            const ntlm = (i, more) => O[13 + i] + "/ntlm?keep" + (more || "")
            queue = queue.concat([
                login("Digest with a user and a password in Latin-1", q(O[0], DIGEST, "", "&pw=" + encodeURIComponent("ü")), "é", "ü"),
                login("Digest with a login Latin-1 cannot write", q(O[1], DIGEST, "", "&pw=" + encodeURIComponent("€é")), "пéa", "€é"),
                login("Digest with a realm in UTF-8", q(O[2], 'Digest realm="ré", nonce="abc", qop="auth"'), "u", "p"),
                login("Digest with a realm in Latin-1", q(O[3], 'Digest realm="ré", nonce="abc", qop="auth"').replace("%C3%A9", "%E9"), "u", "p"),
                login("a nonce that is stale twenty times", q(O[4], 'Digest realm="r", nonce="n{n}", qop="auth", stale=true', 'nonce="n20"'), "u", "p"),
                login("a realm that changes twenty times", q(O[5], 'Digest realm="r{n}", nonce="abc", qop="auth"', 'realm="r20"'), "u", "p"),
                login("Digest, Basic and Digest again", q(O[6], 'Digest realm="r0", nonce="abc"||Basic realm="s"||Digest realm="r7", nonce="abc"', 'realm="r7"'), "u", "p"),
                sync("a synchronous request with a nonce that is stale twenty times", q(O[7], 'Digest realm="r", nonce="n{n}", qop="auth", stale=true', 'nonce="n20"'), "u", "p"),
                own("HTTP/1.0 without a Connection closes", "&v=1.0&conn="),
                own("HTTP/1.0 with keep-alive stays", "&v=1.0&conn=keep-alive"),
                own("HTTP/1.0 with another Connection stays", "&v=1.0&conn=x"),
                own("a Connection with close among others closes", "&conn=Keep-Alive,%20Close"),
                own("a Connection with close inside a word closes", "&conn=disclosed"),
                steps("requests at once after a login", O[9] + "/free?k=p1,p2,p3&keep", [
                    [get(q(O[9], BASIC, "", "&keep"), "u", "p")],
                    [0, 1, 2].map(i => get(O[9] + "/said?keep&hold=p1&of=3&i=" + i)),
                    [0, 1, 2].map(i => get(q(O[9], BASIC, "Basic ", "&keep&hold=p2&of=3&i=" + i))),
                    [0, 1, 2, 3].map(i => get(O[9] + "/said?keep&hold=p3&of=4&i=" + i)),
                ]),
                steps("a seventh request at once waits for a socket", O[10] + "/free?k=w", [
                    [0, 1, 2, 3, 4, 5].map(i => get(O[11] + "/said?hold=w&i=" + i)).concat([get(O[11] + "/gone?k=w"), get(O[10] + "/held?k=w&of=6&go")]),
                ]),
                login("NTLM is answered with a type 1 and a type 3 message", ntlm(0), "u", "p"),
                login("nothing goes with the request after NTLM", O[13] + "/said?keep"),
                login("an NTLM challenge on the socket that answered one", ntlm(0, "&n=1")),
                login("NTLM is answered again on a new socket", ntlm(0, "&n=2")),
                login("NTLM with a domain, target information and its time", ntlm(1, "&info=name,time,end&u=us%20er&d=DOM2&pw=" + encodeURIComponent("pä€")), "DOM2\\us er", "pä€"),
                login("NTLM with target information without a time", ntlm(2, "&info=name,end"), "u", "p"),
                login("NTLM in the charset of the server", ntlm(3, "&flags=00888206&target=T%E9&u=" + encodeURIComponent("ué€") + "&d=" + encodeURIComponent("\ue954")), "ué€", "p"),
                login("NTLM with a user at a domain", ntlm(4, "&u=" + encodeURIComponent("u@x") + "&d="), "u@x", "p"),
                login("an NTLM challenge without flags", ntlm(5, "&flags=0&d=" + encodeURIComponent("\u4f44")), "u", "p"),
                login("NTLM without a user", ntlm(6), "", "p"),
                login("an NTLM type 2 message that is cut short", ntlm(7, "&cut=40"), "u", "p"),
                login("an NTLM answer refused", ntlm(8, "&refuse"), "u", "p"),
                login("the login NTLM refused answers again", ntlm(8, "&n=1")),
                login("NTLM and Basic", ntlm(9, "&more=" + encodeURIComponent(BASIC)), "u", "p"),
                login("Basic and NTLM", ntlm(10, "&first&more=" + encodeURIComponent(BASIC)), "u", "p"),
                login("NTLM and Digest", ntlm(11, "&more=" + encodeURIComponent(DIGEST)), "u", "p"),
                login("Negotiate and NTLM", ntlm(12, "&first&more=Negotiate"), "u", "p"),
                login("Basic in the middle of NTLM", ntlm(13, "&down"), "u", "p"),
                sync("a synchronous request answers NTLM", ntlm(14), "u", "p"),
                login("NTLM, then a Basic challenge", ntlm(15), "u", "p"),
                login("the login of NTLM answers Basic", q(O[28], BASIC, "Basic ", "&keep")),
                login("Basic goes on after NTLM", O[28] + "/said?keep"),
                { name: "a POST answers NTLM with its body", run: x => { x.open("POST", ntlm(16), true, "u", "p"); x.send("body") }, done: x => read(x) },
                login("Negotiate alone is not answered", q(O[30], "Negotiate", "never"), "u", "p"),
                login("Negotiate with a token is not answered", q(O[30], "Negotiate abcd", "never", "&n=1"), "u", "p"),
                login("a login with signs that are escapes in responseURL", O[30] + "/said", "a\"<>\\^`{|}\t\x7f !$&'()*+,-.;=_~b", "p\"<>\\^`{|}\t\x7f !$&'()*+,-.;=_~:q"),
            ])
        }
        next()
    }
}
"#;

/// Both logs of the document `src`: Qt 6.12's and the runtime's. `None` without PySide6.
fn qt_and_ours(src: &str) -> Option<(String, String)> {
    let qt = in_qt(src, QT_RUN)?;
    assert!(
        qt.status.success(),
        "Qt: {}",
        String::from_utf8_lossy(&qt.stderr)
    );
    Some((String::from_utf8(qt.stdout).unwrap(), here(src)))
}

/// The document run in Qt 6.12 to its end, whatever that end is.
fn in_qt(src: &str, run: &str) -> Option<std::process::Output> {
    if !std::path::Path::new(PYTHON).exists() {
        testenv::unavailable(format_args!("{PYTHON} (PySide6) is not installed"));
        return None;
    }
    let dir = tempfile::tempdir().unwrap();
    let doc = dir.path().join("cases.qml");
    std::fs::write(&doc, src).unwrap();
    Some(
        std::process::Command::new(PYTHON)
            .args(["-c", run])
            .arg(&doc)
            .env("QT_QPA_PLATFORM", "offscreen")
            .output()
            .unwrap(),
    )
}

/// The log of the document run here.
fn here(src: &str) -> String {
    let mut rt = qml::runtime::Runtime::new(None);
    let mut lp = event_loop::Loop::new(&mut rt);
    let root = rt.load_str(src).unwrap();
    rt.settle();
    let call = |rt: &mut qml::runtime::Runtime, f: &str| {
        rt.call_method(root, f, vec![])
            .unwrap()
            .unwrap()
            .to_js_string()
    };
    // Stops early when a case never ends; the logs then differ at that case.
    lp.until(&mut rt, |rt| call(rt, "isDone") == "true");
    call(&mut rt, "log")
}

/// Fails on the first case whose two logs differ, naming it.
fn same_cases(want: &str, got: &str) {
    let cases = |log: &str| -> Vec<String> { log.split("== ").map(str::to_string).collect() };
    for (got, want) in cases(got).iter().zip(cases(want)) {
        assert_eq!(*got, want, "\nours:\n{got}\nQt:\n{want}");
    }
    assert_eq!(got, want);
}

/// A deliberate difference, written down in docs/qt-differences.md: a request holds
/// at most 32 MiB of a reply (`http::XHR_MAX`) and fails when more comes, and Qt 6.12 holds
/// whatever comes. A reply of exactly 32 MiB is the same in both.
#[test]
fn a_reply_over_32_mib_fails_here_and_comes_whole_in_qt() {
    let port = response_server();
    let (head, rest) = RESPONSES_QML.split_once("queue = [").unwrap();
    // The cases end at the last `next()`: the logins are queued between `]` and it.
    let (_, tail) = rest.rsplit_once("\n        next()").unwrap();
    let cases = r#"
            { name: "32 MiB", run: x => { x.onerror = () => { root.out += "  onerror\n" }; x.open("GET", B + "/plain/32"); x.responseType = "arraybuffer"; x.send() }, done: x => x.status + " " + x.response.byteLength },
            { name: "33 MiB", run: x => { x.onerror = () => { root.out += "  onerror\n" }; x.open("GET", B + "/plain/33"); x.responseType = "arraybuffer"; x.send() }, done: x => x.status + " " + x.response.byteLength },"#;
    let src = format!("{head}queue = [{cases}\n        ]\n        next(){tail}")
        .replace("@BASE@", &format!("http://127.0.0.1:{port}"));
    let Some((qt, ours)) = qt_and_ours(&src) else {
        return;
    };
    let within = "== 32 MiB\n  ran undefined\n  done \"200 33554432\"\n";
    assert_eq!(
        qt,
        format!("{within}== 33 MiB\n  ran undefined\n  done \"200 34603008\"\n")
    );
    assert_eq!(
        ours,
        format!("{within}== 33 MiB\n  ran undefined\n  done \"0 0\"\n  onerror\n")
    );
}

/// A difference written down in docs/qt-differences.md: a zstd frame of the format 0.5 or
/// 0.7 that comes as two chunks of the transfer coding, the first of one byte. Qt 6.12 gives
/// the reply up and here it is read: libzstd knows an old frame by the first piece of it,
/// QNetworkReply hands it a chunk at a time and ureq what it took out of both.
#[test]
fn an_old_zstd_frame_in_two_chunks_is_read_here_and_dropped_in_qt() {
    let port = response_server();
    let (head, rest) = RESPONSES_QML.split_once("queue = [").unwrap();
    let (_, tail) = rest.rsplit_once("\n        next()").unwrap();
    let cases = r#"
            whole("0.5", "/split1/zstd05"), whole("0.7", "/split1/zstd07"), whole("0.5 whole", "/zstd05"), whole("0.7 whole", "/zstd07"),"#;
    let src = format!("{head}queue = [{cases}\n        ]\n        next(){tail}")
        .replace("@BASE@", &format!("http://127.0.0.1:{port}"));
    let Some((qt, ours)) = qt_and_ours(&src) else {
        return;
    };
    let read = |name: &str, coding: &str| {
        format!(
            "== {name}\n  ran undefined\n  done \"states 1,2,3,4 | 200 | \\\"OK\\\" | \\\"hello\\\" | \\\"content-type: text/plain\\\\r\\\\ncontent-encoding: zstd\\\\r\\\\n{coding}connection: close\\\"\"\n"
        )
    };
    let dropped = |name: &str| {
        format!(
            "== {name}\n  ran undefined\n  done \"states 1,3,4 | 200 | \\\"OK\\\" | \\\"\\\" | \\\"\\\"\"\n"
        )
    };
    let whole = read("0.5 whole", "") + &read("0.7 whole", "");
    assert_eq!(qt, dropped("0.5") + &dropped("0.7") + &whole);
    let chunked = "transfer-encoding: chunked\\\\r\\\\n";
    assert_eq!(ours, read("0.5", chunked) + &read("0.7", chunked) + &whole);
}

/// A deliberate difference, written down in docs/qt-differences.md: a request answers a 401
/// a thousand times (`MAX_ANSWERS` in xhr.rs) and then the 401 is the page's; Qt 6.12
/// answers as long as the server asks. The server here calls every Digest answer stale; its
/// nonces count from `n0`, and it takes the answer made with `n1000`, the 1001st, which only
/// Qt gets to. The runtime's last, the thousandth, is made with `n999`.
#[test]
fn a_challenge_is_answered_a_thousand_times_here_and_without_end_in_qt() {
    let (head, rest) = RESPONSES_QML.split_once("queue = [").unwrap();
    let (_, tail) = rest.rsplit_once("\n        next()").unwrap();
    let cases = r#"
            { name: "stale", run: x => { x.open("GET", "@LOGIN@/q?h=" + encodeURIComponent('Digest realm="r", nonce="n{n}", qop="auth", stale=true') + "&ok=" + encodeURIComponent('nonce="n1000"'), true, "u", "p"); x.send() }, done: x => x.status + " " + x.responseText },
            { name: "stale, synchronous", silent: true, run: x => { x.open("GET", "@LOGIN@/q?h=" + encodeURIComponent('Digest realm="r", nonce="n{n}", qop="auth", stale=true') + "&ok=" + encodeURIComponent('nonce="n1000"') + "&s", false, "u", "p"); x.send(); return x.status + " " + x.responseText } },"#;
    let src = format!("{head}queue = [{cases}\n        ]\n        next(){tail}")
        .replace("@LOGIN@", &format!("http://127.0.0.1:{}", login_server()));
    let Some((qt, ours)) = qt_and_ours(&src) else {
        return;
    };
    let said = |status: &str, nonce: u32| {
        format!(
            "{status}GET Digest username=\\\"u\\\", realm=\\\"r\\\", nonce=\\\"n{nonce}\\\", uri=<this>, response=<right>, qop=auth, nc=00000001, cnonce=<32 hex> [Connection,Accept-Encoding,Accept-Language,User-Agent,Authorization]"
        )
    };
    let log = |status: &str, nonce: u32| {
        let said = said(status, nonce);
        format!(
            "== stale\n  ran undefined\n  done \"{said}\"\n== stale, synchronous\n  ran \"{said}\"\n"
        )
    };
    assert_eq!(qt, log("200 ", 1000));
    assert_eq!(ours, log("401 need: ", 999));
}

/// Written down in docs/qt-differences.md: a synchronous request sent from the handler of
/// DONE of a finished reply. Qt 6.12 runs the handlers of that request and ends with a
/// segmentation fault; here the handlers run and the request is DONE with the second reply.
#[test]
fn a_synchronous_request_from_the_handler_of_done_runs_here_and_ends_qt() {
    use std::os::unix::process::ExitStatusExt;
    let port = response_server();
    let (head, rest) = RESPONSES_QML.split_once("queue = [").unwrap();
    let (_, tail) = rest.rsplit_once("\n        next()").unwrap();
    let cases = r#"
            again("sent again synchronously from its last handler", "/json", x => { x.open("GET", B + "/echo", false); x.send() }),"#;
    let src = format!("{head}queue = [{cases}\n        ]\n        next(){tail}")
        .replace("@BASE@", &format!("http://127.0.0.1:{port}"));
    // Qt does not live to hand its log over: each line is also written as it is made.
    let said = r#"out += "  " + what + " " + v + "\n" }"#;
    assert_eq!(src.matches(said).count(), 1);
    let src = src.replace(
        said,
        r#"out += "  " + what + " " + v + "\n"; console.log("  " + what + " " + v) }"#,
    );
    let quiet = "lambda mode, context, message: None";
    assert!(QT_RUN.contains(quiet));
    let run = QT_RUN.replace(
        quiet,
        "lambda mode, context, message: (sys.stdout.write(message + '\\n'), sys.stdout.flush())",
    );
    let Some(qt) = in_qt(&src, &run) else {
        return;
    };
    let qt_log = String::from_utf8(qt.stdout).unwrap();
    assert_eq!(
        qt.status.signal(),
        Some(11),
        "Qt no longer ends with a segmentation fault: {:?}\n{qt_log}",
        qt.status
    );
    let json = r#"number 200 \"{\\\"a\\\":[1,\\\"é\\\"],\\\"b\\\":null}\" \"content-type: application/json\""#;
    let echo = r#"number 200 \"GET /echo HTTP/1.1\" \"content-type: text/plain\""#;
    // The handlers of the second request run inside its `send()`, before the first handler
    // of DONE returns ("last"). Qt sees the same and ends when that handler returns.
    let seen = format!(
        r#"  state 1 !Error:Invalid state:11
  ran undefined
  state 2 "number 200 \"\" \"content-type: application/json\""
  state 3 "{json}"
  state 4 "{json}"
  state 1 !Error:Invalid state:11
  state 2 "number 200 \"\" \"content-type: text/plain\""
  state 3 "{echo}"
  state 4 "{echo}"
  last undefined
  late "4 {echo}"
"#
    );
    assert_eq!(qt_log, seen);
    assert_eq!(
        here(&src),
        format!("== sent again synchronously from its last handler\n{seen}")
    );
}

/// What `qt_icu.py` prints when run with `args`: the ICU that Qt 6.12 loads, asked itself.
/// `None` without PySide6.
fn icu(args: &[&str]) -> Option<String> {
    if !std::path::Path::new(PYTHON).exists() {
        testenv::unavailable(format_args!("{PYTHON} (PySide6) is not installed"));
        return None;
    }
    let out = std::process::Command::new(PYTHON)
        .args(["-c", include_str!("qt_icu.py")])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "ICU: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Some(String::from_utf8(out.stdout).unwrap())
}

/// A reply of the charset server: the value of its `Content-Type`, its body, and where the
/// body is cut in two: the bytes after the cut are sent once `/go/<n>` is asked for.
/// `/empty/<n>` is the reply `<n>` without its body.
struct Text {
    kind: String,
    body: Vec<u8>,
    cut: Option<usize>,
}

/// Answers `/<n>` with `replies[n]`.
fn charset_server(replies: Vec<Text>) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let replies = std::sync::Arc::new(replies);
    let gone = std::sync::Arc::new((
        std::sync::Mutex::new(std::collections::HashSet::new()),
        std::sync::Condvar::new(),
    ));
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let (replies, gone) = (replies.clone(), gone.clone());
            std::thread::spawn(move || {
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut line = String::new();
                if r.read_line(&mut line).is_err() {
                    return;
                }
                loop {
                    let mut h = String::new();
                    if r.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                        break;
                    }
                }
                let path = line.split(' ').nth(1).unwrap_or("");
                let mut s = s;
                if let Some(n) = path.strip_prefix("/go/") {
                    gone.0.lock().unwrap().insert(n.parse::<usize>().unwrap());
                    gone.1.notify_all();
                    let _ = s.write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                    return;
                }
                let (path, empty) = match path.strip_prefix("/empty") {
                    Some(path) => (path, true),
                    None => (path, false),
                };
                let Some((n, t)) = path[1..]
                    .parse::<usize>()
                    .ok()
                    .and_then(|n| Some((n, replies.get(n)?)))
                else {
                    return;
                };
                let body = if empty { &[] } else { &t.body[..] };
                let mut all = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    t.kind,
                    body.len()
                )
                .into_bytes();
                let first = t.cut.unwrap_or(body.len()).min(body.len());
                all.extend_from_slice(&body[..first]);
                let _ = s.write_all(&all);
                if t.cut.is_some() && !empty {
                    let went = gone.0.lock().unwrap();
                    let mut went = gone.1.wait_while(went, |went| !went.contains(&n)).unwrap();
                    went.remove(&n);
                    drop(went);
                    let _ = s.write_all(&t.body[first..]);
                }
            });
        }
    });
    port
}

/// Asks for `/0`, `/1`, ... in turn and logs the status and the UTF-16 units of each
/// `responseText`. The text is read in LOADING and twice in DONE. For a reply in `cut` the
/// text of LOADING is logged first, and the rest of the reply is asked for there. The
/// request of such a reply first read the same reply without its body: Qt reads the
/// charset of a reply when the reply ends, and reads the text before that in the charset
/// of the reply the request ended last.
const CHARSETS_QML: &str = r#"import QtQml
QtObject {
    id: root
    property string out
    property bool done: false
    property var lines: []
    property var cut: [@CUT@]
    property int at: 0
    signal finished()
    function isDone() { return done }
    function log() { return out }
    function units(t) {
        let s = ""
        for (let k = 0; k < t.length; k++) s += " " + t.charCodeAt(k).toString(16)
        return s
    }
    function next() {
        if (at === @COUNT@) { out = lines.join("\n"); done = true; finished(); return }
        const x = new XMLHttpRequest()
        let line = ""
        let went = false
        let first = cut.indexOf(at) >= 0
        x.onreadystatechange = () => {
            if (first) {
                if (x.readyState !== 4) return
                first = false
                // Not from this handler: a request sent from its last handler has none.
                Qt.callLater(() => { x.open("GET", "@BASE@/" + at); x.send() })
                return
            }
            if (x.readyState === 3) {
                const t = x.responseText
                if (cut.indexOf(at) >= 0 && !went) {
                    went = true
                    line = "3:" + units(t) + " | "
                    const go = new XMLHttpRequest()
                    go.open("GET", "@BASE@/go/" + at)
                    go.send()
                }
            }
            if (x.readyState !== 4) return
            const t = x.responseText
            lines.push(line + x.status + ":" + units(x.responseText) + (t === x.responseText ? "" : " (read anew: another text)"))
            at++
            Qt.callLater(root.next)
        }
        x.open("GET", "@BASE@/" + (first ? "empty/" : "") + at)
        x.send()
    }
    Component.onCompleted: next()
}
"#;

/// The bodies every charset name is asked to read: each byte once, in order; that after
/// each byte order mark; and one with surrogates of UTF-16 with and without their pairs,
/// characters of UTF-8 that are whole, cut and not allowed, and a byte that starts a
/// character at the very end.
fn bodies() -> Vec<Vec<u8>> {
    let every: Vec<u8> = (0..=255).collect();
    let marks: [&[u8]; 6] = [
        &[],
        &[0xFF, 0xFE],
        &[0xFE, 0xFF],
        &[0xEF, 0xBB, 0xBF],
        &[0, 0, 0xFE, 0xFF],
        &[0xFF, 0xFE, 0, 0],
    ];
    let mut all: Vec<Vec<u8>> = marks.iter().map(|m| [m, &every[..]].concat()).collect();
    all.push(
        b"\xD8\x3D\xDE\x00\x3D\xD8\x00\xDE\xD8\x00\x00\x41\x00\xDC\x41\x00\
          \xED\xA0\x80\xED\xB0\x80\xF0\x9F\x98\x80\xC3\xA9\xE2\x82\x41\xC0\xAF\
          \xF4\x90\x80\x80\x1A\x1C\x7F\x41\x81"
            .to_vec(),
    );
    all
}

/// The log of the document run here, a loop turn for each event as it comes.
/// `event_loop::Loop` lets the work of the pool end before a turn, and a reply that waits
/// for a request the document sends meanwhile never ends that way.
fn here_as_events_come(src: &str) -> String {
    let mut rt = qml::runtime::Runtime::new(None);
    let wake = testenv::Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let ep = rt.process_fd();
    let root = rt.load_str(src).unwrap();
    let call = |rt: &mut qml::runtime::Runtime, f: &str| {
        rt.call_method(root, f, vec![])
            .unwrap()
            .unwrap()
            .to_js_string()
    };
    let mut now = 0.0;
    loop {
        rt.poll_native();
        rt.tick(now);
        rt.poll_native();
        rt.settle();
        if call(&mut rt, "isDone") == "true" {
            return call(&mut rt, "log");
        }
        let next = rt.next_deadline(now);
        if next.is_finite() {
            now += next;
        } else if wake.wait(ep, None).fd {
            rt.process_fd_ready();
        }
    }
}

/// A character whose bytes come in two parts of a reply is read whole once both are there,
/// and while only the first is there the text ends before it, without a substitute: in
/// every charset of more bytes than one that is written here, as in Qt. The server sends
/// the second part when the document, having read the text of the first, asks for it.
#[test]
fn a_character_in_two_parts_of_a_reply_is_read_whole_as_in_qt() {
    let cut: [(&str, &[u8], usize); 17] = [
        ("Shift_JIS", b"\x82\xA0\x82\xA2", 3),
        ("EUC-JP", b"\xA4\xA2\xA4\xA4", 3),
        ("EUC-JP", b"\xA4\xA2\x8F\xB0\xA1", 4),
        ("Big5", b"\xA4\x40\xA4\x41", 3),
        ("Big5-HKSCS", b"\xA4\x40\xA4\x41", 3),
        ("GBK", b"\xC4\xE3\xBA\xC3", 3),
        ("GB2312", b"\xC4\xE3\xBA\xC3", 3),
        ("gb18030", b"\xC4\xE3\x81\x30\x81\x30", 4),
        ("EUC-KR", b"\xC7\xD1\xB1\xDB", 3),
        ("windows-949", b"\xC7\xD1\xB1\xDB", 3),
        ("ISO-2022-JP", b"\x1B$B\x24\x22\x24\x24\x1B(B", 4),
        ("ISO-2022-JP", b"\x1B$B\x24\x22\x1B(Ba", 6),
        ("ibm-1208", b"\xE3\x81\x82\xE3\x81\x84", 4),
        ("CESU-8", b"\xE3\x81\x82\xED\xA0\xBD\xED\xB8\x80", 7),
        ("UTF-8", b"\xE3\x81\x82\xE3\x81\x84", 4),
        ("UTF-16", b"\x42\x30\x3D\xD8\x00\xDE", 5),
        ("ucs-2", b"\x30\x42\xD8\x3D\xDE\x00", 4),
    ];
    let replies = cut.iter().map(|c| Text {
        kind: format!("text/plain; charset={}", c.0),
        body: c.1.to_vec(),
        cut: Some(c.2),
    });
    let port = charset_server(replies.collect());
    let every: Vec<String> = (0..cut.len()).map(|n| n.to_string()).collect();
    let src = CHARSETS_QML
        .replace("@COUNT@", &cut.len().to_string())
        .replace("@CUT@", &every.join(","))
        .replace("@BASE@", &format!("http://127.0.0.1:{port}"));
    let Some(qt) = in_qt(&src, QT_RUN) else {
        return;
    };
    assert!(
        qt.status.success(),
        "Qt: {}",
        String::from_utf8_lossy(&qt.stderr)
    );
    let qt = String::from_utf8(qt.stdout).unwrap();
    let ours = here_as_events_come(&src);
    let (qt, ours): (Vec<&str>, Vec<&str>) = (qt.lines().collect(), ours.lines().collect());
    for (n, c) in cut.iter().enumerate() {
        assert_eq!(ours.get(n), qt.get(n), "{} cut after {} bytes", c.0, c.2);
    }
    assert_eq!(ours[0], "3: 3042 | 200: 3042 3044");
    assert_eq!(ours[11], "3: 3042 | 200: 3042 61");
    assert_eq!(ours[16], "3: 3042 | 200: 3042 d83d de00");
}

/// Every name the ICU of Qt 6.12 has for a converter is read here as Qt reads it, or its
/// converter is not written here (docs/qt-differences.md). A reply in a charset that is
/// not written is read as a reply that names a charset nobody knows, and the log has one
/// line about it for the reply, however often its text is read. All the names of a converter are of one kind:
/// none is left unknown. The names come from ICU itself, and the list the runtime looks
/// names up in (`crates/qml/src/xhr_charsets.tsv`) is that list.
#[test]
fn every_charset_name_of_qt_is_read_as_qt_or_is_not_written() {
    const TEST: &str = "every_charset_name_of_qt_is_read_as_qt_or_is_not_written";
    if !testenv::in_own_process(TEST) {
        // The log is read from this process's stderr.
        testenv::run_in_own_process(TEST, [("", ""); 0]);
        return;
    }
    let Some(listed) = icu(&["names"]) else {
        return;
    };
    assert!(
        listed == include_str!("../src/xhr_charsets.tsv"),
        "crates/qml/src/xhr_charsets.tsv is not what `qt_icu.py names` prints"
    );
    let names: Vec<(&str, &str)> = listed
        .lines()
        .map(|l| l.split_once('\t').unwrap())
        .collect();
    let bodies = bodies();
    let plain = |name: &str| -> Vec<Text> {
        let reply = |body: &Vec<u8>| Text {
            kind: format!("text/plain; charset={name}"),
            body: body.clone(),
            cut: None,
        };
        bodies.iter().map(reply).collect()
    };
    let mut replies: Vec<Text> = names.iter().flat_map(|n| plain(n.1)).collect();
    // A name neither Qt nor ICU knows, and names written otherwise than ICU lists them:
    // Qt's own decoders go by their names without `-` and `_`, ICU's by letters and digits.
    const OTHERWISE: [&str; 22] = [
        "x-nobody-knows",
        "utf8",
        "UTF_8",
        "u-t-f-8",
        "utf 8",
        "UTF_16",
        "utf 16",
        "utf-16-le",
        "UTF.16BE",
        "utf_32",
        "utf+32",
        "Latin1",
        "latin-1",
        "l 1",
        "ISO_8859-1",
        "iso-8859-01",
        "IBM-01208",
        "windows 1251",
        "(koi8-r)",
        "CP-0866",
        "shift jis",
        "iso-8859-16",
    ];
    let otherwise_at = replies.len();
    replies.extend(OTHERWISE.iter().flat_map(|n| plain(n)));
    // A charset that is not written here, in replies that name another themselves.
    let named_at = replies.len();
    let named: [(&str, &[u8]); 4] = [
        ("text/html; charset=IBM037", b"<meta charset=koi8-r>\xF0"),
        (
            "application/xml; charset=IBM037",
            b"<?xml version=\"1.0\" encoding=\"koi8-r\"?><a>\xF0</a>",
        ),
        ("text/plain; charset=IBM037", b"\xFF\xFEh\x00"),
        ("text/html; charset=Locale", b"<meta charset=koi8-r>\xF0"),
    ];
    replies.extend(named.iter().map(|n| Text {
        kind: n.0.to_string(),
        body: n.1.to_vec(),
        cut: None,
    }));
    let count = replies.len();
    let port = charset_server(replies);
    let src = CHARSETS_QML
        .replace("@COUNT@", &count.to_string())
        .replace("@CUT@", "")
        .replace("@BASE@", &format!("http://127.0.0.1:{port}"));
    let qt = in_qt(&src, QT_RUN).unwrap();
    assert!(
        qt.status.success(),
        "Qt: {}",
        String::from_utf8_lossy(&qt.stderr)
    );
    let qt = String::from_utf8(qt.stdout).unwrap();
    let (ours, log) = testenv::stderr_with(|| here(&src));
    let (qt, ours): (Vec<&str>, Vec<&str>) = (qt.lines().collect(), ours.lines().collect());
    assert_eq!((qt.len(), ours.len()), (count, count));
    if let Ok(to) = std::env::var("XHR_EQUIV_DUMP") {
        let with = |log: &[&str]| -> String {
            let per = bodies.len();
            let name = |i: usize| names.get(i / per).map_or(("", ""), |n| *n);
            let line = |(i, l): (usize, &&str)| format!("{}\t{}\t{l}\n", name(i).0, name(i).1);
            log.iter().enumerate().map(line).collect()
        };
        std::fs::write(format!("{to}.charsets.qt"), with(&qt)).unwrap();
        std::fs::write(format!("{to}.charsets.ours"), with(&ours)).unwrap();
        std::fs::write(format!("{to}.charsets.log"), &log).unwrap();
    }

    // The lines of the log: which charsets they name, and how many there are.
    let said: Vec<String> = log
        .lines()
        .filter_map(|l| {
            let l = l.strip_prefix("XMLHttpRequest: no decoder here for the charset \"")?;
            Some(l.split_once('"')?.0.to_string())
        })
        .collect();
    let per = bodies.len();
    let of = |lines: &[&str], n: usize| lines[n * per..(n + 1) * per].join("\n");
    let nobody = of(&ours, otherwise_at / per);
    assert_eq!(nobody, of(&qt, otherwise_at / per), "a name nobody knows");
    #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
    enum Kind {
        AsQt,
        NotWritten,
    }
    let mut kinds = std::collections::BTreeMap::<&str, Vec<(Kind, &str)>>::new();
    let mut wrong = String::new();
    for (n, (converter, name)) in names.iter().enumerate() {
        let (qt, ours) = (of(&qt, n), of(&ours, n));
        let kind = if said.iter().any(|s| s.eq_ignore_ascii_case(name)) {
            if ours != nobody {
                wrong += &format!("{name} ({converter}) is not read as a name nobody knows\n");
            }
            Kind::NotWritten
        } else {
            if ours != qt {
                wrong += &format!("{name} ({converter}):\nours:\n{ours}\nQt:\n{qt}\n");
            }
            Kind::AsQt
        };
        kinds.entry(converter).or_default().push((kind, name));
    }
    for (converter, of) in &kinds {
        if of.iter().any(|k| k.0 != of[0].0) {
            wrong += &format!("the names of {converter} are read in more ways than one: {of:?}\n");
        }
    }
    assert!(wrong.is_empty(), "\n{wrong}");
    let counted = |kind: Kind| {
        let of = kinds.values().filter(|of| of[0].0 == kind);
        (of.clone().count(), of.map(Vec::len).sum::<usize>())
    };
    // Converters and names: 228 and 1203 in all.
    assert_eq!(
        (counted(Kind::AsQt), counted(Kind::NotWritten)),
        ((47, 399), (181, 804))
    );
    // One line for a reply: each text was read three times.
    assert_eq!(said.len(), counted(Kind::NotWritten).1 * per + 3);
    for gone in ["ibm-5478", "ISO-2022-KR"] {
        let kind = kinds.values().flatten().find(|k| k.1 == gone).unwrap().0;
        assert_eq!(kind, Kind::NotWritten, "{gone}");
    }

    for (n, name) in OTHERWISE.iter().enumerate() {
        let at = otherwise_at / per + n;
        assert_eq!(of(&ours, at), of(&qt, at), "{name}");
    }
    // What the reply names itself is read, and Qt reads what the header named.
    let ends = |n: usize, with: &str| {
        assert!(ours[named_at + n].ends_with(with), "{}", ours[named_at + n]);
    };
    ends(0, "72 3e 41f");
    ends(1, "3c 61 3e 41f 3c 2f 61 3e");
    ends(2, "200: 68");
    // `Locale` is a name of Qt's own for UTF-8, where the byte starts a character.
    ends(3, "72 3e");
    assert_eq!(ours[named_at + 3], qt[named_at + 3]);
}

/// Requests to `@URL@`, which answers, and `@DEAD@`, where nothing listens, each logged
/// under its name as it goes; `done` once the `loadend` of the last one started ran.
const REFUSED_QML: &str = r#"
import QtQml
QtObject {
    id: root
    property string out
    property bool done: false
    property var kept
    signal finished()
    function made(name) {
        let x = new XMLHttpRequest()
        x.onreadystatechange = () => { root.out += name + " state " + x.readyState + "\n" }
        x.onerror = () => { root.out += name + " error\n" }
        x.onload = () => { root.out += name + " load\n" }
        x.onloadend = () => {
            root.out += name + " loadend " + x.status + " \"" + x.responseText + "\"\n"
            root.done = true
            root.finished()
        }
        return x
    }
    function start(x, name, url, async) {
        done = false
        x.open("GET", url, async)
        x.send()
        out += name + " returned " + x.readyState + "\n"
    }
    function first() { kept = made("first"); start(kept, "first", "@URL@", true) }
    function again() { start(kept, "again", "@URL@", true) }
    function fresh() { start(made("fresh"), "fresh", "@URL@", true) }
    function dead() { start(made("dead"), "dead", "@DEAD@", true) }
    function sync() { start(made("sync"), "sync", "@URL@", false) }
}
"#;

/// Loads the document argv[1] and takes the steps argv[2..] in order. `refuse` has the
/// system refuse this process every new thread (a soft `RLIMIT_NPROC` of 0), `give` ends
/// that, `alarm` has the system end the process in 3 s (for a call that never returns). Any
/// other step is a function of the document to call: it runs until no event is left, and
/// named with a `!` then until `finished`. Prints what Qt itself says as it says it, and
/// the `out` each call added: once no event is left, and again after `finished`.
const QT_STEPS: &str = r#"
import resource, signal, sys, threading
from PySide6.QtCore import QCoreApplication, QEventLoop, QMetaObject, QObject, QUrl, SIGNAL, qInstallMessageHandler
from PySide6.QtQml import QQmlEngine, QQmlComponent
def write(text):
    sys.stdout.write(text)
    sys.stdout.flush()
app = QCoreApplication(sys.argv[:1])
qInstallMessageHandler(lambda mode, context, message: write("Qt: " + message + "\n"))
engine = QQmlEngine()
component = QQmlComponent(engine, QUrl.fromLocalFile(sys.argv[1]))
root = component.create()
if root is None:
    sys.exit("; ".join(e.toString() for e in component.errors()))
QObject.connect(root, SIGNAL("finished()"), app.quit)
given = resource.getrlimit(resource.RLIMIT_NPROC)
for step in sys.argv[2:]:
    if step == "refuse":
        resource.setrlimit(resource.RLIMIT_NPROC, (0, given[1]))
        try:
            threading.Thread(target=lambda: None).start()
            sys.exit("a soft RLIMIT_NPROC of 0 refuses this user no thread")
        except RuntimeError:
            pass
    elif step == "give":
        resource.setrlimit(resource.RLIMIT_NPROC, given)
    elif step == "alarm":
        signal.alarm(3)
    else:
        QMetaObject.invokeMethod(root, step.rstrip("!"))
        loop = QEventLoop()
        while loop.processEvents():
            pass
        write(root.property("out"))
        root.setProperty("out", "")
        if step.endswith("!") and not root.property("done"):
            app.exec()
            write(root.property("out"))
            root.setProperty("out", "")
"#;

/// A server whose n-th connection is answered with the body `n`, and the address of
/// another where nothing listens.
fn numbering_server() -> (String, String) {
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", server.local_addr().unwrap());
    let closed = TcpListener::bind("127.0.0.1:0").unwrap();
    let dead = format!("http://{}/", closed.local_addr().unwrap());
    std::thread::spawn(move || {
        for (n, stream) in server.incoming().enumerate() {
            let mut stream = stream.unwrap();
            let mut head = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            while head.read_line(&mut line).unwrap() > 2 {
                line.clear();
            }
            let body = (n + 1).to_string();
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(reply.as_bytes()).unwrap();
        }
    });
    (url, dead)
}

/// The number of the next connection to the [`numbering_server`] at `url`: one more than
/// the connections made to it so far.
fn next_connection(url: &str) -> String {
    use std::io::Read;
    let host = url.strip_prefix("http://").unwrap().trim_end_matches('/');
    let mut stream = std::net::TcpStream::connect(host).unwrap();
    stream.write_all(b"GET / HTTP/1.0\r\n\r\n").unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    reply.rsplit_once("\r\n\r\n").unwrap().1.to_string()
}

/// The soft `RLIMIT_NPROC` of this process becomes `soft`; returns the one it had. At 0
/// the system refuses the process every new thread with `EAGAIN`, and, unlike after
/// `testenv::refuse_new_threads`, gives them again once the limit is put back.
fn set_nproc(soft: libc::rlim_t) -> libc::rlim_t {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: getrlimit(2) writes the one struct it is given.
    let r = unsafe { libc::getrlimit(libc::RLIMIT_NPROC, &raw mut limit) };
    assert_eq!(r, 0, "getrlimit: {}", std::io::Error::last_os_error());
    let was = limit.rlim_cur;
    limit.rlim_cur = soft;
    // SAFETY: setrlimit(2) reads the one struct it is given.
    let r = unsafe { libc::setrlimit(libc::RLIMIT_NPROC, &raw const limit) };
    assert_eq!(r, 0, "setrlimit: {}", std::io::Error::last_os_error());
    was
}

/// What Qt 6.12 prints for the steps `qt` of [`QT_STEPS`] and how its process ended, then
/// what the runtime logs for the steps `here`, taken the same way, and what it wrote to
/// its stderr meanwhile. Each has a [`numbering_server`] of its own, and its last line is
/// the number of the next connection to it. `None` without PySide6, or where the limit
/// refuses this user no thread (root). The caller is a test in a process of its own.
fn steps_in_qt_and_here(
    qt: &[&str],
    here: &[&str],
) -> Option<(String, std::process::ExitStatus, String, String)> {
    if !std::path::Path::new(PYTHON).exists() {
        testenv::unavailable(format_args!("{PYTHON} (PySide6) is not installed"));
        return None;
    }
    let given = set_nproc(0);
    let refused = std::thread::Builder::new().spawn(|| ()).is_err();
    set_nproc(given);
    if !refused {
        testenv::unavailable("a soft RLIMIT_NPROC of 0 refuses this user no thread");
        return None;
    }
    let document =
        |(url, dead): &(String, String)| REFUSED_QML.replace("@URL@", url).replace("@DEAD@", dead);
    let next = |url: &str| format!("next connection {}\n", next_connection(url));

    let dir = tempfile::tempdir().unwrap();
    let doc = dir.path().join("refused.qml");
    let server = numbering_server();
    std::fs::write(&doc, document(&server)).unwrap();
    let ran = std::process::Command::new(PYTHON)
        .args(["-c", QT_STEPS])
        .arg(&doc)
        .args(qt)
        .env("QT_QPA_PLATFORM", "offscreen")
        .output()
        .unwrap();
    assert!(
        ran.stderr.is_empty(),
        "Qt: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    let in_qt = String::from_utf8(ran.stdout).unwrap() + &next(&server.0);

    let server = numbering_server();
    let mut rt = qml::runtime::Runtime::new(None);
    let mut lp = event_loop::Loop::new(&mut rt);
    let root = rt.load_str(&document(&server)).unwrap();
    rt.settle();
    let said = testenv::stderr_of(|| {
        for step in here {
            match *step {
                "refuse" => _ = set_nproc(0),
                "give" => _ = set_nproc(given),
                _ => {
                    let called = rt.call_method(root, step.trim_end_matches('!'), vec![]);
                    assert!(matches!(called, Some(Ok(_))), "{step}: {called:?}");
                    if step.ends_with('!') {
                        lp.wait_until(&mut rt, |rt| rt.get(root, "done").to_js_string() == "true");
                    } else {
                        lp.drain(&mut rt);
                    }
                }
            }
        }
    });
    set_nproc(given);
    assert_eq!(rt.errors(), Vec::<String>::new());
    let here = rt.get(root, "out").to_js_string() + &next(&server.0);
    Some((in_qt, ran.status, here, said))
}

/// What Qt 6.12 says when the system refuses it a thread.
const QT_REFUSED: &str =
    "Qt: QThread::start: Thread creation error (Resource temporarily unavailable)\n";

/// A deliberate difference, written down in docs/qt-differences.md: a request that needs a
/// new thread while the system refuses one. Qt 6.12 says so in its log and leaves the
/// request opened for good: no handler runs after `send`, and once threads are given again
/// neither that request, opened and sent again, nor a new one goes out: 3 s later no
/// `loadend` has run, and the process waiting for one is ended there. Here the request
/// fails as one does that reached no server: after `send` returned the handlers run for
/// DONE, `onerror` and `onloadend` with status 0, and the refusal is in the log. Once
/// threads are given again the same request, opened and sent again, and a new one both go
/// out. Neither sent anything while refused: the server's first connection is the next.
#[test]
fn a_request_refused_its_thread_fails_here_and_stays_opened_in_qt() {
    use std::os::unix::process::ExitStatusExt;
    const TEST: &str = concat!(
        module_path!(),
        "::a_request_refused_its_thread_fails_here_and_stays_opened_in_qt"
    );
    if !testenv::in_own_process(TEST) {
        return testenv::run_in_own_process(TEST, [("LC_ALL", "C.UTF-8")]);
    }
    let Some((qt, ended, here, said)) = steps_in_qt_and_here(
        &["refuse", "first", "give", "again", "alarm", "fresh!"],
        &["refuse", "first", "give", "again!", "fresh!"],
    ) else {
        return;
    };
    assert_eq!(ended.signal(), Some(libc::SIGALRM), "{ended}");
    assert_eq!(
        qt,
        format!(
            "{QT_REFUSED}first state 1\nfirst returned 1\n\
             first state 1\nagain returned 1\n\
             fresh state 1\nfresh returned 1\n\
             next connection 1\n"
        )
    );
    let sent = |name: &str, started: &str, n: u8| {
        format!(
            "{name} state 1\n{started} returned 1\n{name} state 2\n{name} state 3\n\
             {name} state 4\n{name} load\n{name} loadend 200 \"{n}\"\n"
        )
    };
    assert_eq!(
        here,
        "first state 1\nfirst returned 1\nfirst state 4\nfirst error\nfirst loadend 0 \"\"\n"
            .to_string()
            + &sent("first", "again", 1)
            + &sent("fresh", "fresh", 2)
            + "next connection 3\n"
    );
    let line = "XMLHttpRequest: cannot start the thread for a request:";
    assert_eq!(said.matches(line).count(), 1, "stderr: {said:?}");
}

/// The other side of that difference: with a thread for requests already running (one
/// request ran to its end before the refusal), the next request goes out on it, in Qt 6.12
/// and here alike, and nothing is logged. Before both, a request to an address where
/// nothing listens: the order a refused request fails in here is the order of that one.
#[test]
fn a_request_goes_out_on_a_running_thread_while_new_ones_are_refused() {
    const TEST: &str = concat!(
        module_path!(),
        "::a_request_goes_out_on_a_running_thread_while_new_ones_are_refused"
    );
    if !testenv::in_own_process(TEST) {
        return testenv::run_in_own_process(TEST, [("LC_ALL", "C.UTF-8")]);
    }
    let steps = ["dead!", "first!", "refuse", "first!"];
    let Some((qt, ended, here, said)) = steps_in_qt_and_here(&steps, &steps) else {
        return;
    };
    assert!(ended.success(), "{ended}");
    let sent = |n: u8| {
        format!(
            "first state 1\nfirst returned 1\nfirst state 2\nfirst state 3\n\
             first state 4\nfirst load\nfirst loadend 200 \"{n}\"\n"
        )
    };
    let both = "dead state 1\ndead returned 1\ndead state 4\ndead error\ndead loadend 0 \"\"\n"
        .to_string()
        + &sent(1)
        + &sent(2)
        + "next connection 3\n";
    assert_eq!(qt, both);
    assert_eq!(here, both);
    assert_eq!(said, "");
}

/// The same difference for a synchronous request, written down in docs/qt-differences.md.
/// In Qt 6.12 `send` does not return: 3 s after the call the process is still in it, with
/// the refusal in its log, and is ended there. Here `send` returns in DONE with status 0
/// after the handlers ran, and the refusal is in the log. Neither sent anything: the
/// server's first connection is the next.
#[test]
fn a_synchronous_request_refused_its_thread_fails_here_and_never_returns_in_qt() {
    use std::os::unix::process::ExitStatusExt;
    const TEST: &str = concat!(
        module_path!(),
        "::a_synchronous_request_refused_its_thread_fails_here_and_never_returns_in_qt"
    );
    if !testenv::in_own_process(TEST) {
        return testenv::run_in_own_process(TEST, [("LC_ALL", "C.UTF-8")]);
    }
    let Some((qt, ended, here, said)) =
        steps_in_qt_and_here(&["refuse", "alarm", "sync"], &["refuse", "sync"])
    else {
        return;
    };
    assert_eq!(ended.signal(), Some(libc::SIGALRM), "{ended}");
    assert_eq!(qt, format!("{QT_REFUSED}next connection 1\n"));
    assert_eq!(
        here,
        "sync state 1\nsync state 4\nsync error\nsync loadend 0 \"\"\nsync returned 4\n\
         next connection 1\n"
    );
    let line = "XMLHttpRequest: cannot start the thread for a synchronous request:";
    assert_eq!(said.matches(line).count(), 1, "stderr: {said:?}");
}

/// The same for a synchronous request sent while a thread for requests runs (an asynchronous
/// one ran to its end before the refusal), written down in docs/qt-differences.md: a
/// synchronous request has a thread of its own in Qt 6.12 and here, so the running one does
/// not take it. Qt logs the refusal and `send` does not return: 3 s after the call the
/// process is still in it, and is ended there. Here `send` returns in DONE with status 0
/// after the handlers ran, and the refusal is in the log. Neither sent the synchronous
/// request: the server's second connection is the next.
#[test]
fn a_synchronous_request_is_refused_its_thread_while_another_runs() {
    use std::os::unix::process::ExitStatusExt;
    const TEST: &str = concat!(
        module_path!(),
        "::a_synchronous_request_is_refused_its_thread_while_another_runs"
    );
    if !testenv::in_own_process(TEST) {
        return testenv::run_in_own_process(TEST, [("LC_ALL", "C.UTF-8")]);
    }
    let Some((qt, ended, here, said)) = steps_in_qt_and_here(
        &["first!", "refuse", "alarm", "sync"],
        &["first!", "refuse", "sync"],
    ) else {
        return;
    };
    let sent = "first state 1\nfirst returned 1\nfirst state 2\nfirst state 3\n\
                first state 4\nfirst load\nfirst loadend 200 \"1\"\n";
    assert_eq!(ended.signal(), Some(libc::SIGALRM), "{ended}");
    assert_eq!(qt, format!("{sent}{QT_REFUSED}next connection 2\n"));
    assert_eq!(
        here,
        format!(
            "{sent}sync state 1\nsync state 4\nsync error\nsync loadend 0 \"\"\n\
             sync returned 4\nnext connection 2\n"
        )
    );
    let line = "XMLHttpRequest: cannot start the thread for a synchronous request:";
    assert_eq!(said.matches(line).count(), 1, "stderr: {said:?}");
}

#[test]
fn responses_match_qt() {
    let port = response_server();
    let mut src = RESPONSES_QML.replace("@BASE@", &format!("http://127.0.0.1:{port}"));
    for i in 0..36 {
        let at = format!("http://127.0.0.1:{}", login_server());
        src = src.replace(&format!("@LOGIN{i}@"), &at);
    }
    let Some((want, got)) = qt_and_ours(&src) else {
        return;
    };
    if let Ok(to) = std::env::var("XHR_EQUIV_DUMP") {
        std::fs::write(format!("{to}.qt"), &want).unwrap();
        std::fs::write(format!("{to}.ours"), &got).unwrap();
    }
    assert!(want.contains("== what a request has\n"), "Qt:\n{want}");
    same_cases(&want, &got);
}
