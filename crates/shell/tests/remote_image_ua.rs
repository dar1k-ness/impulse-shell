//! A network Image source is fetched with Qt's default User-Agent, like QQuickImage.

use qml::runtime::Runtime;
use shell::scene;
use std::io::{Read, Write};

#[test]
fn remote_image_sends_qt_user_agent() {
    const TEST: &str = concat!(module_path!(), "::remote_image_sends_qt_user_agent");
    if !testenv::in_own_process(TEST) {
        let cache = tempfile::tempdir().unwrap();
        testenv::run_in_own_process(TEST, [("XDG_CACHE_HOME", cache.path())]);
        return;
    }
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut s = l.incoming().next().unwrap().unwrap();
        let mut buf = [0u8; 4096];
        let n = s.read(&mut buf).unwrap();
        let req = String::from_utf8_lossy(&buf[..n]).into_owned();
        let _ = s
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let _ = tx.send(req);
    });
    let qml = format!(
        r#"
import QtQuick
Item {{
    width: 10; height: 10
    Image {{ anchors.fill: parent; asynchronous: true; source: "http://127.0.0.1:{port}/p.png" }}
}}
"#
    );
    let mut rt = Runtime::new(None);
    let root = rt.load_str(&qml).unwrap();
    rt.settle();
    scene::layout_window(&mut rt, &mut scene::Fonts::default(), root, [10.0, 10.0]);
    let req = rx.recv().expect("no request");
    let ua = req
        .lines()
        .find_map(|l| {
            l.split_once(':')
                .filter(|(k, _)| k.eq_ignore_ascii_case("user-agent"))
                .map(|(_, v)| v.trim().to_string())
        })
        .unwrap_or_default();
    assert_eq!(ua, "Mozilla/5.0");
}
