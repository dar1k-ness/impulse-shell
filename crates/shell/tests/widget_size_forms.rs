//! How the desktop and sidebar widgets draw at the sizes the layout can give them: the media
//! card's form by its size (the cover alone, a circle, in a box with no room for text), the
//! resources gauges' arrangement, the notes and the calendar in a small box, and a sidebar widget
//! narrower than the sidebar as a cell of the grid's width, as high as the toggles. A widget
//! without a size keeps the original's form, and no text of the media card leaves the card.
#![cfg(feature = "interp")]

use qml::runtime::{Runtime, World};
use qml::value::Value;
use shell::{input::Pointer, scene};
use std::path::{Path, PathBuf};
#[path = "support/frames.rs"]
mod frames;

const BTN_LEFT: u32 = 0x110;

/// Desktop boxes: id, type, width and height (0: no size, the widget's own).
const BOXES: [(&str, &str, u32, u32); 20] = [
    ("media-own", "media", 0, 0),
    ("media-64x64", "media", 64, 64),
    ("media-110x110", "media", 110, 110),
    ("media-200x64", "media", 200, 64),
    ("media-420x140", "media", 420, 140),
    ("media-280x400", "media", 280, 400),
    ("media-640x240", "media", 640, 240),
    ("resources-own", "resources", 0, 0),
    ("resources-160x80", "resources", 160, 80),
    ("resources-160x360", "resources", 160, 360),
    ("resources-640x360", "resources", 640, 360),
    ("notes-200x200", "notes", 200, 200),
    ("notes-320x350", "notes", 320, 350),
    ("calendar-own", "calendar", 0, 0),
    ("calendar-160x160", "calendar", 160, 160),
    ("calendar-400x160", "calendar", 400, 160),
    ("calendar-340x340", "calendar", 340, 340),
    ("weather-own", "weather", 0, 0),
    ("weather-100x100", "weather", 100, 100),
    ("weather-360x360", "weather", 360, 360),
];

/// Sidebar grid cells: id, type and width in cells of the five the sidebar has.
const CELLS: [(&str, &str, u32); 10] = [
    ("side-media-1", "media", 1),
    ("side-media-3", "media", 3),
    ("side-clock-2", "clock", 2),
    ("side-weather-2", "weather", 2),
    ("side-resources-1", "resources", 1),
    ("side-calendar-1", "calendar", 1),
    ("side-notes-2", "notes", 2),
    ("side-calendar-3", "calendar", 3),
    ("side-notes-3", "notes", 3),
    ("side-audio-1", "audio", 1),
];

fn layout() -> String {
    let mut s = String::from(
        "version = 5\n[[monitor]]\nconnector = \"DP-1\"\nprimary = true\n\n\
         [[monitor.container]]\nid = \"desktop\"\nkind = \"desktop\"\n",
    );
    let (mut x, mut y, mut row) = (20, 20, 0);
    for (id, t, w, h) in BOXES {
        let (bw, bh) = if w == 0 { (460, 360) } else { (w, h) };
        if x + bw > 2000 {
            (x, y, row) = (20, y + row + 20, 0);
        }
        s += &format!(
            "\n[[monitor.container.widget]]\nid = \"{id}\"\ntype = \"{t}\"\nx = {x}\ny = {y}\n"
        );
        if w > 0 {
            s += &format!("w = {w}\nh = {h}\n");
        }
        x += bw + 20;
        row = row.max(bh);
    }
    s += "\n[[monitor.container]]\nid = \"right\"\nkind = \"sidebar\"\nside = \"right\"\ncolumns = 5\n";
    for (id, t, n) in CELLS {
        s +=
            &format!("\n[[monitor.container.widget]]\nid = \"{id}\"\ntype = \"{t}\"\nsize = {n}\n");
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
    let d = std::env::temp_dir().join(format!("widget-size-forms-{tag}-{}", std::process::id()));
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

/// A widget as its host loaded it.
struct Widget {
    id: String,
    error: String,
    item: usize,
    width: f64,
    height: f64,
}

struct Loaded {
    rt: Runtime,
    main: usize,
    widgets: Vec<Widget>,
    errors: Vec<String>,
}

fn load(tag: &str, mpris: Option<&str>) -> Loaded {
    load_with(tag, mpris, &[])
}

/// As `load`, with these options of `Config.options.background.widgets` set once Config has
/// read its file: (widget, key, value).
fn load_with(tag: &str, mpris: Option<&str>, options: &[(&str, &str, Value)]) -> Loaded {
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
            (
                "trackTitle",
                Value::str("A song with a title far too long for a small card (Remastered)"),
            ),
            ("trackArtist", Value::str("An artist with a long name")),
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
    for (widget, key, v) in options {
        let mut w = rt.world.borrow_mut();
        let mut o = *w.singletons.get("Config").expect("the Config singleton");
        for k in ["options", "background", "widgets", widget] {
            o = match w.read(o, k) {
                Some(Value::Item(i)) => i,
                v => panic!("{k}: {v:?}"),
            };
        }
        w.write(o, key, v.clone());
    }
    frames.frames(&mut rt, &mut layout, 60);
    let errors = rt.errors();
    let mut widgets = vec![];
    {
        let mut w = rt.world.borrow_mut();
        let hosts: Vec<usize> = (0..w.items.len())
            .filter(|&i| !w.dead.contains(&i) && w.items[i].types.iter().any(|t| t == "WidgetHost"))
            .collect();
        for h in hosts {
            let id = w.read(h, "layoutId").unwrap_or_default().to_js_string();
            let error = w.read(h, "errorString").unwrap_or_default().to_js_string();
            if let Some(Value::Item(item)) = w.read(h, "item") {
                widgets.push(Widget {
                    id,
                    error,
                    item,
                    width: num(&mut w, item, "width"),
                    height: num(&mut w, item, "height"),
                });
            }
        }
    }
    std::fs::remove_dir_all(&d).unwrap();
    Loaded {
        rt,
        main,
        widgets,
        errors,
    }
}

fn num(w: &mut World, i: usize, k: &str) -> f64 {
    w.read(i, k).unwrap_or(Value::Num(0.0)).to_number()
}

fn is(w: &World, i: usize, t: &str) -> bool {
    w.items[i]
        .types
        .iter()
        .any(|x| x == t || x.ends_with(&format!(".{t}")))
}

/// The live items below `i`, itself included.
fn below(w: &World, i: usize) -> Vec<usize> {
    let mut out = vec![];
    let mut todo = vec![i];
    while let Some(c) = todo.pop() {
        if w.dead.contains(&c) {
            continue;
        }
        out.push(c);
        todo.extend(w.items[c].children.iter().copied());
    }
    out
}

impl Loaded {
    fn widget(&self, id: &str) -> &Widget {
        let widget = self
            .widgets
            .iter()
            .find(|h| h.id == id)
            .unwrap_or_else(|| panic!("no widget {id}: {:#?}", self.errors));
        assert_eq!(widget.error, "", "{id}");
        widget
    }

    /// The one item of type `t` in widget `id`.
    fn part(&self, id: &str, t: &str) -> usize {
        let w = self.rt.world.borrow();
        let found: Vec<usize> = below(&w, self.widget(id).item)
            .into_iter()
            .filter(|&i| is(&w, i, t))
            .collect();
        assert_eq!(found.len(), 1, "{id}: one {t}");
        found[0]
    }

    fn text(&self, i: usize, k: &str) -> String {
        let mut w = self.rt.world.borrow_mut();
        w.read(i, k).unwrap_or_default().to_js_string()
    }

    fn num(&self, i: usize, k: &str) -> f64 {
        num(&mut self.rt.world.borrow_mut(), i, k)
    }

    fn flag(&self, i: usize, k: &str) -> bool {
        let mut w = self.rt.world.borrow_mut();
        matches!(w.read(i, k), Some(Value::Bool(true)))
    }

    /// Frames: animations run and the window is laid out.
    fn frames(&mut self, n: usize) {
        let mut fonts = scene::Fonts::default();
        for k in 0..n {
            self.rt.tick(1_000_000.0 + 16.0 * k as f64);
            self.rt.settle();
            scene::layout_window(&mut self.rt, &mut fonts, self.main, [2560.0, 1440.0]);
            self.rt.settle();
        }
    }

    /// The to-do list kept in a file of this test's own, with these unfinished tasks in it.
    fn tasks(&mut self, tasks: &[&str]) -> PathBuf {
        let file = std::env::temp_dir().join(format!(
            "widget-size-forms-todo-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&file);
        let todo = *self.rt.world.borrow().singletons.get("Todo").expect("Todo");
        let path = Value::str(file.to_str().unwrap());
        self.rt.world.borrow_mut().write(todo, "filePath", path);
        self.frames(20);
        for task in tasks {
            self.rt
                .call_method(todo, "addTask", vec![Value::str(task)])
                .expect("Todo.addTask")
                .unwrap();
        }
        self.frames(20);
        file
    }

    /// What the small notes `id` show: every shown text outside the hidden full list.
    fn small_notes(&self, id: &str) -> Vec<String> {
        let widget = self.widget(id).item;
        let mut w = self.rt.world.borrow_mut();
        let mut out = vec![];
        for i in below(&w, widget) {
            if !is(&w, i, "Text") || is(&w, i, "MaterialSymbol") {
                continue;
            }
            let mut shown = true;
            let mut at = i;
            while at != widget {
                shown &= matches!(w.read(at, "visible"), Some(Value::Bool(true)));
                at = w.items[at].parent.expect("a text below its widget");
            }
            let text = w.read(i, "text").unwrap_or_default().to_js_string();
            if shown && !text.is_empty() {
                out.push(text);
            }
        }
        out.sort();
        out
    }

    /// A left click in the middle of the first shown `t` of widget `id`.
    fn click_first(&mut self, id: &str, t: &str) {
        let widget = self.widget(id).item;
        let at = {
            let mut w = self.rt.world.borrow_mut();
            let mut found = vec![];
            for i in below(&w, widget) {
                if !is(&w, i, t) {
                    continue;
                }
                let (mut x, mut y, mut shown) = (0.0, 0.0, true);
                let mut at = Some(i);
                while let Some(a) = at {
                    shown &=
                        a == self.main || matches!(w.read(a, "visible"), Some(Value::Bool(true)));
                    x += num(&mut w, a, "x");
                    y += num(&mut w, a, "y");
                    at = w.items[a].parent;
                }
                if shown {
                    let (bw, bh) = (num(&mut w, i, "width"), num(&mut w, i, "height"));
                    found.push((y, x + bw / 2.0, y + bh / 2.0));
                }
            }
            found.sort_by(|a, b| a.0.total_cmp(&b.0));
            let first = found
                .first()
                .unwrap_or_else(|| panic!("{id}: no shown {t}"));
            (first.1, first.2)
        };
        let mut ptr = Pointer::default();
        ptr.enter(&mut self.rt, self.main, at.0, at.1);
        ptr.motion(&mut self.rt, self.main, at.0, at.1);
        self.frames(3);
        ptr.press(&mut self.rt, BTN_LEFT);
        self.frames(3);
        ptr.release(&mut self.rt, BTN_LEFT);
        self.frames(20);
    }

    /// The shown texts of widget `id` that reach outside it: each with its box in the widget.
    fn texts_outside(&self, id: &str) -> Vec<String> {
        let widget = self.widget(id);
        let mut w = self.rt.world.borrow_mut();
        let mut out = vec![];
        for i in below(&w, widget.item) {
            if !is(&w, i, "Text") {
                continue;
            }
            let (mut x, mut y, mut shown) = (0.0, 0.0, true);
            let mut at = i;
            while at != widget.item {
                shown &= matches!(w.read(at, "visible"), Some(Value::Bool(true)));
                x += num(&mut w, at, "x");
                y += num(&mut w, at, "y");
                at = w.items[at].parent.expect("a text below its widget");
            }
            let (tw, th) = (num(&mut w, i, "width"), num(&mut w, i, "height"));
            let text = w.read(i, "text").unwrap_or_default().to_js_string();
            if shown
                && !text.is_empty()
                && (x < -1.0
                    || y < -1.0
                    || x + tw > widget.width + 1.0
                    || y + th > widget.height + 1.0)
            {
                out.push(format!(
                    "{text:?} at {x},{y} {tw}x{th} in {}x{}",
                    widget.width, widget.height
                ));
            }
        }
        out
    }

    fn own_errors(&self) -> Vec<&String> {
        const FILES: [&str; 12] = [
            "DesktopCalendar",
            "DesktopMedia",
            "DesktopResources",
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
        self.errors
            .iter()
            .filter(|e| FILES.iter().any(|f| e.contains(f)))
            .collect()
    }
}

/// The media card's form in each desktop box.
const MEDIA_FORMS: [(&str, &str); 7] = [
    ("media-own", "full"),
    ("media-64x64", "circle"),
    ("media-110x110", "circle"),
    ("media-200x64", "row"),
    ("media-420x140", "full"),
    ("media-280x400", "tall"),
    ("media-640x240", "full"),
];

fn media_takes_the_form_its_box_has_room_for(tag: &str, mpris: Option<&str>) {
    let l = load(tag, mpris);
    assert!(l.own_errors().is_empty(), "{:#?}", l.own_errors());
    for (id, form) in MEDIA_FORMS {
        let card = l.part(id, "MediaCard");
        assert_eq!(l.text(card, "form"), form, "{id}");
        assert_eq!(l.texts_outside(id), Vec::<String>::new(), "{id}");
        if mpris.is_some() {
            let player = l.part(id, "PlayerControl");
            assert_eq!(l.text(player, "form"), form, "{id}: the player's card");
        }
    }
    // Without a size, and in a box of the original's size, the card is the original's.
    for id in ["media-own", "media-420x140"] {
        assert_eq!(l.num(l.part(id, "MediaCard"), "zoom"), 1.0, "{id}");
    }
    // The largest box the type allows draws the card larger.
    assert!(l.num(l.part("media-640x240", "MediaCard"), "zoom") > 1.5);
    // The circle is the box's smaller side.
    assert_eq!(
        l.num(l.part("media-64x64", "MediaCard"), "circleSize"),
        64.0
    );
}

#[test]
fn media_takes_the_form_its_box_has_room_for_with_no_player() {
    media_takes_the_form_its_box_has_room_for("idle", None);
}

#[test]
fn media_takes_the_form_its_box_has_room_for_with_a_player() {
    media_takes_the_form_its_box_has_room_for("player", Some(MPRIS));
}

#[test]
fn resources_gauges_take_the_arrangement_their_box_draws_largest() {
    let l = load("resources", None);
    assert!(l.own_errors().is_empty(), "{:#?}", l.own_errors());
    // (stacked, labels beside the rings, rings alone)
    for (id, arrangement) in [
        ("resources-own", (false, false, false)),
        ("resources-160x80", (false, false, true)),
        ("resources-160x360", (true, false, false)),
        ("resources-640x360", (false, false, false)),
    ] {
        let card = l.part(id, "ResourcesCard");
        let got = (
            l.flag(card, "stacked"),
            l.flag(card, "labelsBeside"),
            l.flag(card, "ringsOnly"),
        );
        assert_eq!(got, arrangement, "{id}");
        // The gauges, scaled, are inside the box.
        let widget = l.widget(id);
        let scale = l.num(card, "scale");
        let (w, h) = (
            l.num(card, "implicitWidth") * scale,
            l.num(card, "implicitHeight") * scale,
        );
        assert!(
            w <= widget.width + 0.5 && h <= widget.height + 0.5,
            "{id}: gauges {w}x{h} in {}x{}",
            widget.width,
            widget.height
        );
    }
    let own = l.part("resources-own", "ResourcesCard");
    assert_eq!((l.num(own, "zoom"), l.num(own, "scale")), (1.0, 1.0));
    // A large box draws the gauges larger; the scale is only what that leaves over.
    let large = l.part("resources-640x360", "ResourcesCard");
    let (zoom, scale) = (l.num(large, "zoom"), l.num(large, "scale"));
    assert!(zoom > 1.3 && scale < 1.5, "zoom {zoom}, scale {scale}");
}

#[test]
fn notes_in_a_box_too_small_for_the_list_are_the_unfinished_tasks_alone() {
    let l = load("notes", None);
    assert!(l.own_errors().is_empty(), "{:#?}", l.own_errors());
    let small = l.widget("notes-200x200").item;
    assert!(l.flag(small, "compact"));
    // Nothing is drawn scaled down: the full list is hidden, not shrunk.
    let list = l.part("notes-200x200", "TodoWidget");
    assert!(!l.flag(list, "visible"));
    assert_eq!(l.num(list, "scale"), 1.0);
    assert_eq!(l.texts_outside("notes-200x200"), Vec::<String>::new());
    let own = l.widget("notes-320x350").item;
    assert!(!l.flag(own, "compact"));
    let list = l.part("notes-320x350", "TodoWidget");
    assert!(l.flag(list, "visible"));
    assert_eq!(
        (
            l.num(list, "scale"),
            l.num(list, "width"),
            l.num(list, "height")
        ),
        (1.0, 300.0, 330.0)
    );
}

#[test]
fn the_tick_of_a_small_notes_task_takes_it_off_the_list() {
    let mut l = load("notes-tick", None);
    let file = l.tasks(&["Buy milk", "Call the bank", "Fetch the parcel"]);
    assert_eq!(
        l.small_notes("notes-200x200"),
        ["3", "Buy milk", "Call the bank", "Fetch the parcel"]
    );
    l.click_first("notes-200x200", "TodoItemActionButton");
    // The task leaves the list, the number above it drops, and the file has it as done.
    assert_eq!(
        l.small_notes("notes-200x200"),
        ["2", "Call the bank", "Fetch the parcel"]
    );
    let saved = std::fs::read_to_string(&file).unwrap();
    std::fs::remove_file(&file).unwrap();
    assert_eq!(
        saved,
        concat!(
            r#"[{"content":"Buy milk","done":true},"#,
            r#"{"content":"Call the bank","done":false},"#,
            r#"{"content":"Fetch the parcel","done":false}]"#
        )
    );
    assert!(l.own_errors().is_empty(), "{:#?}", l.own_errors());
}

#[test]
fn calendar_in_a_box_too_small_for_the_month_is_the_week_or_the_day() {
    let l = load("calendar", None);
    assert!(l.own_errors().is_empty(), "{:#?}", l.own_errors());
    for (id, form) in [
        ("calendar-own", "month"),
        ("calendar-160x160", "day"),
        ("calendar-400x160", "week"),
        ("calendar-340x340", "month"),
    ] {
        let card = l.widget(id).item;
        assert_eq!(l.text(card, "form"), form, "{id}");
        assert!(l.flag(l.part(id, "CalendarWidget"), "visible") == (form == "month"));
    }
    let own = l.widget("calendar-own").item;
    assert_eq!(l.num(own, "monthScale"), 1.0);
    // What is shown instead of the month is never drawn smaller than the month may be.
    let day = l.widget("calendar-160x160").item;
    assert!(l.num(day, "dayScale") >= 1.0, "{}", l.num(day, "dayScale"));
    let week = l.widget("calendar-400x160").item;
    assert!(l.num(week, "weekScale") >= 0.75);
    for id in ["calendar-160x160", "calendar-400x160"] {
        assert_eq!(l.texts_outside(id), Vec::<String>::new(), "{id}");
    }
}

/// The weather in a box of another size is drawn at that size: the shape, the temperature and
/// the icon are that many times the original's, and nothing is stretched (`scale` stays 1).
/// Placed by the wallpaper it is the original's size in any box.
#[test]
fn weather_is_drawn_at_the_size_of_its_box() {
    let on = ("weather", "enable", Value::Bool(true));
    let placed = |by: &str| ("weather", "placementStrategy", Value::Str(by.into()));
    for (tag, options, zooms) in [
        (
            "weather-free",
            vec![on.clone(), placed("free")],
            [1.0, 0.5, 1.8],
        ),
        ("weather-wallpaper", vec![on, placed("leastBusy")], [1.0; 3]),
    ] {
        let l = load_with(tag, None, &options);
        let errors: Vec<&String> = l.errors.iter().filter(|e| e.contains("eather")).collect();
        assert!(errors.is_empty(), "{tag}: {errors:#?}");
        for (id, zoom) in ["weather-own", "weather-100x100", "weather-360x360"]
            .into_iter()
            .zip(zooms)
        {
            let weather = l.part(id, "WeatherWidget");
            assert_eq!(l.num(weather, "zoom"), zoom, "{tag} {id}");
            assert_eq!(l.num(weather, "scale"), 1.0, "{tag} {id}");
            let icon = l.part(id, "MaterialSymbol");
            assert_eq!(l.num(icon, "iconSize"), (80.0 * zoom).round(), "{tag} {id}");
            if tag == "weather-free" {
                let side = 200.0 * zoom;
                assert_eq!(
                    (l.num(weather, "width"), l.num(weather, "height")),
                    (side, side),
                    "{tag} {id}"
                );
                let widget = l.widget(id);
                assert_eq!((widget.width, widget.height), (side, side), "{tag} {id}");
                assert_eq!(l.texts_outside(id), Vec::<String>::new(), "{tag} {id}");
            }
        }
    }
}

#[test]
fn a_sidebar_widget_narrower_than_the_sidebar_is_a_cell_of_the_grid() {
    let l = load("sidebar", Some(MPRIS));
    assert!(l.own_errors().is_empty(), "{:#?}", l.own_errors());
    // The sidebar is 420 wide: SidebarLayoutContent's `gridCellWidth` for five columns.
    let (cell, spacing) = ((420.0 - 6.0 * 2.0 - 6.0 * 5.0) / 5.0, 6.0);
    for (id, _, n) in CELLS {
        let widget = l.widget(id);
        let n = f64::from(n);
        let width = cell * n + spacing * (n - 1.0);
        // The first cell of a row takes what the row has left, as a toggle does.
        assert!(
            widget.width > width - 1.0 && widget.width < width + cell,
            "{id}: {} wide, not {width}",
            widget.width
        );
        // The month and the list keep their shape, scaled; everything else is a toggle high.
        if id == "side-calendar-3" || id == "side-notes-3" {
            assert!(widget.height > 56.0, "{id}: {} high", widget.height);
        } else {
            assert_eq!(widget.height, 56.0, "{id}");
        }
    }
    assert_eq!(
        l.text(l.part("side-media-1", "MediaCard"), "form"),
        "circle"
    );
    assert_eq!(l.text(l.part("side-media-3", "MediaCard"), "form"), "row");
    assert_eq!(l.texts_outside("side-media-3"), Vec::<String>::new());
    assert!(l.flag(l.part("side-resources-1", "ResourcesCard"), "ringsOnly"));
    // The month and the list, scaled, are as wide as their cells at most.
    for (id, part) in [
        ("side-calendar-3", "CalendarWidget"),
        ("side-notes-3", "TodoWidget"),
    ] {
        let widget = l.widget(id);
        // The sidebar's card is a CalendarWidget too: the month is the one below it.
        let w = l.rt.world.borrow();
        let inner: Vec<usize> = below(&w, widget.item)
            .into_iter()
            .filter(|&i| i != widget.item && is(&w, i, part))
            .collect();
        drop(w);
        assert_eq!(inner.len(), 1, "{id}: one {part}");
        let scale = l.num(inner[0], "scale");
        assert!(scale < 1.0, "{id}: scale {scale}");
        assert!(
            l.num(inner[0], "width") * scale <= widget.width + 0.5,
            "{id}"
        );
    }
}
