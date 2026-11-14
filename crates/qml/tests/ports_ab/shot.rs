//! Screenshots: `ScreenshotAction.run` for every action, the region selector's temporary
//! capture and the overlay Recorder's screenshot button, against the shell commands the original ran for
//! them (kept below as the reference). The reference side (`RI_AB_REFERENCE=1`) runs the real
//! `grim` (on the test compositor), `magick`, `wl-copy`, `curl`, `jq`, `tesseract` and
//! `record.sh`; the other side calls `Impulse/Screenshot` and `Impulse/ScreenRecorder` the way
//! the tree's QML does. Only the GUI programs (`swappy`, `satty`, `xdg-open`, `wf-recorder`) are
//! loggers. Compared: clipboard types and contents (pixels for images), the files left behind
//! (pixels), launched argv, uploads, notifications and QML state.

use crate::ab::{self, Notes, Scene};
use std::cell::RefCell;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};

const OUTPUT: &str = "HEADLESS-1";

fn wayland(tools: &[&str]) -> bool {
    let on = std::env::var_os("RI_PORTS_WAYLAND").is_some()
        && std::env::var_os("WAYLAND_DISPLAY").is_some();
    if !on {
        testenv::unavailable(
            "the screenshot A/B tests need a compositor they may use: run under one with RI_PORTS_WAYLAND=1",
        );
        return false;
    }
    ab::require(tools)
}

/// A test pattern (no flat areas, so a crop off by one pixel or a channel mix-up shows).
fn pattern(w: u32, h: u32) -> image::RgbImage {
    image::RgbImage::from_fn(w, h, |x, y| {
        let b = if (x / 37 + y / 23) % 2 == 0 { 40 } else { 210 };
        image::Rgb([(x * 255 / w) as u8, (y * 255 / h) as u8, b ^ (x % 7) as u8])
    })
}

/// The pattern as the output's wallpaper, for as long as it lives.
struct Wallpaper {
    swaybg: std::process::Child,
    _dir: tempfile::TempDir,
}

impl Drop for Wallpaper {
    fn drop(&mut self) {
        let _ = self.swaybg.kill();
        testenv::wait_exit(&mut self.swaybg);
    }
}

/// Puts the pattern on the output as its wallpaper, so captures of the screen have something
/// to compare: a swaybg of the test's own, returned once it committed the pattern's buffer to
/// its surface (its WAYLAND_DEBUG log), which the compositor's next frame, the one a capture
/// copies, shows.
fn screen() -> Wallpaper {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("bg.png");
    pattern(1920, 1080).save(&p).unwrap();
    let mut c = Command::new("swaybg")
        .args(["-o", OUTPUT, "-m", "fill", "-i"])
        .arg(&p)
        .env("WAYLAND_DEBUG", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let log = testenv::Log::capture(c.stderr.take().unwrap());
    // `-> wl_surface#N.attach(wl_buffer#M, 0, 0)` then `-> wl_surface#N.commit()` (`@` for
    // `#` before libwayland 1.23).
    log.wait(|l| {
        l.rfind(".attach(wl_buffer")
            .is_some_and(|i| l[i..].contains(".commit()"))
    })
    .unwrap_or_else(|l| panic!("swaybg exited before drawing:\n{l}"));
    Wallpaper {
        swaybg: c,
        _dir: dir,
    }
}

fn hash(b: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::hash::DefaultHasher::new();
    b.hash(&mut h);
    h.finish()
}

/// What an image looks like (size and pixels), whatever its encoding.
fn pixels(b: &[u8]) -> String {
    match image::load_from_memory(b) {
        Ok(img) => format!(
            "image {}x{} {:016x}",
            img.width(),
            img.height(),
            hash(img.to_rgba8().as_raw())
        ),
        Err(_) if b.len() <= 200 => format!("data {:?}", String::from_utf8_lossy(b)),
        Err(_) => format!("data {} bytes {:016x}", b.len(), hash(b)),
    }
}

fn undated(s: &str) -> String {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"\d{4}-\d\d-\d\d_\d\d\.\d\d\.\d\d").unwrap())
        .replace_all(s, "<date>")
        .into_owned()
}

/// Every file under `dir` (dated names normalized) with its contents.
fn files(dir: &Path) -> String {
    fn walk(base: &Path, d: &Path, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let rel = p.strip_prefix(base).unwrap().display().to_string();
            if p.is_dir() {
                out.push(format!("{}/", undated(&rel)));
                walk(base, &p, out);
            } else {
                out.push(format!(
                    "{} {}",
                    undated(&rel),
                    pixels(&std::fs::read(&p).unwrap_or_default())
                ));
            }
        }
    }
    let mut v = vec![];
    walk(dir, dir, &mut v);
    v.sort();
    v.join("\n")
}

fn wl_paste(args: &[&str]) -> Option<Vec<u8>> {
    let o = Command::new("wl-paste")
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    o.status.success().then_some(o.stdout)
}

/// The clipboard selection: offered types and the contents. The set of types is what a paste
/// sees; wl-clipboard-rs offers its extra text types in hash order.
fn clipboard_now() -> String {
    let Some(types) = wl_paste(&["--list-types"]) else {
        return String::new();
    };
    let mut types: Vec<String> = String::from_utf8_lossy(&types)
        .lines()
        .map(str::to_owned)
        .collect();
    types.sort();
    types.dedup();
    let body = if types.iter().any(|t| t == "image/png") {
        wl_paste(&["--no-newline", "--type", "image/png"]).map(|b| pixels(&b))
    } else {
        wl_paste(&["--no-newline"]).map(|b| format!("text {:?}", String::from_utf8_lossy(&b)))
    };
    format!("types {types:?} {}", body.unwrap_or_default())
}

/// The last clipboard contents seen during a side: the port serves the selection from the
/// shell process, which a long-running shell keeps but the A/B child exits.
struct Clip(RefCell<String>);

impl Clip {
    fn new() -> Clip {
        Clip(RefCell::new(String::new()))
    }

    fn reset(&self) {
        let _ = Command::new("wl-copy").arg("--clear").status();
        self.0.borrow_mut().clear();
    }

    fn get(&self) -> String {
        let now = clipboard_now();
        if !now.is_empty() {
            *self.0.borrow_mut() = now;
        }
        format!("clipboard: {}", self.0.borrow())
    }
}

/// A file-upload endpoint (`curl -sF files[]=@file`): records each request and its parts,
/// answers with an uguu-style `{"files":[{"url":…}]}`.
struct Upload {
    url: String,
    log: Arc<Mutex<Vec<String>>>,
}

const UPLOADED: &str = "https://files.example/u/abc.png";

impl Upload {
    fn start() -> Upload {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}/upload", l.local_addr().unwrap().port());
        let log = Arc::new(Mutex::new(Vec::new()));
        let lg = log.clone();
        std::thread::spawn(move || {
            for c in l.incoming().map_while(Result::ok) {
                let lg = lg.clone();
                std::thread::spawn(move || Upload::serve(c, &lg));
            }
        });
        Upload { url, log }
    }

    /// Records the request before answering it: curl is done once it has the answer.
    fn serve(mut c: std::net::TcpStream, log: &Mutex<Vec<String>>) -> Option<()> {
        let mut buf = Vec::new();
        let mut b = [0u8; 8192];
        let end = loop {
            if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break i;
            }
            let n = c.read(&mut b).ok()?;
            if n == 0 {
                return None;
            }
            buf.extend_from_slice(&b[..n]);
        };
        let head = String::from_utf8_lossy(&buf[..end]).into_owned();
        let mut lines = head.split("\r\n");
        let request = lines.next()?.to_owned();
        let hdr = |name: &str| {
            head.split("\r\n").skip(1).find_map(|l| {
                let (k, v) = l.split_once(':')?;
                k.eq_ignore_ascii_case(name).then(|| v.trim().to_owned())
            })
        };
        if hdr("expect").is_some_and(|e| e.eq_ignore_ascii_case("100-continue")) {
            c.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").ok()?;
        }
        let len: usize = hdr("content-length")?.parse().ok()?;
        let mut body = buf[end + 4..].to_vec();
        while body.len() < len {
            let n = c.read(&mut b).ok()?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&b[..n]);
        }
        let ctype = hdr("content-type").unwrap_or_default();
        let (kind, boundary) = ctype.split_once("; boundary=").unwrap_or((&ctype, ""));
        let mut parts = vec![];
        let sep = format!("--{boundary}");
        let mut rest = &body[..];
        while let Some(i) = find(rest, sep.as_bytes()) {
            rest = &rest[i + sep.len()..];
            if rest.starts_with(b"--") {
                break;
            }
            let Some(h) = find(rest, b"\r\n\r\n") else {
                break;
            };
            let ph = String::from_utf8_lossy(&rest[2..h]).replace("\r\n", " | ");
            let data = &rest[h + 4..];
            let n = find(data, format!("\r\n{sep}").as_bytes()).unwrap_or(data.len());
            parts.push(format!("[{ph}] {}", pixels(&data[..n])));
            rest = &data[n..];
        }
        log.lock()
            .unwrap()
            .push(format!("{request} {kind} {parts:?}"));
        let reply = format!("{{\"success\":true,\"files\":[{{\"url\":\"{UPLOADED}\"}}]}}");
        let _ = write!(
            c,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
            reply.len()
        );
        Some(())
    }

    fn calls(&self) -> String {
        let mut v = self.log.lock().unwrap().clone();
        v.sort();
        v.join("\n")
    }
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

/// The original's `ScreenshotAction.getCommand`, which built the shell command of an action.
const GET_COMMAND: &str = r#"function getCommand(x, y, width, height, screenshotPath, action, saveDir = "") {
        // Set command for action
        const rx = Math.round(x);
        const ry = Math.round(y);
        const rw = Math.round(width);
        const rh = Math.round(height);
        const cropBase = `magick ${StringUtils.shellSingleQuoteEscape(screenshotPath)} `
            + `-crop ${rw}x${rh}+${rx}+${ry} +repage`
        const cropToStdout = `${cropBase} -`
        const cropInPlace = `${cropBase} '${StringUtils.shellSingleQuoteEscape(screenshotPath)}'`
        const cleanup = `rm '${StringUtils.shellSingleQuoteEscape(screenshotPath)}'`
        const slurpRegion = `${rx},${ry} ${rw}x${rh}`
        const uploadAndGetUrl = (filePath) => {
            return `curl -sF files[]=@'${StringUtils.shellSingleQuoteEscape(filePath)}' ${root.fileUploadApiEndpoint} | jq -r '.files[0].url'`
        }
        const annotationCommand = `${Config.options.regionSelector.annotation.useSatty ? "satty" : "swappy"} -f -`;
        switch (action) {
            case ScreenshotAction.Action.Copy:
                if (saveDir === "") {
                    // not saving the screenshot, just copy to clipboard
                    return ["bash", "-c", `${cropToStdout} | wl-copy && ${cleanup}`]
                    break;
                }
                return [
                    "bash", "-c",
                    `mkdir -p '${StringUtils.shellSingleQuoteEscape(saveDir)}' && \
                    saveFileName="screenshot-$(date '+%Y-%m-%d_%H.%M.%S').png" && \
                    savePath="${saveDir}/$saveFileName" && \
                    ${cropToStdout} | tee >(wl-copy) > "$savePath" && \
                    ${cleanup}`
                ]

                break;
            case ScreenshotAction.Action.Edit:
                return ["bash", "-c", `${cropToStdout} | ${annotationCommand} && ${cleanup}`]
                break;
            case ScreenshotAction.Action.Search:
                return ["bash", "-c", `${cropInPlace} && xdg-open "${root.imageSearchEngineBaseUrl}$(${uploadAndGetUrl(screenshotPath)})" && ${cleanup}`]
                break;
            case ScreenshotAction.Action.CharRecognition:
                return ["bash", "-c", `${cropInPlace} && tesseract '${StringUtils.shellSingleQuoteEscape(screenshotPath)}' stdout -l $(tesseract --list-langs | awk 'NR>1{print $1}' | tr '\\n' '+' | sed 's/\\+$/\\n/') | wl-copy && ${cleanup}`]
                break;
            case ScreenshotAction.Action.Record:
                return ["bash", "-c", `${Directories.recordScriptPath} --region '${slurpRegion}'`]
                break;
            case ScreenshotAction.Action.RecordWithSound:
                return ["bash", "-c", `${Directories.recordScriptPath} --region '${slurpRegion}' --sound`]
                break;
            default:
                console.warn("[Region Selector] Unknown snip action, skipping snip.");
                return;
        }
    }"#;

/// The original's `TempScreenshotProcess.command`.
const TEMP_COMMAND: &str = r#"["bash", "-c", `mkdir -p '${StringUtils.shellSingleQuoteEscape(screenshotDir)}' && grim -o '${StringUtils.shellSingleQuoteEscape(screen.name)}' '${StringUtils.shellSingleQuoteEscape(screenshotPath)}'`]"#;

/// The original's Recorder.qml screenshot button `onClicked`.
const RECORDER_CLICKED: &str = r#"{
                        GlobalStates.overlayOpen = false;
                        Rustshell.execDetached(["bash", "-c", "grim - | wl-copy"]);
                    }"#;

/// The script of the tree's `file` that starts with `head`.
fn body(file: &str, head: &str) -> String {
    let text = std::fs::read_to_string(ab::ri().join(file)).unwrap();
    let doc = qml::parse(&text).unwrap();
    let mut v = vec![];
    doc.root.walk(&mut |o| o.scripts(&mut v));
    let s = v.iter().find(|s| s.src.starts_with(head));
    s.unwrap_or_else(|| panic!("{file}: no body {head:?}"))
        .src
        .to_string()
}

/// `root.original` is the reference side: the fixture runs the original's shell command there.
fn fixture(sc: &Scene, body: &str, subs: &[(&str, &str)]) {
    let mut t = format!(
        "import QtQuick\nimport Quickshell\nimport Rustshell.Io\nimport ri.services\nimport Impulse\n\nItem {{\n    id: root\n    readonly property bool original: Rustshell.env(\"RI_AB_REFERENCE\") === \"1\"\n{body}\n}}\n"
    );
    for (k, v) in subs {
        t = t.replace(k, v);
    }
    sc.main(&t);
}

const ACTION_MAIN: &str = r#"    property string result: ""
    function go(input, i) {
        if (i > 0)
            return 0;
        const c = JSON.parse(input);
        Config.options.regionSelector.annotation.useSatty = c.satty === true;
        if (!root.original) {
            if (c.saveDir === undefined)
                ScreenshotAction.run(c.x, c.y, c.w, c.h, c.path, c.action);
            else
                ScreenshotAction.run(c.x, c.y, c.w, c.h, c.path, c.action, c.saveDir);
            return c.wait;
        }
        const command = c.saveDir === undefined
            ? ScreenshotAction.getCommand(c.x, c.y, c.w, c.h, c.path, c.action)
            : ScreenshotAction.getCommand(c.x, c.y, c.w, c.h, c.path, c.action, c.saveDir);
        // the original's RegionSelection.qml snip(): the command ran detached.
        if (command !== undefined)
            Rustshell.execDetached(command);
        return c.wait;
    }
    function state() { return "" }"#;

/// qml/modules/common/utils/ScreenshotAction.qml `run` against the original's `getCommand` +
/// execDetached: every `ScreenshotAction.Action` (and an unknown one).
#[test]
fn screenshot_action_ab() {
    if !wayland(&[
        "magick",
        "wl-copy",
        "wl-paste",
        "curl",
        "jq",
        "tesseract",
        "dbus-daemon",
        "notify-send",
    ]) {
        return;
    }
    let _l = crate::clip::seat();
    let mut sc = Scene::new("shot-action");
    let Some(notes) = Notes::start(&mut sc) else {
        return;
    };
    let upload = Upload::start();
    let dir = sc.dir.clone();
    sc.trace(&[
        "magick",
        "wl-copy",
        "curl",
        "jq",
        "tesseract",
        "notify-send",
    ]);
    for tool in ["swappy", "satty"] {
        sc.logger(tool, &format!("cat > '{}/{tool}.stdin'", dir.display()));
    }
    sc.logger("xdg-open", "");
    sc.logger(
        "wf-recorder",
        &format!("pwd >> '{}/wf-recorder.log'", dir.display()),
    );
    // No PulseAudio/PipeWire server in the test: `pactl get-default-sink` fails, and the port
    // looks for PipeWire's socket in the scene, where there is none.
    sc.stub("pactl", "exit 1");
    let dir_s = dir.display().to_string();
    sc.set("PIPEWIRE_RUNTIME_DIR", &dir_s);
    sc.set("PIPEWIRE_REMOTE", "pipewire-0");
    let videos = dir.join("videos");
    let cfg = dir.join("home/.config/rusty-impulse/config.json");
    std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
    std::fs::write(
        &cfg,
        format!(
            "{{\"screenRecord\":{{\"savePath\":\"{}\"}}}}",
            videos.display()
        ),
    )
    .unwrap();

    sc.service(
        "Config",
        &format!(
            "QtObject {{\n    property var options: ({{ regionSelector: {{ annotation: {{ useSatty: false }} }}, screenRecord: {{ savePath: \"{}\" }}, search: {{ imageSearch: {{ imageSearchEngineBaseUrl: \"https://lens.example/upload?url=\" }} }} }})\n}}",
            videos.display()
        ),
    );
    let record = ab::unshipped().join("scripts/videos/record.sh");
    let record = record.canonicalize().unwrap_or(record);
    sc.service(
        "Directories",
        &format!(
            "QtObject {{\n    property string recordScriptPath: \"{}\"\n}}",
            record.display()
        ),
    );
    sc.service(
        "ScreenshotAction",
        &format!(
            "import Impulse\n\nSingleton {{\n    id: root\n\n    enum Action {{\n        Copy,\n        Edit,\n        Search,\n        CharRecognition,\n        Record,\n        RecordWithSound\n    }}\n\n    property string imageSearchEngineBaseUrl: Config.options.search.imageSearch.imageSearchEngineBaseUrl\n    property string fileUploadApiEndpoint: \"{}\"\n\n    {}\n\n    {}\n}}",
            upload.url,
            GET_COMMAND,
            body(
                "modules/common/utils/ScreenshotAction.qml",
                "function run("
            )
        ),
    );
    fixture(&sc, ACTION_MAIN, &[]);

    // The region selector's temporary screenshot: grim's PNG, no extension.
    let shots = dir.join("shot");
    let shot = shots.join(format!("image-{OUTPUT}"));
    let pat = dir.join("pattern.png");
    pattern(640, 480).save(&pat).unwrap();
    let text = dir.join("text.png");
    // magick's default font draws nothing in the testenv: OCR would read "" on both sides.
    let font = Command::new("fc-match")
        .args(["-f", "%{file}", "sans"])
        .output()
        .unwrap();
    let font = String::from_utf8(font.stdout).unwrap();
    assert!(Path::new(&font).is_file(), "no sans font: {font:?}");
    let o = Command::new("magick")
        .args(["-font", &font])
        .args([
            "-size",
            "900x160",
            "xc:white",
            "-fill",
            "black",
            "-pointsize",
            "64",
        ])
        .args(["-annotate", "+30+105", "Quick brown fox 42"])
        .arg(format!("png:{}", text.display()))
        .output()
        .unwrap();
    assert!(o.status.success(), "{o:?}");
    let save = dir.join("Pictures/Screen shots");

    let input = |action: i32, x: f64, y: f64, w: f64, h: f64, extra: &str| {
        format!(
            r#"{{"action":{action},"x":{x},"y":{y},"w":{w},"h":{h},"path":{:?},"wait":3000{extra}}}"#,
            shot.display().to_string()
        )
    };
    let save_dir = format!(r#","saveDir":{:?}"#, save.display().to_string());
    let inputs = [
        // Copy
        input(0, 10.4, 20.6, 300.0, 200.5, ""),
        input(0, 0.0, 0.0, 640.0, 480.0, r#","saveDir":"""#),
        input(0, 33.0, 44.0, 128.0, 96.0, &save_dir),
        input(0, 600.0, 400.0, 200.0, 200.0, ""),
        input(0, -20.0, -30.0, 100.0, 100.0, ""),
        input(0, 5000.0, 10.0, 100.0, 100.0, ""),
        // Edit: swappy, satty
        input(1, 12.0, 34.0, 256.0, 128.0, ""),
        input(1, 12.0, 34.0, 256.0, 128.0, r#","satty":true"#),
        // Search
        input(2, 100.0, 50.0, 64.0, 48.0, ""),
        // CharRecognition (the text image)
        input(3, 0.0, 0.0, 900.0, 160.0, r#","src":"text","wait":6000"#),
        // Record, RecordWithSound
        input(4, 10.6, 20.4, 300.0, 200.0, ""),
        input(5, 1.0, 2.0, 3.0, 4.0, ""),
        // Unknown action (last: its warning is checked in the child logs below)
        input(42, 0.0, 0.0, 10.0, 10.0, ""),
    ];
    let inputs: Vec<&str> = inputs.iter().map(String::as_str).collect();
    let clip = Clip::new();
    // record.sh with no PulseAudio server passes wf-recorder `--audio=.monitor`, a device that
    // does not exist; the port records without audio then (`record::audio_arg`). With a default
    // sink both pass its monitor (script_ports_equiv `record_script_matches_native`).
    let no_sink = |_: &str, fx: &str| fx.replace("|--audio=.monitor|", "|");
    let runs = ab::ab_with_fx(
        &sc,
        &inputs,
        &mut |input| {
            clip.reset();
            notes.clear();
            upload.log.lock().unwrap().clear();
            for p in [&shots, &save, &videos] {
                let _ = std::fs::remove_dir_all(p);
            }
            for f in [
                "swappy.log",
                "swappy.stdin",
                "satty.log",
                "satty.stdin",
                "xdg-open.log",
                "wf-recorder.log",
            ] {
                sc.rm(f);
            }
            std::fs::create_dir_all(&shots).unwrap();
            let c: serde_json::Value = serde_json::from_str(input).unwrap();
            let from = if c["src"] == "text" { &text } else { &pat };
            std::fs::copy(from, &shot).unwrap();
        },
        &mut || {
            let mut fx = vec![
                clip.get(),
                format!("temp:\n{}", files(&shots)),
                format!("saved:\n{}", files(&save)),
                format!("videos:\n{}", files(&videos)),
                format!("upload: {}", upload.calls()),
                format!("notifications:\n{}", notes.calls()),
            ];
            for f in ["swappy", "satty", "xdg-open", "wf-recorder"] {
                fx.push(format!("{f}: {}", sc.read(&format!("{f}.log"))));
            }
            for f in ["swappy.stdin", "satty.stdin"] {
                if let Ok(b) = std::fs::read(dir.join(f)) {
                    fx.push(format!("{f}: {}", pixels(&b)));
                }
            }
            undated(&fx.join("\n"))
        },
        &|s| s.to_string(),
        &no_sink,
    );
    let by = |a: i32| {
        runs.iter()
            .filter(move |r| r.input.contains(&format!("\"action\":{a},")))
    };
    for r in by(0) {
        assert!(
            r.only_reference_ran("magick") && r.only_reference_ran("wl-copy"),
            "{r:?}"
        );
        assert!(
            r.effects.contains("clipboard: types") && r.effects.contains("image "),
            "{r:?}"
        );
    }
    for r in by(1) {
        assert!(r.only_reference_ran("magick"), "{r:?}");
        assert!(r.effects.contains(".stdin: image "), "{r:?}");
    }
    for r in by(2) {
        assert!(
            r.only_reference_ran("curl") && r.only_reference_ran("jq"),
            "{r:?}"
        );
        assert!(r.effects.contains(UPLOADED), "{r:?}");
    }
    for r in by(3) {
        assert!(
            r.only_reference_ran("magick") && r.only_reference_ran("wl-copy"),
            "{r:?}"
        );
        assert!(r.effects.contains("Quick brown fox 42"), "{r:?}");
    }
    for r in by(4).chain(by(5)) {
        assert!(
            r.only_reference_ran("notify-send") && r.only_reference_ran("jq"),
            "{r:?}"
        );
        assert!(r.effects.contains("--geometry|"), "{r:?}");
        assert!(r.effects.contains("|--audio=.monitor|"), "{r:?}");
    }
    for r in by(42) {
        assert_eq!(r.state, "{\"state\":\"\",\"errors\":[]}", "{r:?}");
        assert!(
            r.reference_trace.is_empty() && r.native_trace.is_empty(),
            "{r:?}"
        );
    }
    for side in ["reference", "native"] {
        let log = sc.read(&format!("child-{side}.log"));
        assert!(
            log.contains("[Region Selector] Unknown snip action, skipping snip."),
            "{side}: {log}"
        );
    }
}

/// `Screenshot.capture` as the tree's TempScreenshotProcess.qml calls it, against the original's
/// `command`: the frozen screen the region selector draws over.
#[test]
fn temp_screenshot_ab() {
    if !wayland(&["grim"]) {
        return;
    }
    let _l = crate::clip::seat();
    let _bg = screen();
    let sc = Scene::new("shot-temp");
    sc.trace(&["grim"]);
    let tmp = sc.dir.join("tmp");
    fixture(
        &sc,
        r#"    property int code: -2
    Process {
        id: screenshotProc
        running: false
        property string screenshotDir: ""
        property var screen: ({ name: "" })
        property string screenshotPath: `${screenshotDir}/image-${screen.name}`
        command: @CMD@
        onExited: (exitCode, exitStatus) => root.code = exitCode
    }
    Connections {
        target: Screenshot
        function onCaptured(path, ok) {
            if (path === screenshotProc.screenshotPath)
                root.code = ok ? 0 : 1;
        }
    }
    function go(input, i) {
        if (i > 0)
            return 0;
        const c = JSON.parse(input);
        screenshotProc.screenshotDir = c.dir;
        screenshotProc.screen = { name: c.output };
        if (root.original)
            screenshotProc.running = true;
        else
            Screenshot.capture(c.output, screenshotProc.screenshotPath);
        return 2500;
    }
    function state() { return JSON.stringify({ code: root.code, path: screenshotProc.screenshotPath }) }"#,
        &[("@CMD@", TEMP_COMMAND)],
    );
    let input = |dir: &str, output: &str| {
        format!(
            r#"{{"dir":{:?},"output":{output:?}}}"#,
            tmp.join(dir).display().to_string()
        )
    };
    let inputs = [
        input("rustshell/screenshot", OUTPUT),
        input("it's here/x", OUTPUT),
        input("bad", "NOPE-1"),
    ];
    let inputs: Vec<&str> = inputs.iter().map(String::as_str).collect();
    let runs = ab::ab(
        &sc,
        &inputs,
        &mut |_| {
            let _ = std::fs::remove_dir_all(&tmp);
        },
        &mut || files(&tmp),
    );
    for r in &runs[..2] {
        assert!(r.only_reference_ran("grim"), "{r:?}");
        assert!(r.effects.contains("image 1920x1080"), "{r:?}");
        assert!(r.state.contains("\\\"code\\\":0"), "{r:?}");
    }
    assert!(!runs[2].state.contains("\\\"code\\\":0"), "{:?}", runs[2]);
}

/// qml/modules/rusty/overlay/recorder/Recorder.qml's "Screenshot" button `onClicked` against
/// the original's: `grim - | wl-copy`, overlay closed.
#[test]
fn recorder_screenshot_ab() {
    if !wayland(&["grim", "wl-copy", "wl-paste"]) {
        return;
    }
    let _l = crate::clip::seat();
    let _bg = screen();
    let sc = Scene::new("shot-recorder");
    sc.trace(&["grim", "wl-copy"]);
    sc.service(
        "GlobalStates",
        "QtObject {\n    property bool overlayOpen: false\n}",
    );
    fixture(
        &sc,
        r#"    property var seen: []
    Item {
        id: button
        signal clicked()
        onClicked: @CLICK@
    }
    Item {
        id: originalButton
        signal clicked()
        onClicked: @ORIGINAL@
    }
    function go(input, i) {
        if (i > 0)
            return 0;
        GlobalStates.overlayOpen = true;
        (root.original ? originalButton : button).clicked();
        root.seen = [GlobalStates.overlayOpen];
        return 1500;
    }
    function state() { return JSON.stringify({ afterClick: root.seen, overlayOpen: GlobalStates.overlayOpen }) }"#,
        &[
            (
                "@CLICK@",
                &body(
                    "modules/rusty/overlay/recorder/Recorder.qml",
                    "{\n                        GlobalStates.overlayOpen = false;\n                        Screenshot.copyScreen();",
                ),
            ),
            ("@ORIGINAL@", RECORDER_CLICKED),
        ],
    );
    let clip = Clip::new();
    let runs = ab::ab(&sc, &["click"], &mut |_| clip.reset(), &mut || clip.get());
    let r = &runs[0];
    assert!(
        r.only_reference_ran("grim") && r.only_reference_ran("wl-copy"),
        "{r:?}"
    );
    assert!(r.effects.contains("image 1920x1080"), "{r:?}");
    assert!(r.state.contains("false"), "{r:?}");
}
