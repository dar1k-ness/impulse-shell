//! Second builtins audit, slice B3: XMLHttpRequest against a local server. The document
//! `audit_b3/xhr.qml` logs one line per case, and `audit_b3/xhr.qt` holds what Qt 6.12
//! (tools/testenv) logged for the same document against the same server on port 18765.
mod event_loop;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;

/// One response: the status line's code and reason, the Content-Type, further header lines
/// and the body.
fn reply(method: &str, status: &str, ctype: &str, extra: &str, body: &[u8]) -> Vec<u8> {
    let mut head = format!("HTTP/1.1 {status}\r\n");
    if !ctype.is_empty() {
        head += &format!("Content-Type: {ctype}\r\n");
    }
    head += &format!(
        "Content-Length: {}\r\n{extra}Connection: close\r\n\r\n",
        body.len()
    );
    let mut out = head.into_bytes();
    if method != "HEAD" {
        out.extend_from_slice(body);
    }
    out
}

/// The server the fixture talks to: `/ok` answers with the request it got, the other paths
/// with a fixed response each.
fn server() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            std::thread::spawn(move || {
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut line = String::new();
                if r.read_line(&mut line).unwrap_or(0) == 0 {
                    return;
                }
                let mut parts = line.trim_end().split(' ');
                let method = parts.next().unwrap_or("").to_string();
                let target = parts.next().unwrap_or("").to_string();
                let mut headers = std::collections::BTreeMap::<String, String>::new();
                loop {
                    let mut h = String::new();
                    if r.read_line(&mut h).unwrap_or(0) == 0 || h.trim_end().is_empty() {
                        break;
                    }
                    let (k, v) = h.trim_end().split_once(':').unwrap_or((&h, ""));
                    let (k, v) = (k.to_ascii_lowercase(), v.trim());
                    match headers.get_mut(&k) {
                        Some(old) => *old += &format!(" ;; {v}"),
                        None => {
                            headers.insert(k, v.to_string());
                        }
                    }
                }
                let n = headers
                    .get("content-length")
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(0);
                let mut body = vec![0; n];
                r.read_exact(&mut body).unwrap();
                let xml = b"<root a='1'><kid>t</kid></root>";
                let all: Vec<u8> = (0..=255).collect();
                let out = match target.split('?').next().unwrap_or("") {
                    "/ok" => {
                        // What the client's transport adds on its own is not shown.
                        let skip = [
                            "accept",
                            "accept-encoding",
                            "accept-language",
                            "connection",
                            "host",
                            "cookie",
                        ];
                        let shown: String = headers
                            .iter()
                            .filter(|(k, _)| !skip.contains(&k.as_str()))
                            .map(|(k, v)| format!(" [{k}: {v}]"))
                            .collect();
                        let mut out = format!("{method} {target}{shown} body=").into_bytes();
                        out.extend_from_slice(&body);
                        let extra = "X-Custom: a\r\nX-Custom: b\r\nX-Other:   spaced  \r\n\
                                     Set-Cookie: k=v\r\nx-lower: L\r\n";
                        reply(&method, "200 Fine", "text/plain", extra, &out)
                    }
                    "/json" => reply(
                        &method,
                        "200 OK",
                        "application/json",
                        "",
                        br#"{"a": [1, 2], "b": "x"}"#,
                    ),
                    "/no" => reply(&method, "404 Nope", "text/plain", "", b"missing"),
                    "/no0" => reply(&method, "404 Nope", "text/plain", "", b""),
                    "/redir" => reply(
                        &method,
                        "302 Moved",
                        "",
                        "Location: /ok?from=redir\r\n",
                        b"",
                    ),
                    "/xml" => reply(&method, "200 OK", "text/xml", "", xml),
                    "/xmlastext" => reply(&method, "200 OK", "text/plain", "", xml),
                    "/xmlbad" => reply(&method, "200 OK", "text/xml", "", b"<root><kid></root>"),
                    "/bin" => reply(&method, "200 OK", "application/octet-stream", "", &all),
                    "/badjson" => reply(&method, "200 OK", "application/json", "", b"{nope"),
                    "/latin1" => reply(
                        &method,
                        "200 OK",
                        "text/plain; charset=iso-8859-1",
                        "",
                        b"caf\xe9!",
                    ),
                    "/empty" => b"HTTP/1.1 204 \r\nConnection: close\r\n\r\n".to_vec(),
                    "/noreason" => {
                        b"HTTP/1.1 200\r\nContent-Length: 2\r\nConnection: close\r\n\r\nhi".to_vec()
                    }
                    _ => reply(&method, "500 Internal Thing", "text/plain", "", b"err"),
                };
                let mut s = s;
                let _ = s.write_all(&out);
            });
        }
    });
    port
}

#[test]
fn xhr_matches_qt() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/audit_b3");
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).unwrap();
    // A case's line is `name| answer`; what Qt reports besides them (handler errors) is not
    // compared.
    let is_case = |l: &&str| {
        l.split_once("| ")
            .is_some_and(|(name, _)| name.chars().all(|c| c.is_ascii_alphanumeric()))
    };
    let qt = read("xhr.qt");
    let mut want: Vec<&str> = qt.lines().filter(is_case).collect();
    want.sort_unstable();
    let port = server();
    let recorded = "127.0.0.1:18765";
    let src = read("xhr.qml").replace(recorded, &format!("127.0.0.1:{port}"));
    // The engine keeps no console lines: the test takes them as they are printed.
    thread_local! {
        static PRINTED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    let _ = qml::builtins::CONSOLE_SINK.set(|_, line| {
        PRINTED.with_borrow_mut(|p| p.push(line.to_string()));
    });
    let mut rt = qml::runtime::Runtime::new(None);
    let mut lp = event_loop::Loop::new(&mut rt);
    rt.load_str(&src).unwrap();
    rt.settle();
    lp.block_until(&mut rt, |_| PRINTED.with_borrow(Vec::len) >= want.len());
    let got: Vec<String> = PRINTED
        .take()
        .iter()
        .map(|l| l.replace(&format!("127.0.0.1:{port}"), recorded))
        .collect();
    let mut got: Vec<&str> = got.iter().map(String::as_str).collect();
    got.sort_unstable();
    let differ: Vec<String> = want
        .iter()
        .filter(|w| !got.contains(w))
        .map(|w| {
            let name = w.split('|').next().unwrap_or("");
            let ours: Vec<&&str> = got
                .iter()
                .filter(|g| g.split('|').next() == Some(name))
                .collect();
            format!("qt:   {w}\nours: {ours:?}")
        })
        .collect();
    assert!(
        differ.is_empty(),
        "{} of {} cases differ from Qt 6.12:\n{}",
        differ.len(),
        want.len(),
        differ.join("\n")
    );
    assert_eq!(got.len(), want.len(), "{got:#?}");
}
