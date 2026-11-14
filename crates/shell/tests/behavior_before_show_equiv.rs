//! A Behavior animates a change made as its window is shown for the first time: Qt arms a
//! Behavior when its component completes, whether or not the window was ever shown. The waffle
//! task preview slides in that way (`TaskPreview.qml`: a PopupWindow made hidden, a Behavior on
//! a property bound to its `visible`), the tray menu fades in. The native shell arms a Behavior
//! once its item went through a layout, so it lays hidden windows out in its idle time, of
//! every kind and whenever they were made. The real quickshell running the fixture vs the
//! native shell (`rusty-impulse -p`): the first value each property changes to after the window was
//! shown.
use std::path::Path;
use std::process::{Child, Command, Stdio};

use testenv::Log;

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The window is made hidden, at the start or 300 ms later (`LATE`), and shown 700 ms after
/// it was made. `bound` follows its `visible`, `set` is written in the handler that shows it.
/// The line has the first value each of them changed to and whether the window is still
/// open, once `set` is at its end.
const SHELL: &str = r#"import QtQuick
import Quickshell

Scope {
    id: root
    property var first: ({})
    function note(name, value) {
        if (first[name] === undefined)
            first[name] = value
        // The compositor may dismiss a popup, and `bound` then turns back: `set` ends.
        if (ld.item.it.set == 100) {
            console.log("BEH " + first.bound + " " + first.set + " " + ld.item.visible)
            Qt.quit()
        }
    }
    FloatingWindow { id: host; visible: true; implicitWidth: 200; implicitHeight: 200 }
    Loader {
        id: ld
        active: !LATE
        onLoaded: show.start()
        sourceComponent: KIND {
            id: win
            ANCHOR
            visible: false
            implicitWidth: 120
            implicitHeight: 120
            property alias it: it
            Item {
                id: it
                width: 50; height: 50
                property real bound: win.visible ? 100 : 0
                property real set: 0
                Behavior on bound { NumberAnimation { duration: 400 } }
                Behavior on set { NumberAnimation { duration: 400 } }
                onBoundChanged: root.note("bound", bound)
                onSetChanged: root.note("set", set)
            }
        }
    }
    Timer { interval: 300; running: true; onTriggered: ld.active = true }
    Timer {
        id: show
        interval: 700
        onTriggered: { ld.item.it.set = 100; ld.item.visible = true }
    }
}
"#;

/// The first values `bound` and `set` changed to after the window was shown, or none when
/// `bound` did not change at all and the window is not open.
fn shown(bin: &str, dir: &Path, kind: &str, late: bool) -> Option<(f64, f64)> {
    let anchor = match kind {
        "PopupWindow" => "anchor.window: host",
        _ => "",
    };
    let qml = (SHELL.replace("KIND", kind))
        .replace("ANCHOR", anchor)
        .replace("LATE", if late { "true" } else { "false" });
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("shell.qml"), qml).unwrap();
    let (r, w) = std::io::pipe().unwrap();
    let mut c = Command::new(bin);
    c.arg("-p")
        .arg(dir)
        .env("RI_INTERP", "1")
        .env("QS_NO_RELOAD_POPUP", "1")
        .stdin(Stdio::null())
        .stdout(w.try_clone().unwrap())
        .stderr(w);
    let _k = Kill(c.spawn().unwrap());
    drop(c);
    let log = Log::capture(r);
    let line =
        |s: &str| (s.lines()).find_map(|l| l.find("BEH ").map(|i| l[i + 4..].trim().to_string()));
    if let Err(s) = log.wait(|s| line(s).is_some()) {
        panic!("{bin}: {kind} exited before `set` was at its end; log:\n{s}");
    }
    let text = log.text();
    let line = line(&text).unwrap();
    let odd = || -> ! { panic!("{bin}: {kind} printed `BEH {line}`; log:\n{text}") };
    let number = |s: &str| s.parse::<f64>().unwrap_or_else(|_| odd());
    match line.split(' ').collect::<Vec<_>>()[..] {
        ["undefined", set, "false"] => {
            number(set);
            None
        }
        [bound, set, "true" | "false"] => Some((number(bound), number(set))),
        _ => odd(),
    }
}

/// How often a shell is started for a popup that was closed before `bound` changed.
const TRIES: usize = 5;

/// [`shown`] of a run in which the window was open as `bound` changed. Sway closes a popup
/// when another window opens, and the one sway of the tests has the windows of the tests
/// beside this one. A window that opens as the popup is shown closes it before `bound`
/// changed, under either shell (`BEH undefined 4 false`): with ten of these tests at once
/// beside eight busy loops, 6 starts of Quickshell in 206 and 1 of the native shell in 201.
/// Such a run shows nothing of the Behavior, so the shell is started again. A window of
/// another kind is not closed by anyone, and a `bound` that did not change in a window still
/// open is not such a run.
fn first_shown(bin: &str, dir: &Path, kind: &str, late: bool) -> (f64, f64) {
    let tries = if kind == "PopupWindow" { TRIES } else { 1 };
    for _ in 0..tries {
        if let Some(first) = shown(bin, dir, kind, late) {
            return first;
        }
    }
    panic!(
        "{bin}: `bound` did not change and the {kind} was not open as `set` came to its end, \
         in {tries} of {tries} starts"
    );
}

fn animates_as_in_quickshell(kind: &str, late: bool) {
    if !testenv::require("qs") {
        return;
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        testenv::unavailable("no Wayland compositor (WAYLAND_DISPLAY)");
        return;
    }
    let base = tempfile::tempdir().unwrap();
    let qt = first_shown("qs", &base.path().join("orig"), kind, late);
    let ours = first_shown(
        env!("CARGO_BIN_EXE_rusty-impulse"),
        &base.path().join("native"),
        kind,
        late,
    );
    // The animations step by the wall clock, so the first value on the way differs from run
    // to run under both: what is compared is that there is one, short of the end.
    let on_the_way = |(bound, set): (f64, f64)| (bound < 100.0, set < 100.0);
    assert_eq!(
        on_the_way(qt),
        (true, true),
        "Quickshell animates both values of a {kind} as it is shown: {qt:?}"
    );
    assert_eq!(
        on_the_way(ours),
        on_the_way(qt),
        "{kind}: the first values after it was shown, here {ours:?}, under Quickshell {qt:?}"
    );
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_behavior_animates_as_a_popup_is_first_shown() {
    animates_as_in_quickshell("PopupWindow", false);
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_behavior_animates_as_a_floating_window_is_first_shown() {
    animates_as_in_quickshell("FloatingWindow", false);
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_behavior_animates_as_a_panel_is_first_shown() {
    animates_as_in_quickshell("PanelWindow", false);
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_behavior_animates_as_a_panel_made_later_is_first_shown() {
    animates_as_in_quickshell("PanelWindow", true);
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_behavior_animates_as_a_popup_made_later_is_first_shown() {
    animates_as_in_quickshell("PopupWindow", true);
}
