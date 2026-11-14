//! QML `XMLHttpRequest` sends Qt's default `User-Agent: Mozilla/5.0` (recorded from `qs`,
//! Qt 6) unless the QML sets one.
mod event_loop;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;

/// Answers every request with `200` and its User-Agent header (or `-`) as the body.
fn server() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            std::thread::spawn(move || {
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut ua = "-".to_string();
                loop {
                    let mut h = String::new();
                    if r.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                        break;
                    }
                    if let Some((k, v)) = h.trim_end().split_once(": ")
                        && k.eq_ignore_ascii_case("user-agent")
                    {
                        ua = v.to_string();
                    }
                }
                let mut s = s;
                let _ = write!(
                    s,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{ua}",
                    ua.len()
                );
            });
        }
    });
    port
}

#[test]
fn default_user_agent_matches_qt() {
    let port = server();
    let src = format!(
        r#"import QtQuick
Item {{
    property var got: []
    function result() {{ return got.length === 2 ? got.join("|") : "" }}
    function get(ua) {{
        const x = new XMLHttpRequest()
        x.onreadystatechange = () => {{
            if (x.readyState === XMLHttpRequest.DONE) got = [...got, (ua ?? "") + "=" + x.responseText]
        }}
        x.open("GET", "http://127.0.0.1:{port}/")
        if (ua) x.setRequestHeader("User-Agent", ua)
        x.send()
    }}
    Component.onCompleted: {{ get(null); get("MyUA/1") }}
}}"#
    );
    let mut rt = qml::runtime::Runtime::new(None);
    let mut lp = event_loop::Loop::new(&mut rt);
    let root = rt.load_str(&src).unwrap();
    rt.settle();
    let result = |rt: &mut qml::runtime::Runtime| {
        rt.call_method(root, "result", vec![])
            .unwrap()
            .unwrap()
            .to_js_string()
    };
    lp.block_until(&mut rt, |rt| !result(rt).is_empty());
    let got = result(&mut rt);
    let mut v: Vec<&str> = got.split('|').collect();
    v.sort();
    assert_eq!(v, ["=Mozilla/5.0", "MyUA/1=MyUA/1"], "{:?}", rt.errors());
}
