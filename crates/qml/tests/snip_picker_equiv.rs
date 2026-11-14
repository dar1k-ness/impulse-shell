//! The region snips of the tree's QML end to end: RegionSelection `snip()` and
//! WRegionSelectionPanel `dragArea.onDragReleased`, the tree's own bodies in a scene, through
//! `ScreenshotAction.run` to `Impulse/Screenshot` and `Impulse/ScreenRecorder`. Every case runs
//! in a child test process with fake `wf-recorder` and `swappy` first on PATH recording their
//! working directory, argv and stdin (`notify-send`, `pactl`, `hyprctl` fakes stay silent).
//! What the same selections gave with the original's shell commands (`magick`, record.sh) is what is
//! expected here; every action of the commands themselves is compared in `ports_ab`.

use qml::runtime::Runtime;
use qml::value::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;
use testenv::Wakeups;

fn ri() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

/// The tree's source of the body in `file` that starts with `head`.
fn body(file: &str, head: &str) -> String {
    let text = std::fs::read_to_string(ri().join(file)).unwrap();
    let doc = qml::parse(&text).unwrap();
    let mut v = vec![];
    doc.root.walk(&mut |o| o.scripts(&mut v));
    let s = v.iter().find(|s| s.src.starts_with(head));
    s.unwrap_or_else(|| panic!("{file}: no body {head:?}"))
        .src
        .to_string()
}

const FAKE: &str = r#"#!/bin/sh
n=${0##*/}
f="$RI_AB_LOG/$n.$$"
case $n in
swappy) cat > "$f.stdin" ;;
hyprpicker|wf-recorder) ;;
*) exit 0 ;;
esac
{ echo "$n"; pwd; printf '%s\n' "$@"; } > "$f.tmp"
mv "$f.tmp" "$f.call"
"#;

/// The fake tools, written once before any test starts a process (no ETXTBSY).
fn fakes() -> &'static Path {
    static D: OnceLock<PathBuf> = OnceLock::new();
    D.get_or_init(|| {
        let d = std::env::temp_dir().join(format!("ri-snip-ab-{}-bin", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for n in [
            "hyprpicker",
            "wf-recorder",
            "swappy",
            "notify-send",
            "pactl",
            "hyprctl",
        ] {
            testenv::install_script(&d.join(n), FAKE);
        }
        d
    })
}

/// The screenshot the snips crop (an extensionless PNG, as TempScreenshotProcess writes).
fn screenshot() -> image::RgbaImage {
    image::RgbaImage::from_fn(64, 48, |x, y| {
        image::Rgba([(x * 4) as u8, (y * 5) as u8, ((x ^ y) * 3) as u8, 255])
    })
}

fn pixels(img: &image::RgbaImage) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::hash::DefaultHasher::new();
    img.as_raw().hash(&mut h);
    format!("stdin {}x{} {:016x}", img.width(), img.height(), h.finish())
}

fn crop(x: u32, y: u32, w: u32, h: u32) -> String {
    pixels(&image::imageops::crop_imm(&screenshot(), x, y, w, h).to_image())
}

struct Side {
    state: Vec<String>,
    calls: Vec<Vec<String>>,
}

/// Runs `case` in a child test process; `want` tool calls.
fn side(case: &str, want: usize) -> Side {
    let base = std::env::temp_dir().join(format!("ri-snip-ab-{}-{case}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let home = base.join("home");
    std::fs::create_dir_all(home.join(".config/rusty-impulse")).unwrap();
    std::fs::create_dir_all(base.join("log")).unwrap();
    let videos = home.join("Videos");
    let config = match case {
        // The user's config says nothing about recordings.
        "snip-record-no-save-path" => "{}".to_owned(),
        // The user's own directory.
        "snip-record-own-save-path" => format!(
            r#"{{"screenRecord": {{"savePath": "{}"}}}}"#,
            base.join("my recordings").display()
        ),
        _ => format!(
            r#"{{"screenRecord": {{"savePath": "{}"}}}}"#,
            videos.display()
        ),
    };
    std::fs::write(home.join(".config/rusty-impulse/config.json"), config).unwrap();
    screenshot()
        .save_with_format(base.join("image-DP-1"), image::ImageFormat::Png)
        .unwrap();
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args([
        "--exact",
        "helper",
        "--ignored",
        "--nocapture",
        "--test-threads=1",
    ])
    .env("RI_AB_CASE", case)
    .env("RI_AB_BASE", &base)
    .env("RI_AB_LOG", base.join("log"))
    .env("RI_AB_WANT", want.to_string())
    .env(
        "PATH",
        format!("{}:{}", fakes().display(), std::env::var("PATH").unwrap()),
    )
    .env("HOME", &home)
    .env("XDG_CONFIG_HOME", home.join(".config"))
    .env("XDG_STATE_HOME", base.join("state"))
    .env("XDG_CACHE_HOME", base.join("cache"))
    // No PipeWire: the port looks for its socket in `base`, where there is none.
    .env("PIPEWIRE_RUNTIME_DIR", &base)
    .env("PIPEWIRE_REMOTE", "pipewire-0");
    let o = c.output().unwrap();
    let out = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(o.status.success(), "{case}:\n{out}");
    let b = base.display().to_string();
    let state = std::fs::read_to_string(base.join("state.txt"))
        .unwrap_or_else(|_| panic!("{case} wrote no state:\n{out}"))
        .lines()
        .map(|l| l.replace(&b, "$BASE"))
        .collect();
    let mut calls = vec![];
    for e in std::fs::read_dir(base.join("log")).unwrap().flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "call") {
            continue;
        }
        let mut call: Vec<String> = std::fs::read_to_string(&p)
            .unwrap()
            .lines()
            .map(|l| normalize_date(&l.replace(&b, "$BASE")))
            .collect();
        if let Ok(png) = std::fs::read(p.with_extension("stdin")) {
            call.push(match image::load_from_memory(&png) {
                Ok(i) => pixels(&i.to_rgba8()),
                Err(e) => format!("stdin not an image ({} bytes): {e}", png.len()),
            });
        }
        calls.push(call);
    }
    calls.sort();
    let _ = std::fs::remove_dir_all(&base);
    Side { state, calls }
}

/// `./recording_<date '+%Y-%m-%d_%H.%M.%S'>.mp4` with the time left out.
fn normalize_date(l: &str) -> String {
    match l
        .strip_prefix("./recording_")
        .and_then(|r| r.strip_suffix(".mp4"))
    {
        Some(d)
            if d.len() == 19
                && d.char_indices().all(|(i, c)| match i {
                    4 | 7 => c == '-',
                    10 => c == '_',
                    13 | 16 => c == '.',
                    _ => c.is_ascii_digit(),
                }) =>
        {
            "./recording_DATE.mp4".into()
        }
        _ => l.to_string(),
    }
}

/// One case at a time: the recorder stops (instead of starts) a recording when any
/// `wf-recorder` runs, so a parallel case's fake one would turn a start into a stop.
fn run(case: &str, want: usize) -> Side {
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    side(case, want)
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn tools(extra: &[&str]) -> bool {
    let mut t = vec!["bash", "sh", "mv", "cat", "pwd"];
    t.extend(extra);
    testenv::require_all(&t)
}

/// The `wf-recorder` call for a region recording. There is no PipeWire in the test
/// environment, so it records without audio (`record::audio_arg`); with a default sink its
/// monitor is passed (script_ports_equiv `record_script_matches_native`).
fn recorder(geometry: &str) -> Vec<String> {
    s(&[
        "wf-recorder",
        "$BASE/home/Videos",
        "--pixel-format",
        "yuv420p",
        "-f",
        "./recording_DATE.mp4",
        "-t",
        "--geometry",
        geometry,
    ])
}

fn has(side: &Side, kv: &str) -> bool {
    side.state.iter().any(|l| l == kv)
}

/// RegionSelection.qml `snip()`: the region clamped to the screen and scaled, record modes
/// leaving the selector in its Post phase.
#[test]
fn region_selection_snip_records() {
    if !tools(&[]) {
        return;
    }
    let n = run("snip-record", 1);
    assert_eq!(n.calls, [recorder("2430,0 450x300")]);
    assert!(
        has(&n, "phase=1") && has(&n, "dismissed=0"),
        "{:?}",
        n.state
    );
    assert!(
        has(&n, "regionX=1620") && has(&n, "regionY=0"),
        "{:?}",
        n.state
    );
    let n = run("snip-sound", 1);
    assert_eq!(n.calls, [recorder("16,31 150x75")]);
    assert!(
        has(&n, "phase=1") && has(&n, "dismissed=0"),
        "{:?}",
        n.state
    );
}

/// The recording is saved where `Config.options.screenRecord.savePath` says: the user's
/// directory when the config file names one, else the default the QML declares (the videos
/// directory). A config file without the key never made a directory called `null`.
#[test]
fn recording_is_saved_in_the_configured_directory() {
    if !tools(&[]) {
        return;
    }
    let n = run("snip-record-no-save-path", 1);
    assert_eq!(n.calls, [recorder("2430,0 450x300")]);
    let mut own = recorder("2430,0 450x300");
    own[1] = "$BASE/my recordings".into();
    let n = run("snip-record-own-save-path", 1);
    assert_eq!(n.calls, [own]);
}

/// RegionSelection.qml `snip()` with Copy and the right button: Edit, the crop piped to
/// `swappy -f -` and the screenshot removed.
#[test]
fn region_selection_snip_edits() {
    if !tools(&[]) {
        return;
    }
    let n = run("snip-edit", 1);
    let mut want = s(&["swappy", env!("CARGO_MANIFEST_DIR"), "-f", "-"]);
    want.push(crop(8, 9, 30, 15));
    assert_eq!(n.calls, [want]);
    assert!(
        has(&n, "shot=false") && has(&n, "dismissed=1"),
        "{:?}",
        n.state
    );
}

/// An empty region only dismisses (the original went on to run record.sh with a 0-wide `--region`).
#[test]
fn region_selection_snip_empty_region_only_dismisses() {
    if !tools(&[]) {
        return;
    }
    let n = run("snip-empty", 0);
    assert!(n.calls.is_empty(), "{:?}", n.calls);
    assert!(
        has(&n, "dismissed=1") && has(&n, "phase=0"),
        "{:?}",
        n.state
    );
}

/// WRegionSelectionPanel.qml `dragArea.onDragReleased`: the menu action edits the scaled
/// selection, a 0-wide one does nothing, a snip closes the panel.
#[test]
fn wregion_selection_panel_on_drag_released_snips() {
    if !tools(&[]) {
        return;
    }
    let n = run("wsnip-edit", 1);
    let mut want = s(&["swappy", env!("CARGO_MANIFEST_DIR"), "-f", "-"]);
    want.push(crop(5, 5, 20, 15));
    assert_eq!(n.calls, [want]);
    assert!(has(&n, "closes=1"), "{:?}", n.state);
    let n = run("wsnip-zero", 0);
    assert!(n.calls.is_empty(), "{:?}", n.calls);
    assert!(has(&n, "closes=0"), "{:?}", n.state);
}

/// A video selection records (the original read `root.videoAction`, which the panel never declared, so
/// it got no command and only closed).
#[test]
fn wregion_selection_panel_on_drag_released_video_records() {
    if !tools(&[]) {
        return;
    }
    let n = run("wsnip-video", 1);
    assert_eq!(n.calls, [recorder("125,250 800x450")]);
    assert!(has(&n, "closes=1"), "{:?}", n.state);
}

// The child side.

const HEAD: &str = "import QtQuick\nimport Quickshell\nimport Rustshell.Io\nimport ri\nimport ri.services\nimport ri.modules.common\nimport ri.modules.common.utils\n";

fn region_selection(action: &str, button: &str, rect: [f64; 4], shot: &Path) -> String {
    let file = "modules/rusty/regionSelector/RegionSelection.qml";
    format!(
        "{HEAD}Item {{\n    id: root\n\
         enum SnipAction {{ Copy, Edit, Search, CharRecognition, Record, RecordWithSound }}\n\
         enum SelectionMode {{ RectCorners, Circle }}\n\
         enum Phase {{ Select, Post }}\n\
         property var action: RegionSelection.SnipAction.{action}\n\
         property var selectionMode: RegionSelection.SelectionMode.Circle\n\
         property var phase: RegionSelection.Phase.Select\n\
         property int dismissed: 0\n\
         // The shell has read its config file long before a snip.\n\
         property bool configReady: Config.ready\n\
         signal dismiss()\n\
         onDismiss: root.dismissed += 1\n\
         property var screen: ({{ width: 1920, height: 1080, name: \"DP-1\" }})\n\
         property real monitorScale: 1.5\n\
         property string screenshotPath: \"{}\"\n\
         property var mouseButton: {button}\n\
         property real regionX: {}\n property real regionY: {}\n\
         property real regionWidth: {}\n property real regionHeight: {}\n\
         {}\n{}\n}}\n",
        shot.display(),
        rect[0],
        rect[1],
        rect[2],
        rect[3],
        body(file, "function getScreenshotAction()"),
        body(file, "function snip()"),
    )
}

fn wregion_selection_panel(media: &str, image: &str, sel: [i32; 4], shot: &Path) -> String {
    let file = "modules/waffle/screenSnip/WRegionSelectionPanel.qml";
    format!(
        "{HEAD}Item {{\n    id: root\n\
         enum MediaType {{ Image, Video }}\n\
         enum ImageAction {{ Copy, Menu, CharRecognition, Search }}\n\
         enum VideoAction {{ Record, RecordWithSound }}\n\
         property var mediaType: WRegionSelectionPanel.MediaType.{media}\n\
         property var imageAction: WRegionSelectionPanel.ImageAction.{image}\n\
         property int closes: 0\n\
         signal closed()\n\
         onClosed: root.closes += 1\n\
         function close() {{ root.closed(); }}\n\
         property real monitorScale: 1.25\n\
         property string screenshotPath: \"{}\"\n\
         property var videoAction: WRegionSelectionPanel.VideoAction.Record\n\
         {}\n\
         Item {{\n id: dragArea\n\
         property int selectionX: {}\n property int selectionY: {}\n\
         property int selectionWidth: {}\n property int selectionHeight: {}\n\
         signal dragReleased(real diffX, real diffY)\n\
         onDragReleased: {}\n }}\n\
         function fire() {{ dragArea.dragReleased(0, 0); }}\n}}\n",
        shot.display(),
        body(file, "function getScreenshotAction()"),
        sel[0],
        sel[1],
        sel[2],
        sel[3],
        body(file, "(diffX, diffY) =>"),
    )
}

/// One case, in a child process (see [`side`]).
/// The runtime on a virtual clock (`Runtime::tick`) instead of wall time. Runs what is due,
/// blocking on the runtime's wakeups while work it started is unanswered (`busy`); a hang is
/// nextest's slow-timeout to end.
struct Clock {
    wake: Wakeups,
    ep: Option<i32>,
    now: f64,
}

impl Clock {
    fn new(rt: &mut Runtime) -> Clock {
        let wake = Wakeups::new();
        rt.world.borrow().waker.set_ping(wake.ping());
        let ep = rt.process_fd();
        Clock { wake, ep, now: 0.0 }
    }

    /// Runs everything due at `now`, until no work is unanswered and no wakeup is pending.
    fn drain(&mut self, rt: &mut Runtime) {
        loop {
            workpool::wait_idle();
            rt.poll_native();
            rt.tick(self.now);
            rt.poll_native();
            rt.settle();
            let busy = busy(&rt.world.borrow());
            let woke = self.wake.wait(self.ep, (!busy).then_some(Duration::ZERO));
            if woke.fd {
                rt.process_fd_ready();
            }
            if !busy && !woke.any() && rt.next_deadline(self.now) > 0.0 {
                return;
            }
        }
    }

    /// Moves the clock `ms` on, timer by timer, draining at each.
    fn advance(&mut self, rt: &mut Runtime, ms: f64) {
        let end = self.now + ms;
        loop {
            self.drain(rt);
            let d = rt.next_deadline(self.now);
            if self.now + d > end {
                self.now = end;
                self.drain(rt);
                return;
            }
            self.now += d;
        }
    }
}

/// A process the shell runs is unanswered.
fn busy(w: &qml::runtime::World) -> bool {
    !w.procs.is_empty()
}

/// Reaps every child left (the detached tools, reparented here by `PR_SET_CHILD_SUBREAPER`)
/// as each exits; whether there was any.
fn reap_all() -> bool {
    let mut any = false;
    loop {
        // SAFETY: waitpid(2) with a null status pointer writes no memory.
        if unsafe { libc::waitpid(-1, std::ptr::null_mut(), 0) } > 0 {
            any = true;
        } else if std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
            return any;
        }
    }
}

#[test]
#[ignore = "child process: the tests of this file run it for each case (RI_AB_CASE)"]
fn helper() {
    let Ok(case) = std::env::var("RI_AB_CASE") else {
        return;
    };
    let base = PathBuf::from(std::env::var("RI_AB_BASE").unwrap());
    let log = PathBuf::from(std::env::var("RI_AB_LOG").unwrap());
    let want: usize = std::env::var("RI_AB_WANT").unwrap().parse().unwrap();
    let shot = base.join("image-DP-1");
    // The tools `execDetached` double-forks are reparented to this process, which waits for
    // them (`reap_all`) instead of watching the clock.
    // SAFETY: PR_SET_CHILD_SUBREAPER takes an integer flag; prctl(2) reads no memory for it.
    assert_eq!(unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1) }, 0);
    let mut rt = Runtime::new(Some(ri()));
    rt.load_singletons_lazy(&[
        ri(),
        ri().join("services"),
        ri().join("modules/common"),
        ri().join("modules/common/functions"),
        ri().join("modules/common/utils"),
    ]);
    let mut clock = Clock::new(&mut rt);
    let scene = |rt: &mut Runtime, name: &str, src: String| {
        let f = base.join(name);
        std::fs::write(&f, src).unwrap();
        rt.load_file(&f).unwrap_or_else(|e| panic!("{name}: {e}"))
    };
    let (root, call, keys): (usize, &str, &[&str]) = match case.as_str() {
        c if c.starts_with("snip-") => {
            let (action, button, rect) = match c {
                "snip-record" | "snip-record-no-save-path" | "snip-record-own-save-path" => {
                    ("Record", "null", [1800.0, -20.0, 300.0, 200.0])
                }
                "snip-sound" => ("RecordWithSound", "null", [10.4, 20.6, 100.2, 50.0]),
                "snip-edit" => ("Copy", "Qt.RightButton", [5.0, 6.0, 20.0, 10.0]),
                "snip-empty" => ("Record", "null", [100.0, 100.0, 0.0, 50.0]),
                _ => panic!("{c}"),
            };
            let src = region_selection(action, button, rect, &shot);
            let keys: &[&str] = &[
                "action",
                "phase",
                "selectionMode",
                "dismissed",
                "regionX",
                "regionY",
                "regionWidth",
                "regionHeight",
            ];
            (scene(&mut rt, "RegionSelection.qml", src), "snip", keys)
        }
        c if c.starts_with("wsnip-") => {
            let (media, image, sel) = match c {
                "wsnip-edit" => ("Image", "Menu", [4, 4, 16, 12]),
                "wsnip-zero" => ("Image", "Copy", [4, 4, 0, 12]),
                "wsnip-video" => ("Video", "Copy", [100, 200, 640, 360]),
                _ => panic!("{c}"),
            };
            let src = wregion_selection_panel(media, image, sel, &shot);
            (
                scene(&mut rt, "WRegionSelectionPanel.qml", src),
                "fire",
                &["closes"],
            )
        }
        c => panic!("unknown case {c}"),
    };
    clock.advance(&mut rt, 300.0);
    if case.starts_with("snip-") {
        assert!(rt.get(root, "configReady").truthy(), "{:?}", rt.errors());
    }
    match rt.call_method(root, call, vec![]) {
        Some(Ok(_)) => {}
        r => panic!("{call}: {r:?} {:?}", rt.errors()),
    }
    let calls = || {
        std::fs::read_dir(&log)
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "call"))
            .count()
    };
    let edits = case.ends_with("-edit");
    // On the runtime's clock, each 50 ms step ending when every tool it started has exited.
    let until = clock.now + if want == 0 { 2000.0 } else { 8000.0 };
    loop {
        clock.advance(&mut rt, 50.0);
        let reaped = reap_all();
        if !reaped && (clock.now >= until || (calls() >= want && !(edits && shot.exists()))) {
            break;
        }
    }
    clock.advance(&mut rt, 300.0);
    reap_all();
    let mut state: Vec<String> = keys
        .iter()
        .map(|k| {
            format!(
                "{k}={}",
                match rt.get(root, k) {
                    Value::Undefined => "undefined".into(),
                    v => v.to_js_string(),
                }
            )
        })
        .collect();
    state.push(format!("shot={}", shot.exists()));
    eprintln!("{case}: {state:?} errors {:?}", rt.errors());
    std::fs::write(base.join("state.txt"), state.join("\n")).unwrap();
}
