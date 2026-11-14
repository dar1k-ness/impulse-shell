//! Qt5Compat's effects are QML files: a property the document never sets holds what the file
//! binds it to, and follows it, as in Qt 6.12.
//!
//! The expected text is what Qt 6.12.0 gives for the same document (PySide6 6.12.0,
//! QQmlComponent, `Qt5Compat.GraphicalEffects` of the image's Qt): GaussianBlur.qml and
//! DropShadowBase.qml have `samples: 9`, `radius: Math.floor(samples / 2)`, GaussianBlur.qml
//! `deviation: (radius + 1.0) / 3.3333`, RadialGradient.qml `horizontalRadius: width`,
//! `verticalRadius: height` and a `gradient` of a white and a black stop.
#![cfg(feature = "interp")]

use qml::runtime::Runtime;

const SRC: &str = r#"import QtQuick
import Qt5Compat.GraphicalEffects
Item {
    id: root
    property string result
    property string log: ""
    GaussianBlur { id: g; samples: 17 }
    GaussianBlur { id: g2; radius: 10 }
    GaussianBlur { id: g3; onRadiusChanged: root.log += "r" + radius + " "; onDeviationChanged: root.log += "d" + deviation.toFixed(4) + " " }
    DropShadow { id: d; samples: 25 }
    DropShadow { id: d2 }
    RadialGradient { id: r; width: 100; height: 40; onHorizontalRadiusChanged: root.log += "h" + horizontalRadius + " " }
    RadialGradient { id: r2; width: 100; height: 40; horizontalRadius: 7 }
    property real follows: g3.radius * 2
    Component.onCompleted: {
        var out = []
        out.push("g " + g.radius + " " + g.deviation.toFixed(4))
        out.push("g2 " + g2.samples + " " + g2.deviation.toFixed(4))
        out.push("d " + d.radius + " " + d.transparentBorder + " " + d.color + " " + d.fast)
        out.push("d2 " + d2.radius + " " + d2.samples)
        out.push("r " + r.horizontalRadius + " " + r.verticalRadius)
        out.push("r2 " + r2.horizontalRadius + " " + r2.verticalRadius)
        r.width = 60
        out.push("r " + r.horizontalRadius)
        var s = r.gradient.stops
        out.push("stops " + s.length + " " + s[0].position + " " + s[0].color + " " + s[1].position + " " + s[1].color)
        out.push("same " + (r.gradient === r.gradient) + " " + (r.gradient === r2.gradient) + " " + r.gradient.orientation)
        out.push("follows " + follows)
        g3.samples = 21
        out.push("follows " + follows + " " + g3.radius + " " + g3.deviation.toFixed(4))
        out.push("log " + log)
        g3.radius = 3
        g3.samples = 31
        out.push("after write " + g3.radius + " " + g3.deviation.toFixed(4))
        out.push("log " + log)
        result = out.join("\n")
    }
}
"#;

/// What Qt gives, less `h100 r4 d1.5000 ` at the start of both logs: there the file's
/// bindings are evaluated for the first time, which changes the three properties from the
/// zero they start as. Here they hold the value from the start and no signal is sent
/// (docs/qt-differences.md).
const QT: &str = "g 8 2.7000
g2 9 3.3000
d 12 true #000000 false
d2 4 9
r 100 40
r2 7 40
r 60
stops 2 0 #ffffff 1 #000000
same true false 2
follows 8
follows 20 10 3.3000
log h60 r10 d3.3000 
after write 3 1.2000
log h60 r10 d3.3000 r3 d1.2000 ";

#[test]
fn an_effect_property_nothing_set_follows_what_its_file_binds_it_to() {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(SRC).unwrap();
    rt.settle();
    assert_eq!(rt.world.borrow().errors, Vec::<String>::new());
    assert_eq!(rt.get(root, "result").to_js_string(), QT);
}

/// The same document with all Qt prints: `h100 r4 d1.5000 ` starts both logs.
const QT_WITH_FIRST_SIGNALS: &str = "g 8 2.7000
g2 9 3.3000
d 12 true #000000 false
d2 4 9
r 100 40
r2 7 40
r 60
stops 2 0 #ffffff 1 #000000
same true false 2
follows 8
follows 20 10 3.3000
log h100 r4 d1.5000 h60 r10 d3.3000 
after write 3 1.2000
log h100 r4 d1.5000 h60 r10 d3.3000 r3 d1.2000 ";

/// Which signals Qt sends when the effects are created, and in what order: the last effect
/// created is first, and a value that stays 0 sends none (`d0`, `r0`).
const FIRST_SIGNALS: &str = r#"
import QtQuick
import Qt5Compat.GraphicalEffects
Item {
    id: root
    property string log: ""
    DropShadow { id: d; onRadiusChanged: root.log += "dr" + radius + " "; onSamplesChanged: root.log += "ds" + samples + " " }
    DropShadow { id: d0; samples: 1; onRadiusChanged: root.log += "d0r" + radius + " " }
    GaussianBlur { id: g; onRadiusChanged: root.log += "gr" + radius + " "; onDeviationChanged: root.log += "gd" + deviation.toFixed(4) + " "; onSamplesChanged: root.log += "gs" + samples + " " }
    RadialGradient { id: r; width: 100; height: 40; onHorizontalRadiusChanged: root.log += "h" + horizontalRadius + " "; onVerticalRadiusChanged: root.log += "v" + verticalRadius + " "; onGradientChanged: root.log += "grad " }
    RadialGradient { id: r0; onHorizontalRadiusChanged: root.log += "h0 "; onVerticalRadiusChanged: root.log += "v0 " }
    Component.onCompleted: { root.log += "| "; d.samples = 21; g.samples = 21; r.height = 10 }
}
"#;

const QT_FIRST_SIGNALS: &str = "h100 v40 gr4 gd1.5000 dr4 | ds21 dr10 gs21 gr10 gd3.3000 v10 ";

/// Red: the signals Qt sends when an effect is created are not sent here, the row "The change
/// signals of a property the effect's file binds, when the effect is created" of
/// docs/qt-differences.md, section "`RadialGradient` and the unset properties of the Qt5Compat
/// effects". The work that removes that row starts by taking the `#[ignore]` away.
#[test]
#[ignore = "red, a recorded difference from Qt 6.12: docs/qt-differences.md, the change signals of a Qt5Compat effect when it is created"]
fn an_effect_sends_the_signals_of_its_files_bindings_when_it_is_created() {
    let mut rt = Runtime::new(None);
    let root = rt.load_str(SRC).unwrap();
    rt.settle();
    assert_eq!(rt.world.borrow().errors, Vec::<String>::new());
    assert_eq!(rt.get(root, "result").to_js_string(), QT_WITH_FIRST_SIGNALS);
    let mut rt = Runtime::new(None);
    let root = rt.load_str(FIRST_SIGNALS).unwrap();
    rt.settle();
    assert_eq!(rt.world.borrow().errors, Vec::<String>::new());
    assert_eq!(rt.get(root, "log").to_js_string(), QT_FIRST_SIGNALS);
}
