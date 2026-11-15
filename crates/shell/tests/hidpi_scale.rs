//! HiDPI under a real compositor (a headless sway of the test's own, so the scale it sets is no
//! other test's, and no other client repaints its output): at output scale 2 and then 1.5 the
//! native shell renders its layer surface at device pixels
//! (wp_fractional_scale_v1 + wp_viewporter, or an integer buffer scale) and reports the scale
//! as `devicePixelRatio`, as Qt does. A shell drawing at scale 1 leaves the compositor to
//! upscale its buffer, so a rectangle on a half logical pixel comes out with blended edges.
//!
//! A region screenshot at a fractional scale cuts what was selected: the capture is the
//! output's frame at its fractional scale, the one the QML multiplies the region by.
use std::path::Path;
use std::process::{Child, Command, Stdio};

use testenv::Log;
use testenv::wayland::presented_after;

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

const SHELL: &str = r##"import QtQuick
import Rustshell
ShellRoot {
    PanelWindow {
        anchors { top: true; left: true }
        implicitWidth: 64; implicitHeight: 32
        exclusionMode: ExclusionMode.Ignore
        color: "#000000"
        onDevicePixelRatioChanged: console.log("HIDPI dpr " + devicePixelRatio)
        Component.onCompleted: console.log("HIDPI ready")
        // Device pixels 21..24 at scale 2.
        Rectangle { x: 10.5; y: 10.5; width: 1.5; height: 1.5; color: "#ff0000" }
        // Device pixels 45..48 at scale 1.5.
        Rectangle { x: 30; y: 10; width: 2; height: 2; color: "#00ff00" }
    }
}
"##;

fn swaymsg(sway: &testenv::Sway, cmd: &str) {
    let st = Command::new("swaymsg")
        .env("SWAYSOCK", sway.ipc_socket())
        .arg(cmd)
        .stdout(Stdio::null())
        .status()
        .expect("swaymsg");
    assert!(st.success(), "swaymsg {cmd}");
}

/// HEADLESS-1 as RGB rows, in device pixels (grim at the output's scale).
fn shot(sway: &testenv::Sway, dir: &Path) -> Option<(usize, usize, Vec<u8>)> {
    let f = dir.join("shot.ppm");
    let ok = sway
        .env(&mut Command::new("grim"))
        .args(["-t", "ppm", "-o", "HEADLESS-1"])
        .arg(&f)
        .status()
        .ok()?
        .success();
    if !ok {
        return None;
    }
    let b = std::fs::read(&f).ok()?;
    // P6\n<w> <h>\n255\n<rgb>
    let mut fields = Vec::new();
    let mut i = 0;
    while fields.len() < 4 {
        while b.get(i)?.is_ascii_whitespace() {
            i += 1;
        }
        let s = i;
        while !b.get(i)?.is_ascii_whitespace() {
            i += 1;
        }
        fields.push(String::from_utf8_lossy(&b[s..i]).into_owned());
    }
    let (w, h): (usize, usize) = (fields[1].parse().ok()?, fields[2].parse().ok()?);
    Some((w, h, b[i + 1..].to_vec()))
}

/// The device pixels `lo..hi` on both axes are exactly `want`, and the ring around them is
/// the black window background: no blending from an upscaled buffer.
fn crisp(
    img: &(usize, usize, Vec<u8>),
    lo: usize,
    hi: usize,
    y0: usize,
    want: [u8; 3],
) -> Result<(), String> {
    let (w, _, px) = img;
    let at = |x: usize, y: usize| {
        let o = (y * w + x) * 3;
        [px[o], px[o + 1], px[o + 2]]
    };
    let y1 = y0 + (hi - lo);
    for y in y0 - 1..=y1 {
        for x in lo - 1..=hi {
            let inside = (lo..hi).contains(&x) && (y0..y1).contains(&y);
            let exp = if inside { want } else { [0, 0, 0] };
            if at(x, y) != exp {
                return Err(format!("pixel {x},{y} is {:?}, want {exp:?}", at(x, y)));
            }
        }
    }
    Ok(())
}

/// The screenshot taken once impulse-shell's frame at the new scale is on screen: the frame drawn after
/// the `devicePixelRatio` change was `presented`.
fn assert_crisp(
    sway: &testenv::Sway,
    dir: &Path,
    scale: f64,
    (lo, hi, y0): (usize, usize, usize),
    want: [u8; 3],
) {
    let img = shot(sway, dir).expect("grim");
    // The 1920x1080 mode, whatever the scale.
    assert_eq!((img.0, img.1), (1920, 1080));
    if let Err(e) = crisp(&img, lo, hi, y0, want) {
        panic!("scale {scale}: not drawn at device pixels: {e}");
    }
}

#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn layer_surface_renders_at_output_scale() {
    if !testenv::require_all(&["sway", "swaymsg", "grim"]) {
        return;
    }
    let sway = testenv::Sway::start();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("shell.qml"), SHELL).unwrap();
    swaymsg(&sway, "output HEADLESS-1 scale 2");
    let (r, w) = std::io::pipe().unwrap();
    let _shell = Kill(
        sway.env(&mut Command::new(env!("CARGO_BIN_EXE_impulse-shell")))
            .arg("-p")
            .arg(dir.path())
            .env("RI_INTERP", "1")
            .env("WAYLAND_DEBUG", "1")
            .env("RI_LOG", "ri::present=debug")
            .stdin(Stdio::null())
            .stdout(w.try_clone().unwrap())
            .stderr(w)
            .spawn()
            .unwrap(),
    );
    let log = Log::capture(r);
    let wait = |what: &str, f: &dyn Fn(&str) -> bool| {
        if let Err(s) = log.wait(f) {
            let keep: Vec<&str> = s
                .lines()
                .filter(|l| l.contains("HIDPI") || l.contains("qml") || l.contains("scale"))
                .collect();
            panic!("impulse-shell exited before {what}:\n{}", keep.join("\n"));
        }
    };
    wait("it loaded", &|s| s.contains("HIDPI ready"));
    wait("devicePixelRatio 2", &|s| s.contains("HIDPI dpr 2"));
    wait("the frame at scale 2 on screen", &|s| {
        presented_after(s, |l| l.contains("HIDPI dpr 2"))
    });
    assert_crisp(&sway, dir.path(), 2.0, (21, 24, 21), [255, 0, 0]);

    swaymsg(&sway, "output HEADLESS-1 scale 1.5");
    wait("devicePixelRatio 1.5", &|s| s.contains("HIDPI dpr 1.5"));
    wait("the frame at scale 1.5 on screen", &|s| {
        presented_after(s, |l| l.contains("HIDPI dpr 1.5"))
    });
    assert_crisp(&sway, dir.path(), 1.5, (45, 48, 15), [0, 255, 0]);
}

/// The region selector's calls (TempScreenshotProcess.qml, RegionSelection.qml,
/// ScreenshotAction.qml): the screen is captured to a file, then the selected region, times the
/// monitor's scale, is cut from it. The region is the red rectangle; the green ring is the
/// logical pixel around it.
const SNIP: &str = r##"import QtQuick
import Rustshell
import Rustshell.Io
import Impulse
ShellRoot {
    PanelWindow {
        id: panel
        anchors { top: true; left: true; right: true; bottom: true }
        exclusionMode: ExclusionMode.Ignore
        color: "#000000"
        readonly property rect region: Qt.rect(200, 100, 160, 80)
        readonly property real monitorScale: devicePixelRatio
        onDevicePixelRatioChanged: console.log("SNIP dpr " + devicePixelRatio)
        Component.onCompleted: console.log("SNIP dpr " + devicePixelRatio)
        Rectangle { x: 199; y: 99; width: 162; height: 82; color: "#00ff00" }
        Rectangle { x: 200; y: 100; width: 160; height: 80; color: "#ff0000" }
        Rectangle { x: 200; y: 100; width: 16; height: 8; color: "#0000ff" }
        Rectangle { x: 344; y: 172; width: 16; height: 8; color: "#ffff00" }
        Process {
            command: ["cat", "@DIR@/go"]
            running: true
            onExited: Screenshot.capture(panel.screen.name, "@DIR@/tmp/image")
        }
        Connections {
            target: Screenshot
            function onCaptured(path, ok) {
                console.log("SNIP captured " + ok)
                Screenshot.copy(path, //
                    panel.region.x * panel.monitorScale, //
                    panel.region.y * panel.monitorScale, //
                    panel.region.width * panel.monitorScale, //
                    panel.region.height * panel.monitorScale, //
                    "@DIR@/saved")
            }
        }
    }
}
"##;

/// Hyprland and sway send wl_output.scale 2 for an output at 1.25. A capture blown up to that
/// scale is 3840x2160, and the region times 1.25 lands on another part of it.
#[test]
#[cfg_attr(not(feature = "interp"), ignore = "temp tree; AOT-only refuses it")]
fn region_screenshot_cuts_the_selected_area_at_fractional_scale() {
    if !testenv::require_all(&["sway", "swaymsg", "grim"]) {
        return;
    }
    let sway = testenv::Sway::start();
    let dir = tempfile::tempdir().unwrap();
    let at = |name: &str| dir.path().join(name);
    let qml = SNIP.replace("@DIR@", dir.path().to_str().unwrap());
    std::fs::write(at("shell.qml"), qml).unwrap();
    std::fs::create_dir(at("saved")).unwrap();
    testenv::fifo(&at("go"));
    // 1920x1080 at 1.25: 1536x864 logical.
    swaymsg(&sway, "output HEADLESS-1 scale 1.25");
    let (r, w) = std::io::pipe().unwrap();
    let _shell = Kill(
        sway.env(&mut Command::new(env!("CARGO_BIN_EXE_impulse-shell")))
            .arg("-p")
            .arg(dir.path())
            .env("RI_INTERP", "1")
            .env("WAYLAND_DEBUG", "1")
            .env("RI_LOG", "ri::present=debug")
            .env("XDG_CONFIG_HOME", at("config"))
            .env("XDG_STATE_HOME", at("state"))
            .env("XDG_CACHE_HOME", at("cache"))
            .stdin(Stdio::null())
            .stdout(w.try_clone().unwrap())
            .stderr(w)
            .spawn()
            .unwrap(),
    );
    let log = Log::capture(r);
    let wait = |what: &str, f: &dyn Fn(&str) -> bool| {
        if let Err(s) = log.wait(f) {
            let keep: Vec<&str> = s
                .lines()
                .filter(|l| l.contains("SNIP") || l.contains("qml") || l.contains("screenshot"))
                .collect();
            panic!("impulse-shell exited before {what}:\n{}", keep.join("\n"));
        }
    };
    wait("the frame at scale 1.25 on screen", &|s| {
        presented_after(s, |l| l.contains("SNIP dpr 1.25"))
    });
    // What was selected, as grim cuts it: `-g` is in logical pixels.
    let want = at("want.png");
    let ok = sway
        .env(&mut Command::new("grim"))
        .args(["-g", "200,100 160x80"])
        .arg(&want)
        .status()
        .is_ok_and(|s| s.success());
    assert!(ok, "grim -g");
    let want = image::open(&want).unwrap().to_rgba8();
    assert_eq!(want.dimensions(), (200, 100), "grim's cut of the region");

    let saved = testenv::DirWatch::new(&at("saved"));
    testenv::release_fifo(&at("go"));
    wait("the capture", &|s| s.contains("SNIP captured"));
    assert!(log.text().contains("SNIP captured true"), "capture failed");
    let cut = || {
        let f = std::fs::read_dir(at("saved")).ok()?.flatten().next()?;
        Some(image::open(f.path()).ok()?.to_rgba8())
    };
    saved.until(|| cut().is_some());
    let got = cut().unwrap();
    assert_eq!(
        got.dimensions(),
        want.dimensions(),
        "size of the cut region"
    );
    let differ = (got.pixels().zip(want.pixels()))
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        differ,
        0,
        "{differ} of {} pixels of the cut are not the selected region's; its corners are {:?} \
         and {:?}, the region's blue and yellow",
        200 * 100,
        got.get_pixel(0, 0),
        got.get_pixel(199, 99)
    );
    // The selection itself: red inside, its blue and yellow corners, nothing of the ring.
    assert_eq!(got.get_pixel(0, 0).0, [0, 0, 255, 255]);
    assert_eq!(got.get_pixel(199, 99).0, [255, 255, 0, 255]);
    assert_eq!(got.get_pixel(100, 50).0, [255, 0, 0, 255]);
    assert!(got.pixels().all(|p| p.0 != [0, 255, 0, 255]));
}
