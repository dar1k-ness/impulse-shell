//! An item with only pixmaps (no icon names) shows its attention pixmap while it needs
//! attention and its normal one otherwise: the tray icon's source changes with the pixmap it
//! shows, as the scene caches a decoded source by URL.

mod sni_watcher;

use qml::runtime::Runtime;
use sni_watcher::*;
use std::sync::{Arc, Mutex};
use zbus::object_server::SignalEmitter;

const NORMAL: [u8; 4] = [255, 255, 0, 0];
const ATTENTION: [u8; 4] = [255, 0, 0, 255];

struct Pixmaps(Arc<Mutex<&'static str>>);

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Pixmaps {
    #[zbus(property)]
    fn id(&self) -> String {
        "pix".into()
    }
    #[zbus(property)]
    fn status(&self) -> String {
        self.0.lock().unwrap().to_string()
    }
    #[zbus(property)]
    fn icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> {
        vec![(1, 1, NORMAL.to_vec())]
    }
    #[zbus(property)]
    fn attention_icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> {
        vec![(1, 1, ATTENTION.to_vec())]
    }
    #[zbus(signal)]
    async fn new_status(e: &SignalEmitter<'_>, status: &str) -> zbus::Result<()>;
}

const QML: &str = "import QtQuick\nimport Rustshell.Services.SystemTray\nItem {\n\
    readonly property var item: SystemTray.items.values.length ? SystemTray.items.values[0] : null\n\
    property string icon: item ? item.icon : ''\n property int status: item ? item.status : -1 }";

/// The premultiplied RGBA pixel the tray shows for `src`.
fn shown(src: &str) -> [u8; 4] {
    let (w, h, px) = qml::sni::pixmap(src, 1, 1).expect("an sni pixmap");
    assert_eq!((w, h), (1, 1));
    px[0]
}

#[test]
fn the_attention_pixmap_shows_while_the_item_needs_attention() {
    let Some(_bus) = bus(concat!(
        module_path!(),
        "::the_attention_pixmap_shows_while_the_item_needs_attention"
    )) else {
        return;
    };
    let mut rt = Runtime::new(None);
    let lp = Loop::new(&mut rt);
    let owners = watcher_owners(lp.ping());
    let root = rt.load_str(QML).unwrap();
    lp.recv_until(&mut rt, &owners, "the tray serves the Watcher", |o| {
        !o.is_empty()
    });
    let status = Arc::new(Mutex::new("Active"));
    let app = zbus::blocking::connection::Builder::session()
        .unwrap()
        .serve_at("/StatusNotifierItem", Pixmaps(status.clone()))
        .unwrap()
        .build()
        .unwrap();
    let me = app.unique_name().unwrap().to_string();
    app.call_method(
        Some(WATCHER),
        "/StatusNotifierWatcher",
        Some(WATCHER),
        "RegisterStatusNotifierItem",
        &(me.as_str(),),
    )
    .unwrap();
    let icon = |rt: &Runtime| rt.get(root, "icon").to_js_string();
    let status_of = |rt: &Runtime| rt.get(root, "status").to_number();
    lp.until(&mut rt, "the item", |rt| status_of(rt) == 1.0);
    let normal = icon(&rt);
    assert!(normal.starts_with("image://service.sni/"), "{normal}");
    // Opaque pixels premultiply to themselves.
    assert_eq!(shown(&normal), [255, 0, 0, 255], "ARGB {NORMAL:?} as RGBA");

    let set = |rt: &mut Runtime, s: &'static str, want: f64| {
        *status.lock().unwrap() = s;
        app.emit_signal(
            None::<&str>,
            "/StatusNotifierItem",
            "org.kde.StatusNotifierItem",
            "NewStatus",
            &(s,),
        )
        .unwrap();
        lp.until(rt, s, |rt| status_of(rt) == want);
        icon(rt)
    };
    let attention = set(&mut rt, "NeedsAttention", 2.0);
    assert_ne!(
        attention, normal,
        "the attention pixmap kept the normal one's source"
    );
    assert_eq!(shown(&attention), [0, 0, 255, 255]);
    let back = set(&mut rt, "Active", 1.0);
    assert_ne!(
        back, attention,
        "the normal pixmap kept the attention one's source"
    );
    assert_eq!(shown(&back), [255, 0, 0, 255]);
    let errs = rt.errors();
    assert!(errs.is_empty(), "{errs:?}");
}
