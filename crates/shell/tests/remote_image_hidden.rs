//! A remote Image hidden until it is Ready (StyledImage: `visible: opacity > 0`, `opacity:
//! status === Image.Ready ? 1 : 0`) turns Ready once its download lands, like QQuickImage.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;
use std::io::{Read, Write};

const PNG: [u8; 74] = [
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x08, 0x02, 0x00, 0x00, 0x00, 0xfd, 0xd4, 0x9a,
    0x73, 0x00, 0x00, 0x00, 0x11, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
    0x44, 0x0c, 0x60, 0xf2, 0x3f, 0x00, 0x1b, 0xf2, 0x03, 0xfd, 0xf3, 0xe7, 0x4b, 0xb1, 0x00, 0x00,
    0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

fn serve() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let mut s = s.unwrap();
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                PNG.len()
            );
            let _ = s.write_all(head.as_bytes());
            let _ = s.write_all(&PNG);
        }
    });
    port
}

#[test]
fn hidden_remote_image_becomes_ready() {
    const TEST: &str = concat!(module_path!(), "::hidden_remote_image_becomes_ready");
    if !testenv::in_own_process(TEST) {
        let cache = tempfile::tempdir().unwrap();
        testenv::run_in_own_process(TEST, [("XDG_CACHE_HOME", cache.path())]);
        return;
    }
    let port = serve();
    let qml = format!(
        r#"
import QtQuick
Item {{
    width: 300; height: 200
    property var img: im
    Column {{
        Item {{ width: 300; height: 150; Image {{
            asynchronous: true
            anchors.fill: parent
            visible: opacity > 0
            opacity: status === Image.Ready ? 1 : 0
            source: "http://127.0.0.1:{port}/a.png"
        }} }}
        Item {{ width: 300; height: 150; Image {{
            id: im
            anchors.fill: parent
            asynchronous: true
            visible: opacity > 0
            opacity: status === Image.Ready ? 1 : 0
            source: "http://127.0.0.1:{port}/p.png"
        }} }}
    }}
}}
"#
    );
    let mut rt = Runtime::new(None);
    // The image loads report on a channel that wakes the world's host service.
    let wake = testenv::Wakeups::new();
    rt.world.borrow().waker.set_ping(wake.ping());
    let root = rt.load_str(&qml).unwrap();
    rt.settle();
    let Some(Value::Item(img)) = rt.world.borrow_mut().read(root, "img") else {
        panic!("no img");
    };
    let mut fonts = scene::Fonts::default();
    let status = |rt: &mut Runtime| {
        rt.world
            .borrow_mut()
            .read(img, "status")
            .map_or(-1.0, |v| v.to_number())
    };
    // The shell lays a window out in full once; a later frame at the same size only relayouts
    // the items take_finished_images reports (layout_rev alone keeps the window as laid).
    scene::layout_window(&mut rt, &mut fonts, root, [300.0, 200.0]);
    rt.settle();
    // The readiness is taken before the loads are drained: a report after the take pings.
    loop {
        rt.world.borrow().waker.take();
        let (hit, _) = scene::take_finished_images(&mut rt.world.borrow_mut());
        if !hit.is_empty() {
            scene::relayout_targets(&mut rt.world.borrow_mut(), &mut fonts, &hit, 0);
            rt.settle();
        }
        if status(&mut rt) == 1.0 {
            break;
        }
        wake.wait(None, None);
    }
    assert_eq!(status(&mut rt), 1.0, "Image.Ready");
}
