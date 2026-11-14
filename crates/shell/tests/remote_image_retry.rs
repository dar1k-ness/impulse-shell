//! A network Image source that failed stays Error, as in QQuickImage, and no clock fetches it
//! again: the next fetch is asked for by an event, the Image's `source` being assigned
//! (`source = ""; source = url`, how QML retries) or another Image asking for the same source.

use qml::runtime::Runtime;
use qml::value::Value;
use shell::scene;
use std::io::{Read, Write};
use std::sync::mpsc;

/// Answers every request with 404 and reports each one.
fn serve() -> (u16, mpsc::Receiver<()>) {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let mut s = s.unwrap();
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let _ = tx.send(());
            let _ = s.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    });
    (port, rx)
}

struct Scene {
    rt: Runtime,
    fonts: scene::Fonts,
    wake: testenv::Wakeups,
    root: usize,
}

impl Scene {
    fn new(qml: &str) -> Scene {
        let mut rt = Runtime::new(None);
        // A finished download reports on a channel that wakes the world's host service.
        let wake = testenv::Wakeups::new();
        rt.world.borrow().waker.set_ping(wake.ping());
        let root = rt.load_str(qml).unwrap();
        rt.settle();
        let mut s = Scene {
            rt,
            fonts: scene::Fonts::default(),
            wake,
            root,
        };
        s.layout();
        s
    }

    fn layout(&mut self) {
        scene::layout_window(&mut self.rt, &mut self.fonts, self.root, [10.0, 10.0]);
        self.rt.settle();
    }

    fn item(&self, name: &str) -> usize {
        match self.rt.world.borrow_mut().read(self.root, name) {
            Some(Value::Item(i)) => i,
            v => panic!("{name}: {v:?}"),
        }
    }

    fn status(&self, img: usize) -> f64 {
        self.rt.get(img, "status").to_number()
    }

    /// Lays out until `img` is no longer Loading, sleeping on the download's report.
    fn settled(&mut self, img: usize) -> f64 {
        loop {
            self.rt.world.borrow().waker.take();
            let (hit, all) = scene::take_finished_images(&mut self.rt.world.borrow_mut());
            if all || !hit.is_empty() {
                self.layout();
            }
            let s = self.status(img);
            if s != 2.0 {
                return s;
            }
            self.wake.wait(None, None);
        }
    }
}

fn own_cache(test: &str) -> bool {
    if testenv::in_own_process(test) {
        return true;
    }
    let cache = tempfile::tempdir().unwrap();
    testenv::run_in_own_process(test, [("XDG_CACHE_HOME", cache.path())]);
    false
}

#[test]
fn a_failed_source_is_fetched_again_only_when_asked_anew() {
    if !own_cache(concat!(
        module_path!(),
        "::a_failed_source_is_fetched_again_only_when_asked_anew"
    )) {
        return;
    }
    let (port, requests) = serve();
    let url = format!("http://127.0.0.1:{port}/p.png");
    let mut s = Scene::new(&format!(
        r#"
import QtQuick
Item {{
    id: root
    width: 10; height: 10
    property var img: im
    property var other: null
    property bool more: false
    function again() {{ im.source = ""; im.source = "{url}" }}
    Image {{ id: im; anchors.fill: parent; asynchronous: true; source: "{url}" }}
    Loader {{
        active: root.more
        sourceComponent: Image {{
            asynchronous: true
            source: "{url}"
            Component.onCompleted: root.other = this
        }}
    }}
}}
"#
    ));
    let img = s.item("img");
    assert_eq!(s.settled(img), 3.0, "Image.Error");
    requests.recv().unwrap();

    // Laid out again and again, the failed source is left alone.
    for _ in 0..3 {
        s.rt.world.borrow_mut().layout_rev += 1;
        s.layout();
        assert_eq!(s.status(img), 3.0, "still Image.Error");
    }
    assert!(requests.try_recv().is_err(), "fetched again unasked");

    // The source is assigned again: one more fetch.
    s.rt.call_method(s.root, "again", vec![]).unwrap().unwrap();
    s.rt.settle();
    s.layout();
    assert_eq!(s.status(img), 2.0, "Image.Loading");
    requests.recv().unwrap();
    assert_eq!(s.settled(img), 3.0, "Image.Error");
    s.layout();
    assert!(requests.try_recv().is_err(), "fetched again unasked");

    // Another Image asks for the same source: one more fetch.
    s.rt.set(s.root, "more", Value::Bool(true));
    s.rt.settle();
    s.layout();
    let other = s.item("other");
    requests.recv().unwrap();
    assert_eq!(s.settled(other), 3.0, "Image.Error");
    s.layout();
    assert!(requests.try_recv().is_err(), "fetched again unasked");
}
