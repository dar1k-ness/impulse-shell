//! Pictures that arrive over D-Bus are premultiplied and painted on the processor, and the
//! tray and a notification show them as Quickshell itself does. Quickshell (the image's, in
//! a window of a headless sway, through OpenGL) and we are given the same three pictures of
//! 256x256 on one private session bus, each of every pair of bytes once:
//!
//! - a tray item's IconPixmap of every grey at every alpha (`QPixmap::fromImage` premultiplies
//!   it with `qPremultiply`);
//! - a tray item's opaque IconPixmap of every grey under an OverlayIconPixmap of black at
//!   every alpha (QPainter's source-over keeps `grey * (255 - alpha) / 255` of what is under);
//! - a notification's `image-data` of every grey at every alpha (premultiplied when Qt Quick
//!   makes a texture of the QImage).
//!
//! Over black each shows the product, which in Quickshell is Qt's `qt_div_255`
//! (`mul255::qt`) in all 65536 pairs, one less than the nearest byte in 24 of them.

#[path = "../../qml/tests/sni_watcher/mod.rs"]
mod sni_watcher;

use qsg::raster::Canvas;
use shell::scene::{self, Target};
use sni_watcher::{Loop, WATCHER};
use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use zbus::blocking::Connection;

const NOTIFICATIONS: &str = "org.freedesktop.Notifications";
const TEST: &str = concat!(module_path!(), "::every_pair_of_bytes_of_a_dbus_picture");

/// `@GRAB@` is what saves the three pictures once they are all shown.
const SOURCE: &str = "import QtQuick
import Quickshell
import Quickshell.Services.SystemTray
import Quickshell.Services.Notifications
FloatingWindow {
    id: win
    implicitWidth: 768; implicitHeight: 256
    color: \"black\"
    property string picture: \"\"
    property string sizes: \"\"
    NotificationServer {
        imageSupported: true
        onNotification: n => { n.tracked = true; win.picture = n.image; }
    }
    Item {
        id: shown
        width: 768; height: 256
        Rectangle { anchors.fill: parent; color: \"black\" }
        Repeater {
            id: tray
            model: SystemTray.items
            Image {
                required property var modelData
                x: modelData.id === \"over\" ? 256 : 0
                sourceSize: Qt.size(256, 256)
                source: modelData.icon
                onStatusChanged: win.look()
            }
        }
        Image { id: sent; x: 512; source: win.picture; onStatusChanged: win.look() }
    }
    property bool saved: false
    function look() {
@GRAB@
    }
}
";

const GRAB: &str = "        if (saved || tray.count != 2 || sent.status != Image.Ready)
            return;
        for (let i = 0; i < 2; i++)
            if (!tray.itemAt(i) || tray.itemAt(i).status != Image.Ready)
                return;
        saved = true;
        shown.grabToImage(r => console.log(\"SAVED\", r.saveToFile(Quickshell.env(\"SHOT\"))));";

/// A tray item with pixmaps only.
struct Pictures {
    id: &'static str,
    /// The pixmaps are squares of this side.
    side: i32,
    icon: Vec<u8>,
    overlay: Option<Vec<u8>>,
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Pictures {
    #[zbus(property)]
    fn id(&self) -> String {
        self.id.into()
    }
    #[zbus(property)]
    fn status(&self) -> String {
        "Active".into()
    }
    #[zbus(property)]
    fn icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> {
        vec![(self.side, self.side, self.icon.clone())]
    }
    #[zbus(property)]
    fn overlay_icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> {
        let overlay = self.overlay.iter();
        overlay
            .map(|px| (self.side, self.side, px.clone()))
            .collect()
    }
}

/// 256 rows of 256 pixels, `pixel(row, column)` each.
fn picture(pixel: impl Fn(u8, u8) -> [u8; 4]) -> Vec<u8> {
    let at = (0..256 * 256).map(|i| pixel((i / 256) as u8, (i % 256) as u8));
    at.flatten().collect()
}

/// A tray item that gives no icon: no name and no pixmap (Qt's own `SystemTrayIcon` whose icon
/// its theme does not have).
struct NoIcon;

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl NoIcon {
    #[zbus(property)]
    fn id(&self) -> String {
        "bare".into()
    }
    #[zbus(property)]
    fn status(&self) -> String {
        "Active".into()
    }
    #[zbus(property)]
    fn icon_name(&self) -> String {
        String::new()
    }
    #[zbus(property)]
    fn icon_pixmap(&self) -> Vec<(i32, i32, Vec<u8>)> {
        Vec::new()
    }
}

/// `items` registered with whoever is the Watcher now; the connections serve them while they
/// live.
fn serve<T: zbus::object_server::Interface>(items: Vec<T>) -> Vec<Connection> {
    let mut held = Vec::new();
    for item in items {
        let c = zbus::blocking::connection::Builder::session()
            .unwrap()
            .serve_at("/StatusNotifierItem", item)
            .unwrap()
            .build()
            .unwrap();
        let me = c.unique_name().unwrap().to_string();
        let path = "/StatusNotifierWatcher";
        c.call_method(
            Some(WATCHER),
            path,
            Some(WATCHER),
            "RegisterStatusNotifierItem",
            &(me.as_str(),),
        )
        .unwrap();
        held.push(c);
    }
    held
}

/// The two tray items of every pair of bytes, and the notification sent to whoever serves
/// notifications now.
fn send_pairs() -> Vec<Connection> {
    // A pixmap is ARGB in network byte order.
    let held = serve(vec![
        Pictures {
            id: "plain",
            side: 256,
            icon: picture(|grey, alpha| [alpha, grey, grey, grey]),
            overlay: None,
        },
        Pictures {
            id: "over",
            side: 256,
            icon: picture(|grey, _| [255, grey, grey, grey]),
            overlay: Some(picture(|_, kept| [255 - kept, 0, 0, 0])),
        },
    ]);
    notify(
        &held[0],
        256,
        picture(|grey, alpha| [grey, grey, grey, alpha]),
    );
    held
}

/// A notification with `data`, RGBA rows of a square of `side`, for its `image-data`, sent
/// over `c` to whoever serves notifications now.
fn notify(c: &Connection, side: i32, data: Vec<u8>) {
    let mut hints: HashMap<&str, zbus::zvariant::Value> = HashMap::new();
    hints.insert(
        "image-data",
        (side, side, side * 4, true, 8, 4, data).into(),
    );
    let none: Vec<&str> = Vec::new();
    let notify = zbus::Message::method_call("/org/freedesktop/Notifications", "Notify")
        .unwrap()
        .destination(NOTIFICATIONS)
        .unwrap()
        .interface(NOTIFICATIONS)
        .unwrap()
        .build(&("pairs", 0u32, "", "a picture", "", none, hints, -1))
        .unwrap();
    // Not waited for: our server answers from the loop of the test's own thread.
    c.send(&notify).unwrap();
}

/// Each change of who owns the Watcher's or the notification server's name, as (name, has an
/// owner), from a match rule added before this returns; `ping` is called after each.
fn owners(ping: Arc<dyn Fn() + Send + Sync>) -> Receiver<(String, bool)> {
    let c = Connection::session().unwrap();
    let dbus = zbus::blocking::fdo::DBusProxy::new(&c).unwrap();
    let changes = dbus.receive_name_owner_changed().unwrap();
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let _c = c;
        for change in changes {
            let Ok(a) = change.args() else { continue };
            let name = a.name().to_string();
            if name != WATCHER && name != NOTIFICATIONS {
                continue;
            }
            if tx.send((name, a.new_owner().is_some())).is_err() {
                return;
            }
            ping();
        }
    });
    rx
}

/// The names `source` serves on the bus: the Watcher's, and the notification server's.
fn served_by(source: &str) -> &'static [&'static str] {
    if source.contains("NotificationServer") {
        &[WATCHER, NOTIFICATIONS]
    } else {
        &[WATCHER]
    }
}

/// Notes each change `rx` has; whether all `names` have an owner (`served`) or none has.
fn all(
    names: &[&str],
    rx: &Receiver<(String, bool)>,
    owned: &mut HashMap<String, bool>,
    served: bool,
) -> bool {
    owned.extend(rx.try_iter());
    let has = |name: &&str| owned.get(*name).copied().unwrap_or(false) == served;
    names.iter().all(has)
}

struct Kill(std::process::Child);

impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// What Quickshell shows of `source` (which saves the picture and logs `SAVED`) once it has
/// what `send` sends, and what it printed; the bus is left without its names.
fn quickshell(
    dir: &std::path::Path,
    source: &str,
    send: fn() -> Vec<Connection>,
) -> (Canvas, String) {
    let shot = dir.join("quickshell.png");
    std::fs::create_dir(dir.join("config")).unwrap();
    std::fs::write(dir.join("config/shell.qml"), source).unwrap();
    let woken = Arc::new((Mutex::new(0u64), std::sync::Condvar::new()));
    let wake = woken.clone();
    let names = owners(Arc::new(move || {
        *wake.0.lock().unwrap() += 1;
        wake.1.notify_all();
    }));
    let mut owned = HashMap::new();
    // Sleeps until the next change of an owner, then asks `done` again.
    let mut wait = |done: &mut dyn FnMut(&mut HashMap<String, bool>) -> bool| {
        let mut seen = woken.0.lock().unwrap();
        while !done(&mut owned) {
            let n = *seen;
            while *seen == n {
                seen = woken.1.wait(seen).unwrap();
            }
        }
    };

    let sway = testenv::Sway::start();
    let (rd, wr) = std::io::pipe().unwrap();
    let mut cmd = Command::new("quickshell");
    cmd.arg("-p")
        .arg(dir.join("config"))
        .env("SHOT", &shot)
        .env("QT_QPA_PLATFORM", "wayland")
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .env("QT_SCALE_FACTOR", "1")
        .env("QSG_INFO", "1")
        .stdin(Stdio::null())
        .stdout(wr.try_clone().unwrap())
        .stderr(wr);
    let child = Kill(sway.env(&mut cmd).spawn().unwrap());
    drop(cmd);
    let log = testenv::Log::capture(rd);
    let serves = served_by(source);
    wait(&mut |owned| all(serves, &names, owned, true));
    let held = send();
    log.wait(|l| l.contains("SAVED"))
        .unwrap_or_else(|l| panic!("Quickshell saved no picture:\n{l}"));
    let text = log.text();
    assert!(text.contains("SAVED true"), "Quickshell printed:\n{text}");
    assert!(text.contains("llvmpipe"), "Quickshell drew with:\n{text}");
    drop(held);
    drop(child);
    wait(&mut |owned| all(serves, &names, owned, false));
    (qsg::image::load_png(shot.to_str().unwrap()).unwrap(), text)
}

/// What we show of `source` in a window of `w` x `h` once we have what `send` sends and
/// `painted` holds (asked after each frame, with the root's `sizes`); and the root's `picture`
/// and `sizes`.
fn ours(
    source: &str,
    send: fn() -> Vec<Connection>,
    (w, h): (usize, usize),
    painted: impl Fn(&Canvas, &str) -> bool,
) -> (Canvas, String, String) {
    let mut rt = qml::runtime::Runtime::new(None);
    // The hook the shell gives a world (`main.rs`): an Image loads as it completes.
    let fonts = std::rc::Rc::new(std::cell::RefCell::new(scene::Fonts::default()));
    scene::image_hook(&mut rt.world.borrow_mut(), &fonts);
    let lp = Loop::new(&mut rt);
    let names = owners(lp.ping());
    let root = rt.load_str(source).unwrap();
    let mut owned = HashMap::new();
    lp.until(&mut rt, "our tray and notification server", |_| {
        all(served_by(source), &names, &mut owned, true)
    });
    let _held = send();
    let mut c = Canvas::new(w, h, [0, 0, 0, 0]);
    lp.until(&mut rt, "the pictures painted", |rt| {
        scene::layout_window(rt, &mut fonts.borrow_mut(), root, [w as f64, h as f64]);
        rt.settle();
        scene::paint_window(rt, &mut fonts.borrow_mut(), root, &mut Target::Cpu(&mut c));
        painted(&c, &rt.get(root, "sizes").to_js_string())
    });
    let errors = rt.errors();
    assert!(errors.is_empty(), "{errors:?}");
    let read = |name: &str| rt.get(root, name).to_js_string();
    (c, read("picture"), read("sizes"))
}

/// How many of the 65536 pixels of picture `k` are not the grey `value(row, column)`.
fn unlike(c: &Canvas, k: usize, value: fn(u32, u32) -> u32) -> usize {
    (0..256 * 256)
        .filter(|i| {
            let v = value((i / 256) as u32, (i % 256) as u32) as u8;
            c.px[(i / 256) * 768 + k * 256 + i % 256] != [v, v, v, 255]
        })
        .count()
}

#[test]
fn every_pair_of_bytes_of_a_dbus_picture() {
    if !testenv::in_own_process(TEST) {
        if !testenv::require_all(&["quickshell", "sway", "dbus-daemon"]) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let bus = format!("unix:path={}", dir.path().join("bus").display());
        let cache = dir.path().join("cache");
        let vars = [
            ("DBUS_SESSION_BUS_ADDRESS", bus.as_str()),
            ("XDG_CACHE_HOME", cache.to_str().unwrap()),
        ];
        testenv::run_in_own_process(TEST, vars);
        return;
    }
    let Some(_bus) = sni_watcher::bus(TEST) else {
        panic!("no session bus");
    };
    let dir = tempfile::tempdir().unwrap();
    let (qt, _) = quickshell(dir.path(), &SOURCE.replace("@GRAB@", GRAB), send_pairs);
    assert_eq!((qt.w, qt.h), (768, 256));
    let pictures = ["a tray pixmap", "a tray overlay", "a notification"];
    for (k, what) in pictures.iter().enumerate() {
        assert_eq!(unlike(&qt, k, mul255::qt), 0, "Quickshell, {what}");
        assert_eq!(unlike(&qt, k, mul255::nearest), 24, "Quickshell, {what}");
        assert_eq!(qt.px[253 * 768 + k * 256 + 191], [189, 189, 189, 255]);
    }

    // The last pixel of each picture is white (255 * 255 / 255) once it is painted.
    let (ours, source, _) = ours(
        &SOURCE.replace("@GRAB@", ""),
        send_pairs,
        (768, 256),
        |c, _| (0..3).all(|k| c.px[255 * 768 + k * 256 + 255] == [255; 4]),
    );
    for (k, what) in pictures.iter().enumerate() {
        let at = |i: usize| (i / 256) * 768 + k * 256 + i % 256;
        let differ: Vec<_> = (0..256 * 256)
            .filter(|i| ours.px[at(*i)] != qt.px[at(*i)])
            .map(|i| (i / 256, i % 256, ours.px[at(i)], qt.px[at(i)]))
            .collect();
        assert!(
            differ.is_empty(),
            "{what}: {} of 65536 pixels differ from Quickshell, (row, column, ours, Quickshell): {:?}",
            differ.len(),
            &differ[..differ.len().min(8)]
        );
    }

    // The copy kept for the next run of the shell reads back as the same pixels. No pair of
    // bytes in the file tells the two formulas apart: it holds `round(p * 255 / a)` of the
    // pixels we premultiplied when they arrived, and for every `a` and `p <= a` both formulas
    // give `p` back from that.
    let live = qml::notifications::pixmap(&source).unwrap();
    qml::notifications::wait_disk_idle();
    let (key, change) = source
        .strip_prefix("image://qsimage/")
        .and_then(|s| s.split_once('/'))
        .unwrap();
    let saved = std::path::PathBuf::from(std::env::var("XDG_CACHE_HOME").unwrap())
        .join("rusty-impulse/notification-images");
    std::fs::rename(
        saved.join(format!("{key}-{change}.png")),
        saved.join("0-7-1.png"),
    )
    .unwrap();
    let kept = qml::notifications::pixmap("image://qsimage/0-7/1").unwrap();
    assert!(kept == live, "the saved copy differs from the picture");
}

/// The sides of the pixmaps of the five tray items `p16` ... `p512`.
const SIDES: [usize; 5] = [16, 22, 48, 256, 512];

/// Each item three times: with no `sourceSize` (clipped to 104x104), at a `sourceSize` of
/// 24x24, and the largest at a `sourceSize` of its own 512x512. `sizes` is the implicit size
/// of each once all are ready. Quickshell 0.3.2 scales the pixmap closest to the size asked
/// for to that size, and to 100x100 when none is asked for (TrayImageHandle::requestPixmap);
/// it keeps a pixmap of 512x512 as it came.
const EVERY_SIZE: &str = "import QtQuick
import Quickshell
import Quickshell.Services.SystemTray
FloatingWindow {
    id: win
    implicitWidth: 560; implicitHeight: 652
    color: \"black\"
    property string picture: \"\"
    property string sizes: \"\"
    Item {
        id: shown
        width: 560; height: 652
        Rectangle { anchors.fill: parent; color: \"black\" }
        Repeater {
            id: tray
            model: SystemTray.items
            // A pixmap is ready as soon as it is asked for, before its item is counted.
            onItemAdded: win.look()
            Item {
                id: cell
                required property var modelData
                readonly property int k: [\"p16\", \"p22\", \"p48\", \"p256\", \"p512\"].indexOf(modelData.id)
                readonly property bool ready: bare.status == Image.Ready && sized.status == Image.Ready
                    && (k != 4 || own.status == Image.Ready)
                // onStatusChanged runs before `ready` is evaluated again.
                onReadyChanged: win.look()
                readonly property string report: modelData.id + \" \" + bare.implicitWidth + \"x\" + bare.implicitHeight
                    + \" \" + sized.implicitWidth + \"x\" + sized.implicitHeight
                    + \" \" + own.implicitWidth + \"x\" + own.implicitHeight
                Item {
                    x: cell.k * 110; width: 104; height: 104; clip: true
                    Image { id: bare; source: cell.modelData.icon; onStatusChanged: win.look() }
                }
                Image {
                    id: sized
                    x: cell.k * 110; y: 110
                    sourceSize: Qt.size(24, 24)
                    source: cell.modelData.icon
                    onStatusChanged: win.look()
                }
                Image {
                    id: own
                    y: 140
                    sourceSize: Qt.size(512, 512)
                    source: cell.k == 4 ? cell.modelData.icon : \"\"
                    onStatusChanged: win.look()
                }
            }
        }
    }
    property bool saved: false
    function look() {
        if (tray.count != 5)
            return;
        let all = [];
        for (let i = 0; i < 5; i++) {
            let c = tray.itemAt(i);
            if (!c || !c.ready)
                return;
            all.push(c.report);
        }
        all.sort();
        sizes = all.join(\"; \");
@GRAB@
    }
}
";

const GRAB_SIZES: &str = "        if (saved)
            return;
        saved = true;
        shown.grabToImage(r => console.log(\"SAVED\", r.saveToFile(Quickshell.env(\"SHOT\")), \"SIZES \" + sizes + \" END\"));";

/// Five items, each with one opaque pixmap whose red grows to the right and green downwards,
/// and whose blue is all or nothing in turn, so that a pixmap scaled twice shows.
fn send_every_size() -> Vec<Connection> {
    let items = SIDES.iter().zip(["p16", "p22", "p48", "p256", "p512"]);
    let items = items.map(|(&n, id)| {
        let ramp = |v: usize| (v * 255 / (n - 1)) as u8;
        let checker = |i: usize| ((i % n + i / n) % 2 * 255) as u8;
        let px = (0..n * n).flat_map(|i| [255, ramp(i % n), ramp(i / n), checker(i)]);
        Pictures {
            id,
            side: n as i32,
            icon: px.collect(),
            overlay: None,
        }
    });
    serve(items.collect())
}

const SIZES_TEST: &str = concat!(module_path!(), "::tray_pixmaps_of_every_size");

#[test]
fn tray_pixmaps_of_every_size() {
    if !testenv::in_own_process(SIZES_TEST) {
        if !testenv::require_all(&["quickshell", "sway", "dbus-daemon"]) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let bus = format!("unix:path={}", dir.path().join("bus").display());
        testenv::run_in_own_process(SIZES_TEST, [("DBUS_SESSION_BUS_ADDRESS", bus)]);
        return;
    }
    let Some(_bus) = sni_watcher::bus(SIZES_TEST) else {
        panic!("no session bus");
    };
    let dir = tempfile::tempdir().unwrap();
    let source = EVERY_SIZE.replace("@GRAB@", GRAB_SIZES);
    let (qt, log) = quickshell(dir.path(), &source, send_every_size);
    assert_eq!((qt.w, qt.h), (560, 652));
    let qt_sizes = log
        .split_once("SIZES ")
        .and_then(|(_, rest)| rest.split_once(" END"))
        .map(|(sizes, _)| sizes.to_string())
        .unwrap_or_else(|| panic!("Quickshell printed:\n{log}"));

    let source = EVERY_SIZE.replace("@GRAB@", "");
    let (ours, _, our_sizes) = ours(&source, send_every_size, (560, 652), |_, sizes| {
        !sizes.is_empty()
    });
    // (differing pixels, largest difference of a channel) in a rectangle.
    let differ = |x0: usize, y0: usize, w: usize, h: usize| {
        let at = (0..w * h).map(|i| (y0 + i / w) * 560 + x0 + i % w);
        let off = at.filter(|i| ours.px[*i] != qt.px[*i]);
        let most = |i: usize| (0..4).map(|c| ours.px[i][c].abs_diff(qt.px[i][c])).max();
        off.fold((0, 0), |(n, m), i| (n + 1, m.max(most(i).unwrap())))
    };
    let mut report = format!(
        "implicit sizes (no sourceSize, 24x24, 512x512):\n  Quickshell: {qt_sizes}\n  ours:       {our_sizes}\n"
    );
    let mut wrong = qt_sizes != our_sizes;
    for (k, side) in SIDES.iter().enumerate() {
        let (bare, sized) = (differ(k * 110, 0, 104, 104), differ(k * 110, 110, 24, 24));
        report += &format!("p{side}: no sourceSize {bare:?}, at 24x24 {sized:?}\n");
        // Every one is the same to the pixel, stretched or shrunk.
        wrong |= bare != (0, 0) || sized != (0, 0);
    }
    let own = differ(0, 140, 512, 512);
    report += &format!("p512 at 512x512: {own:?}\n");
    wrong |= own != (0, 0);
    if let Ok(keep) = std::env::var("TRAY_SHOTS") {
        qt.save_png(&format!("{keep}/sizes-quickshell.png"))
            .unwrap();
        ours.save_png(&format!("{keep}/sizes-ours.png")).unwrap();
    }
    assert!(
        !wrong,
        "(pixels that differ, by how much at most)\n{report}"
    );
}

/// The sizes each item of [`send_scaled`] is asked at, and the top of each in its column.
const ASKED: [(usize, usize, usize); 5] = [
    (16, 16, 0),
    (24, 24, 20),
    (32, 32, 48),
    (30, 20, 84),
    (100, 100, 108),
];

/// The items of [`send_scaled`]: `p` ones are opaque, `a` ones have every kind of alpha.
const SCALED_IDS: [&str; 5] = ["p16", "p22", "p48", "a22", "a48"];

/// Each item in a column of its own, asked at each size of [`ASKED`].
fn scaled_source(grab: &str) -> String {
    let images = ASKED.map(|(w, h, top)| {
        format!(
            "                Image {{ y: {top}; sourceSize: Qt.size({w}, {h}); \
             source: cell.modelData.icon; onStatusChanged: win.look() }}\n"
        )
    });
    format!(
        "import QtQuick
import Quickshell
import Quickshell.Services.SystemTray
FloatingWindow {{
    id: win
    implicitWidth: 550; implicitHeight: 208
    color: \"black\"
    property string picture: \"\"
    property string sizes: \"\"
    Item {{
        id: shown
        width: 550; height: 208
        Rectangle {{ anchors.fill: parent; color: \"black\" }}
        Repeater {{
            id: tray
            model: SystemTray.items
            // A pixmap is ready as soon as it is asked for, before its item is counted.
            onItemAdded: win.look()
            Item {{
                id: cell
                required property var modelData
                x: {ids}.indexOf(modelData.id) * 110
{images}            }}
        }}
    }}
    property bool saved: false
    function look() {{
        if (tray.count != 5)
            return;
        let all = [];
        for (let i = 0; i < 5; i++) {{
            let c = tray.itemAt(i);
            if (!c || c.children.length != 5)
                return;
            for (let k = 0; k < 5; k++) {{
                if (c.children[k].status != Image.Ready)
                    return;
                all.push(c.modelData.id + \" \" + c.children[k].implicitWidth + \"x\" + c.children[k].implicitHeight);
            }}
        }}
        all.sort();
        sizes = all.join(\"; \");
{grab}
    }}
}}
",
        ids = format_args!("{SCALED_IDS:?}"),
        images = images.concat(),
    )
}

/// Three opaque pixmaps of 16, 22 and 48 a side (red grows to the right, green downwards,
/// blue is all or nothing in turn) and two of 22 and 48 with the same colours whose left
/// third is opaque, whose top right corner is clear and whose alpha elsewhere is every kind.
fn send_scaled() -> Vec<Connection> {
    let items = SCALED_IDS.map(|id| {
        let n: usize = id[1..].parse().unwrap();
        let ramp = |v: usize| (v * 255 / (n - 1)) as u8;
        let checker = |i: usize| ((i % n + i / n) % 2 * 255) as u8;
        let alpha = |i: usize| match (i % n, i / n) {
            _ if id.starts_with('p') => 255,
            (x, _) if x < n / 3 => 255,
            (x, y) if x >= n - n / 3 && y < n / 3 => 0,
            (x, y) => ((x * 37 + y * 91) % 256) as u8,
        };
        let px = (0..n * n).flat_map(|i| [alpha(i), ramp(i % n), ramp(i / n), checker(i)]);
        Pictures {
            id,
            side: n as i32,
            icon: px.collect(),
            overlay: None,
        }
    });
    serve(items.into())
}

const SCALED_TEST: &str = concat!(
    module_path!(),
    "::tray_pixmaps_scaled_as_qimage_scales_them"
);

/// Quickshell 0.3.2 scales the pixmap of a tray item to the size asked for with
/// `QImage::scaled(size, Qt::IgnoreAspectRatio, Qt::SmoothTransformation)` before it is a
/// QPixmap: the pixels are straight ARGB32 while they are scaled. Each pixel we show is the
/// one Quickshell shows.
#[test]
fn tray_pixmaps_scaled_as_qimage_scales_them() {
    if !testenv::in_own_process(SCALED_TEST) {
        if !testenv::require_all(&["quickshell", "sway", "dbus-daemon"]) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let bus = format!("unix:path={}", dir.path().join("bus").display());
        testenv::run_in_own_process(SCALED_TEST, [("DBUS_SESSION_BUS_ADDRESS", bus)]);
        return;
    }
    let Some(_bus) = sni_watcher::bus(SCALED_TEST) else {
        panic!("no session bus");
    };
    let dir = tempfile::tempdir().unwrap();
    let (qt, log) = quickshell(dir.path(), &scaled_source(GRAB_SIZES), send_scaled);
    assert_eq!((qt.w, qt.h), (550, 208));
    let qt_sizes = log
        .split_once("SIZES ")
        .and_then(|(_, rest)| rest.split_once(" END"))
        .map(|(sizes, _)| sizes.to_string())
        .unwrap_or_else(|| panic!("Quickshell printed:\n{log}"));
    // Something of each picture is shown: not a row of each is black.
    for (k, id) in SCALED_IDS.iter().enumerate() {
        for (w, h, top) in ASKED {
            let lit =
                |y: usize| (0..w).any(|x| qt.px[(top + y) * 550 + k * 110 + x] != [0, 0, 0, 255]);
            assert!((0..h).all(lit), "Quickshell, {id} at {w}x{h}");
        }
    }

    let (ours, _, our_sizes) = ours(&scaled_source(""), send_scaled, (550, 208), |_, sizes| {
        !sizes.is_empty()
    });
    let mut report =
        format!("implicit sizes:\n  Quickshell: {qt_sizes}\n  ours:       {our_sizes}\n");
    let mut wrong = qt_sizes != our_sizes;
    for (k, id) in SCALED_IDS.iter().enumerate() {
        for (w, h, top) in ASKED {
            let at = (0..w * h).map(|i| (top + i / w) * 550 + k * 110 + i % w);
            let off = at.filter(|i| ours.px[*i] != qt.px[*i]);
            let most = |i: usize| (0..4).map(|c| ours.px[i][c].abs_diff(qt.px[i][c])).max();
            let differ = off.fold((0, 0), |(n, m), i| (n + 1, m.max(most(i).unwrap())));
            report += &format!("{id} at {w}x{h}: {differ:?}\n");
            wrong |= differ != (0, 0);
        }
    }
    if let Ok(keep) = std::env::var("TRAY_SHOTS") {
        qt.save_png(&format!("{keep}/scaled-quickshell.png"))
            .unwrap();
        ours.save_png(&format!("{keep}/scaled-ours.png")).unwrap();
    }
    assert!(
        !wrong,
        "(pixels that differ, by how much at most)\n{report}"
    );
}

/// A notification's picture with no `sourceSize`, in a window of its own 512x512.
const BIG_PICTURE: &str = "import QtQuick
import Quickshell
import Quickshell.Services.SystemTray
import Quickshell.Services.Notifications
FloatingWindow {
    id: win
    implicitWidth: 512; implicitHeight: 512
    color: \"black\"
    property string picture: \"\"
    property string sizes: \"\"
    NotificationServer {
        imageSupported: true
        onNotification: n => { n.tracked = true; win.picture = n.image; }
    }
    Item {
        id: shown
        width: 512; height: 512
        Rectangle { anchors.fill: parent; color: \"black\" }
        Repeater { model: SystemTray.items; Item {} }
        Image { id: sent; source: win.picture; onStatusChanged: win.look() }
    }
    property bool saved: false
    function look() {
        if (sent.status != Image.Ready)
            return;
        sizes = sent.implicitWidth + \"x\" + sent.implicitHeight;
@GRAB@
    }
}
";

/// A notification whose `image-data` is 512x512, opaque: red grows to the right, green
/// downwards, blue is all or nothing in turn, so that a picture scaled twice shows.
fn send_big_picture() -> Vec<Connection> {
    let n = 512;
    let ramp = |v: usize| (v * 255 / (n - 1)) as u8;
    let checker = |i: usize| ((i % n + i / n) % 2 * 255) as u8;
    let px = (0..n * n).flat_map(|i| [ramp(i % n), ramp(i / n), checker(i), 255]);
    let c = Connection::session().unwrap();
    notify(&c, n as i32, px.collect());
    vec![c]
}

const BIG_TEST: &str = concat!(module_path!(), "::a_notification_picture_of_512");

/// Quickshell 0.3.2 keeps a notification's `image-data` as it came (a QImage of the sender's
/// rows) and shows it pixel for pixel.
#[test]
fn a_notification_picture_of_512() {
    if !testenv::in_own_process(BIG_TEST) {
        if !testenv::require_all(&["quickshell", "sway", "dbus-daemon"]) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let bus = format!("unix:path={}", dir.path().join("bus").display());
        let cache = dir.path().join("cache");
        let vars = [
            ("DBUS_SESSION_BUS_ADDRESS", bus.as_str()),
            ("XDG_CACHE_HOME", cache.to_str().unwrap()),
        ];
        testenv::run_in_own_process(BIG_TEST, vars);
        return;
    }
    let Some(_bus) = sni_watcher::bus(BIG_TEST) else {
        panic!("no session bus");
    };
    let dir = tempfile::tempdir().unwrap();
    let source = BIG_PICTURE.replace("@GRAB@", GRAB_SIZES);
    let (qt, log) = quickshell(dir.path(), &source, send_big_picture);
    assert_eq!((qt.w, qt.h), (512, 512));
    let qt_size = log
        .split_once("SIZES ")
        .and_then(|(_, rest)| rest.split_once(" END"))
        .map(|(sizes, _)| sizes.to_string())
        .unwrap_or_else(|| panic!("Quickshell printed:\n{log}"));
    assert_eq!(qt_size, "512x512", "Quickshell's implicit size");

    let source = BIG_PICTURE.replace("@GRAB@", "");
    let (ours, _, our_size) = ours(&source, send_big_picture, (512, 512), |_, sizes| {
        !sizes.is_empty()
    });
    let off = (0..512 * 512).filter(|i| ours.px[*i] != qt.px[*i]);
    let most = |i: usize| (0..4).map(|c| ours.px[i][c].abs_diff(qt.px[i][c])).max();
    let differ = off.fold((0, 0), |(n, m), i| (n + 1, m.max(most(i).unwrap())));
    assert!(
        our_size == qt_size && differ == (0, 0),
        "implicit size: Quickshell {qt_size}, ours {our_size}; (pixels of 262144 that differ, \
         by how much at most): {differ:?}"
    );
}

/// A tray item's pixmap at a `sourceSize` of 1025x1025, and beside it a notification's
/// picture with no `sourceSize`.
const OVER_1024: &str = "import QtQuick
import Quickshell
import Quickshell.Services.SystemTray
import Quickshell.Services.Notifications
FloatingWindow {
    id: win
    implicitWidth: 2050; implicitHeight: 1025
    color: \"black\"
    property string picture: \"\"
    property string sizes: \"\"
    NotificationServer {
        imageSupported: true
        onNotification: n => { n.tracked = true; win.picture = n.image; }
    }
    Item {
        id: shown
        width: 2050; height: 1025
        Rectangle { anchors.fill: parent; color: \"black\" }
        Repeater {
            id: tray
            model: SystemTray.items
            onItemAdded: win.look()
            Image {
                required property var modelData
                sourceSize: Qt.size(1025, 1025)
                source: modelData.icon
                onStatusChanged: win.look()
            }
        }
        Image { id: sent; x: 1025; source: win.picture; onStatusChanged: win.look() }
    }
    property bool saved: false
    function look() {
        let icon = tray.itemAt(0);
        if (!icon || icon.status != Image.Ready || sent.status != Image.Ready)
            return;
        sizes = icon.implicitWidth + \"x\" + icon.implicitHeight + \" \" + sent.implicitWidth + \"x\" + sent.implicitHeight;
@GRAB@
    }
}
";

/// Whether the pixel at (`x`, `y`) of a picture of [`send_over_1024`] has all its blue.
fn blue_of_1025(x: usize, y: usize) -> bool {
    (x + y) % 2 == 1
}

/// A tray item and a notification with the same opaque picture of 1025x1025: blue is all or
/// nothing in turn, so a picture that was averaged shows.
fn send_over_1024() -> Vec<Connection> {
    let n = 1025;
    let blue = |i: usize| blue_of_1025(i % n, i / n) as u8 * 255;
    let held = serve(vec![Pictures {
        id: "over",
        side: n as i32,
        icon: (0..n * n).flat_map(|i| [255, 0, 0, blue(i)]).collect(),
        overlay: None,
    }]);
    let px = (0..n * n).flat_map(|i| [0, 0, blue(i), 255]);
    notify(&held[0], n as i32, px.collect());
    held
}

const OVER_TEST: &str = concat!(module_path!(), "::pictures_over_1024_are_averaged");

/// docs/quickshell-differences.md: Quickshell 0.3.2 keeps a tray pixmap and a notification's
/// picture of 1025x1025 as they came; we keep them averaged down to 1024x1024.
#[test]
fn pictures_over_1024_are_averaged() {
    if !testenv::in_own_process(OVER_TEST) {
        if !testenv::require_all(&["quickshell", "sway", "dbus-daemon"]) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let bus = format!("unix:path={}", dir.path().join("bus").display());
        let cache = dir.path().join("cache");
        let vars = [
            ("DBUS_SESSION_BUS_ADDRESS", bus.as_str()),
            ("XDG_CACHE_HOME", cache.to_str().unwrap()),
        ];
        testenv::run_in_own_process(OVER_TEST, vars);
        return;
    }
    let Some(_bus) = sni_watcher::bus(OVER_TEST) else {
        panic!("no session bus");
    };
    // Of each picture, how many of its 1025 * 1025 pixels are not the ones sent.
    let changed = |c: &Canvas| {
        [0, 1025].map(|left| {
            let sent = |x: usize, y: usize| [0, 0, blue_of_1025(x, y) as u8 * 255, 255];
            let at = (0..1025 * 1025).map(|i| (i % 1025, i / 1025));
            at.filter(|(x, y)| c.px[y * 2050 + left + x] != sent(*x, *y))
                .count()
        })
    };
    let dir = tempfile::tempdir().unwrap();
    let source = OVER_1024.replace("@GRAB@", GRAB_SIZES);
    let (qt, log) = quickshell(dir.path(), &source, send_over_1024);
    assert_eq!((qt.w, qt.h), (2050, 1025));
    let qt_sizes = log
        .split_once("SIZES ")
        .and_then(|(_, rest)| rest.split_once(" END"))
        .map(|(sizes, _)| sizes.to_string())
        .unwrap_or_else(|| panic!("Quickshell printed:\n{log}"));
    assert_eq!(
        (qt_sizes.as_str(), changed(&qt)),
        ("1025x1025 1025x1025", [0, 0]),
        "Quickshell"
    );

    let source = OVER_1024.replace("@GRAB@", "");
    let (ours, _, our_sizes) = ours(&source, send_over_1024, (2050, 1025), |_, sizes| {
        !sizes.is_empty()
    });
    let changed = changed(&ours);
    eprintln!("ours: {our_sizes}, {changed:?} of 1050625 pixels are not the ones sent");
    assert_eq!(our_sizes, "1025x1025 1024x1024");
    // The notification's picture loses its last row and column to the ones before them; the
    // pixmap is then also scaled back to the 1025x1025 asked for.
    assert!(changed.iter().all(|n| *n > 0), "{changed:?}");
}

/// The icon of the one tray item with no `sourceSize`, at a `sourceSize` of 24x24 and at one
/// of 33x17.
const NO_ICON: &str = "import QtQuick
import Quickshell
import Quickshell.Services.SystemTray
FloatingWindow {
    id: win
    implicitWidth: 200; implicitHeight: 104
    color: \"black\"
    property string picture: \"\"
    property string sizes: \"\"
    Item {
        id: shown
        width: 200; height: 104
        Rectangle { anchors.fill: parent; color: \"black\" }
        Repeater {
            id: tray
            model: SystemTray.items
            onItemAdded: win.look()
            Item {
                id: cell
                required property var modelData
                readonly property bool ready: bare.status == Image.Ready && sized.status == Image.Ready
                    && odd.status == Image.Ready
                // onStatusChanged runs before `ready` is evaluated again.
                onReadyChanged: win.look()
                readonly property string report: bare.implicitWidth + \"x\" + bare.implicitHeight
                    + \" \" + sized.implicitWidth + \"x\" + sized.implicitHeight
                    + \" \" + odd.implicitWidth + \"x\" + odd.implicitHeight
                Image { id: bare; source: cell.modelData.icon; onStatusChanged: win.look() }
                Image {
                    id: sized
                    x: 104
                    sourceSize: Qt.size(24, 24)
                    source: cell.modelData.icon
                    onStatusChanged: win.look()
                }
                Image {
                    id: odd
                    x: 132
                    sourceSize: Qt.size(33, 17)
                    source: cell.modelData.icon
                    onStatusChanged: win.look()
                }
            }
        }
    }
    property bool saved: false
    function look() {
        let c = tray.itemAt(0);
        if (!c || !c.ready)
            return;
        sizes = c.report;
@GRAB@
    }
}
";

fn send_no_icon() -> Vec<Connection> {
    serve(vec![NoIcon])
}

const NO_ICON_TEST: &str = concat!(module_path!(), "::a_tray_item_with_no_icon");

/// Quickshell 0.3.2 draws a tray item that gives neither a name nor a pixmap as its missing
/// picture (IconImageProvider::missingPixmap from TrayImageHandle::requestPixmap): black with
/// magenta in its top right and bottom left quarters, at the size asked for rounded up to even.
#[test]
fn a_tray_item_with_no_icon() {
    if !testenv::in_own_process(NO_ICON_TEST) {
        if !testenv::require_all(&["quickshell", "sway", "dbus-daemon"]) {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let bus = format!("unix:path={}", dir.path().join("bus").display());
        testenv::run_in_own_process(NO_ICON_TEST, [("DBUS_SESSION_BUS_ADDRESS", bus)]);
        return;
    }
    let Some(_bus) = sni_watcher::bus(NO_ICON_TEST) else {
        panic!("no session bus");
    };
    let dir = tempfile::tempdir().unwrap();
    let source = NO_ICON.replace("@GRAB@", GRAB_SIZES);
    let (qt, log) = quickshell(dir.path(), &source, send_no_icon);
    assert_eq!((qt.w, qt.h), (200, 104));
    let qt_sizes = log
        .split_once("SIZES ")
        .and_then(|(_, rest)| rest.split_once(" END"))
        .map(|(sizes, _)| sizes.to_string())
        .unwrap_or_else(|| panic!("Quickshell printed:\n{log}"));
    assert_eq!(qt_sizes, "100x100 24x24 34x18", "Quickshell");
    let (black, magenta) = ([0, 0, 0, 255], [0xd9, 0x00, 0xd8, 255]);
    let quarters = |c: &Canvas, x0: usize, w: usize, h: usize| {
        [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)].map(|(x, y)| c.px[y * 200 + x0 + x])
    };
    for (x0, w, h) in [(0, 100, 100), (104, 24, 24), (132, 34, 18)] {
        assert_eq!(
            quarters(&qt, x0, w, h),
            [black, magenta, magenta, black],
            "Quickshell at {w}x{h}"
        );
    }

    let source = NO_ICON.replace("@GRAB@", "");
    let (ours, _, our_sizes) = ours(&source, send_no_icon, (200, 104), |_, sizes| {
        !sizes.is_empty()
    });
    let differ = (0..200 * 104).filter(|i| ours.px[*i] != qt.px[*i]).count();
    assert!(
        our_sizes == qt_sizes && differ == 0,
        "implicit sizes: Quickshell {qt_sizes}, ours {our_sizes}; {differ} of 20800 pixels differ"
    );
}
