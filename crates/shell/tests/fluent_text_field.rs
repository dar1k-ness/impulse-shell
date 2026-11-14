//! `import QtQuick.Controls.FluentWinUI3` TextField (the waffle polkit password field): Qt's
//! dark textfield 9-patch background, its paddings, the placeholder (in the colour of the
//! palette, as the style's file says) and the 2px focus stroke.

use qml::runtime::Runtime;
use qsg::raster::Canvas;
use shell::keys::Keyboard;
use shell::scene::{self, Target};

const QML: &str = r##"
import QtQuick
import QtQuick.Controls
import QtQuick.Controls.FluentWinUI3 as Fluent
import Rustshell
FloatingWindow {
    width: 200; height: 60
    color: "#2c2c2c"
    Fluent.TextField { x: 10; y: 10; width: 180; placeholderText: "Password" }
}
"##;

fn paint(rt: &mut Runtime, fonts: &mut scene::Fonts, win: usize) -> Canvas {
    for _ in 0..3 {
        rt.settle();
        scene::layout_window(rt, fonts, win, [200.0, 60.0]);
    }
    let mut c = Canvas::new(200, 60, [0, 0, 0, 0]);
    scene::paint_window(rt, fonts, win, &mut Target::Cpu(&mut c));
    c
}

fn gray(c: &Canvas, x: usize, y: usize) -> u8 {
    c.px[y * c.w + x][1]
}

#[test]
fn fluent_text_field_draws_the_style_background() {
    const TEST: &str = concat!(
        module_path!(),
        "::fluent_text_field_draws_the_style_background"
    );
    if !testenv::in_own_process(TEST) {
        // The generic platform theme (accent #308cc6).
        testenv::run_in_own_process_env(
            TEST,
            std::iter::empty::<(&str, &str)>(),
            &[
                "QT_QPA_PLATFORMTHEME",
                "XDG_CURRENT_DESKTOP",
                "KDE_SESSION_VERSION",
            ],
        );
        return;
    }
    // Imported QML module types resolve for a file under a config root (not `load_str`).
    let dir = std::env::temp_dir().join(format!("ri-fluent-tf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("shell.qml"), QML).unwrap();
    let mut rt = Runtime::new(Some(dir.clone()));
    let win = rt.load_file(&dir.join("shell.qml")).unwrap();
    let tf = rt.world.borrow_mut().of_type("TextField")[0];
    let mut fonts = scene::Fonts::default();
    let c = paint(&mut rt, &mut fonts, win);
    assert_eq!(rt.get(tf, "height").to_number(), 30.0);
    assert_eq!(rt.get(tf, "leftPadding").to_number(), 12.0);
    // palette.placeholderText of the style in use, Basic (QtQuick.Controls is imported first).
    assert_eq!(
        rt.get(tf, "placeholderTextColor").to_js_string(),
        "#88353637"
    );
    // #2c2c2c under white at alpha 19 (border), 15 (fill) and 138 (unfocused bottom line).
    assert!(
        (58..=61).contains(&gray(&c, 100, 10)),
        "border {}",
        gray(&c, 100, 10)
    );
    assert!(
        (55..=57).contains(&gray(&c, 150, 25)),
        "fill {}",
        gray(&c, 150, 25)
    );
    assert!(
        (156..=160).contains(&gray(&c, 100, 39)),
        "bottom {}",
        gray(&c, 100, 39)
    );
    assert_eq!(gray(&c, 100, 41), 44);
    // The placeholder is drawn left-padded inside the field, a little darker than the fill
    // (#353637 at alpha 136, the light Basic palette's, as Qt draws it under an unknown scheme).
    let fill = gray(&c, 150, 25);
    assert!(
        (22..80).any(|x| (15..35).any(|y| gray(&c, x, y) < fill)),
        "no placeholder"
    );

    let mut kb = Keyboard::default();
    kb.win = Some(win);
    kb.sync(&mut rt);
    rt.world.borrow_mut().force_active_focus(tf);
    kb.sync(&mut rt);
    let c = paint(&mut rt, &mut fonts, win);
    for y in [38, 39] {
        assert_eq!(
            c.px[y * c.w + 100][..3],
            [0x30, 0x8c, 0xc6],
            "focus stroke row {y}"
        );
    }
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    std::fs::remove_dir_all(&dir).unwrap();
}
