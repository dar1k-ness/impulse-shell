//! qml's per-surface variants of the media, resources, calendar and notes widgets: each
//! loads on the desktop (`DesktopLayoutContent`) and in the right sidebar
//! (`SidebarLayoutContent`) from the shipped `widgets.toml` without errors, reports a natural
//! size, and a desktop variant fills the box the layout gives it.
#![cfg(feature = "interp")]

use qml::runtime::{Runtime, World};
use qml::value::Value;
use shell::scene;
use std::path::{Path, PathBuf};
#[path = "support/frames.rs"]
mod frames;

const TYPES: [&str; 4] = ["media", "resources", "calendar", "notes"];

/// What each type's variants draw, on both surfaces: a desktop variant is a free item over the
/// wallpaper, a sidebar one a `Rectangle` card.
const DRAWS: [(&str, &str); 4] = [
    ("media", "MediaCard"),
    ("resources", "ResourcesCard"),
    ("calendar", "CalendarWidget"),
    ("notes", "TodoWidget"),
];

/// Each type twice on the desktop: without a size (natural) and in a 600x400 box.
fn layout() -> String {
    let mut s = String::from(
        "version = 5\n[[monitor]]\nconnector = \"DP-1\"\nprimary = true\n\n\
         [[monitor.container]]\nid = \"desktop\"\nkind = \"desktop\"\n",
    );
    for (i, t) in TYPES.iter().enumerate() {
        let x = 20 + i * 460;
        s += &format!(
            "\n[[monitor.container.widget]]\nid = \"{t}-natural\"\ntype = \"{t}\"\nx = {x}\ny = 20\n"
        );
        let x = 20 + (i % 3) * 620;
        let y = 480 + (i / 3) * 420;
        s += &format!(
            "\n[[monitor.container.widget]]\nid = \"{t}-box\"\ntype = \"{t}\"\nx = {x}\ny = {y}\nw = 600\nh = 400\n"
        );
    }
    s += "\n[[monitor.container]]\nid = \"right\"\nkind = \"sidebar\"\nside = \"right\"\ncolumns = 5\n";
    for t in TYPES {
        s += &format!(
            "\n[[monitor.container.widget]]\nid = \"{t}-side\"\ntype = \"{t}\"\nsize = 5\n"
        );
    }
    s
}

const MAIN: &str = r#"import QtQuick
import ri.modules.rusty.background
import ri.modules.rusty.sidebarRight
Item {
    width: 2560
    height: 1440
    DesktopLayoutContent { anchors.fill: parent; connector: "DP-1" }
    SidebarLayoutContent { x: 2100; width: 420; height: 1440; connector: "DP-1" }
}
"#;

/// A player MprisController reports as active, for PlayerControl's side of the media card.
const MPRIS: &str = r#"pragma Singleton
import QtQuick
import Rustshell
import Rustshell.Services.Mpris
Singleton {
    property list<var> players: activePlayer ? [activePlayer] : []
    property var trackedPlayer: activePlayer
    // An MprisPlayer, which only the engine makes (`run`).
    property MprisPlayer activePlayer: null
    signal trackChanged(reverse: bool)
}
"#;

/// qml mirrored by symlinks, with `services/MprisController.qml` replaced when `mpris` is set.
fn tree(tag: &str, mpris: Option<&str>) -> PathBuf {
    let ri = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../qml")
        .canonicalize()
        .unwrap();
    let d = std::env::temp_dir().join(format!("cross-surface-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(&ri).unwrap() {
        let e = e.unwrap();
        if let Some(mpris) = mpris
            && e.file_name() == "services"
        {
            std::fs::create_dir_all(d.join("services")).unwrap();
            for s in std::fs::read_dir(e.path()).unwrap() {
                let s = s.unwrap();
                if s.file_name() != "MprisController.qml" {
                    std::os::unix::fs::symlink(s.path(), d.join("services").join(s.file_name()))
                        .unwrap();
                }
            }
            std::fs::write(d.join("services/MprisController.qml"), mpris).unwrap();
        } else {
            std::os::unix::fs::symlink(e.path(), d.join(e.file_name())).unwrap();
        }
    }
    std::fs::write(d.join("layout.toml"), layout()).unwrap();
    std::fs::write(d.join("main.qml"), MAIN).unwrap();
    d
}

/// A loaded widget's own types, every type below it, its size and implicit size.
type Loaded = (Vec<String>, Vec<String>, f64, f64, f64, f64);

struct Host {
    id: String,
    error: String,
    status: f64,
    item: Option<Loaded>,
}

fn run(tag: &str, mpris: Option<&str>) -> (Vec<Host>, Vec<String>) {
    let d = tree(tag, mpris);
    let mut rt = Runtime::new(Some(d.clone()));
    let mut frames = frames::Frames::new(&mut rt);
    rt.load_singletons(&[
        d.join("modules/common"),
        d.join("modules/common/functions"),
        d.join("services"),
        d.clone(),
    ]);
    rt.world.borrow_mut().open_layout(d.join("layout.toml"));
    rt.settle();
    if mpris.is_some() {
        let mut w = rt.world.borrow_mut();
        let controller = *w
            .singletons
            .get("MprisController")
            .expect("the MprisController singleton");
        let player = qml::types::native_singleton(&mut w, "MprisPlayer");
        for (name, v) in [
            ("trackTitle", Value::str("Song")),
            ("trackArtist", Value::str("Artist")),
            ("trackArtUrl", Value::str("")),
            ("position", Value::Num(30.0)),
            ("length", Value::Num(200.0)),
            ("canSeek", Value::Bool(true)),
            ("isPlaying", Value::Bool(true)),
            ("playbackState", Value::Num(0.0)),
        ] {
            w.write(player, name, v);
        }
        w.write(controller, "activePlayer", Value::Item(player));
    }
    rt.settle();
    let main = rt.load_file(&d.join("main.qml")).unwrap();
    rt.settle();
    let mut fonts = scene::Fonts::default();
    // Config reads its file and the widgets' loaders come in over a few frames.
    let mut layout =
        |rt: &mut Runtime| scene::layout_window(rt, &mut fonts, main, [2560.0, 1440.0]);
    frames.until_config(&mut rt, &mut layout);
    frames.frames(&mut rt, &mut layout, 60);
    let errors = rt.errors();
    let mut w = rt.world.borrow_mut();
    let num =
        |w: &mut World, i: usize, k: &str| w.read(i, k).unwrap_or(Value::Num(0.0)).to_number();
    let hosts: Vec<usize> = (0..w.items.len())
        .filter(|&i| !w.dead.contains(&i) && w.items[i].types.iter().any(|t| t == "WidgetHost"))
        .collect();
    let mut out = vec![];
    for h in hosts {
        let id = w.read(h, "layoutId").unwrap_or_default().to_js_string();
        let error = w.read(h, "errorString").unwrap_or_default().to_js_string();
        let status = num(&mut w, h, "status");
        let item = match w.read(h, "item") {
            Some(Value::Item(i)) => Some((
                w.items[i].types.iter().map(|t| t.to_string()).collect(),
                below(&w, i),
                num(&mut w, i, "width"),
                num(&mut w, i, "height"),
                num(&mut w, i, "implicitWidth"),
                num(&mut w, i, "implicitHeight"),
            )),
            _ => None,
        };
        out.push(Host {
            id,
            error,
            status,
            item,
        });
    }
    drop(w);
    std::fs::remove_dir_all(&d).unwrap();
    (out, errors)
}

fn below(w: &World, i: usize) -> Vec<String> {
    let mut out = vec![];
    let mut todo = w.items[i].children.clone();
    while let Some(c) = todo.pop() {
        if w.dead.contains(&c) {
            continue;
        }
        out.extend(w.items[c].types.iter().map(|t| t.to_string()));
        todo.extend(w.items[c].children.iter().copied());
    }
    out
}

/// Errors that name one of the variants or what they draw.
fn own_errors(errors: &[String]) -> Vec<&String> {
    const FILES: [&str; 12] = [
        "DesktopMedia",
        "DesktopResources",
        "DesktopCalendar",
        "DesktopNotes",
        "MediaCard",
        "ResourcesCard",
        "PlayerControl",
        "sidebarRight/widgets/",
        "CalendarWidget",
        "TodoWidget",
        "TaskList",
        "WidgetHost",
    ];
    errors
        .iter()
        .filter(|e| FILES.iter().any(|f| e.contains(f)))
        .collect()
}

fn check(tag: &str, mpris: Option<&str>) {
    let (hosts, errors) = run(tag, mpris);
    let own = own_errors(&errors);
    assert!(own.is_empty(), "{own:#?}");
    let host = |id: &str| {
        hosts
            .iter()
            .find(|h| h.id == id)
            .unwrap_or_else(|| panic!("no host {id}"))
    };
    // A type through a qualified import shows as `Ns.Type`.
    let is = |v: &[String], t: &str| v.iter().any(|x| x == t || x.ends_with(&format!(".{t}")));
    for (t, draws) in DRAWS {
        for id in [format!("{t}-natural"), format!("{t}-box")] {
            let h = host(&id);
            assert_eq!((h.error.as_str(), h.status), ("", 1.0), "{id}");
            let (ty, below, w, ht, iw, ih) = h
                .item
                .clone()
                .unwrap_or_else(|| panic!("{id}: nothing loaded"));
            assert!(
                !is(&ty, "Rectangle") && is(&below, draws),
                "{id}: {ty:?} {below:?}"
            );
            if t == "media" {
                assert_eq!(
                    is(&below, "PlayerControl"),
                    mpris.is_some(),
                    "{id}: the player's card"
                );
            }
            assert!(iw > 0.0 && ih > 0.0, "{id}: natural size {iw}x{ih}");
            if id.ends_with("-box") {
                assert_eq!((w, ht), (600.0, 400.0), "{id}: fills its box");
            } else {
                assert_eq!((w, ht), (iw, ih), "{id}: natural size");
            }
        }
    }
    for (t, draws) in DRAWS {
        let id = format!("{t}-side");
        let h = host(&id);
        assert_eq!((h.error.as_str(), h.status), ("", 1.0), "{id}");
        let (ty, below, w, ht, _, ih) = h
            .item
            .clone()
            .unwrap_or_else(|| panic!("{id}: nothing loaded"));
        assert!(
            is(&ty, "Rectangle") && is(&below, draws),
            "{id}: {ty:?} {below:?}"
        );
        if t == "media" {
            assert_eq!(
                is(&below, "PlayerControl"),
                mpris.is_some(),
                "{id}: the player's card"
            );
        }
        assert!(
            ih > 0.0 && ht > 0.0 && w > 0.0,
            "{id}: {w}x{ht} (implicit height {ih})"
        );
    }
}

#[test]
fn every_variant_loads_on_its_surface() {
    check("idle", None);
}

#[test]
fn media_variants_load_with_an_active_player() {
    check("player", Some(MPRIS));
}
