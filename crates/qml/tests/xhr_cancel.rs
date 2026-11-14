//! An XMLHttpRequest to a server that never answers is stopped by `abort()` and fails after
//! http::STALL on its own, instead of keeping a connection (and a pool thread) open forever.
mod event_loop;

use event_loop::Loop;
use std::io::{BufRead, BufReader, Read};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// What the silent server saw: the whole request head arrived, then the client closed.
struct Silent {
    port: u16,
    requested: mpsc::Receiver<()>,
    closed: mpsc::Receiver<()>,
}

/// Accepts one connection, reads the request and never answers; reports once the request head
/// arrived and once the client closed it.
fn silent_server() -> Silent {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let (req_tx, requested) = mpsc::channel();
    let (tx, closed) = mpsc::channel();
    std::thread::spawn(move || {
        let (s, _) = l.accept().unwrap();
        let mut r = BufReader::new(s);
        let mut line = String::new();
        while r.read_line(&mut line).is_ok_and(|n| n > 0) && line != "\r\n" {
            line.clear();
        }
        let _ = req_tx.send(());
        let mut buf = [0u8; 4096];
        while r.read(&mut buf).is_ok_and(|n| n > 0) {}
        let _ = tx.send(());
    });
    Silent {
        port,
        requested,
        closed,
    }
}

fn load(port: u16) -> (qml::runtime::Runtime, usize, Loop) {
    let src = format!(
        r#"import QtQuick
Item {{
    property string state: "sent"
    property var req: null
    function result() {{ return state }}
    function abortNow() {{ req.abort(); state = "aborted" }}
    Component.onCompleted: {{
        req = new XMLHttpRequest()
        req.onreadystatechange = () => {{
            if (req.readyState === XMLHttpRequest.DONE) state = "done " + req.status
        }}
        req.open("GET", "http://127.0.0.1:{port}/hang")
        req.send()
    }}
}}"#
    );
    let mut rt = qml::runtime::Runtime::new(None);
    let lp = Loop::new(&mut rt);
    let root = rt.load_str(&src).unwrap();
    rt.settle();
    (rt, root, lp)
}

fn state(rt: &mut qml::runtime::Runtime, root: usize) -> String {
    rt.call_method(root, "result", vec![])
        .unwrap()
        .unwrap()
        .to_js_string()
}

#[test]
fn abort_closes_the_connection() {
    let srv = silent_server();
    let (mut rt, root, mut lp) = load(srv.port);
    // `send` queues the request; the loop turn starts it.
    lp.flush(&mut rt);
    srv.requested.recv().expect("request never arrived");
    lp.flush(&mut rt);
    assert_eq!(state(&mut rt, root), "sent");
    rt.call_method(root, "abortNow", vec![]).unwrap().unwrap();
    lp.flush(&mut rt);
    srv.closed.recv().expect("server thread died");
    lp.flush(&mut rt);
    assert_eq!(state(&mut rt, root), "aborted");
}

#[test]
fn stalled_request_fails() {
    let srv = silent_server();
    let (mut rt, root, mut lp) = load(srv.port);
    let t = Instant::now();
    lp.block_until(&mut rt, |rt| state(rt, root) != "sent");
    assert_eq!(state(&mut rt, root), "done 0");
    // The production stall limit ended it, not a reset: it lasted about that long.
    let waited = t.elapsed();
    assert!(
        waited + Duration::from_secs(1) >= qml::http::STALL,
        "{waited:?}"
    );
}
