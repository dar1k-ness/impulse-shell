//! A network Image source is given up when its body passes scene::REMOTE_IMAGE_MAX or the
//! server stalls, instead of filling the cache (and memory) or hanging forever.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;
use std::io::{Read, Write};
use std::time::{Duration, Instant};

/// Whether this is `test`'s own process (see `testenv::run_in_own_process`). Elsewhere runs it
/// there, with an empty download cache of its own.
fn own_cache(test: &str) -> bool {
    if testenv::in_own_process(test) {
        return true;
    }
    let cache = tempfile::tempdir().unwrap();
    testenv::run_in_own_process(test, [("XDG_CACHE_HOME", cache.path())]);
    false
}

fn cache() -> std::path::PathBuf {
    std::env::var_os("XDG_CACHE_HOME").unwrap().into()
}

/// Serves one request with `body(stream)`.
fn serve(body: impl FnOnce(&mut std::net::TcpStream) + Send + 'static) -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (mut s, _) = l.accept().unwrap();
        let mut buf = [0u8; 4096];
        let _ = s.read(&mut buf);
        body(&mut s);
    });
    port
}

/// The Image's status once its download is no longer pending, and how long that took.
fn load_status(port: u16) -> (f64, Duration) {
    let qml = format!(
        r#"
import QtQuick
Item {{
    width: 10; height: 10
    property var img: im
    Image {{ id: im; anchors.fill: parent; asynchronous: true; source: "http://127.0.0.1:{port}/p.png" }}
}}
"#
    );
    let mut rt = Runtime::new(None);
    // The download reports on a channel that wakes the world's host service.
    let wake = testenv::Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let root = rt.load_str(&qml).unwrap();
    rt.settle();
    let Some(Value::Item(img)) = rt.world.borrow_mut().read(root, "img") else {
        panic!("no img");
    };
    let mut fonts = scene::Fonts::default();
    scene::layout_window(&mut rt, &mut fonts, root, [10.0, 10.0]);
    let t = Instant::now();
    // The readiness is taken before the loads are drained: a report after the take pings.
    loop {
        rt.world.borrow().waker.take();
        let (hit, _) = scene::take_finished_images(&mut rt.world.borrow_mut());
        if !hit.is_empty() {
            scene::relayout_targets(&mut rt.world.borrow_mut(), &mut fonts, &hit, 0);
            rt.settle();
        }
        let s = rt
            .world
            .borrow_mut()
            .read(img, "status")
            .map_or(-1.0, |v| v.to_number());
        if s != 2.0 {
            return (s, t.elapsed());
        }
        wake.wait(None, None);
    }
}

/// Bytes kept in the download cache.
fn cached_bytes() -> u64 {
    std::fs::read_dir(cache().join("impulse-shell/net")).map_or(0, |d| {
        d.flatten()
            .map(|e| e.metadata().map_or(0, |m| m.len()))
            .sum()
    })
}

#[test]
fn oversized_body_is_not_kept() {
    if !own_cache(concat!(module_path!(), "::oversized_body_is_not_kept")) {
        return;
    }
    let port = serve(|s| {
        let _ =
            s.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nConnection: close\r\n\r\n");
        let chunk = vec![0u8; 1 << 20];
        for _ in 0..(scene::REMOTE_IMAGE_MAX >> 20) + 8 {
            if s.write_all(&chunk).is_err() {
                return;
            }
        }
    });
    let before = cached_bytes();
    let (status, _) = load_status(port);
    assert_eq!(status, 3.0, "Image.Error");
    assert_eq!(cached_bytes(), before, "nothing cached");
}

#[test]
fn stalled_server_fails_the_image() {
    if !own_cache(concat!(module_path!(), "::stalled_server_fails_the_image")) {
        return;
    }
    let port = serve(|s| {
        let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n");
        // Sends nothing more and holds the connection until the client gives up on it.
        let _ = s.read(&mut [0u8; 1]);
    });
    let (status, took) = load_status(port);
    assert_eq!(status, 3.0, "Image.Error");
    assert!(
        took >= qml::http::STALL - Duration::from_secs(1),
        "{took:?}"
    );
}
