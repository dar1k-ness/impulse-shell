//! A reload shows, and shows one image at a time. What is above the wallpaper fades away
//! before the `exec` (wp_alpha_modifier_v1), the windows the next image does not start with
//! (a sidebar or a menu opened since the start, the one the reload was asked from) with it:
//! those leave at the `exec`, nothing takes their place. The windows the next image makes
//! above the wallpaper come as at a start, seen from their first frame: the compositor
//! animates them in and the tree's own first animations show. The wallpaper's window does
//! not fade: the old one stays until the new one, unseen while it draws, takes its place in
//! one frame. The tree hears `reloadCompleted` when its windows are all there, and on
//! Hyprland the compositor animates the new windows in and none of the old ones away.
//!
//! The test runs its own sway: the panels of tests running meanwhile would sit at the same
//! spot of the shared one's screen the screenshots read.

use std::path::Path;
use std::process::{Child, Command, Stdio};

use testenv::Log;

const NATIVE: &str = env!("CARGO_BIN_EXE_impulse-shell");

/// A bar (blue at the start, yellow once `open` is called) with a picture that fades in for
/// 1.5 s, and a menu on the overlay layer over the bar's corner, closed at the start.
const SHELL: &str = r##"import QtQuick
import Rustshell
import Rustshell.Io
ShellRoot {
    id: root
    property color fill: "#0000ff"
    property bool menu: false
    property bool fading: false
    PanelWindow {
        anchors { top: true; left: true }
        implicitWidth: 64; implicitHeight: 32
        exclusionMode: ExclusionMode.Ignore
        WlrLayershell.namespace: "handover:bar"
        color: root.fill
        Image {
            x: 32; width: 32; height: 32
            asynchronous: true
            source: "red.png"
            opacity: status === Image.Ready ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: 1500 } }
            onOpacityChanged: {
                if (opacity === 1)
                    console.log("SHOWN");
                else if (opacity > 0.2 && !root.fading) {
                    root.fading = true;
                    console.log("FADING");
                }
            }
        }
    }
    PanelWindow {
        visible: root.menu
        anchors { top: true; left: true }
        implicitWidth: 16; implicitHeight: 16
        exclusionMode: ExclusionMode.Ignore
        WlrLayershell.namespace: "handover:menu"
        WlrLayershell.layer: WlrLayer.Overlay
        color: "#00ff00"
    }
    Connections {
        target: Rustshell
        function onReloadCompleted() { console.log("RELOADED") }
    }
    IpcHandler {
        target: "t"
        function reload(): void { Rustshell.reload(true) }
        function open(): void { root.menu = true; root.fill = "#ffff00" }
    }
}
"##;

const BLUE: [u8; 3] = [0, 0, 255];
const YELLOW: [u8; 3] = [255, 255, 0];
const GREEN: [u8; 3] = [0, 255, 0];
const RED: [u8; 3] = [255, 0, 0];
const MAGENTA: [u8; 3] = [255, 0, 255];

fn sway() -> bool {
    testenv::require_all(&["sway", "grim"])
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn fails_without_sway() {
    testenv::assert_fails_when_missing(
        "the_old_bar_fades_away_and_the_new_one_comes_as_at_a_start_at_a_reload",
        &[("PATH", Some("/nonexistent"))],
    );
}

struct Shell(Child);

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// What one screenshot shows: the corner the menu covers, the bar beside it, the picture.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Seen {
    corner: [u8; 3],
    bar: [u8; 3],
    picture: [u8; 3],
}

/// The pixel at (`x`, `y`) of each of `at`, off one screenshot of the screen's top left
/// 64x64.
fn pixels<const N: usize>(
    sway: &testenv::Sway,
    dir: &Path,
    at: [(usize, usize); N],
) -> [[u8; 3]; N] {
    let f = dir.join("shot.ppm");
    let ok = sway
        .env(&mut Command::new("grim"))
        .args(["-t", "ppm", "-g", "0,0 64x64"])
        .arg(&f)
        .status()
        .is_ok_and(|s| s.success());
    assert!(ok, "grim");
    let b = std::fs::read(&f).unwrap();
    // P6\n64 64\n255\n<rgb rows>
    assert!(
        b.starts_with(b"P6") && b.len() >= 64 * 64 * 3,
        "grim's capture"
    );
    let rgb = &b[b.len() - 64 * 64 * 3..];
    at.map(|(x, y)| {
        let i = (y * 64 + x) * 3;
        [rgb[i], rgb[i + 1], rgb[i + 2]]
    })
}

fn seen(sway: &testenv::Sway, dir: &Path) -> Seen {
    let [corner, bar, picture] = pixels(sway, dir, [(8, 8), (24, 16), (48, 16)]);
    Seen {
        corner,
        bar,
        picture,
    }
}

/// How much of `full` shows over `behind` in `s`, read off the bar and its picture: 0 is
/// none of it, 1 all of it. None when `s` is no blend of the two.
fn share(s: Seen, full: Seen, behind: Seen) -> Option<f64> {
    let px = [
        (s.bar, full.bar, behind.bar),
        (s.picture, full.picture, behind.picture),
    ];
    let ch = |c: [u8; 3], i: usize| f64::from(c[i]);
    let (mut num, mut den) = (0.0, 0.0);
    for (s, f, b) in px {
        for i in 0..3 {
            let d = ch(f, i) - ch(b, i);
            num += (ch(s, i) - ch(b, i)) * d;
            den += d * d;
        }
    }
    let a = (num / den).clamp(0.0, 1.0);
    let fits = px.iter().all(|&(s, f, b)| {
        (0..3).all(|i| (ch(b, i) + a * (ch(f, i) - ch(b, i)) - ch(s, i)).abs() <= 4.0)
    });
    fits.then_some(a)
}

fn wait_seen(sway: &testenv::Sway, dir: &Path, want: Seen) {
    let at = std::time::Instant::now();
    loop {
        let s = seen(sway, dir);
        if s == want {
            return;
        }
        assert!(
            at.elapsed().as_secs() < 20,
            "the screen shows {s:?}, not {want:?}"
        );
    }
}

/// The shell running `qml` in `dir`, with what it writes.
fn run(sway: &testenv::Sway, dir: &Path, qml: &str) -> (Shell, std::sync::Arc<Log>) {
    run_on(sway, dir, qml, None)
}

/// [`run`], on the Hyprland instance `hypr`.
fn run_on(
    sway: &testenv::Sway,
    dir: &Path,
    qml: &str,
    hypr: Option<&FakeHypr>,
) -> (Shell, std::sync::Arc<Log>) {
    std::fs::write(dir.join("shell.qml"), qml).unwrap();
    let red = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/image_status/a.png");
    std::fs::copy(red, dir.join("red.png")).unwrap();
    let xdg = dir.join("xdg");
    let (r, w) = std::io::pipe().unwrap();
    let child = sway
        .env(&mut Command::new(NATIVE))
        .arg("-p")
        .arg(dir)
        .env("RI_INTERP", "1")
        .envs(hypr.map(|h| ("HYPRLAND_INSTANCE_SIGNATURE", &h.signature)))
        .env("XDG_CONFIG_HOME", xdg.join("config"))
        .env("XDG_STATE_HOME", xdg.join("state"))
        .env("XDG_CACHE_HOME", xdg.join("cache"))
        .stdout(Stdio::null())
        .stderr(w)
        .spawn()
        .unwrap();
    (Shell(child), Log::capture(r))
}

/// Whether the shell in `dir` took the call of `function`.
fn called(sway: &testenv::Sway, dir: &Path, function: &str) -> bool {
    sway.env(&mut Command::new(NATIVE))
        .arg("-p")
        .arg(dir)
        .args(["ipc", "call", "t", function])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}

fn said(log: &Log, what: &str, n: usize) -> bool {
    log.text().matches(what).count() >= n
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn the_old_bar_fades_away_and_the_new_one_comes_as_at_a_start_at_a_reload() {
    if !sway() {
        return;
    }
    let sway = testenv::Sway::start();
    let dir = tempfile::tempdir().unwrap();
    // No window yet: what shows behind the bar.
    let behind = seen(&sway, dir.path());
    let (_shell, log) = run(&sway, dir.path(), SHELL);
    let shown = |s: &str, n: usize| s.matches("SHOWN").count() >= n;
    log.wait(|s| shown(s, 1))
        .unwrap_or_else(|l| panic!("the picture never showed:\n{l}"));
    let call = |function: &str| {
        assert!(called(&sway, dir.path(), function), "ipc call t {function}");
    };
    let start = Seen {
        corner: BLUE,
        bar: BLUE,
        picture: RED,
    };
    wait_seen(&sway, dir.path(), start);
    call("open");
    let open = Seen {
        corner: GREEN,
        bar: YELLOW,
        picture: RED,
    };
    wait_seen(&sway, dir.path(), open);

    call("reload");
    // The old image's bar is yellow with its menu green in the corner; the new image's is
    // blue with no menu, and its picture fades in for 1.5 s before it says SHOWN. The old
    // bar and its menu fade away, what is behind them shows, and the new bar comes as at a
    // start, seen from its first frame with its picture fading in: never something of both.
    let at = std::time::Instant::now();
    let mut steps = vec![open];
    let mut end = None;
    let (mut gone, mut came, mut came_drawing) = (false, false, false);
    let mut of_old = 1.0;
    let mut red = 0;
    loop {
        let told = said(&log, "RELOADED", 1);
        let s = seen(&sway, dir.path());
        let new_drawn = shown(&log.text(), 2);
        assert!(
            s == start || !told,
            "the tree heard reloadCompleted before its bar was all there: {steps:?}"
        );
        if steps.last() != Some(&s) {
            steps.push(s);
        }
        if !gone {
            let a = share(s, open, behind).unwrap_or_else(|| {
                panic!("the old bar had not gone when something else showed: {steps:?}")
            });
            assert!(a <= of_old + 0.02, "the old bar came back: {steps:?}");
            of_old = a;
            gone = a == 0.0;
            assert!(
                !gone || s.corner == behind.corner,
                "the menu stays without its bar: {steps:?}"
            );
        } else if !came && s == behind {
            // No bar while the next image starts.
        } else {
            assert!(
                s.bar == BLUE && s.corner == BLUE,
                "something else than the new bar comes, or it went again: {steps:?}"
            );
            // Its picture over its blue.
            let [r, g, b] = s.picture;
            assert!(g == 0 && r.abs_diff(255 - b) <= 4, "the picture: {steps:?}");
            assert!(
                r.saturating_add(4) >= red,
                "the picture went back: {steps:?}"
            );
            red = r;
            came = true;
            came_drawing |= !new_drawn;
        }
        if end.is_none() && new_drawn {
            end = Some(std::time::Instant::now());
        }
        if end.is_some_and(|e| e.elapsed().as_secs() >= 2) {
            break;
        }
        assert!(
            at.elapsed().as_secs() < 60,
            "the reloaded shell never showed its picture:\n{}",
            log.text()
        );
    }
    assert!(gone, "the old bar never left: {steps:?}");
    assert!(
        came_drawing,
        "the new bar was kept unseen until it had drawn: {steps:?}"
    );
    assert_eq!(steps.last(), Some(&start), "the new bar: {steps:?}");
    assert!(
        said(&log, "RELOADED", 1),
        "the tree never heard reloadCompleted:\n{}",
        log.text()
    );
}

/// What is under the windows (the wallpaper's layer and the one above it) is no part of the
/// fade: the screen with nothing but the pointer on it would read as a crash. The old
/// image's stays while the bar is away, and the new one's takes its place in one frame.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn the_wallpaper_stays_while_the_bar_is_away_at_a_reload() {
    if !sway() {
        return;
    }
    let sway = testenv::Sway::start();
    let dir = tempfile::tempdir().unwrap();
    // A wallpaper under the bar and below it, on the layer the shell's own is on.
    let wall = r##"    PanelWindow {
        anchors { top: true; left: true }
        implicitWidth: 64; implicitHeight: 64
        exclusionMode: ExclusionMode.Ignore
        WlrLayershell.namespace: "handover:wall"
        WlrLayershell.layer: WlrLayer.Bottom
        color: "#ff00ff"
    }
    Connections {"##;
    let tree = SHELL.replacen("    Connections {", wall, 1);
    assert_ne!(tree, SHELL);
    let (_shell, log) = run(&sway, dir.path(), &tree);
    log.wait(|s| s.contains("SHOWN"))
        .unwrap_or_else(|l| panic!("the picture never showed:\n{l}"));
    // The bar, and the wallpaper below it.
    let look = || pixels(&sway, dir.path(), [(24, 16), (24, 48)]);
    let at = std::time::Instant::now();
    while look() != [BLUE, MAGENTA] {
        assert!(at.elapsed().as_secs() < 20, "the bar over the wallpaper");
    }
    assert!(called(&sway, dir.path(), "reload"), "ipc call t reload");
    let mut away = false;
    let mut end = None;
    loop {
        let drawn = said(&log, "SHOWN", 2);
        let [bar, wall] = look();
        assert_eq!(wall, MAGENTA, "the wallpaper, {:?} in", at.elapsed());
        // The wallpaper shows where the bar was.
        away |= bar == MAGENTA;
        if end.is_none() && drawn {
            end = Some(std::time::Instant::now());
        }
        if end.is_some_and(|e| e.elapsed().as_secs() >= 2) {
            break;
        }
        assert!(
            at.elapsed().as_secs() < 60,
            "the reloaded shell never showed its picture:\n{}",
            log.text()
        );
    }
    assert!(away, "the bar never left");
    assert_eq!(look(), [BLUE, MAGENTA], "the new bar over the wallpaper");
}

/// A Hyprland instance that answers every request `ok` and keeps them, in order; it says
/// events to whoever listens when it has an event socket.
struct FakeHypr {
    signature: String,
    asked: std::sync::Arc<(std::sync::Mutex<Vec<String>>, std::sync::Condvar)>,
    listeners: std::sync::Arc<(
        std::sync::Mutex<Vec<std::os::unix::net::UnixStream>>,
        std::sync::Condvar,
    )>,
}

impl FakeHypr {
    fn start(sway: &testenv::Sway, events: bool) -> Self {
        use std::io::{Read, Write};
        use std::os::unix::net::UnixListener;
        let mut cmd = Command::new("true");
        let rt = sway.env(&mut cmd).get_envs();
        let rt = rt.into_iter().find(|(k, _)| *k == "XDG_RUNTIME_DIR");
        let rt = rt
            .and_then(|(_, v)| v)
            .expect("the sway's runtime directory");
        let signature = "ri-test-handover".to_string();
        let dir = Path::new(rt).join("hypr").join(&signature);
        std::fs::create_dir_all(&dir).unwrap();
        let asked =
            std::sync::Arc::new((std::sync::Mutex::new(Vec::new()), std::sync::Condvar::new()));
        let listeners =
            std::sync::Arc::new((std::sync::Mutex::new(Vec::new()), std::sync::Condvar::new()));
        let requests = UnixListener::bind(dir.join(".socket.sock")).unwrap();
        let a = asked.clone();
        std::thread::spawn(move || {
            for mut c in requests.incoming().flatten() {
                let mut buf = [0u8; 4096];
                let n = c.read(&mut buf).unwrap_or(0);
                let cmd = String::from_utf8_lossy(&buf[..n]).into_owned();
                a.0.lock().unwrap().push(cmd);
                a.1.notify_all();
                let _ = c.write_all(b"ok");
            }
        });
        if events {
            let events = UnixListener::bind(dir.join(".socket2.sock")).unwrap();
            let l = listeners.clone();
            std::thread::spawn(move || {
                for c in events.incoming().flatten() {
                    l.0.lock().unwrap().push(c);
                    l.1.notify_all();
                }
            });
        }
        FakeHypr {
            signature,
            asked,
            listeners,
        }
    }

    /// Writes the line `event` to every listener of the event socket, once `n` listen: the
    /// image before the reload and the one after it.
    fn say(&self, n: usize, event: &str) {
        use std::io::Write;
        let listeners = self.listeners.0.lock().unwrap();
        let fewer = |l: &mut Vec<std::os::unix::net::UnixStream>| l.len() < n;
        let listeners = self.listeners.1.wait_while(listeners, fewer).unwrap();
        for mut c in listeners.iter() {
            // The image before a reload hung up.
            let _ = c.write_all(format!("{event}\n").as_bytes());
        }
    }

    /// What `asked` holds of the shell's requests about its layers' animations, in order:
    /// `true` to keep them still, `false` to animate them again.
    fn still_of(asked: &[String]) -> Vec<bool> {
        let (on, off) = (
            shell::hyprclose::still_request(true),
            shell::hyprclose::still_request(false),
        );
        let rule = asked.iter().filter(|c| **c == on || **c == off);
        rule.map(|c| *c == on).collect()
    }

    fn still(&self) -> Vec<bool> {
        Self::still_of(&self.asked.0.lock().unwrap())
    }

    /// Waits for the shell to have asked `want` about its layers' animations.
    fn wait_still(&self, want: &[bool]) {
        let asked = self.asked.0.lock().unwrap();
        let not_yet = |a: &mut Vec<String>| Self::still_of(a).len() < want.len();
        let asked = self.asked.1.wait_while(asked, not_yet).unwrap();
        assert_eq!(
            Self::still_of(&asked),
            want,
            "what the shell asked Hyprland"
        );
    }

    /// Waits for the shell to have asked `cmd` `n` times.
    fn wait_asked(&self, cmd: &str, n: usize) {
        let asked = self.asked.0.lock().unwrap();
        let fewer = |a: &mut Vec<String>| a.iter().filter(|c| *c == cmd).count() < n;
        drop(self.asked.1.wait_while(asked, fewer).unwrap());
    }
}

/// Opens the menu and reloads the shell on `hypr`, and waits until the old image's windows
/// have left. Hyprland animates a closed layer away on a snapshot, over what is under it,
/// and a new one in: the next image starts with the shell's layers animated (its bar comes
/// as at a start) and asks to keep them still before the old ones leave.
fn reload_on(sway: &testenv::Sway, dir: &Path, hypr: &FakeHypr) -> Shell {
    let (shell, log) = run_on(sway, dir, SHELL, Some(hypr));
    log.wait(|s| s.contains("SHOWN"))
        .unwrap_or_else(|l| panic!("the picture never showed:\n{l}"));
    assert!(called(sway, dir, "open"), "ipc call t open");
    let open = Seen {
        corner: GREEN,
        bar: YELLOW,
        picture: RED,
    };
    wait_seen(sway, dir, open);
    // A start that is no reload takes back what an image that ended may have left on.
    hypr.wait_still(&[false]);
    assert!(called(sway, dir, "reload"), "ipc call t reload");
    // The new bar is there, drawing, with the old windows not gone yet.
    let at = std::time::Instant::now();
    while seen(sway, dir).bar != BLUE {
        assert!(
            at.elapsed().as_secs() < 60,
            "the reloaded shell never showed its bar:\n{}",
            log.text()
        );
    }
    assert!(!said(&log, "RELOADED", 1), "the old windows left at once");
    assert_eq!(
        hypr.still(),
        [false, false],
        "Hyprland does not animate the new bar in"
    );
    // The tree hears of the reload once the old windows have left.
    log.wait(|s| s.contains("RELOADED"))
        .unwrap_or_else(|l| panic!("the old windows never left:\n{l}"));
    assert_eq!(
        hypr.still().get(2),
        Some(&true),
        "the old windows left with Hyprland animating the shell's layers"
    );
    shell
}

/// Hyprland says when it closed the old layers: it animates the shell's again from then.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn hyprland_animates_no_layer_of_the_shell_until_the_old_ones_closed() {
    if !sway() {
        return;
    }
    let sway = testenv::Sway::start();
    let dir = tempfile::tempdir().unwrap();
    let hypr = FakeHypr::start(&sway, true);
    let _shell = reload_on(&sway, dir.path(), &hypr);
    // Another client's layer closing is not the old ones: the shell has read it once it
    // answers the event after it, with a second read of Hyprland's settings.
    // Each image reads them once as it starts.
    hypr.wait_asked("j/getoption animations:enabled", 2);
    hypr.say(2, "closelayer>>another:client");
    hypr.say(2, "configreloaded>>");
    hypr.wait_asked("j/getoption animations:enabled", 3);
    assert_eq!(
        hypr.still(),
        [false, false, true],
        "the layers animate again before Hyprland closed the old ones"
    );
    hypr.say(2, "closelayer>>handover:bar");
    hypr.wait_still(&[false, false, true, false]);
}

/// With no event socket nothing says when: the layers animate again as the old ones leave.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn without_hyprlands_events_the_layers_animate_again_as_the_old_ones_leave() {
    if !sway() {
        return;
    }
    let sway = testenv::Sway::start();
    let dir = tempfile::tempdir().unwrap();
    let hypr = FakeHypr::start(&sway, false);
    let _shell = reload_on(&sway, dir.path(), &hypr);
    hypr.wait_still(&[false, false, true, false]);
}

/// A reload asked while the image before the running one still has its windows on the
/// screen: those end at the `exec`, and the running image's are what the screen keeps under
/// the next one. Its bar, there since its first frame, fades away as the first one's did,
/// and the last image's comes.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_reload_during_a_reload_fades_the_running_images_bar_before_the_last_ones_comes() {
    if !sway() {
        return;
    }
    let sway = testenv::Sway::start();
    let dir = tempfile::tempdir().unwrap();
    let behind = seen(&sway, dir.path());
    // The picture fades for 4 s: the second reload is asked a second into that.
    let (_shell, log) = run(&sway, dir.path(), &SHELL.replace("1500", "4000"));
    log.wait(|s| s.contains("SHOWN"))
        .unwrap_or_else(|l| panic!("the picture never showed:\n{l}"));
    assert!(called(&sway, dir.path(), "open"), "ipc call t open");
    let open = Seen {
        corner: GREEN,
        bar: YELLOW,
        picture: RED,
    };
    wait_seen(&sway, dir.path(), open);
    assert!(called(&sway, dir.path(), "reload"), "ipc call t reload");
    // The second image draws its picture a fifth in; the first one's bar has gone.
    log.wait(|s| s.matches("FADING").count() >= 2)
        .unwrap_or_else(|l| panic!("the reloaded shell never drew its picture:\n{l}"));
    let s = seen(&sway, dir.path());
    assert_eq!((s.corner, s.bar), (BLUE, BLUE), "the second image's bar");

    let at = std::time::Instant::now();
    while !called(&sway, dir.path(), "reload") {
        assert!(at.elapsed().as_secs() < 20, "ipc call t reload, the second");
    }
    let start = Seen {
        corner: BLUE,
        bar: BLUE,
        picture: RED,
    };
    // How much of a blue bar shows over what is behind it.
    let blue = |s: Seen| {
        let full = Seen {
            bar: BLUE,
            ..behind
        };
        share(
            Seen {
                bar: s.bar,
                ..behind
            },
            full,
            behind,
        )
    };
    let mut steps = vec![s];
    let mut end = None;
    let (mut gone, mut of_second) = (false, 1.0);
    loop {
        let s = seen(&sway, dir.path());
        // The third image is the second one to show all of its picture.
        let drawn = said(&log, "SHOWN", 2);
        if steps.last() != Some(&s) {
            steps.push(s);
        }
        assert_eq!(s.corner, s.bar, "a menu, or half a bar: {steps:?}");
        let a = blue(s).unwrap_or_else(|| panic!("something else than a bar: {steps:?}"));
        if !gone {
            assert!(a <= of_second + 0.02, "the second bar came back: {steps:?}");
            of_second = a;
            gone = a == 0.0;
            assert!(!gone || s == behind, "its picture stays: {steps:?}");
        } else {
            assert!(a == 0.0 || a == 1.0, "the last bar fades: {steps:?}");
        }
        if end.is_none() && drawn {
            end = Some(std::time::Instant::now());
        }
        if end.is_some_and(|e| e.elapsed().as_secs() >= 2) {
            break;
        }
        assert!(
            at.elapsed().as_secs() < 60,
            "the shell reloaded twice never showed its picture:\n{}",
            log.text()
        );
    }
    assert!(gone, "the second image's bar never left: {steps:?}");
    assert_eq!(
        steps.last(),
        Some(&start),
        "the third image's bar: {steps:?}"
    );
}

/// A menu asked for while the previous image's windows are still on the screen shows as it
/// draws: only the windows the next image starts with wait for those to leave.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_menu_opened_during_a_reload_shows_at_once() {
    if !sway() {
        return;
    }
    let sway = testenv::Sway::start();
    let dir = tempfile::tempdir().unwrap();
    let (_shell, log) = run(&sway, dir.path(), &SHELL.replace("1500", "4000"));
    log.wait(|s| s.contains("SHOWN"))
        .unwrap_or_else(|l| panic!("the picture never showed:\n{l}"));
    let start = Seen {
        corner: BLUE,
        bar: BLUE,
        picture: RED,
    };
    wait_seen(&sway, dir.path(), start);
    assert!(called(&sway, dir.path(), "reload"), "ipc call t reload");
    log.wait(|s| s.matches("FADING").count() >= 2)
        .unwrap_or_else(|l| panic!("the reloaded shell never drew its picture:\n{l}"));
    assert_eq!(seen(&sway, dir.path()).bar, BLUE, "the bar as it draws");

    let at = std::time::Instant::now();
    while !called(&sway, dir.path(), "open") {
        assert!(at.elapsed().as_secs() < 20, "ipc call t open");
    }
    // The menu over the second image's bar, yellow now.
    let at = std::time::Instant::now();
    loop {
        let s = seen(&sway, dir.path());
        if (s.corner, s.bar) == (GREEN, YELLOW) {
            break;
        }
        assert!(at.elapsed().as_secs() < 20, "the menu: {s:?}");
    }
    assert!(
        !said(&log, "SHOWN", 2),
        "the menu showed only once the new image had drawn"
    );
    let open = Seen {
        corner: GREEN,
        bar: YELLOW,
        picture: RED,
    };
    wait_seen(&sway, dir.path(), open);
}
