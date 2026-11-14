//! The edit overlay's input path end to end inside the shell: wl_pointer presses, motions and
//! releases reach the overlay's MouseArea and through it the native EditController, and
//! keys reach it through the focused item's `Keys.onPressed`, as in
//! qml/modules/rusty/editMode/EditOverlay.qml. A drag moves a desktop widget and writes
//! layout.toml (the widget follows the pointer before that), Esc mid-drag cancels it, Ctrl+Z undoes, Esc leaves edit mode.
#![cfg(feature = "interp")]

use qml::edit_mode::WinRect;
use qml::runtime::Runtime;
use qml::value::Value;
use shell::{input::Pointer, keys::Keyboard, scene};
use std::path::PathBuf;

const BTN_LEFT: u32 = 0x110;
// XKB keysyms, and Qt::ControlModifier as the keyboard reports it.
const KEY_ESC: u32 = 0xff1b;
const KEY_Z: u32 = 0x7a;
const KEY_RIGHT: u32 = 0xff53;
const CTRL: u32 = 0x0400_0000;
const SHIFT: u32 = 0x0200_0000;

const MANIFEST: &str = r#"
[[widget]]
type = "clock"
name = "Clock"
icon = "schedule"
placements = ["desktop"]
component = "W.qml"
size = { min = [40, 20], max = [800, 400], default = [200, 100] }
"#;

const LAYOUT: &str = r#"version = 5

[[monitor]]
connector = "DP-1"
primary = true

[[monitor.container]]
id = "desk"
kind = "desktop"

[[monitor.container.widget]]
id = "clock"
type = "clock"
x = 100
y = 100
w = 200
h = 100
"#;

// The background (desktop container with its hosts) and the overlay, wired like
// EditOverlay.qml: one MouseArea forwarding the pointer, its Keys forwarding keys.
const MAIN: &str = r#"import QtQuick
import Impulse
Item {
    id: root
    width: 800
    height: 600
    property var ctl: controller
    property var overlay: overlayWin
    property var background: bg
    property var desk: desk
    property bool inputFocus: inputArea.activeFocus
    Item {
        id: bg
        width: 800
        height: 600
        DesktopContainer {
            id: desk
            connector: "DP-1"
            width: 800
            height: 600
            Repeater {
                model: desk.widgets
                WidgetHost {
                    required property var modelData
                    layoutId: modelData.id
                    widgetType: modelData.type
                    x: modelData.x
                    y: modelData.y
                    width: modelData.width
                    height: modelData.height
                }
            }
        }
    }
    Item {
        id: overlayWin
        width: 800
        height: 600
        EditController {
            id: controller
            connector: "DP-1"
        }
        MouseArea {
            id: inputArea
            anchors.fill: parent
            focus: true
            onPressed: mouse => controller.press(mouse.x, mouse.y)
            onPositionChanged: mouse => controller.move(mouse.x, mouse.y)
            onReleased: mouse => controller.release(mouse.x, mouse.y)
            onCanceled: controller.cancel()
            Keys.onPressed: event => {
                event.accepted = controller.key(event.key, event.modifiers);
            }
        }
    }
    function startEditing(): bool { return controller.start() }
}
"#;

fn item(rt: &Runtime, root: usize, k: &str) -> usize {
    match rt.world.borrow_mut().read(root, k) {
        Some(Value::Item(i)) => i,
        v => panic!("{k}: {v:?}"),
    }
}

fn field(v: &Value, k: &str) -> Value {
    match v {
        Value::Obj(o) => o.borrow().props.get(k).cloned().unwrap_or_default(),
        v => panic!("{k} of {v:?}"),
    }
}

fn layout_x(rt: &Runtime) -> i32 {
    let w = rt.world.borrow();
    let l = w.layout.as_ref().unwrap().editor().layout();
    match l.monitors[0].containers[0].widgets[0].place {
        qml::layout_model::Place::Desktop { x, .. } => x,
        _ => panic!(),
    }
}

#[test]
fn overlay_input_drags_cancels_undoes_and_exits() {
    let dir = std::env::temp_dir().join(format!("edit-overlay-input-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("widgets.toml"), MANIFEST).unwrap();
    std::fs::write(dir.join("W.qml"), "import QtQuick\nItem {}\n").unwrap();
    std::fs::write(dir.join("main.qml"), MAIN).unwrap();
    std::fs::write(dir.join("layout.toml"), LAYOUT).unwrap();
    let mut rt = Runtime::new(Some(PathBuf::from(&dir)));
    rt.world.borrow_mut().open_layout(dir.join("layout.toml"));
    let root = rt.load_file(&dir.join("main.qml")).unwrap();
    rt.settle();
    let mut fonts = scene::Fonts::default();
    scene::layout_window(&mut rt, &mut fonts, root, [800.0, 600.0]);
    let ctl = item(&rt, root, "ctl");
    // One top-level item stands for both surfaces here.
    rt.world.borrow_mut().window_rects.insert(
        root,
        WinRect {
            connector: "DP-1".into(),
            x: 0.0,
            y: 0.0,
            w: 800.0,
            h: 600.0,
        },
    );
    rt.call_method(root, "startEditing", vec![])
        .unwrap()
        .unwrap();
    rt.settle();
    let read = |rt: &Runtime, k: &str| {
        rt.world
            .borrow_mut()
            .read(ctl, k)
            .unwrap_or(Value::Undefined)
    };
    assert!(read(&rt, "active").truthy());

    let mut ptr = Pointer::default();
    let mut kb = Keyboard::default();
    kb.win = Some(root);
    let drag =
        |rt: &mut Runtime, ptr: &mut Pointer, from: (f64, f64), to: (f64, f64), release: bool| {
            ptr.enter(rt, root, from.0, from.1);
            ptr.motion(rt, root, from.0, from.1);
            ptr.press(rt, BTN_LEFT);
            for k in 1..=8 {
                let t = k as f64 / 8.0;
                ptr.motion(
                    rt,
                    root,
                    from.0 + (to.0 - from.0) * t,
                    from.1 + (to.1 - from.1) * t,
                );
                rt.settle();
            }
            if release {
                ptr.release(rt, BTN_LEFT);
            }
            rt.settle();
        };

    // A drag moves the clock by the pointer's travel.
    drag(&mut rt, &mut ptr, (150.0, 150.0), (250.0, 200.0), true);
    assert_eq!(layout_x(&rt), 200);
    assert_eq!(read(&rt, "selectedId").to_js_string(), "clock");
    let saved = std::fs::read_to_string(dir.join("layout.toml")).unwrap();
    assert!(saved.contains("x = 200"), "{saved}");

    // Esc mid-drag: the ghost goes away and the release changes nothing.
    drag(&mut rt, &mut ptr, (250.0, 200.0), (450.0, 400.0), false);
    assert!(read(&rt, "dragging").truthy());
    // Moving on mid-drag moves the ghost and leaves `frames` as it was, so the overlay's
    // Repeater keeps its delegates.
    let (frames, ghost) = (read(&rt, "frames"), read(&rt, "ghost"));
    ptr.motion(&mut rt, root, 400.0, 350.0);
    rt.settle();
    assert!(read(&rt, "frames").strict_eq(&frames));
    assert!(!read(&rt, "ghost").strict_eq(&ghost));
    // An accepted drag shows no "not allowed" badge.
    assert!(matches!(read(&rt, "refusal"), Value::Null));
    // The clock itself follows the drag on the desktop, where the ghost lands; nothing is
    // written until the drop.
    let desk = item(&rt, root, "desk");
    let shown_x = |rt: &Runtime| {
        let shown = rt.world.borrow_mut().read(desk, "shown").unwrap();
        let clock = field(&shown, "clock");
        field(&clock, "x").to_number()
    };
    let ghost_x = field(&read(&rt, "ghost"), "x").to_number();
    assert_eq!(shown_x(&rt), ghost_x);
    assert_ne!(shown_x(&rt), 200.0);
    assert_eq!(layout_x(&rt), 200);
    kb.sync(&mut rt);
    assert!(
        rt.world
            .borrow_mut()
            .read(root, "inputFocus")
            .is_some_and(|v| v.truthy())
    );
    kb.key(&mut rt, KEY_ESC, None, true, false);
    kb.key(&mut rt, KEY_ESC, None, false, false);
    rt.settle();
    assert!(!read(&rt, "dragging").truthy());
    assert_eq!(shown_x(&rt), 200.0);
    ptr.release(&mut rt, BTN_LEFT);
    rt.settle();
    assert_eq!(layout_x(&rt), 200);
    assert!(read(&rt, "active").truthy());

    // Ctrl+Z puts the clock back.
    kb.mods = CTRL;
    kb.key(&mut rt, KEY_Z, Some("z"), true, false);
    kb.key(&mut rt, KEY_Z, None, false, false);
    kb.mods = 0;
    rt.settle();
    assert_eq!(layout_x(&rt), 100);

    // One Shift+Right press and release nudges the selected clock once, by 10 px; each
    // auto-repeat of the held key nudges it once more.
    kb.mods = SHIFT;
    kb.key(&mut rt, KEY_RIGHT, None, true, false);
    rt.settle();
    assert_eq!(layout_x(&rt), 110);
    kb.key(&mut rt, KEY_RIGHT, None, false, false);
    rt.settle();
    assert_eq!(layout_x(&rt), 110);
    kb.key(&mut rt, KEY_RIGHT, None, true, false);
    kb.auto_repeat(&mut rt, KEY_RIGHT, None);
    kb.key(&mut rt, KEY_RIGHT, None, false, false);
    kb.mods = 0;
    rt.settle();
    assert_eq!(layout_x(&rt), 130);

    // Esc with no drag leaves edit mode.
    kb.key(&mut rt, KEY_ESC, None, true, false);
    kb.key(&mut rt, KEY_ESC, None, false, false);
    rt.settle();
    assert!(!read(&rt, "active").truthy());
}
