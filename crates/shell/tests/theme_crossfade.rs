//! A theme switch crossfades every impulse-shell window from its previous frame (`Crossfade.request`). On a
//! compositor with wl_subcompositor and wp_alpha_modifier_v1 (the headless sway of
//! tools/testenv, as Hyprland) the previous frame goes on a subsurface once and the compositor
//! fades it out: the window itself draws nothing more while its content holds still. Blending
//! inside the window's own buffer redraws all of it on every frame of the fade, on every
//! window at once, which is what dropped frames during a light/dark switch.
//!
//! Each test runs its own sway: the panels of tests running meanwhile would sit at the same
//! spot of the shared one's screen the screenshots read.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

use testenv::Log;

const SHELL: &str = r##"import QtQuick
import Rustshell
import Impulse
ShellRoot {
    id: root
    property color fill: "#ff0000"
    PanelWindow {
        anchors { top: true; left: true }
        implicitWidth: 64; implicitHeight: 32
        exclusionMode: ExclusionMode.Ignore
        color: root.fill
        // Antialiased geometry: the frame draws triangles from vertex buffers.
        Rectangle { x: 40; y: 6; width: 20; height: 20; radius: 6; color: "white" }
    }
    IpcHandler {
        target: "fx"
        function fade(): void {
            Crossfade.request(1500);
            root.fill = "#00ff00";
            console.log("FADED");
        }
        function fade2(): void {
            Crossfade.request(1500);
            root.fill = "#0000ff";
            console.log("FADED2");
        }
    }
}
"##;

fn sway() -> bool {
    testenv::require_all(&["sway", "grim"])
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn fails_without_sway() {
    testenv::assert_fails_when_missing(
        "theme_fade_leaves_the_window_undrawn",
        &[("PATH", Some("/nonexistent"))],
    );
}

struct Shell {
    child: Child,
    sway: testenv::Sway,
    dir: tempfile::TempDir,
    log: Arc<Log>,
}

impl Shell {
    fn start() -> Self {
        Self::start_with(SHELL)
    }

    fn start_with(qml: &str) -> Self {
        Self::start_in(tempfile::tempdir().unwrap(), qml, &[])
    }

    /// The shell of `qml` in `dir`, with `env` set as well.
    fn start_in(dir: tempfile::TempDir, qml: &str, env: &[(&str, &str)]) -> Self {
        let sway = testenv::Sway::start();
        std::fs::write(dir.path().join("shell.qml"), qml).unwrap();
        let (r, w) = std::io::pipe().unwrap();
        let xdg = dir.path().join("xdg");
        let child = sway
            .env(&mut Command::new(env!("CARGO_BIN_EXE_impulse-shell")))
            .arg("-p")
            .arg(dir.path())
            .env("RI_INTERP", "1")
            // Qt's fixed animation step: every frame moves the fade 16 ms, so its 1500 ms take
            // the same ~94 frames on a loaded host, where the default driver falls back to wall
            // time and would end the fade in a few slow frames.
            .env("QSG_FIXED_ANIMATION_STEP", "1")
            .env("WAYLAND_DEBUG", "1")
            .env("RI_LOG", "ri::fade=debug,ri::stats=debug,ri::present=debug")
            .env("XDG_CONFIG_HOME", xdg.join("config"))
            .env("XDG_STATE_HOME", xdg.join("state"))
            .env("XDG_CACHE_HOME", xdg.join("cache"))
            .envs(env.iter().copied())
            .stdout(Stdio::null())
            .stderr(w)
            .spawn()
            .unwrap();
        Shell {
            child,
            sway,
            dir,
            log: Log::capture(r),
        }
    }

    fn wait(&self, what: &str, f: impl Fn(&str) -> bool) {
        if self.log.wait(f).is_err() {
            let s = self.log.text();
            let mut keep: Vec<&str> = s
                .lines()
                .filter(|l| l.contains("ri::") || l.contains("qml"))
                .collect();
            if keep.is_empty() {
                keep = s.lines().collect();
            }
            panic!(
                "impulse-shell exited before {what}; impulse-shell log tail:\n{}",
                keep[keep.len().saturating_sub(40)..].join("\n")
            );
        }
    }

    /// Waits until the commit that requested the wp_presentation_feedback on line `pick(log)`
    /// (both logged by WAYLAND_DEBUG) is on screen: that feedback was `presented`.
    fn wait_presented(&self, what: &str, pick: impl Fn(&[&str]) -> Option<usize>) {
        self.wait(what, |s| {
            let lines: Vec<&str> = s.lines().collect();
            let Some((i, id)) = pick(&lines).and_then(|i| Some((i, feedback(lines[i])?))) else {
                return false;
            };
            let (presented, discarded) = (
                format!("wp_presentation_feedback#{id}.presented("),
                format!("wp_presentation_feedback#{id}.discarded("),
            );
            lines[i + 1..]
                .iter()
                .find(|l| l.contains(&presented) || l.contains(&discarded))
                .is_some_and(|l| l.contains(&presented))
        });
    }

    /// Blocks until a commit requested after log line `from` is on screen (its
    /// wp_presentation_feedback `presented`) or `stop(log)` holds; returns whether a commit was.
    fn presented_since(&self, what: &str, from: usize, stop: impl Fn(&str) -> bool) -> bool {
        let shown = |s: &str| {
            let lines: Vec<&str> = s.lines().collect();
            (from..lines.len()).any(|i| {
                feedback(lines[i]).is_some_and(|id| {
                    let presented = format!("wp_presentation_feedback#{id}.presented(");
                    lines[i + 1..].iter().any(|l| l.contains(&presented))
                })
            })
        };
        self.wait(what, |s| shown(s) || stop(s));
        shown(&self.log.text())
    }

    fn lines(&self) -> usize {
        self.log.text().lines().count()
    }

    fn ipc(&self, func: &str) {
        let st = self
            .sway
            .env(&mut Command::new(env!("CARGO_BIN_EXE_impulse-shell")))
            .arg("-p")
            .arg(self.dir.path())
            .args(["ipc", "call", "fx", func])
            .stdout(Stdio::null())
            .status()
            .unwrap();
        assert!(st.success(), "ipc call fx {func}");
    }
}

impl Drop for Shell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The wp_presentation_feedback id a `wp_presentation.feedback` request line creates.
fn feedback(l: &str) -> Option<&str> {
    l.split_once("-> wp_presentation#")
        .and_then(|(_, r)| r.split_once("new id wp_presentation_feedback#"))
        .and_then(|(_, r)| r.split(')').next())
}

/// The first feedback request after the last line `mark` matches (from the start without one).
fn after(mark: &str) -> impl Fn(&[&str]) -> Option<usize> + '_ {
    move |ls| {
        let from = ls
            .iter()
            .rposition(|l| l.contains(mark))
            .map_or(0, |m| m + 1);
        (from..ls.len()).find(|&i| feedback(ls[i]).is_some())
    }
}

/// The last feedback request before the first line `mark` matches: the commit that line
/// is logged after.
fn before(mark: &str) -> impl Fn(&[&str]) -> Option<usize> + '_ {
    move |ls| {
        let m = ls.iter().position(|l| l.contains(mark))?;
        (0..m).rev().find(|&i| feedback(ls[i]).is_some())
    }
}

/// The last feedback request before the last line `mark` matches.
fn before_last(mark: &str) -> impl Fn(&[&str]) -> Option<usize> + '_ {
    move |ls| {
        let m = ls.iter().rposition(|l| l.contains(mark))?;
        (0..m).rev().find(|&i| feedback(ls[i]).is_some())
    }
}

/// The RGB pixel at `x,y` of `sway`'s screen: grim captures only that pixel, so a sample costs
/// about nothing next to a fade step.
fn pixel(sway: &testenv::Sway, dir: &Path, x: usize, y: usize) -> [u8; 3] {
    let f: PathBuf = dir.join("shot.ppm");
    let ok = sway
        .env(&mut Command::new("grim"))
        .args(["-t", "ppm", "-g", &format!("{x},{y} 1x1")])
        .arg(&f)
        .status()
        .is_ok_and(|s| s.success());
    assert!(ok, "grim");
    let b = std::fs::read(&f).unwrap();
    // P6\n1 1\n255\n<rgb>
    let mut fields = Vec::new();
    let mut i = 0;
    while fields.len() < 4 {
        while b[i].is_ascii_whitespace() {
            i += 1;
        }
        let s = i;
        while !b[i].is_ascii_whitespace() {
            i += 1;
        }
        fields.push(String::from_utf8_lossy(&b[s..i]).into_owned());
    }
    assert_eq!(fields[..3], ["P6", "1", "1"], "grim's 1x1 capture");
    let o = i + 1;
    [b[o], b[o + 1], b[o + 2]]
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn theme_fade_leaves_the_window_undrawn() {
    if !sway() {
        return;
    }
    let sh = Shell::start();
    sh.wait("the panel's first frame", |s| s.contains("draw win0"));
    sh.wait_presented("the red panel on screen", after("FADED"));
    sh.ipc("fade");
    sh.wait("the veil", |s| s.contains("win0 veil 64x32 1500ms"));
    sh.wait_presented("the first faded frame", after("win0 veil 64x32"));
    let [r, g, b] = pixel(&sh.sway, sh.dir.path(), 10, 10);
    assert!(
        r > 128 && g < 128 && b == 0,
        "the fade does not start from the red frame: {:?}",
        [r, g, b]
    );
    sh.wait("the veil's end", |s| s.contains("win0 veil end"));
    sh.wait_presented("the green panel on screen", before("win0 veil end"));
    assert_eq!(
        pixel(&sh.sway, sh.dir.path(), 10, 10),
        [0, 255, 0],
        "panel after the fade"
    );
    let log = sh.log.text();
    let lines: Vec<&str> = log.lines().collect();
    let from = lines
        .iter()
        .position(|l| l.contains("win0 veil 64x32"))
        .unwrap();
    let to = lines
        .iter()
        .position(|l| l.contains("win0 veil end"))
        .unwrap();
    let steps = lines[from..to]
        .iter()
        .filter(|l| l.contains("draw win0"))
        .count();
    let drawn: Vec<&&str> = lines[from..to]
        .iter()
        .filter(|l| l.contains("draw win0") && !l.contains("damage 0x0"))
        .collect();
    assert!(steps > 10, "the fade stepped {steps} times");
    assert!(
        drawn.len() <= 1,
        "the window redrew {} of {steps} fade frames: {drawn:?}",
        drawn.len()
    );
}

/// A second theme change while the first still fades (a quick double toggle) fades in under the
/// first: the first start frame's share of the screen keeps falling, it never steps back.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn overlapping_fades_never_bring_the_old_frame_back() {
    if !sway() {
        return;
    }
    let sh = Shell::start();
    sh.wait("the panel's first frame", |s| s.contains("draw win0"));
    sh.wait_presented("the red panel on screen", after("FADED"));
    sh.ipc("fade");
    sh.wait("the veil", |s| s.contains("win0 veil 64x32 1500ms"));
    sh.wait_presented("the first faded frame", after("win0 veil 64x32"));
    // The red frame's share of the screen once per presented fade step: a few of the first
    // fade alone, then every one until both fades ended.
    let first_end = |s: &str| s.contains("win0 veil end");
    let mut red = vec![pixel(&sh.sway, sh.dir.path(), 10, 10)[0]];
    for n in 0..5 {
        let from = sh.lines();
        assert!(
            sh.presented_since("a step of the first fade", from, first_end),
            "the first fade ended after {n} presented steps"
        );
        red.push(pixel(&sh.sway, sh.dir.path(), 10, 10)[0]);
    }
    sh.ipc("fade2");
    sh.wait("the second change", |s| s.contains("FADED2"));
    let both_ended = |s: &str| s.matches("win0 veil end").count() == 2;
    while sh.presented_since("a step of both fades", sh.lines(), both_ended) {
        red.push(pixel(&sh.sway, sh.dir.path(), 10, 10)[0]);
    }
    for w in red.windows(2) {
        assert!(
            w[1] <= w[0].saturating_add(2),
            "the red frame came back from {} to {}: {red:?}",
            w[0],
            w[1]
        );
    }
    sh.wait_presented("the blue panel on screen", before_last("win0 veil end"));
    assert_eq!(
        pixel(&sh.sway, sh.dir.path(), 10, 10),
        [0, 0, 255],
        "panel after both fades"
    );
}

/// The veil is the window's for good: a later theme switch draws its start frame into the
/// same subsurface and EGL surface (allocated buffers, the window's uploaded vertex buffers)
/// instead of creating them anew, which made every switch freeze for a cold full draw per
/// window. Between fades the veil is unmapped (a null buffer), not destroyed.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn repeated_fades_reuse_one_veil() {
    if !sway() {
        return;
    }
    let sh = Shell::start();
    sh.wait("the panel's first frame", |s| s.contains("draw win0"));
    sh.wait_presented("the red panel on screen", after("FADED"));
    sh.ipc("fade");
    sh.wait("the first veil's end", |s| s.contains("win0 veil end"));
    sh.wait_presented("the green panel on screen", before("win0 veil end"));
    sh.ipc("fade2");
    sh.wait("the second veil", |s| {
        s.matches("win0 veil 64x32").count() == 2
    });
    sh.wait_presented("the second fade's first frame", |ls| {
        let m = ls.iter().rposition(|l| l.contains("win0 veil 64x32"))?;
        (m + 1..ls.len()).find(|&i| feedback(ls[i]).is_some())
    });
    let [r, g, b] = pixel(&sh.sway, sh.dir.path(), 10, 10);
    assert!(
        g > 128 && r == 0 && b < 128,
        "the second fade does not start from the green frame: {:?}",
        [r, g, b]
    );
    sh.wait("the second veil's end", |s| {
        s.matches("win0 veil end").count() == 2
    });
    sh.wait_presented("the blue panel on screen", before_last("win0 veil end"));
    assert_eq!(
        pixel(&sh.sway, sh.dir.path(), 10, 10),
        [0, 0, 255],
        "panel after the second fade"
    );
    let log = sh.log.text();
    let lines: Vec<&str> = log.lines().collect();
    let veils: Vec<&&str> = lines
        .iter()
        .filter(|l| l.contains("win0 veil 64x32"))
        .collect();
    assert_eq!(veils.len(), 2, "{veils:?}");
    for v in &veils {
        assert!(
            v.ends_with("1500ms uploads 0"),
            "a start frame uploaded vertex buffers again: {v}"
        );
    }
    assert_eq!(log.matches("win0 veil new").count(), 1, "veils created");
    // The frame that draws the start frame onto the veil commits the window unchanged; the
    // next draws the new frame: a switch's two full draws land in two frames.
    for (i, _) in lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.contains("win0 veil 64x32"))
    {
        let draws: Vec<&&str> = lines[i..]
            .iter()
            .filter(|l| l.contains("draw win0"))
            .take(2)
            .collect();
        assert!(
            draws.len() == 2 && draws[0].contains("damage 0x0") && !draws[1].contains("damage 0x0"),
            "the window's draws after its veil's: {draws:?}"
        );
    }
    let subs: Vec<&&str> = lines
        .iter()
        .filter(|l| l.contains(".get_subsurface("))
        .collect();
    assert_eq!(subs.len(), 1, "subsurfaces created: {subs:?}");
    let first = lines
        .iter()
        .position(|l| l.contains("win0 veil 64x32"))
        .unwrap();
    let late: Vec<&&str> = lines[first..]
        .iter()
        .filter(|l| {
            l.contains(".create_surface(")
                || (l.contains("wl_subsurface#") && l.contains(".destroy("))
        })
        .collect();
    assert!(
        late.is_empty(),
        "surfaces made or dropped after the first veil: {late:?}"
    );
    // `-> wl_subcompositor#N.get_subsurface(new id wl_subsurface#S, wl_surface#V, wl_surface#P)`
    let veil = subs[0]
        .split("wl_surface#")
        .nth(1)
        .and_then(|r| r.split([',', ')']).next())
        .unwrap();
    let unmaps = lines
        .iter()
        .filter(|l| l.contains(&format!("wl_surface#{veil}.attach(nil")))
        .count();
    assert_eq!(unmaps, 2, "veil wl_surface#{veil} unmapped after each fade");
}

const PAGE: &str = r##"import QtQuick
import Rustshell
import Impulse
ShellRoot {
    id: root
    property color fill: "#ff0000"
    PanelWindow {
        anchors { top: true; left: true }
        implicitWidth: 128; implicitHeight: 64
        exclusionMode: ExclusionMode.Ignore
        color: "#00ff00"
        Rectangle { x: 8; y: 8; width: 24; height: 24; color: root.fill }
    }
    IpcHandler {
        target: "fx"
        function page(): void {
            Crossfade.request(1500);
            root.fill = "#0000ff";
            console.log("PAGED");
        }
    }
}
"##;

/// A change of a part of a window (a page of the settings window) crossfades inside the window,
/// over that part: no veil over the whole window for the compositor to blend on every frame of
/// the fade, and each of its frames damages the part only.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_page_fades_inside_the_window_over_the_page_only() {
    if !sway() {
        return;
    }
    let sh = Shell::start_with(PAGE);
    sh.wait("the panel's first frame", |s| s.contains("draw win0"));
    sh.wait_presented("the panel on screen", after("PAGED"));
    sh.ipc("page");
    sh.wait("the fade", |s| s.contains("win0 128x64 fade 1500ms"));
    sh.wait_presented("the first faded frame", after("win0 128x64 fade"));
    let [r, g, b] = pixel(&sh.sway, sh.dir.path(), 16, 16);
    assert!(
        r > 128 && g == 0 && b < 128,
        "the fade does not start from the red square: {:?}",
        [r, g, b]
    );
    sh.wait("the fade's end", |s| {
        s.contains("frame 128x64 t 1.00") || s.contains("win0 veil end")
    });
    let log = sh.log.text();
    let lines: Vec<&str> = log.lines().collect();
    let veils: Vec<&&str> = lines
        .iter()
        .filter(|l| l.contains(".get_subsurface(") || l.contains("win0 veil"))
        .collect();
    assert!(veils.is_empty(), "a veil for a part: {veils:?}");
    sh.wait_presented("the blue square on screen", after("frame 128x64 t 1.00"));
    let at = |x, y| pixel(&sh.sway, sh.dir.path(), x, y);
    assert_eq!(at(16, 16), [0, 0, 255], "square after the fade");
    assert_eq!(at(100, 40), [0, 255, 0], "panel after the fade");
    let from = lines
        .iter()
        .position(|l| l.contains("win0 128x64 fade"))
        .unwrap();
    let to = lines
        .iter()
        .position(|l| l.contains("frame 128x64 t 1.00"))
        .unwrap();
    // `draw win0 128x64 layout 0ms paint 0ms gpu 0ms damage 26x26 swap 0ms`
    let damage: Vec<(u32, u32)> = lines[from..to]
        .iter()
        .filter(|l| l.contains("draw win0"))
        .filter_map(|l| {
            l.split_once("damage ")?
                .1
                .split(' ')
                .next()?
                .split_once('x')
        })
        .map(|(w, h)| (w.parse().unwrap(), h.parse().unwrap()))
        .collect();
    assert!(damage.len() > 10, "the fade stepped {} times", damage.len());
    let wide: Vec<&(u32, u32)> = damage.iter().filter(|d| d.0 > 32 || d.1 > 32).collect();
    assert!(
        wide.is_empty() && damage.iter().any(|d| d.0 >= 24),
        "damage of the fade's frames: {damage:?}"
    );
}

/// A window whose picture is a file, read off the thread as the wallpaper is.
const PICTURE: &str = r##"import QtQuick
import Rustshell
ShellRoot {
    PanelWindow {
        anchors { top: true; left: true }
        implicitWidth: 64; implicitHeight: 32
        exclusionMode: ExclusionMode.Ignore
        color: "#ff0000"
        Image {
            anchors.fill: parent
            asynchronous: true
            source: "blue.png"
            onStatusChanged: console.log("STATUS " + status)
        }
    }
}
"##;

struct Kill(Vec<Child>);

impl Drop for Kill {
    fn drop(&mut self) {
        for c in &mut self.0 {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// A light/dark or accent switch rewrites kdeglobals, and plasma-integration tells every
/// application with `ConfigChanged` on `/kdeglobals`. The shell then forgets what it drew and
/// the icons it decoded for the old scheme. A picture from a file is none of the theme's: its
/// window keeps the frame it shows. Forgotten with the icons, the picture was missing from
/// the window's next frame, until the file was decoded again: the wallpaper gave way to the
/// compositor's background on every switch.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn a_platform_theme_change_keeps_a_file_picture() {
    if !sway() || !testenv::require_all(&["dbus-daemon", "dbus-monitor", "dbus-send"]) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    qsg::raster::Canvas::new(64, 32, [0, 0, 255, 255])
        .save_png(dir.path().join("blue.png").to_str().unwrap())
        .unwrap();
    let socket = dir.path().join("bus");
    let bus = format!("unix:path={}", socket.display());
    let mut kids = Kill(vec![]);
    let daemon = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--nopidfile"])
        .arg(format!("--address={bus}"))
        .spawn();
    kids.0.push(daemon.unwrap());
    testenv::wait_exists(&socket);
    // The bus's monitor prints each AddMatch as the bus takes it: the shell's subscription.
    let (r, w) = std::io::pipe().unwrap();
    let monitor = Command::new("dbus-monitor")
        .args(["--address", &bus])
        .arg("type='method_call',interface='org.freedesktop.DBus',member='AddMatch'")
        .stdout(w)
        .spawn();
    kids.0.push(monitor.unwrap());
    let monitor = Log::capture(r);
    // dbus-monitor prints its own NameLost once the bus made it a monitor.
    monitor
        .wait(|s| s.contains("member=NameLost"))
        .expect("dbus-monitor on the test's bus");
    let env = [
        ("DBUS_SESSION_BUS_ADDRESS", bus.as_str()),
        ("QT_QPA_PLATFORMTHEME", "kde"),
        ("KDE_SESSION_VERSION", "6"),
    ];
    let sh = Shell::start_in(dir, PICTURE, &env);
    sh.wait("the picture read", |s| s.contains("STATUS 1"));
    sh.wait_presented("the picture on screen", after("STATUS 1"));
    assert_eq!(
        pixel(&sh.sway, sh.dir.path(), 10, 10),
        [0, 0, 255],
        "the picture before the change"
    );
    monitor
        .wait(|s| s.contains("org.kde.kconfig.notify"))
        .expect("the shell's subscription to ConfigChanged");
    let from = sh.lines();
    // KConfig's signal carries a{saay}, which dbus-send cannot write: the shell takes any
    // ConfigChanged of /kdeglobals as a change, whatever groups it names.
    let sent = Command::new("dbus-send")
        .arg(format!("--bus={bus}"))
        .args(["--type=signal", "/kdeglobals"])
        .arg("org.kde.kconfig.notify.ConfigChanged")
        .arg("dict:string:string:General,ColorScheme")
        .status()
        .unwrap();
    assert!(sent.success(), "dbus-send");
    let drawn = |s: &str| s.lines().skip(from).any(|l| l.contains("draw win0"));
    sh.wait("the window under the changed settings", drawn);
    let log = sh.log.text();
    let since: Vec<&str> = log.lines().skip(from).collect();
    let draw = since.iter().find(|l| l.contains("draw win0")).unwrap();
    assert!(
        draw.contains("damage 0x0"),
        "the window's frame changed: {draw}"
    );
    assert_eq!(
        pixel(&sh.sway, sh.dir.path(), 10, 10),
        [0, 0, 255],
        "the picture after the change"
    );
}
