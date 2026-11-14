//! `Rectangle.antialiasing` as QML reads it, against Qt itself. Left unset it is QQuickItem's
//! implicit value, which QQuickRectangle turns on for a positive `radius` or set corner radius
//! (maybeSetImplicitAntialiasing); a value QML sets stands until it is reset with `undefined`.

use shell::scene;

/// Rectangles whose `antialiasing` QML reads: `state()` lists it for the declared ones,
/// `heard` collects their change signals.
const VALUES: &str = r#"
    id: root; width: 200; height: 200
    property string log
    property string heard
    property real five: 5
    function say(what) { log += what + "\n" }
    // Each object's signals apart, in the order it emitted them: where a signal of one object
    // changes a binding of another, Qt notifies that one inside the emission and we do after it.
    function told() {
        var all = heard.split(" ").filter(s => s.length > 0)
        var of = s => s.split(/[.=]/)[0]
        var names = all.map(of).filter((n, i, a) => a.indexOf(n) === i).sort()
        say("  heard " + names.map(n => all.filter(s => of(s) === n).join(" ")).join(" | "))
        heard = ""
    }
    component Rect: Rectangle { width: 40; height: 40 }
    // Declared after its target, it completes and so connects before it: Qt's signal
    // handlers of the rectangle itself would also hear the values set while it is created.
    component Ear: Connections {
        function onAntialiasingChanged() { root.heard += target.objectName + "=" + target.antialiasing + " " }
        function onRadiusChanged() { root.heard += target.objectName + ".r=" + target.radius + " " }
        function onTopLeftRadiusChanged() { root.heard += target.objectName + ".tl=" + target.topLeftRadius + " " }
    }
    // Read by a binding that is declared before the radius it follows is bound.
    property bool early: late.antialiasing
    Rect { id: plain; objectName: "plain" }
    Rect { id: round; objectName: "round"; radius: 5 }
    Rect { id: off; objectName: "off"; radius: 5; antialiasing: false }
    Rect { id: on; objectName: "on"; antialiasing: true }
    Rect { id: corner; objectName: "corner"; topLeftRadius: 5 }
    Rect { id: sharp; objectName: "sharp"; radius: 5; topLeftRadius: 0; topRightRadius: 0 }
    Rect { id: below; objectName: "below"; radius: -3 }
    Rect { id: zero; objectName: "zero"; bottomLeftRadius: 0 }
    Rect { id: late; objectName: "late"; radius: root.five }
    Rect { id: follows; objectName: "follows"; antialiasing: plain.antialiasing }
    Rect { id: own; objectName: "own"; radius: 2; property bool seen: antialiasing }
    Ear { target: plain } Ear { target: round } Ear { target: off } Ear { target: on }
    Ear { target: corner } Ear { target: sharp } Ear { target: below } Ear { target: zero }
    Ear { target: late } Ear { target: follows } Ear { target: own }
    property bool bound: plain.antialiasing
    Component {
        id: maker
        Rect {
            objectName: "made"
            onAntialiasingChanged: root.heard += "made=" + antialiasing + " "
            onRadiusChanged: root.heard += "made.r=" + radius + " "
        }
    }
    function read(r) { return r.objectName + ":" + r.antialiasing }
    function state() {
        return [plain, round, off, on, corner, sharp, below, zero, late, follows, own].map(read).join(" ")
                + " bound " + bound + " early " + early + " seen " + own.seen
    }
    Component.onCompleted: {
        say("completed " + state()); told()
        var made = maker.createObject(root); say("made " + read(made))
        made.radius = 3; say("made radius 3 " + read(made))
        made.radius = 0; say("made radius 0 " + read(made))
        var given = maker.createObject(root, { radius: 4 }); say("given radius 4 " + read(given))
        var set = maker.createObject(root, { antialiasing: true }); say("given antialiasing " + read(set))
        told()
    }
    function step() {
        say("shown " + state()); told()
        plain.radius = 4; say("plain radius 4 " + state())
        plain.radius = 6; say("plain radius 6 " + read(plain))
        plain.radius = 0; say("plain radius 0 " + state())
        plain.radius = -2; say("plain radius -2 " + read(plain))
        plain.topLeftRadius = 2; say("plain topLeftRadius 2 " + read(plain))
        plain.topLeftRadius = 0; say("plain topLeftRadius 0 " + read(plain))
        plain.bottomRightRadius = 1; say("plain bottomRightRadius 1 " + read(plain))
        plain.bottomRightRadius = undefined; say("plain bottomRightRadius undefined " + read(plain))
        plain.radius = 3; say("plain radius 3 " + read(plain))
        plain.topLeftRadius = undefined; plain.radius = 0; say("plain reset " + read(plain))
        told()
        plain.antialiasing = true; say("plain antialiasing true " + state())
        plain.antialiasing = true; plain.radius = 2; say("plain true, radius 2 " + read(plain))
        plain.antialiasing = undefined; say("plain antialiasing undefined " + read(plain))
        plain.antialiasing = undefined; plain.radius = 0; say("plain undefined, radius 0 " + state())
        told()
        round.antialiasing = true; say("round antialiasing true " + read(round))
        round.radius = 0; say("round true, radius 0 " + read(round))
        round.antialiasing = undefined; say("round antialiasing undefined " + read(round))
        round.antialiasing = false; say("round antialiasing false " + read(round))
        round.radius = 7; say("round false, radius 7 " + read(round))
        round.antialiasing = undefined; say("round antialiasing undefined " + read(round))
        told()
        off.antialiasing = undefined; say("off antialiasing undefined " + read(off))
        off.radius = 0; say("off radius 0 " + read(off))
        on.antialiasing = false; on.radius = 3; say("on false, radius 3 " + read(on))
        on.antialiasing = undefined; say("on antialiasing undefined " + read(on))
        told()
        corner.topLeftRadius = 0; say("corner topLeftRadius 0 " + read(corner))
        sharp.radius = 0; say("sharp radius 0 " + read(sharp))
        below.radius = 1; say("below radius 1 " + read(below))
        zero.bottomLeftRadius = 3; zero.bottomLeftRadius = -1; say("zero -1 " + read(zero))
        five = 0; say("five 0 " + state())
        five = 2; say("five 2 " + state())
        own.radius = 0; say("own radius 0 " + state())
        told()
    }
"#;

/// What `VALUES` logs in Qt and in our runtime.
fn value_logs() -> [String; 2] {
    let dir = tempfile::tempdir().unwrap();
    let at = |n: &str| dir.path().join(n);
    // Qt runs step() at the first frame on screen and prints the log.
    let frame = "property bool stepped: false\n    function frame() {\n        if (stepped)\n            \
                 return;\n        stepped = true;\n        root.step();\n        \
                 console.log(\"LOG\\n\" + root.log + \"END\");\n        Qt.quit();\n    }\n";
    let qt = format!(
        "import QtQuick\nItem {{{VALUES}    {frame}    Connections {{\n        \
         target: root.Window.window\n        function onFrameSwapped() {{ root.frame() }}\n    \
         }}\n}}\n"
    );
    std::fs::write(at("qt.qml"), qt).unwrap();
    std::fs::write(
        at("ours.qml"),
        format!("import QtQuick\nItem {{{VALUES}}}\n"),
    )
    .unwrap();
    let out = std::process::Command::new("qml6")
        .arg(at("qt.qml"))
        .env("QT_QPA_PLATFORM", "wayland-egl")
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        .env("QT_FORCE_STDERR_LOGGING", "1")
        .env("XDG_CACHE_HOME", dir.path())
        .output()
        .unwrap();
    let printed = String::from_utf8_lossy(&out.stderr);
    let qt = printed
        .split_once("LOG\n")
        .and_then(|(_, l)| l.split_once("END"))
        .unwrap_or_else(|| panic!("qml6 printed:\n{printed}"))
        .0
        .to_string();
    let mut rt = qml::runtime::Runtime::new(Some(dir.path().to_path_buf()));
    let win = rt.load_file(&at("ours.qml")).expect("scene loads");
    let mut fonts = scene::Fonts::default();
    rt.settle();
    scene::layout_window(&mut rt, &mut fonts, win, [200.0, 200.0]);
    rt.settle();
    rt.call_method(win, "step", Vec::new())
        .expect("step()")
        .expect("step() runs");
    let ours = match rt.world.borrow_mut().read(win, "log") {
        Some(qml::value::Value::Str(s)) => s.to_string(),
        v => panic!("log is {v:?}"),
    };
    [qt, ours]
}

#[test]
fn rectangle_antialiasing_reads_like_qt() {
    if !testenv::require("qml6") {
        return;
    }
    let [qt, ours] = value_logs();
    assert!(
        qt.contains("round:true") && qt.contains("plain:false"),
        "Qt logged:\n{qt}"
    );
    assert_eq!(ours, qt, "ours, then Qt's");
}
