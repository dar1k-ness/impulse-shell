use super::*;
use std::process::Command;

// Equivalence with the shell commands a desktop usually runs for the same work: the same
// inputs go through the commands (bash, jq, grep, ImageMagick, a recording fake `cliphist`)
// and through this module. A test whose tool is missing fails (`crate::testenv`).

fn have(t: &str) -> bool {
    let ok = Command::new("bash")
        .args(["-c", &format!("command -v {t}")])
        .output()
        .is_ok_and(|o| o.status.success());
    if !ok {
        crate::testenv::unavailable(format!("no {t}"));
    }
    ok
}

fn bash(script: &str, args: &[&str]) -> (String, i32) {
    let o = Command::new("bash")
        .arg("-c")
        .arg(script)
        .arg("bash")
        .args(args)
        .output()
        .unwrap();
    (
        String::from_utf8_lossy(&o.stdout).into_owned(),
        o.status.code().unwrap_or(-1),
    )
}

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("ri-ports-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// No default sink (none configured, or no daemon reached): the recording has no audio
/// instead of `--audio=.monitor`.
#[test]
fn record_audio_is_the_default_sink_monitor_or_none() {
    let d = |sink: &str| [sink.to_owned(), "mic".into(), String::new(), String::new()];
    assert_eq!(
        record::audio_arg(&d("alsa_output.pci")),
        Some("--audio=alsa_output.pci.monitor".into())
    );
    assert_eq!(record::audio_arg(&d("")), None);
    assert_eq!(
        record::audio_arg(&crate::pw::Snap::default().defaults),
        None
    );
}

#[test]
fn focused_monitor_matches_jq() {
    if !have("jq") {
        return;
    }
    let jsons = [
        r#"[{"name":"DP-1","focused":false},{"name":"HDMI-A-2","focused":true}]"#,
        r#"[{"name":"DP-1","focused":false}]"#,
        r#"[]"#,
        r#"[{"name":"A","focused":true},{"name":"B","focused":true}]"#,
    ];
    for j in jsons {
        let (want, _) = bash(
            r#"printf %s "$(printf %s "$1" | jq -r '.[] | select(.focused == true) | .name')""#,
            &[j],
        );
        assert_eq!(record::focused_in(j), want, "{j}");
    }
}

#[test]
fn source_check_matches_pactl_grep() {
    use crate::pw::{NodeSnap, Snap};
    let node = |name: &str, ty: u8| {
        std::sync::Arc::new(NodeSnap {
            name: name.into(),
            ty,
            ..Default::default()
        })
    };
    let snap = Snap {
        nodes: vec![
            node("alsa_output.pci-0000_00_1f.3.analog-stereo", 17),
            node("alsa_input.usb-mic.mono", 9),
            node("spotify", 21),
        ],
        defaults: [
            "alsa_output.pci-0000_00_1f.3.analog-stereo".into(),
            "alsa_input.usb-mic.mono".into(),
            String::new(),
            String::new(),
        ],
        connected: true,
        ..Default::default()
    };
    // What `pactl list short sources` lists for this graph (id, name, driver, spec, state).
    let pactl = "55\talsa_output.pci-0000_00_1f.3.analog-stereo.monitor\tPipeWire\ts32le 2ch 48000Hz\tSUSPENDED\n\
                 56\talsa_input.usb-mic.mono\tPipeWire\ts16le 1ch 48000Hz\tSUSPENDED\n";
    let mut names = songrec::source_names(&snap);
    names.sort();
    assert_eq!(
        names,
        [
            "alsa_input.usb-mic.mono",
            "alsa_output.pci-0000_00_1f.3.analog-stereo.monitor"
        ]
    );
    let check = |dev: &str| bash(r#"printf '%s' "$1" | grep -q "$2""#, &[pactl, dev]).1 == 0;
    for (src, defaults) in [
        ("monitor", snap.defaults.clone()),
        ("input", snap.defaults.clone()),
        (
            "monitor",
            [
                "alsa_input.usb-mic.mono".into(),
                String::new(),
                String::new(),
                String::new(),
            ],
        ),
        (
            "input",
            [String::new(), String::new(), String::new(), String::new()],
        ),
        (
            "input",
            [
                "x".into(),
                "alsa_input.usb-mic".into(),
                String::new(),
                String::new(),
            ],
        ),
        ("bogus", snap.defaults.clone()),
    ] {
        let s = Snap {
            defaults: defaults.clone(),
            ..snap.clone()
        };
        let dev = match src {
            "monitor" => format!("{}.monitor", defaults[0]),
            "input" => defaults[1].clone(),
            _ => String::new(),
        };
        let want = match src {
            "bogus" => Err((1, "Invalid source type\n")),
            _ if dev.is_empty() || !check(&dev) => Err((1, "")),
            _ => Ok(dev.clone()),
        };
        assert_eq!(songrec::pick(src, Some(&s)), want, "{src} {defaults:?}");
    }
}

#[test]
fn songrec_lists_virtual_sources_like_pactl() {
    use crate::pw::{NodeSnap, Snap};
    // `pactl load-module module-null-sink sink_name=mic media.class=Audio/Source/Virtual`
    // (also EasyEffects' `easyeffects_source`): pipewire-pulse lists it as a source, so
    // `-s input` records from it although quickshell leaves its `PwNodeType` untyped.
    let node = |name: &str, class: &str| {
        std::sync::Arc::new(NodeSnap {
            name: name.into(),
            ty: crate::pw::node_type(class),
            props: vec![("media.class".into(), class.into())],
            ..Default::default()
        })
    };
    let snap = Snap {
        nodes: vec![
            node("nul", "Audio/Sink"),
            node("mic", "Audio/Source/Virtual"),
            node("duplex", "Audio/Duplex"),
            node("spotify", "Stream/Output/Audio"),
        ],
        defaults: ["nul".into(), "mic".into(), String::new(), String::new()],
        connected: true,
        ..Default::default()
    };
    let mut names = songrec::source_names(&snap);
    names.sort();
    assert_eq!(names, ["duplex.monitor", "mic", "nul.monitor"]);
    assert_eq!(songrec::pick("input", Some(&snap)), Ok("mic".into()));
    assert_eq!(
        songrec::pick("monitor", Some(&snap)),
        Ok("nul.monitor".into())
    );
}

#[test]
fn upload_reply_matches_jq() {
    if !have("jq") {
        return;
    }
    for t in [
        r#"{"success":true,"files":[{"url":"https://u.example/a.png","size":3}]}"#,
        r#"{"files":[]}"#,
        r#"{"success":false}"#,
        r#"{"files":[{"url":null}]}"#,
        "<html>502</html>",
        "",
    ] {
        let (want, _) = bash(
            r#"printf %s "$(printf %s "$1" | jq -r '.files[0].url' 2>/dev/null)""#,
            &[t],
        );
        assert_eq!(shot::jq_files0_url(t), want, "{t}");
    }
}

#[test]
fn tesseract_langs_match_pipeline() {
    for list in [
        "List of available languages in \"/usr/share/tessdata/\" (3):\neng\nosd\nrus\n",
        "List of available languages (1):\neng\n",
        "List of available languages (0):\n",
    ] {
        let (want, _) = bash(
            r#"printf '%s' "$(printf '%s' "$1" | awk 'NR>1{print $1}' | tr '\n' '+' | sed 's/\+$/\n/')""#,
            &[list],
        );
        assert_eq!(shot::langs_from(list), want, "{list:?}");
    }
}

fn magick() -> Option<&'static str> {
    ["magick", "convert"].into_iter().find(|t| {
        Command::new("bash")
            .args(["-c", &format!("command -v {t}")])
            .output()
            .is_ok_and(|o| o.status.success())
    })
}

#[test]
fn crop_matches_magick() {
    let Some(m) = magick() else {
        crate::testenv::unavailable("no ImageMagick (`magick`)");
        return;
    };
    let d = tmp("crop");
    let src = d.join("image-DP-1");
    let mut img = image::RgbaImage::new(97, 61);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = image::Rgba([
            (x * 7 % 256) as u8,
            (y * 13 % 256) as u8,
            ((x ^ y) * 5 % 256) as u8,
            255,
        ]);
    }
    std::fs::write(&src, wl::png(&img)).unwrap();
    for rect in [
        (0, 0, 97, 61),
        (10, 5, 30, 20),
        (80, 50, 40, 40),
        (96, 60, 1, 1),
        (0, 0, 1, 61),
        (90, 0, 20, 5),
        (-20, -30, 100, 100),
        (5000, 10, 100, 100),
        (10, 5000, 100, 100),
    ] {
        let (x, y, w, h) = rect;
        let out = d.join("out.png");
        let st = Command::new(m)
            .arg(&src)
            .args(["-crop", &format!("{w}x{h}+{x}+{y}"), "+repage"])
            .arg(&out)
            .status()
            .unwrap();
        assert!(st.success());
        let want = image::open(&out).unwrap().to_rgba8();
        let got = shot::crop(&img, rect);
        assert_eq!(got.dimensions(), want.dimensions(), "{rect:?}");
        assert!(got.as_raw() == want.as_raw(), "{rect:?}: pixels differ");
    }
}

/// A `cliphist` that logs its argv and stdin and prints a canned decode.
fn fake_cliphist(d: &std::path::Path, code: u8) -> String {
    let bin = d.join(format!("cliphist{code}"));
    crate::testenv::install_script(
        &bin,
        &format!(
            "#!/bin/bash\nprintf '%s\\n' \"$*\" >> '{0}/argv'\ncat >> '{0}/stdin'\n[ \"$1\" = decode ] && printf 'DATA\\x00\\x01png'\nexit {1}\n",
            d.display(),
            code
        ),
    );
    bin.to_string_lossy().into_owned()
}

fn take_log(d: &std::path::Path) -> (String, Vec<u8>) {
    let a = std::fs::read_to_string(d.join("argv")).unwrap_or_default();
    let s = std::fs::read(d.join("stdin")).unwrap_or_default();
    let _ = std::fs::remove_file(d.join("argv"));
    let _ = std::fs::remove_file(d.join("stdin"));
    (a, s)
}

/// Shell single-quote escape as the original's `StringUtils.shellSingleQuoteEscape`.
fn sq(s: &str) -> String {
    s.replace('\'', "'\\''")
}

const ENTRIES: &[&str] = &[
    "12\thello world",
    "7\t[[ binary data 31 KiB png 640x480 ]]",
    "3\tit's 100% \"quoted\" \\n $HOME `x`",
    "41\tünïcödé ✓",
];

#[test]
fn cliphist_delete_matches_pipeline() {
    let d = tmp("cdel");
    for e in ENTRIES {
        for code in [0, 1] {
            let bin = fake_cliphist(&d, code);
            let st = Command::new("bash")
                .args(["-c", &format!("echo '{}' | {bin} delete", sq(e))])
                .status()
                .unwrap();
            let want = take_log(&d);
            let got_code = clip::Job::Delete {
                bin: bin.clone(),
                entry: e.to_string(),
            }
            .run(&Io::detached());
            assert_eq!(take_log(&d), want, "{e}");
            assert_eq!(got_code, st.code().unwrap(), "{e}");
        }
    }
}

#[test]
fn cliphist_image_decode_matches_pipeline() {
    let d = tmp("cimg");
    let bin = fake_cliphist(&d, 0);
    let run_orig = |path: &str, e: &str| {
        Command::new("bash")
            .args([
                "-c",
                &format!(
                    "[ -f {path} ] || echo '{}' | {bin} decode > '{path}'",
                    sq(e)
                ),
            ])
            .status()
            .unwrap()
            .code()
            .unwrap()
    };
    for e in ENTRIES {
        let p = d.join("12").to_string_lossy().into_owned();
        // Missing file: decoded into it. Existing file: left alone, cliphist not run.
        for _ in 0..2 {
            let _ = std::fs::remove_file(&p);
            let c1 = run_orig(&p, e);
            let f1 = std::fs::read(&p).unwrap();
            let l1 = take_log(&d);
            let c1b = run_orig(&p, e);
            let l1b = take_log(&d);
            std::fs::remove_file(&p).unwrap();
            let job = clip::Job::DecodeTo {
                bin: bin.clone(),
                entry: e.to_string(),
                path: p.clone(),
            };
            let c2 = job.clone().run(&Io::detached());
            let f2 = std::fs::read(&p).unwrap();
            let l2 = take_log(&d);
            let c2b = job.run(&Io::detached());
            let l2b = take_log(&d);
            assert_eq!((c2, f2, l2, c2b, l2b), (c1, f1, l1, c1b, l1b), "{e}");
        }
        // Missing directory: the redirection fails.
        let p = d.join("no/such/dir/1").to_string_lossy().into_owned();
        let c1 = run_orig(&p, e);
        let l1 = take_log(&d);
        let c2 = clip::Job::DecodeTo {
            bin: bin.clone(),
            entry: e.to_string(),
            path: p,
        }
        .run(&Io::detached());
        assert_eq!((c2, take_log(&d)), (c1, l1));
    }
}

/// Runs `job` on a thread until the program it waits for has written its pid into `fifo`,
/// then stops the job: it ends with 143 (the script's status after a SIGTERM), and the
/// program, which led a process group of its own as a [`crate::child::spawn`] child, is gone.
/// A program started with a plain `Command` stayed in the shell's group, untracked, and its
/// job waited for it whatever `running = false`, the shell's exit or a reload asked.
pub(super) fn stop_ends_the_program(
    what: &str,
    fifo: &std::path::Path,
    job: impl FnOnce(&Io) -> i32 + Send + 'static,
) {
    let io = Io::detached();
    let stop = io.stop.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let t = std::thread::spawn(move || {
        let _ = tx.send(job(&io));
    });
    // Blocks until the program runs.
    let pid: i32 = std::fs::read_to_string(fifo)
        .unwrap()
        .trim()
        .parse()
        .unwrap_or_else(|e| panic!("{what}: no pid: {e}"));
    // SAFETY: getpgid(2) takes a plain integer and touches no memory of this process.
    let pgid = unsafe { libc::getpgid(pid) };
    stop.store(true, std::sync::atomic::Ordering::Release);
    // A job that still waited for its program after the stop would send nothing.
    let code = rx.recv().unwrap();
    assert_eq!(code, 143, "{what}");
    assert_eq!(pgid, pid, "{what}: the program leads no group of its own");
    crate::testenv::wait_gone(pid);
    t.join().unwrap();
}

/// `slurp` (the recorder's region), `tesseract` (text recognition) and `xdg-open` (image
/// search) are children of their job: `running = false` ends them and the job.
#[test]
fn stopping_a_job_ends_the_program_it_waits_for() {
    const TEST: &str = concat!(
        module_path!(),
        "::stopping_a_job_ends_the_program_it_waits_for"
    );
    if !crate::testenv::in_own_process(TEST) {
        // The three programs, found first in PATH: each tells its pid and runs until a
        // signal ends it.
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        for program in ["slurp", "tesseract", "xdg-open"] {
            crate::testenv::install_script(
                &bin.join(program),
                "#!/bin/sh\necho $$ > \"$FAKE_FIFO\"\nexec sleep infinity\n",
            );
        }
        let fifo = dir.path().join("pid");
        crate::testenv::fifo(&fifo);
        let mut path = bin.into_os_string();
        path.push(":");
        path.push(std::env::var_os("PATH").unwrap());
        crate::testenv::run_in_own_process(
            TEST,
            [
                ("PATH", path.as_os_str()),
                ("HOME", dir.path().as_os_str()),
                ("FAKE_FIFO", fifo.as_os_str()),
            ],
        );
        return;
    }
    let fifo = std::path::PathBuf::from(std::env::var_os("FAKE_FIFO").unwrap());
    let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
    stop_ends_the_program("slurp", &fifo, |io| {
        let region = record::Args {
            silent: true,
            ..Default::default()
        };
        record::run(&region, io)
    });
    // The temporary screenshot a crop reads.
    let shot = |name: &str| {
        let p = home.join(name);
        let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([9, 9, 9, 255]));
        std::fs::write(&p, wl::png(&img)).unwrap();
        p.to_str().unwrap().to_owned()
    };
    let path = shot("ocr");
    stop_ends_the_program("tesseract", &fifo, move |io| {
        let job = shot::Job::Crop {
            path,
            rect: (0, 0, 2, 2),
            after: shot::After::Ocr,
        };
        job.run(io)
    });
    let path = shot("search");
    let endpoint = shot::upload_tests::serve(200, r#"{"files":[{"url":"https://host/x.png"}]}"#);
    stop_ends_the_program("xdg-open", &fifo, move |io| {
        let job = shot::Job::Crop {
            path,
            rect: (0, 0, 2, 2),
            after: shot::After::Search {
                endpoint,
                base: "https://lens.example/?url=".into(),
            },
        };
        job.run(io)
    });
}

/// Runs `job` on a thread against a compositor that takes its requests and never answers
/// (`mute`, the socket `WAYLAND_DISPLAY` names), then stops the job; what it returned. The
/// connection stays open until then: only the stop can have ended the wait.
fn stop_ends_the_wait<T: Send + 'static>(
    what: &str,
    mute: &std::os::unix::net::UnixListener,
    job: impl FnOnce(&Io) -> T + Send + 'static,
) -> T {
    use std::io::Read as _;
    let io = Io::detached();
    let stop = io.stop.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let t = std::thread::spawn(move || {
        let _ = tx.send(job(&io));
    });
    // The job connected and sent its first requests: it waits for their answers now.
    let (mut client, _) = mute.accept().unwrap();
    assert!(client.read(&mut [0; 64]).unwrap() > 0, "{what}: no request");
    stop.store(true, std::sync::atomic::Ordering::Release);
    // A job that still waited for the compositor after the stop would send nothing.
    let got = rx.recv().unwrap();
    drop(client);
    t.join().unwrap();
    got
}

/// A socket at the path `WAYLAND_DISPLAY` names (see [`mute_compositor_var`]), served by
/// nobody: a client connects and writes its requests, and no event ever comes back.
pub(crate) fn mute_compositor() -> std::os::unix::net::UnixListener {
    std::os::unix::net::UnixListener::bind(std::env::var_os("WAYLAND_DISPLAY").unwrap()).unwrap()
}

/// The `WAYLAND_DISPLAY` of a [`mute_compositor`] in `dir`, for the test's own process.
pub(crate) fn mute_compositor_var(
    dir: &std::path::Path,
) -> [(&'static str, std::ffi::OsString); 1] {
    [("WAYLAND_DISPLAY", dir.join("wayland-mute").into_os_string())]
}

/// A screenshot waits for the compositor's frame; `running = false` ends that wait (143, as
/// `grim` ended by SIGTERM). It used to wait without looking at the stop, so a compositor
/// that did not answer held the job's thread for good.
#[test]
fn stopping_a_screenshot_ends_its_wait_for_the_compositor() {
    const TEST: &str = concat!(
        module_path!(),
        "::stopping_a_screenshot_ends_its_wait_for_the_compositor"
    );
    if !crate::testenv::in_own_process(TEST) {
        let dir = tempfile::tempdir().unwrap();
        crate::testenv::run_in_own_process_env(
            TEST,
            mute_compositor_var(dir.path()),
            &["WAYLAND_SOCKET"],
        );
        return;
    }
    let mute = mute_compositor();
    let dir = tempfile::tempdir().unwrap();
    let code = stop_ends_the_wait("copyScreen", &mute, |io| shot::Job::FullCopy.run(io));
    assert_eq!(code, 143);
    let path = dir.path().join("image-DP-1").to_str().unwrap().to_owned();
    let temp = shot::Job::Temp {
        dir: dir.path().to_str().unwrap().into(),
        output: "DP-1".into(),
        path: path.clone(),
    };
    let code = stop_ends_the_wait("capture", &mute, |io| temp.run(io));
    assert_eq!(code, 143);
    assert!(!std::path::Path::new(&path).exists());
    let lightness = stop_ends_the_wait("measureLightness", &mute, |io| {
        screen::lightness_of("DP-1", &io.stop)
    });
    assert_eq!(lightness, None);
}

// Under a running compositor (`RI_PORTS_WAYLAND=1`, WAYLAND_DISPLAY set; the guest runs
// these under Hyprland): capture against `grim`, clipboard against `wl-copy`.

fn wayland() -> bool {
    let on = std::env::var_os("RI_PORTS_WAYLAND").is_some()
        && std::env::var_os("WAYLAND_DISPLAY").is_some();
    if !on {
        crate::testenv::unavailable(
            "the Wayland ports need a compositor they may use: run under one with RI_PORTS_WAYLAND=1",
        );
    }
    on
}

/// One test at a time touches the clipboard; the `wl-copy` servers it started.
static CLIP: std::sync::Mutex<Vec<std::process::Child>> = std::sync::Mutex::new(Vec::new());

/// The clipboard, held by one test. `wl-copy` runs in the foreground as its child: by default
/// it forks a server that outlives the test, holding the test's stdout and stderr, until
/// another selection replaces it.
struct Clip(std::sync::MutexGuard<'static, Vec<std::process::Child>>);

fn clip() -> Clip {
    Clip(CLIP.lock().unwrap_or_else(|e| e.into_inner()))
}

impl Clip {
    /// `printf data | wl-copy [--type mime]`; the selection is set once the compositor
    /// announces it (see [`clipboard_after`]).
    fn wl_copy(&mut self, data: &[u8], mime: Option<&str>) {
        let mut c = Command::new("wl-copy");
        c.arg("--foreground");
        if let Some(m) = mime {
            c.args(["--type", m]);
        }
        let mut ch = c.stdin(std::process::Stdio::piped()).spawn().unwrap();
        use std::io::Write;
        ch.stdin.take().unwrap().write_all(data).unwrap();
        self.0.push(ch);
    }

    /// `bash -c "{cmd} | wl-copy"`: `cmd`'s output copied by a `wl-copy` of ours.
    fn sh_copy(&mut self, cmd: &str) {
        let o = Command::new("bash").args(["-c", cmd]).output().unwrap();
        assert!(o.status.success(), "{cmd}: {}", o.status);
        self.wl_copy(&o.stdout, None);
    }
}

impl Drop for Clip {
    /// Clears the selection, which cancels the source of the `wl-copy` serving it (a replaced
    /// one already got `cancelled`), and reaps every server: each exits on `cancelled`.
    fn drop(&mut self) {
        if self.0.is_empty() {
            return;
        }
        let cleared = Command::new("wl-copy")
            .arg("--clear")
            .status()
            .is_ok_and(|s| s.success());
        for mut ch in self.0.drain(..) {
            if !cleared {
                let _ = ch.kill();
            }
            let _ = ch.wait();
        }
    }
}

/// Tests that read the screen run apart from the one that switches the outputs off (taken
/// before `CLIP`).
static OUTPUT: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn grim(args: &[&str]) -> image::RgbaImage {
    let o = Command::new("grim").args(args).arg("-").output().unwrap();
    assert!(o.status.success());
    image::load_from_memory(&o.stdout).unwrap().to_rgba8()
}

#[test]
fn capture_matches_grim() {
    if !wayland() || !have("grim") {
        return;
    }
    let _o = OUTPUT.lock().unwrap_or_else(|e| e.into_inner());
    let outs = String::from_utf8(
        Command::new("bash")
            .args(["-c", "hyprctl monitors -j | jq -r '.[].name'"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    for name in outs.lines() {
        let want = grim(&["-o", name]);
        let got = wl::capture(Some(name)).unwrap();
        assert_eq!(got.dimensions(), want.dimensions(), "{name}");
        assert!(got.as_raw() == want.as_raw(), "{name}: pixels differ");
        let (w, h, px) = super::capture_output(name, false).unwrap();
        assert_eq!((w, h), want.dimensions(), "{name}");
        assert!(px == *want.as_raw(), "{name}: capture_output pixels differ");
    }
    let want = grim(&[]);
    let got = wl::capture(None).unwrap();
    assert_eq!(got.dimensions(), want.dimensions());
    assert!(got.as_raw() == want.as_raw(), "all outputs: pixels differ");
}

/// A copy of an output that is switched off (DPMS) ends failed at once: the compositor presents
/// nothing there, so Hyprland would otherwise hold the frame until the output is back on.
#[test]
fn capture_of_powered_off_output_ends() {
    if !wayland() {
        return;
    }
    let _o = OUTPUT.lock().unwrap_or_else(|e| e.into_inner());
    // sway's IPC socket: SWAYSOCK, or the one it made in XDG_RUNTIME_DIR (the testenv's).
    let swaysock = std::env::var_os("SWAYSOCK")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::fs::read_dir(std::env::var_os("XDG_RUNTIME_DIR")?)
                .ok()?
                .flatten()
                .map(|e| e.path())
                .find(|p| p.to_string_lossy().contains("sway-ipc."))
        });
    let sway = swaysock.is_some();
    let ctl = |on: bool| {
        let st = if let Some(sock) = &swaysock {
            Command::new("swaymsg")
                .env("SWAYSOCK", sock)
                .args(["output", "*", "power", if on { "on" } else { "off" }])
                .status()
        } else {
            Command::new("hyprctl")
                .args(["dispatch", "dpms", if on { "on" } else { "off" }])
                .status()
        };
        assert!(st.unwrap().success());
    };
    let name = String::from_utf8(
        Command::new("bash")
            .args([
                "-c",
                if sway {
                    "SWAYSOCK=\"$0\" swaymsg -t get_outputs -r | jq -r '.[0].name'"
                } else {
                    "hyprctl monitors -j | jq -r '.[0].name'"
                },
            ])
            .arg(swaysock.clone().unwrap_or_default())
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();
    // Back on however the capture ends (a panic included); a capture that never ends is the
    // bug, and nextest's slow-timeout ends the test.
    struct PowerOn<F: Fn(bool)>(F);
    impl<F: Fn(bool)> Drop for PowerOn<F> {
        fn drop(&mut self) {
            (self.0)(true);
        }
    }
    ctl(false);
    let on = PowerOn(ctl);
    let got = wl::capture(Some(&name));
    drop(on);
    assert!(
        got.is_err(),
        "{name}: capture of an output that is off did not end failed"
    );
}

/// Runs `copy`, then reads the clipboard once the compositor announced the selection it set:
/// `wl-paste --watch` runs its command on every selection event, on the current selection first.
fn clipboard_after(copy: impl FnOnce()) -> (Vec<String>, Vec<Vec<u8>>) {
    use std::io::BufRead;
    let mut watch = Command::new("wl-paste")
        .args([
            "--watch",
            "sh",
            "-c",
            "cat >/dev/null; echo \"$CLIPBOARD_STATE\"",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut events = std::io::BufReader::new(watch.stdout.take().unwrap()).lines();
    let mut next = || {
        events
            .next()
            .and_then(Result::ok)
            .expect("wl-paste --watch ended")
    };
    next();
    copy();
    let state = next();
    let _ = watch.kill();
    let _ = watch.wait();
    assert_eq!(state, "data", "the copy set no selection");
    clipboard()
}

/// (offered types sorted, content of each type) of the current clipboard.
fn clipboard() -> (Vec<String>, Vec<Vec<u8>>) {
    let o = Command::new("wl-paste")
        .arg("--list-types")
        .output()
        .unwrap();
    let mut types: Vec<String> = String::from_utf8_lossy(&o.stdout)
        .lines()
        .map(str::to_owned)
        .collect();
    // wl-copy offers text/plain twice for text; the set of types is what a paste sees.
    types.sort();
    types.dedup();
    let data = types
        .iter()
        .map(|t| {
            Command::new("wl-paste")
                .args(["--no-newline", "--type", t])
                .output()
                .unwrap()
                .stdout
        })
        .collect();
    (types, data)
}

fn samples() -> Vec<Vec<u8>> {
    let mut img = image::RgbaImage::new(8, 4);
    img.put_pixel(1, 1, image::Rgba([255, 0, 0, 255]));
    vec![
        b"hello world".to_vec(),
        "ünïcödé ✓\nsecond line\n".as_bytes().to_vec(),
        wl::png(&img),
        b"".to_vec(),
    ]
}

#[test]
fn copy_matches_wl_copy() {
    if !wayland() || !have("wl-copy") {
        return;
    }
    let mut clip = clip();
    for data in samples() {
        for mime in [None, Some("image/png"), Some("text/plain")] {
            let want = clipboard_after(|| clip.wl_copy(&data, mime));
            let got = clipboard_after(|| {
                wl::copy(data.clone(), mime, &Default::default()).unwrap();
            });
            assert_eq!(got, want, "{mime:?} {} bytes", data.len());
        }
    }
}

#[test]
fn copy_text_matches_qt_clipboard() {
    if !wayland() {
        return;
    }
    let _clip = clip();
    let text = "ünïcödé ✓\nsecond line\n";
    let (types, data) = clipboard_after(|| {
        copy_text(text.to_owned());
    });
    // What `qs` offers after `Rustshell.clipboardText = text` (wl-paste --list-types).
    assert_eq!(types, ["text/plain", "text/plain;charset=utf-8"]);
    assert!(data.iter().all(|d| d == text.as_bytes()), "{data:?}");
}

/// `QClipboard::setText`, returning once the compositor has the selection: its source.
fn copy_text(text: String) -> u64 {
    clipsrc::set_wait(clipsrc::text_offers(text), &Default::default()).unwrap()
}

fn wl_paste() -> String {
    let o = Command::new("wl-paste")
        .arg("--no-newline")
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// `Rustshell.clipboardText = a; … = b` leaves `b` (QClipboard::setText: the later call wins)
/// however long `a` takes to hand over.
#[test]
fn clipboard_writes_keep_call_order() {
    if !wayland() {
        return;
    }
    let _clip = clip();
    clip::set_text("x".repeat(64 << 20));
    clip::set_text("second".into());
    assert!(clipsrc::sync(&Default::default()).is_some());
    assert_eq!(wl_paste(), "second");
}

/// A reader that stops reading holds a non-blocking pipe of the source's host, whose loop
/// goes on serving; the compositor cancelling the source drops it with its reads.
#[test]
fn a_stalled_reader_blocks_nothing_and_ends_with_its_source() {
    if !wayland() {
        return;
    }
    let _clip = clip();
    let big = "y".repeat(4 << 20);
    let first = copy_text(big.clone());
    // wl-paste fills its stdout pipe, which nobody reads yet, and stops reading ours.
    let mut stalled = Command::new("wl-paste")
        .arg("--no-newline")
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    clipsrc::wait(|s| s.source != first || s.sends > 0);
    let second = copy_text("second".into());
    assert_ne!(first, second);
    assert_eq!(wl_paste(), "second");
    let mut got = String::new();
    std::io::Read::read_to_string(&mut stalled.stdout.take().unwrap(), &mut got).unwrap();
    let _ = stalled.wait();
    assert!(
        got.len() < big.len() && big.starts_with(&got),
        "the replaced source went on serving: {} bytes",
        got.len()
    );
}

/// Superpaste presses the paste key only once the shell lost the keyboard focus, and copies
/// the next entry only once the target has read the one before; no timer anywhere.
#[test]
fn superpaste_follows_keyboard_leave_and_the_targets_reads() {
    use std::io::BufRead;
    use std::os::fd::AsRawFd;
    if !wayland() {
        return;
    }
    let _clip = clip();
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("cliphist");
    crate::testenv::install_script(&bin, "#!/bin/bash\ncut -f2\n");
    let fifo = dir.path().join("pressed");
    let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    // SAFETY: `c` is a live NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    // Read and write: the paste command's echo never blocks on opening it, and its close is
    // no end of file.
    let pressed = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&fifo)
        .unwrap();
    let unread = || {
        let mut n: libc::c_int = 0;
        // SAFETY: FIONREAD stores the byte count in the live int `n`.
        let r = unsafe { libc::ioctl(pressed.as_raw_fd(), libc::FIONREAD, &mut n) };
        assert_eq!(r, 0);
        n
    };
    let mut presses = std::io::BufReader::new(pressed.try_clone().unwrap()).lines();
    let rt = crate::runtime::Runtime::new(None);
    let waker = rt.world.borrow().waker.clone();
    clip::keyboard_focus(&waker, true);
    let before = clipsrc::wait(|_| true).source;
    let job = clip::Job::Superpaste {
        bin: bin.display().to_string(),
        entries: vec!["1\ta".into(), "2\tb".into()],
        paste: format!("echo pressed > '{}'", fifo.display()),
    };
    let run = std::thread::spawn(move || job.run(&Io::detached()));
    let a = clipsrc::wait(|s| s.source != before).source;
    assert_eq!(unread(), 0, "pasted while the shell had the keyboard focus");
    clip::keyboard_focus(&waker, false);
    assert_eq!(presses.next().unwrap().unwrap(), "pressed");
    assert_eq!(
        clipsrc::wait(|_| true).source,
        a,
        "copied the next entry before a read"
    );
    assert_eq!(wl_paste(), "a\n");
    assert_eq!(presses.next().unwrap().unwrap(), "pressed");
    assert_eq!(wl_paste(), "b\n");
    assert_eq!(run.join().unwrap(), 0);
}

/// Superpaste into a window of this process: the paste takes the text of the process's own
/// source ([`clip::own_text`]) and asks the source for nothing, so no `send` comes; the next
/// entry is copied once that paste went in ([`clipsrc::took`]).
#[test]
fn superpaste_follows_the_pastes_of_this_process() {
    use std::io::BufRead;
    if !wayland() {
        return;
    }
    let _clip = clip();
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("cliphist");
    crate::testenv::install_script(&bin, "#!/bin/bash\ncut -f2\n");
    let fifo = dir.path().join("pressed");
    let c = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
    // SAFETY: `c` is a live NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    let pressed = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&fifo)
        .unwrap();
    let mut presses = std::io::BufReader::new(pressed).lines();
    let rt = crate::runtime::Runtime::new(None);
    let waker = rt.world.borrow().waker.clone();
    clip::keyboard_focus(&waker, false);
    let before = clipsrc::wait(|_| true).source;
    let job = clip::Job::Superpaste {
        bin: bin.display().to_string(),
        entries: vec!["1\ta".into(), "2\tb".into()],
        paste: format!("echo pressed > '{}'", fifo.display()),
    };
    let run = std::thread::spawn(move || job.run(&Io::detached()));
    let a = clipsrc::wait(|s| s.source != before).source;
    assert_eq!(presses.next().unwrap().unwrap(), "pressed");
    let st = clipsrc::wait(|_| true);
    assert_eq!(
        (st.source, st.sends, st.taken),
        (a, 0, 0),
        "copied the next entry before the paste"
    );
    assert_eq!(clip::own_text().as_deref(), Some("a\n"));
    clipsrc::took();
    assert_eq!(presses.next().unwrap().unwrap(), "pressed");
    let st = clipsrc::wait(|_| true);
    assert_ne!(st.source, a);
    assert_eq!((st.sends, st.taken), (0, 0));
    assert_eq!(clip::own_text().as_deref(), Some("b\n"));
    clipsrc::took();
    assert_eq!(run.join().unwrap(), 0);
}

/// A host of the clipboard source whose loop is the test's thread: the queue is dispatched
/// only where the test says so.
#[derive(Default)]
struct LoopOfTheTest {
    seat: Option<wayland_client::protocol::wl_seat::WlSeat>,
    ext: Option<u32>,
    wlr: Option<u32>,
    pipes: Vec<(u64, std::os::fd::OwnedFd)>,
}

impl clipsrc::Host for LoopOfTheTest {
    fn watch(&mut self, read: u64, pipe: std::os::fd::OwnedFd) {
        self.pipes.push((read, pipe));
    }

    fn unwatch(&mut self, read: u64) {
        self.pipes.retain(|(r, _)| *r != read);
    }
}

impl wayland_client::Dispatch<wayland_client::protocol::wl_registry::WlRegistry, ()>
    for LoopOfTheTest
{
    fn event(
        s: &mut Self,
        r: &wayland_client::protocol::wl_registry::WlRegistry,
        e: wayland_client::protocol::wl_registry::Event,
        _: &(),
        _: &wayland_client::Connection,
        qh: &wayland_client::QueueHandle<Self>,
    ) {
        if let wayland_client::protocol::wl_registry::Event::Global {
            name, interface, ..
        } = e
        {
            match interface.as_str() {
                "wl_seat" if s.seat.is_none() => s.seat = Some(r.bind(name, 1, qh, ())),
                "ext_data_control_manager_v1" => s.ext = Some(name),
                "zwlr_data_control_manager_v1" => s.wlr = Some(name),
                _ => {}
            }
        }
    }
}

wayland_client::delegate_noop!(LoopOfTheTest: ignore wayland_client::protocol::wl_seat::WlSeat);
crate::delegate_clipboard_source!(LoopOfTheTest);

/// The source's objects are on the host's connection, so a copy waits for an event the
/// host's loop dispatches. A loop that stops a job and waits for its thread dispatches
/// nothing meanwhile: the stop ends the job's wait, or both would wait for each other for
/// good. In a process of its own: a process has one host.
#[test]
fn a_copy_waiting_for_the_hosts_loop_ends_with_its_job() {
    const TEST: &str = concat!(
        module_path!(),
        "::a_copy_waiting_for_the_hosts_loop_ends_with_its_job"
    );
    if !wayland() {
        return;
    }
    if !crate::testenv::in_own_process(TEST) {
        crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("cliphist");
    crate::testenv::install_script(&bin, "#!/bin/bash\ncut -f2\n");
    let conn = wayland_client::Connection::connect_to_env().unwrap();
    let mut q = conn.new_event_queue();
    let qh = q.handle();
    let reg = conn.display().get_registry(&qh, ());
    let mut host = LoopOfTheTest::default();
    q.roundtrip(&mut host).unwrap();
    assert!(clipsrc::host(&conn, &qh, &reg, host.ext, host.wlr));
    clipsrc::seat(host.seat.as_ref().unwrap());
    let copy = |entry: &str| {
        let io = Io::detached();
        let stop = io.stop.clone();
        let job = clip::Job::Copy {
            bin: bin.display().to_string(),
            args: vec![],
            stdin: Some(entry.to_owned()),
        };
        (std::thread::spawn(move || job.run(&io)), stop)
    };

    // The job has asked for the selection and waits for the compositor's answer, which
    // only this thread can dispatch. This thread stops the job and waits for it instead.
    let (run, stop) = copy("1\tfirst");
    let first = clipsrc::wait(|s| s.source != 0).source;
    stop.store(true, std::sync::atomic::Ordering::Release);
    assert_eq!(run.join().unwrap(), 143);
    // The request went out before the stop: the selection is the process's.
    assert_eq!(clip::own_text().as_deref(), Some("first\n"));

    // With the loop running, as the shell's does, a copy ends once the compositor has
    // answered it.
    std::thread::spawn(move || while q.blocking_dispatch(&mut host).is_ok() {});
    let (run, _stop) = copy("2\tsecond");
    assert_eq!(run.join().unwrap(), 0);
    assert_ne!(clipsrc::wait(|_| true).source, first);
    assert_eq!(clip::own_text().as_deref(), Some("second\n"));
    // A reader gets it from that loop, and the read counts as a `send`.
    let out = Command::new("wl-paste")
        .arg("--no-newline")
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "second\n");
    assert_eq!(clipsrc::wait(|s| s.sends == s.answered).sends, 1);
}

#[test]
fn cliphist_copy_matches_pipeline() {
    if !wayland() || !have("cliphist") || !have("wl-copy") {
        return;
    }
    let mut clip = clip();
    let d = tmp("chcopy");
    let db = d.join("db");
    let bin = format!("cliphist -db-path {}", db.display());
    for data in samples().into_iter().filter(|s| !s.is_empty()) {
        let mut ch = Command::new("bash")
            .args(["-c", &format!("{bin} store")])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write;
        ch.stdin.take().unwrap().write_all(&data).unwrap();
        ch.wait().unwrap();
    }
    let list = Command::new("bash")
        .args(["-c", &format!("{bin} list")])
        .output()
        .unwrap()
        .stdout;
    for entry in String::from_utf8_lossy(&list).lines() {
        let orig = format!("printf '{}' | {bin} decode", sq(entry));
        let want = clipboard_after(|| clip.sh_copy(&orig));
        let got = clipboard_after(|| {
            let code = clip::Job::Copy {
                bin: bin.clone(),
                args: vec![],
                stdin: Some(entry.to_owned()),
            }
            .run(&Io::detached());
            assert_eq!(code, 0);
        });
        assert_eq!(got, want, "{entry}");
    }
}

#[test]
fn full_screenshot_copy_matches_grim_wl_copy() {
    if !wayland() || !have("grim") || !have("wl-copy") {
        return;
    }
    let _o = OUTPUT.lock().unwrap_or_else(|e| e.into_inner());
    let mut clip = clip();
    let (wt, wd) = clipboard_after(|| clip.sh_copy("grim -"));
    let (gt, gd) = clipboard_after(|| assert_eq!(shot::Job::FullCopy.run(&Io::detached()), 0));
    assert_eq!(gt, wt);
    let px = |d: &[Vec<u8>]| {
        image::load_from_memory(&d[0])
            .unwrap()
            .to_rgba8()
            .into_raw()
    };
    assert!(px(&gd) == px(&wd), "pixels differ");
}

#[test]
fn region_copy_matches_magick_wl_copy() {
    let Some(m) = magick() else {
        crate::testenv::unavailable("no ImageMagick (`magick`)");
        return;
    };
    if !wayland() || !have("grim") || !have("wl-copy") {
        return;
    }
    let _o = OUTPUT.lock().unwrap_or_else(|e| e.into_inner());
    let mut clip = clip();
    let d = tmp("region");
    let p = d.join("image-shot").to_string_lossy().into_owned();
    let save = d.join("saved").to_string_lossy().into_owned();
    std::fs::write(&p, wl::png(&grim(&[]))).unwrap();
    let rect = (13, 7, 120, 90);
    let orig = format!("{m} '{p}' -crop 120x90+13+7 +repage -");
    let (wt, wd) = clipboard_after(|| clip.sh_copy(&orig));
    let job = shot::Job::Crop {
        path: p.clone(),
        rect,
        after: shot::After::Copy(Some(save.clone())),
    };
    let (gt, gd) = clipboard_after(|| assert_eq!(job.run(&Io::detached()), 0));
    assert_eq!(gt, wt);
    let px = |d: &[u8]| image::load_from_memory(d).unwrap().to_rgba8().into_raw();
    assert!(px(&gd[0]) == px(&wd[0]), "pixels differ");
    // Saved copy, temporary file removed.
    let saved: Vec<_> = std::fs::read_dir(&save).unwrap().flatten().collect();
    assert_eq!(saved.len(), 1);
    let n = saved[0].file_name().to_string_lossy().into_owned();
    assert!(
        n.starts_with("screenshot-")
            && n.ends_with(".png")
            && n.len() == "screenshot-2026-01-01_00.00.00.png".len(),
        "{n}"
    );
    assert!(px(&std::fs::read(saved[0].path()).unwrap()) == px(&wd[0]));
    assert!(!std::path::Path::new(&p).exists());
}

/// Key events for the keyboard comparison: sends the shipped paste sequence and an
/// on-screen-keyboard shift sequence to the focused window; `tools/reference/ports-keys.sh` records them
/// with `wev` next to the same sequence from ydotool.
#[test]
#[cfg_attr(
    not(feature = "live"),
    ignore = "live: --features live, docs/benchmarks.md"
)]
fn vkbd_sequence() {
    assert!(
        std::env::var_os("RI_PORTS_VKBD").is_some(),
        "types into the focused window: run it through tools/reference/ports-keys.sh (RI_PORTS_VKBD=1)"
    );
    assert!(vkbd::send_wait(&[
        (29, true),
        (47, true),
        (47, false),
        (29, false)
    ]));
    assert!(vkbd::send_wait(&[
        (42, true),
        (30, true),
        (30, false),
        (42, false),
        (30, true),
        (30, false)
    ]));
    vkbd::send(&[(56, true), (29, true)]);
    vkbd::release_all();
    assert!(vkbd::send_wait(&[]));
}

// Measurements: `RI_PORTS_BENCH=orig|port` runs every operation `RI_PORTS_N` times the
// original way (the shell command the original ran) or through the port, under a compositor with
// cliphist, grim, ImageMagick, ydotoold, fake `wf-recorder`/`songrec` on PATH.
// `tools/reference/ports-bench.sh` runs both, alternating, and counts exec()s with strace.

fn rusage() -> f64 {
    let mut t = 0.0;
    for who in [libc::RUSAGE_SELF, libc::RUSAGE_CHILDREN] {
        // SAFETY: rusage is plain integers, for which all zeros is valid.
        let mut r: libc::rusage = unsafe { std::mem::zeroed() };
        // SAFETY: `r` is a live rusage for the kernel to fill.
        unsafe { libc::getrusage(who, &mut r) };
        let tv = |v: libc::timeval| v.tv_sec as f64 + v.tv_usec as f64 / 1e6;
        t += tv(r.ru_utime) + tv(r.ru_stime);
    }
    t
}

fn sh(cmd: &str) {
    let s = Command::new("bash")
        .args(["-c", cmd])
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(s.success() || cmd.contains("record.sh"), "{cmd}");
}

#[test]
#[cfg_attr(
    not(feature = "live"),
    ignore = "live: --features live, docs/benchmarks.md"
)]
fn bench_ports() {
    let mode = std::env::var("RI_PORTS_BENCH")
        .expect("RI_PORTS_BENCH=orig|port: run it through tools/reference/ports-bench.sh");
    let orig = mode == "orig";
    let n: usize = std::env::var("RI_PORTS_N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20);
    let only = std::env::var("RI_PORTS_ONLY").unwrap_or_default();
    // Comma-separated operations to leave out (e.g. the key ones on a desktop in use).
    let skip = std::env::var("RI_PORTS_SKIP").unwrap_or_default();
    let skip: Vec<&str> = skip.split(',').filter(|s| !s.is_empty()).collect();
    let picked = |name: &str| (only.is_empty() || only == name) && !skip.contains(&name);
    let ri = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/original-unshipped/scripts"
    );
    let _o = OUTPUT.lock().unwrap_or_else(|e| e.into_inner());
    let _clip = clip();
    if !orig {
        crate::pw::start_detached();
    }
    let d = tmp("bench");
    let db = d.join("db");
    let bin = format!("cliphist -db-path {}", db.display());
    sh(&format!("printf 'bench entry text' | {bin} store"));
    let entry = String::from_utf8(
        Command::new("bash")
            .args(["-c", &format!("{bin} list | head -1")])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim_end()
    .to_owned();
    let shot = d.join("image-shot").to_string_lossy().into_owned();
    let src_png = wl::png(&grim(&[]));
    let out = std::env::var("WAYLAND_OUTPUT").unwrap_or_else(|_| "Virtual-1".into());
    type Op<'a> = (&'a str, Box<dyn Fn() + 'a>);
    let ops: Vec<Op> = vec![
        (
            "cliphist copy",
            Box::new(|| {
                if orig {
                    sh(&format!("printf '{}' | {bin} decode | wl-copy", sq(&entry)));
                } else {
                    clip::Job::Copy {
                        bin: bin.clone(),
                        args: vec![],
                        stdin: Some(entry.clone()),
                    }
                    .run(&Io::detached());
                }
            }),
        ),
        (
            "cliphist image decode",
            Box::new(|| {
                let p = d.join("1").to_string_lossy().into_owned();
                let _ = std::fs::remove_file(&p);
                if orig {
                    sh(&format!(
                        "[ -f {p} ] || echo '{}' | {bin} decode > '{p}'",
                        sq(&entry)
                    ));
                } else {
                    clip::Job::DecodeTo {
                        bin: bin.clone(),
                        entry: entry.clone(),
                        path: p,
                    }
                    .run(&Io::detached());
                }
            }),
        ),
        (
            "temp screenshot",
            Box::new(|| {
                let dir = d.join("tmpshot").to_string_lossy().into_owned();
                let p = format!("{dir}/image-{out}");
                if orig {
                    sh(&format!("mkdir -p '{dir}' && grim -o '{out}' '{p}'"));
                } else {
                    shot::Job::Temp {
                        dir,
                        output: out.clone(),
                        path: p,
                    }
                    .run(&Io::detached());
                }
            }),
        ),
        (
            "screenshot copy",
            Box::new(|| {
                if orig {
                    sh("grim - | wl-copy");
                } else {
                    shot::Job::FullCopy.run(&Io::detached());
                }
            }),
        ),
        (
            "region copy",
            Box::new(|| {
                std::fs::write(&shot, &src_png).unwrap();
                if orig {
                    sh(&format!(
                        "magick {shot} -crop 300x200+40+30 +repage - | wl-copy && rm '{shot}'"
                    ));
                } else {
                    shot::Job::Crop {
                        path: shot.clone(),
                        rect: (40, 30, 300, 200),
                        after: shot::After::Copy(None),
                    }
                    .run(&Io::detached());
                }
            }),
        ),
        (
            "key press+release",
            Box::new(|| {
                if orig {
                    sh("ydotool key --key-delay 0 30:1");
                    sh("ydotool key --key-delay 0 30:0");
                } else {
                    vkbd::send_wait(&[(30, true)]);
                    vkbd::send_wait(&[(30, false)]);
                }
            }),
        ),
        (
            "paste key",
            Box::new(|| {
                if orig {
                    sh("ydotool key -d 1 29:1 47:1 47:0 29:0");
                } else {
                    vkbd::send_wait(&[(29, true), (47, true), (47, false), (29, false)]);
                }
            }),
        ),
        (
            "record.sh --region",
            Box::new(|| {
                if orig {
                    sh(&format!(
                        "{ri}/videos/record.sh --region '10,10 100x100' --sound"
                    ));
                } else {
                    record::run(
                        &record::Args {
                            region: Some("10,10 100x100".into()),
                            ..Default::default()
                        },
                        &Io::detached(),
                    );
                }
            }),
        ),
        (
            "recognize-music.sh",
            Box::new(|| {
                if orig {
                    sh(&format!(
                        "{ri}/musicRecognition/recognize-music.sh -i 2 -t 30 -s monitor"
                    ));
                } else {
                    songrec::run(
                        &songrec::Args {
                            interval: "2".into(),
                            total: "30".into(),
                            source: "monitor".into(),
                        },
                        &Io::detached(),
                    );
                }
            }),
        ),
    ];
    // Warm-up (page cache, first connections).
    for (name, f) in &ops {
        if picked(name) {
            f();
        }
    }
    for (name, f) in &ops {
        if !picked(name) {
            continue;
        }
        let (t0, c0) = (std::time::Instant::now(), rusage());
        for _ in 0..n {
            f();
        }
        let (wall, cpu) = (
            t0.elapsed().as_secs_f64() / n as f64,
            (rusage() - c0) / n as f64,
        );
        println!(
            "BENCH {mode} {name}: wall {:.2} ms, cpu {:.2} ms",
            wall * 1e3,
            cpu * 1e3
        );
    }
}

#[test]
fn clipboard_text_matches_wl_paste() {
    if !wayland() || !have("wl-copy") || !have("wl-paste") {
        return;
    }
    let mut clip = clip();
    let (tx, rx) = std::sync::mpsc::channel();
    wl::watch_text(move |t| {
        let _ = tx.send(t);
    });
    // Blocks until the watcher reports `want`; false once it stopped without.
    let seen = |want: &str| rx.iter().any(|t| t == want);
    for t in ["hello world", "ünïcödé ✓\nsecond line\n", "😀"] {
        copy_text(t.to_owned());
        assert!(seen(t), "watcher missed {t:?}");
        let o = Command::new("wl-paste")
            .args(["--no-newline", "--type", "text/plain"])
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&o.stdout), t);
        clip.wl_copy(t.as_bytes(), None);
        assert!(seen(t), "watcher missed wl-copy {t:?}");
    }
    clip.wl_copy(&wl::png(&image::RgbaImage::new(2, 2)), Some("image/png"));
    assert!(seen(""), "an image selection reads as no text");
}

/// ImageMagick 7 (`magick`); IM6's `convert -colorspace Gray` uses other weights.
fn im7() -> bool {
    if !have("magick") {
        return false;
    }
    let seven = Command::new("magick")
        .arg("-version")
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("ImageMagick 7"));
    if !seven {
        crate::testenv::unavailable("`magick` is not ImageMagick 7");
    }
    seven
}

/// The PNG `grim` writes for these pixels: RGB when all are opaque, RGBA otherwise.
fn grim_png(img: &image::RgbaImage) -> Vec<u8> {
    if img.pixels().all(|p| p[3] == 255) {
        let rgb = image::DynamicImage::ImageRgba8(img.clone()).to_rgb8();
        let mut out = std::io::Cursor::new(Vec::new());
        rgb.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    } else {
        wl::png(img)
    }
}

fn lightness_samples() -> Vec<image::RgbaImage> {
    let mut v = vec![];
    for c in [
        [0, 0, 0],
        [255, 255, 255],
        [255, 0, 0],
        [0, 255, 0],
        [0, 0, 255],
        [10, 20, 30],
    ] {
        v.push(image::RgbaImage::from_pixel(
            1,
            1,
            image::Rgba([c[0], c[1], c[2], 255]),
        ));
    }
    let mut seed = 0x2545f491u32;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    for (w, h, alpha) in [
        (97, 61, false),
        (320, 200, false),
        (64, 64, true),
        (5, 3, true),
    ] {
        let mut img = image::RgbaImage::new(w, h);
        for p in img.pixels_mut() {
            let r = rnd();
            let a = if alpha { (r >> 24) as u8 } else { 255 };
            *p = image::Rgba([r as u8, (r >> 8) as u8, (r >> 16) as u8, a]);
        }
        v.push(img);
    }
    let mut grad = image::RgbaImage::new(256, 40);
    for (x, y, p) in grad.enumerate_pixels_mut() {
        *p = image::Rgba([x as u8, (y * 6) as u8, 255 - x as u8, 255]);
    }
    v.push(grad);
    v.push(image::RgbaImage::from_pixel(
        7,
        7,
        image::Rgba([200, 100, 50, 128]),
    ));
    v
}

#[test]
fn lightness_matches_magick() {
    if !im7() {
        return;
    }
    for (i, img) in lightness_samples().iter().enumerate() {
        let mut ch = Command::new("magick")
            .args([
                "png:-",
                "-colorspace",
                "Gray",
                "-format",
                "%[fx:mean*100]",
                "info:",
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write;
        ch.stdin.take().unwrap().write_all(&grim_png(img)).unwrap();
        let o = ch.wait_with_output().unwrap();
        assert!(o.status.success());
        assert_eq!(
            screen::lightness(img),
            String::from_utf8_lossy(&o.stdout),
            "sample {i}"
        );
    }
}

/// The original probe command (fake `grim` printing a fixture, real `magick`) and the port
/// with the same fixture as its capture: same stdout, status and grim output. The port
/// creates no directory (the original's `mkdir -p` is for a file nothing writes).
#[test]
fn lightness_probe_matches_pipeline() {
    if !im7() {
        return;
    }
    let d = tmp("probe");
    let bin = d.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let img = &lightness_samples()[6];
    std::fs::write(d.join("shot.png"), grim_png(img)).unwrap();
    std::fs::write(d.join("file"), "").unwrap();
    // `grim -o NAME -`: prints the fixture for output DP-1, fails like grim otherwise.
    crate::testenv::install_script(
        &bin.join("grim"),
        &format!(
            "#!/bin/sh\necho \"$*\" >> '{0}/grim.log'\n[ \"$1 $2 $3\" = \"-o DP-1 -\" ] || {{ echo 'grim: unknown output' >&2; exit 1; }}\ncat '{0}/shot.png'\n",
            d.display()
        ),
    );
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let dir = d.join("it's here").display().to_string();
    for (i, output) in ["DP-1", "HDMI-A-1", "it's"].into_iter().enumerate() {
        let sq = |s: &str| s.replace('\'', "'\\''");
        let script = format!(
            "mkdir -p '{}' && grim -o '{}' - | magick png:- -colorspace Gray -format \"%[fx:mean*100]\" info:",
            sq(&dir),
            sq(output)
        );
        let _ = std::fs::remove_file(d.join("grim.log"));
        let o = Command::new("bash")
            .args(["-c", &script])
            .env("PATH", &path)
            .stderr(std::process::Stdio::null())
            .output()
            .unwrap();
        let want = (
            String::from_utf8_lossy(&o.stdout).into_owned(),
            o.status.code().unwrap(),
        );
        let grim = std::fs::read_to_string(d.join("grim.log")).unwrap_or_default();
        let _ = std::fs::remove_dir_all(&dir);
        let mut asked = None;
        let (out, code) = screen::lightness_probe(output, |o| {
            asked = o.map(str::to_owned);
            if o == Some("DP-1") {
                Ok(img.clone())
            } else {
                Err("unknown output".into())
            }
        });
        assert_eq!((out, code), want, "case {i}");
        assert!(!Path::new(&dir).exists());
        assert_eq!(
            asked.map(|o| format!("-o {o} -\n")).unwrap_or_default(),
            grim,
            "case {i}"
        );
    }
}

use std::path::Path;

/// The script the original's `applyLimit` handed to bash.
fn fps_command(value: &str) -> String {
    let p = "~/.config/MangoHud/MangoHud.conf";
    format!(
        "if grep -q '^fps_limit=' {p}; then sed -i 's/^fps_limit=.*/fps_limit={value}/' {p}; \
         else echo 'fps_limit={value}' >> {p}; fi; pkill -SIGUSR2 mangohud"
    )
}

/// The original MangoHud update script (fake `pkill`) and the port on the same config
/// trees: same resulting file (bytes, mode, symlink or not), same `pkill` arguments. Where
/// the original is wrong the expected result is the intended one: a missing directory is
/// created, an appended key starts its own line, the whole value is replaced, and a
/// symlinked config stays a symlink with its target updated.
#[test]
fn fps_limit_matches_bash() {
    if !have("sed") || !have("grep") {
        return;
    }
    let d = tmp("fps");
    let bin = d.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    crate::testenv::install_script(
        &bin.join("pkill"),
        &format!("#!/bin/sh\necho \"$*\" >> '{}/pkill.log'\n", d.display()),
    );
    use std::os::unix::fs::PermissionsExt;
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    // (case, config dir exists, config content (None: absent), symlinked, mode,
    //  intended content where the original gets it wrong, with V for the value)
    type Case<'a> = (&'a str, bool, Option<&'a [u8]>, bool, u32, Option<&'a str>);
    let cases: &[Case] = &[
        ("no dir", false, None, false, 0o644, Some("fps_limit=V\n")),
        ("no file", true, None, false, 0o644, None),
        ("empty", true, Some(b""), false, 0o644, None),
        (
            "other keys",
            true,
            Some(b"fps_limit_method=late\nlegacy_layout=0\n"),
            false,
            0o644,
            None,
        ),
        (
            "no newline",
            true,
            Some(b"gpu_stats"),
            false,
            0o600,
            Some("gpu_stats\nfps_limit=V\n"),
        ),
        (
            "replace",
            true,
            Some(b"a=1\nfps_limit=30\nb=2\n"),
            false,
            0o640,
            None,
        ),
        (
            "multi",
            true,
            Some(b"fps_limit=30,60\n#fps_limit=1\nfps_limit=\nfps_limit=9"),
            false,
            0o644,
            None,
        ),
        (
            "bad utf8",
            true,
            Some(b"fps_limit=30\xff tail\nfps_limit=\xc3\xa9x\r\n"),
            false,
            0o644,
            Some("fps_limit=V\nfps_limit=V\r\n"),
        ),
        (
            "indented",
            true,
            Some(b" fps_limit=30\n"),
            false,
            0o644,
            None,
        ),
        (
            "symlink",
            true,
            Some(b"fps_limit=30\n"),
            true,
            0o644,
            Some("fps_limit=V\n"),
        ),
        (
            "binary",
            true,
            Some(b"\0x\nfps_limit=1\n\0"),
            false,
            0o644,
            None,
        ),
    ];
    for value in ["60", "0", "1e+23", "Infinity"] {
        for &(name, dir, content, link, mode, fixed) in cases {
            let run = |native: bool| {
                let home = d.join(if native { "native" } else { "bash" });
                let _ = std::fs::remove_dir_all(&home);
                let cfg_dir = home.join(".config/MangoHud");
                std::fs::create_dir_all(&home).unwrap();
                if dir {
                    std::fs::create_dir_all(&cfg_dir).unwrap();
                }
                let cfg = cfg_dir.join("MangoHud.conf");
                if let Some(c) = content {
                    let real = if link {
                        home.join("real.conf")
                    } else {
                        cfg.clone()
                    };
                    std::fs::write(&real, c).unwrap();
                    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(mode)).unwrap();
                    if link {
                        std::os::unix::fs::symlink(&real, &cfg).unwrap();
                    }
                }
                let _ = std::fs::remove_file(d.join("pkill.log"));
                if native {
                    screen::set_fps_limit(&cfg, value).unwrap();
                    std::fs::write(d.join("pkill.log"), "-SIGUSR2 mangohud\n").unwrap();
                } else {
                    let st = Command::new("bash")
                        .args(["-c", &fps_command(value)])
                        .env("HOME", &home)
                        .env("PATH", &path)
                        .env("LC_ALL", "C.UTF-8")
                        .stderr(std::process::Stdio::null())
                        .status()
                        .unwrap();
                    assert!(st.code().is_some());
                }
                let meta = std::fs::symlink_metadata(&cfg).ok();
                let mut files: Vec<String> = std::fs::read_dir(home.join(".config/MangoHud"))
                    .map(|r| {
                        r.flatten()
                            .map(|e| e.file_name().to_string_lossy().into())
                            .collect()
                    })
                    .unwrap_or_default();
                files.sort();
                (
                    std::fs::read(&cfg).ok(),
                    meta.as_ref()
                        .map(|m| (m.file_type().is_symlink(), m.permissions().mode())),
                    files,
                    std::fs::read(home.join("real.conf")).ok(),
                    std::fs::read_to_string(d.join("pkill.log")).unwrap_or_default(),
                )
            };
            let got = run(true);
            let Some(fixed) = fixed else {
                assert_eq!(got, run(false), "{name} {value}");
                continue;
            };
            let fixed = fixed.replace('V', value).into_bytes();
            assert_eq!(got.0.as_deref(), Some(fixed.as_slice()), "{name} {value}");
            assert_eq!(got.1.map(|m| m.0), Some(link), "{name} {value}");
            if content.is_some() {
                assert_eq!(
                    got.1.map(|m| m.1 & 0o7777),
                    Some(if link { 0o777 } else { mode })
                );
            }
            assert_eq!(got.2, ["MangoHud.conf"], "{name} {value}");
            assert_eq!(got.3, link.then_some(fixed), "{name} {value}");
            assert_eq!(got.4, "-SIGUSR2 mangohud\n");
        }
    }
}

/// Where the original script is wrong the port writes what was meant: `echo >>` into a missing
/// `~/.config/MangoHud` failed (no limit saved), `>>` onto a last line without a newline
/// glued `fps_limit=` to it, sed's `.*` left the old value's bytes after one that is not
/// UTF-8, and `sed -i` replaced a symlinked config with a regular file.
#[test]
fn fps_limit_fixes_original_flaws() {
    use std::os::unix::fs::PermissionsExt;
    let d = tmp("fpsfix");
    let cfg = d.join("home/.config/MangoHud/MangoHud.conf");
    screen::set_fps_limit(&cfg, "60").unwrap();
    assert_eq!(std::fs::read(&cfg).unwrap(), b"fps_limit=60\n");

    std::fs::write(&cfg, b"gpu_stats").unwrap();
    screen::set_fps_limit(&cfg, "60").unwrap();
    assert_eq!(std::fs::read(&cfg).unwrap(), b"gpu_stats\nfps_limit=60\n");

    std::fs::write(&cfg, b"fps_limit=30\xff tail\nfps_limit=\xc3\xa9x\r\nb=1").unwrap();
    std::fs::set_permissions(&cfg, std::fs::Permissions::from_mode(0o640)).unwrap();
    screen::set_fps_limit(&cfg, "144").unwrap();
    assert_eq!(
        std::fs::read(&cfg).unwrap(),
        b"fps_limit=144\nfps_limit=144\r\nb=1"
    );
    assert_eq!(
        std::fs::metadata(&cfg).unwrap().permissions().mode() & 0o777,
        0o640
    );

    let real = d.join("dotfiles/MangoHud.conf");
    std::fs::create_dir_all(real.parent().unwrap()).unwrap();
    std::fs::write(&real, b"a=1\nfps_limit=30\n").unwrap();
    std::fs::remove_file(&cfg).unwrap();
    std::os::unix::fs::symlink(&real, &cfg).unwrap();
    screen::set_fps_limit(&cfg, "0").unwrap();
    assert!(
        std::fs::symlink_metadata(&cfg)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read(&real).unwrap(), b"a=1\nfps_limit=0\n");
    let leftovers: Vec<_> = std::fs::read_dir(cfg.parent().unwrap())
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(leftovers, ["MangoHud.conf"]);

    let gone = d.join("dotfiles/new/MangoHud.conf");
    std::fs::remove_file(&cfg).unwrap();
    std::os::unix::fs::symlink(&gone, &cfg).unwrap();
    screen::set_fps_limit(&cfg, "30").unwrap();
    assert!(
        std::fs::symlink_metadata(&cfg)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read(&gone).unwrap(), b"fps_limit=30\n");
}

/// `pkill -SIGUSR2 mangohud` and the port signal the same processes: those whose name
/// contains `mangohud`, and nothing else.
#[test]
fn pkill_matches_procps() {
    if !have("pkill") {
        return;
    }
    let d = tmp("pkill");
    let sleep = String::from_utf8(
        Command::new("bash")
            .args(["-c", "command -v sleep"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    // install(1) writes the copies in its own process (see `testenv::install_script`).
    for n in ["mangohud", "xmangohud-game", "mangoh"] {
        let st = Command::new("install")
            .args(["-m", "755", sleep.trim()])
            .arg(d.join(n))
            .status()
            .unwrap();
        assert!(st.success(), "install {n}: {st}");
    }
    // The children block SIGUSR2 (the mask survives execve(2)), so a signal sent to one stays
    // pending in its process-wide set (`ShdPnd`, proc_pid_status(5)) instead of ending it. Once
    // kill(2) returned, in pkill or the port, the signal was generated: reading that set then
    // tells which processes were signalled, with no window to wait out.
    let usr2 = {
        // SAFETY: sigset_t is plain data; sigemptyset(3) initialises it next.
        let mut set: libc::sigset_t = unsafe { std::mem::zeroed() };
        // SAFETY: sigemptyset(3) and sigaddset(3) write only the set they are given.
        unsafe { libc::sigemptyset(&mut set) };
        // SAFETY: as above.
        unsafe { libc::sigaddset(&mut set, libc::SIGUSR2) };
        set
    };
    let run = |native: bool| {
        let mut kids: Vec<_> = ["mangohud", "xmangohud-game", "mangoh"]
            .iter()
            .map(|n| {
                use std::os::unix::process::CommandExt;
                let mut c = Command::new(d.join(n));
                c.arg("infinity");
                let block = move || {
                    // SAFETY: sigprocmask(2) reads only the copied set.
                    if unsafe { libc::sigprocmask(libc::SIG_BLOCK, &usr2, std::ptr::null_mut()) }
                        == 0
                    {
                        Ok(())
                    } else {
                        Err(std::io::Error::last_os_error())
                    }
                };
                // SAFETY: the hook calls only sigprocmask(2), async-signal-safe (signal-safety(7))
                // as a pre_exec hook must be.
                unsafe { c.pre_exec(block) };
                c.spawn().unwrap()
            })
            .collect();
        // `spawn` returns once the child's execve succeeded, so each already carries its name.
        for (k, n) in kids.iter().zip(["mangohud", "xmangohud-game", "mangoh"]) {
            let comm = std::fs::read_to_string(format!("/proc/{}/comm", k.id())).unwrap();
            assert_eq!(comm.trim(), n);
        }
        if native {
            crate::osutil::pkill(libc::SIGUSR2, "mangohud");
        } else {
            Command::new("pkill")
                .args(["-SIGUSR2", "mangohud"])
                .status()
                .unwrap();
        }
        kids.iter_mut()
            .map(|k| {
                let status = std::fs::read_to_string(format!("/proc/{}/status", k.id())).unwrap();
                let pending = status
                    .lines()
                    .find_map(|l| l.strip_prefix("ShdPnd:"))
                    .map(|h| u64::from_str_radix(h.trim(), 16).unwrap())
                    .expect("ShdPnd in /proc/<pid>/status");
                k.kill().unwrap();
                k.wait().unwrap();
                pending & 1 << (libc::SIGUSR2 - 1) != 0
            })
            .collect::<Vec<_>>()
    };
    let want = run(false);
    assert_eq!(want, [true, true, false]);
    assert_eq!(run(true), want);
}

// --- notify-send ------------------------------------------------------------------------------

/// notify-send's log without its two `GetServerInformation` calls: libnotify only uses the
/// answer for image hints the original never passes, so the port sends `Notify` (and, with actions,
/// `GetCapabilities`) alone. Asserts the port made the round trip once per call it needs.
fn no_server_info(log: Vec<String>) -> Vec<String> {
    log.into_iter()
        .filter(|l| l != "GetServerInformation" && l != "info")
        .collect()
}

/// A notification server on a private bus that logs every call it gets (what `dbus-monitor`
/// shows for the sender) and invokes the first action of a notification that has some.
struct NotifySrv {
    log: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    no_actions: std::sync::Arc<std::sync::atomic::AtomicBool>,
    next: u32,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl NotifySrv {
    fn get_server_information(&self) -> (String, String, String, String) {
        self.log.lock().unwrap().push("GetServerInformation".into());
        ("test".into(), "test".into(), "1".into(), "1.2".into())
    }
    fn get_capabilities(&self) -> Vec<String> {
        self.log.lock().unwrap().push("GetCapabilities".into());
        if self.no_actions.load(std::sync::atomic::Ordering::Relaxed) {
            return vec!["body".into()];
        }
        vec!["actions".into(), "body".into()]
    }
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &mut self,
        #[zbus(connection)] conn: &zbus::Connection,
        app: String,
        replaces: u32,
        icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
        timeout: i32,
    ) -> zbus::object_server::ResponseDispatchNotifier<u32> {
        self.next += 1;
        let id = self.next;
        let mut h: Vec<String> = hints
            .iter()
            .filter(|(k, _)| *k != "sender-pid")
            .map(|(k, v)| format!("{k}={v:?}"))
            .collect();
        h.sort();
        h.push(format!(
            "sender-pid:{}",
            hints
                .get("sender-pid")
                .map(|v| v.value_signature().to_string())
                .unwrap_or_default()
        ));
        self.log.lock().unwrap().push(format!(
            "Notify {app}|{replaces}|{icon}|{summary}|{body:?}|{actions:?}|{h:?}|{timeout}"
        ));
        // The action is invoked once the reply carrying the id is sent: the bus delivers a
        // sender's messages in order, so the client knows the id the signal names.
        let (reply, sent) = zbus::object_server::ResponseDispatchNotifier::new(id);
        if let Some(key) = actions.first().cloned() {
            let conn = conn.clone();
            std::thread::spawn(move || {
                zbus::block_on(async {
                    sent.await;
                    let _ = conn
                        .emit_signal(
                            None::<()>,
                            "/org/freedesktop/Notifications",
                            "org.freedesktop.Notifications",
                            "ActionInvoked",
                            &(id, key),
                        )
                        .await;
                })
            });
        }
        reply
    }
    async fn close_notification(&self, #[zbus(connection)] conn: &zbus::Connection, id: u32) {
        self.log.lock().unwrap().push("CloseNotification".into());
        let _ = conn
            .emit_signal(
                None::<()>,
                "/org/freedesktop/Notifications",
                "org.freedesktop.Notifications",
                "NotificationClosed",
                &(id, 3u32),
            )
            .await;
    }
}

/// Every notification `ports::notify` sends, against `notify-send` with the argv the original builds:
/// the same D-Bus calls (arguments, hints and their types), stdout and exit code.
#[test]
fn notes_match_notify_send() {
    use crate::notifier::Note;
    use std::io::BufRead;
    let v = Command::new("notify-send").arg("--version").output();
    if !v
        .as_ref()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(" 0.8"))
        || !have("dbus-daemon")
    {
        return crate::equiv_missing("libnotify 0.8 notify-send and dbus-daemon");
    }
    let mut c = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut addr = String::new();
    std::io::BufReader::new(c.stdout.take().unwrap())
        .read_line(&mut addr)
        .unwrap();
    let addr = addr.trim().to_string();
    struct Kill(std::process::Child);
    impl Drop for Kill {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let _daemon = Kill(c);
    let log = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
    let no_actions = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let _srv = zbus::blocking::connection::Builder::address(addr.as_str())
        .unwrap()
        .name("org.freedesktop.Notifications")
        .unwrap()
        .serve_at(
            "/org/freedesktop/Notifications",
            NotifySrv {
                log: log.clone(),
                no_actions: no_actions.clone(),
                next: 0,
            },
        )
        .unwrap()
        .build()
        .unwrap();
    let usage = "Usage: <tt>/superpaste NUM_OF_ENTRIES[i]</tt>\nSupply <tt>i</tt> when you want images\nExamples:\n<tt>/superpaste 4i</tt> for the last 4 images\n<tt>/superpaste 7</tt> for the last 7 entries";
    let notes = [
        Note::shell("Low battery", "Consider plugging in your device")
            .critical()
            .transient(),
        Note::shell(
            "Critically low battery",
            "Please charge!\nAutomatic suspend triggers at 3%",
        )
        .critical()
        .transient(),
        Note::shell("Battery full", "Please unplug the charger").transient(),
        Note::shell(
            "Weather Service",
            "Cannot find a GPS service. Using the fallback method instead.",
        ),
        Note::shell("Weather", "Refreshing (manually triggered)"),
        Note::shell("Pomodoro", "🌿 Long break: 15 minutes"),
        Note::shell(
            "Couldn't recognize music",
            "Perhaps what you're listening to is too niche",
        ),
        Note::shell("Superpaste", usage),
        Note::shell("-u", "x").action("A"),
        Note::shell("Music Recognized", "-x - y"),
        Note::shell("-", "back\\slash \\n \\101 'q' \"d\" $HOME"),
        Note::shell("Music Recognized", "undefined - undefined")
            .action("Shazam")
            .action("YouTube"),
        Note::shell("Music Recognized", "Title = x - Artist")
            .action("Shazam")
            .action("YouTube"),
        Note::shell("Music Recognized", "a - b")
            .action(" Sha zam ")
            .action("b"),
        Note::shell("Music Recognized", "no actions cap")
            .action("Shazam")
            .action("YouTube"),
    ];
    let last = notes.len() - 1;
    for (i, n) in notes.into_iter().enumerate() {
        no_actions.store(i == last, std::sync::atomic::Ordering::Relaxed);
        let argv = n.argv();
        // The intended notification: the original's options, then the summary and body after `--`, so
        // text starting with `-` is shown instead of parsed as options.
        let mut reference = vec![argv[0].clone()];
        reference.extend(argv[3..].iter().cloned());
        reference.push("--".into());
        reference.extend(argv[1..3].iter().cloned());
        log.lock().unwrap().clear();
        let o = Command::new(&reference[0])
            .args(&reference[1..])
            .env("DBUS_SESSION_BUS_ADDRESS", &addr)
            .output()
            .unwrap();
        let real = (
            o.status.code().unwrap_or(-1),
            String::from_utf8(o.stdout).unwrap(),
        );
        // Intended differences: the body goes out verbatim (notify-send unescapes it) and action
        // keys and the printed action are 0-based indexes, which the original's handlers compare against.
        let sent = format!("|{:?}|", crate::libnotify::strcompress(&n.body));
        let verbatim = format!("|{:?}|", n.body);
        let mut real_log: Vec<String> = std::mem::take(&mut *log.lock().unwrap())
            .into_iter()
            .map(|l| {
                l.replacen(&sent, &verbatim, 1)
                    .replace("[\"1\", ", "[\"0\", ")
                    .replace(", \"2\", ", ", \"1\", ")
            })
            .collect();
        let real = match real.1.trim().parse::<u32>() {
            Ok(k) => (real.0, format!("{}\n", k - 1)),
            Err(_) => real,
        };
        let ours = n.show(Some(&addr), &Default::default());
        let our_log = std::mem::take(&mut *log.lock().unwrap());
        // notify-send's GetServerInformation round trips change nothing; the port skips them,
        // except when it runs notify-send itself for option-like args.
        if !our_log.iter().any(|l| l == "GetServerInformation") {
            real_log.retain(|l| l != "GetServerInformation");
        }
        assert_eq!(our_log, real_log, "{argv:?}");
        assert_eq!(ours, real, "{argv:?}");
    }
    // Every notification went out on one connection (notify-send opens one per process).
    let me = zbus::blocking::connection::Builder::address(addr.as_str())
        .unwrap()
        .build()
        .unwrap();
    let dbus = zbus::blocking::fdo::DBusProxy::new(&me).unwrap();
    let skip = [
        me.unique_name().unwrap().to_string(),
        _srv.unique_name().unwrap().to_string(),
    ];
    let pid = std::process::id();
    let ours = dbus
        .list_names()
        .unwrap()
        .into_iter()
        .filter(|n| n.starts_with(':') && !skip.iter().any(|s| s == n.as_str()))
        .filter(|n| {
            dbus.get_connection_unix_process_id(n.clone().into())
                .is_ok_and(|p| p == pid)
        })
        .count();
    assert_eq!(ours, 1, "notification connections");
}

/// The argv each port shows is the one its QML source builds.
#[test]
fn note_argv_is_the_source_argv() {
    use crate::notifier::Note;
    let n = Note::shell("S", "B").critical().transient();
    assert_eq!(
        n.argv(),
        [
            "notify-send",
            "S",
            "B",
            "-u",
            "critical",
            "-a",
            "Shell",
            "--hint=int:transient:1"
        ]
    );
    let n = Note::shell("S", "B").action("Shazam").action("YouTube");
    assert_eq!(
        n.argv(),
        [
            "notify-send",
            "S",
            "B",
            "-A",
            "Shazam",
            "-A",
            "YouTube",
            "-a",
            "Shell"
        ]
    );
}

// The WARP toggles' failure notifications: the D-Bus calls `notify-send <summary> <body> -a
// Shell` makes, captured by a fake notification server on a private bus.
struct WarpNotifySrv(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl WarpNotifySrv {
    fn get_server_information(&self) -> (String, String, String, String) {
        self.0.lock().unwrap().push("info".into());
        ("test".into(), "test".into(), "1".into(), "1.2".into())
    }
    fn get_capabilities(&self) -> Vec<String> {
        self.0.lock().unwrap().push("caps".into());
        vec!["body".into()]
    }
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &mut self,
        app: String,
        replaces: u32,
        icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
        timeout: i32,
    ) -> u32 {
        let mut h: Vec<String> = hints
            .iter()
            .map(|(k, v)| match k.as_str() {
                "sender-pid" => format!("{k}:{}", v.value_signature()),
                _ => format!("{k}={v:?}"),
            })
            .collect();
        h.sort();
        self.0.lock().unwrap().push(format!(
            "notify {app}|{replaces}|{icon}|{summary}|{body}|{actions:?}|{h:?}|{timeout}"
        ));
        1
    }
}

#[test]
fn warp_notify_same_as_notify_send() {
    use std::io::BufRead;
    if !have("notify-send") || !have("dbus-daemon") {
        return;
    }
    let mut d = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut addr = String::new();
    std::io::BufReader::new(d.stdout.take().unwrap())
        .read_line(&mut addr)
        .unwrap();
    let addr = addr.trim().to_string();
    let _d = Daemon(d);
    let log = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
    let _srv = zbus::blocking::connection::Builder::address(addr.as_str())
        .unwrap()
        .name("org.freedesktop.Notifications")
        .unwrap()
        .serve_at("/org/freedesktop/Notifications", WarpNotifySrv(log.clone()))
        .unwrap()
        .build()
        .unwrap();
    let client = zbus::blocking::connection::Builder::address(addr.as_str())
        .unwrap()
        .build()
        .unwrap();
    for body in [
        "Connection failed. Please inspect manually with the <tt>warp-cli</tt> command",
        "Registration failed. Please inspect manually with the <tt>warp-cli</tt> command",
        // A translation with C escapes: notify-send unescapes them, the port shows them as
        // written.
        "Échec \\n de \\t connexion \\101 \\\\",
    ] {
        let st = Command::new("notify-send")
            .args(["Cloudflare WARP", body, "-a", "Shell"])
            .env("DBUS_SESSION_BUS_ADDRESS", &addr)
            .status()
            .unwrap();
        assert!(st.success());
        let real = no_server_info(std::mem::take(&mut *log.lock().unwrap()));
        notify_on(&client, "Cloudflare WARP", body, "Shell");
        let ours = std::mem::take(&mut *log.lock().unwrap());
        // libnotify >= 0.8 (Arch) adds `sender-pid`; older ones (this VM's 0.7) do not.
        assert_eq!(ours.len(), 1, "{ours:?}");
        assert!(ours[0].contains("\"sender-pid:x\", "), "{ours:?}");
        let pidless = |v: Vec<String>| -> Vec<String> {
            v.into_iter()
                .filter(|l| l.starts_with("notify "))
                .map(|l| l.replace("\"sender-pid:x\", ", ""))
                .collect()
        };
        let verbatim = |v: Vec<String>| -> Vec<String> {
            v.into_iter()
                .map(|l| l.replacen(&crate::libnotify::strcompress(body), body, 1))
                .collect()
        };
        assert_eq!(pidless(ours), verbatim(pidless(real)), "{body}");
    }
}

struct Daemon(std::process::Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn fps_limit_reports_write_failure() {
    let d = tmp("fpsfail");
    std::fs::write(d.join(".config"), "").unwrap();
    assert!(screen::set_fps_limit(&d.join(".config/MangoHud/MangoHud.conf"), "60").is_err());
}

/// The original's region search/OCR/edit chains (`crop && upload && xdg-open && rm`) left the temporary
/// screenshot behind on any failure; the port removes it on every outcome.
#[test]
fn region_temp_screenshot_removed_on_failure() {
    let d = tmp("region-fail");
    let run = |name: &str, data: &[u8], after: shot::After| {
        let p = d.join(name).to_string_lossy().into_owned();
        std::fs::write(&p, data).unwrap();
        let code = shot::Job::Crop {
            path: p.clone(),
            rect: (0, 0, 2, 2),
            after,
        }
        .run(&Io::detached());
        (code, std::path::Path::new(&p).exists())
    };
    let png = wl::png(&image::RgbaImage::from_pixel(
        4,
        4,
        image::Rgba([1, 2, 3, 255]),
    ));
    // Broken image: the crop fails.
    assert_eq!(run("broken", b"not an image", shot::After::Ocr), (1, false));
    // Upload fails (nothing listens): no search page opens.
    let search = shot::After::Search {
        endpoint: "http://127.0.0.1:9/upload".into(),
        base: "https://lens.google.com/uploadbyurl?url=".into(),
    };
    assert_eq!(run("upload", &png, search), (1, false));
    // Editor missing.
    assert_eq!(
        run(
            "edit",
            &png,
            shot::After::Edit("/nonexistent/editor".into())
        ),
        (127, false)
    );
}

/// Closing the region selector without a snip (Esc, click-away) deletes its capture; the original left it
/// in the screenshot dir.
#[test]
fn region_temp_screenshot_removed_on_cancel() {
    let d = tmp("region-cancel");
    let p = |n: &str| d.join(n).to_string_lossy().into_owned();
    let exists = |p: &str| std::path::Path::new(p).exists();
    let (written, claimed, late) = (p("written"), p("claimed"), p("late"));
    shot::temp_started(&written);
    std::fs::write(&written, b"x").unwrap();
    shot::temp_written(&written);
    // Handed to a crop job first: the job owns (and removes) it.
    shot::temp_started(&claimed);
    std::fs::write(&claimed, b"x").unwrap();
    shot::temp_written(&claimed);
    shot::temp_claimed(&claimed);
    // Still being captured when the selector closes.
    shot::temp_started(&late);
    shot::temps_discard();
    assert!(!exists(&written));
    assert!(exists(&claimed));
    std::fs::write(&late, b"x").unwrap();
    shot::temp_written(&late);
    assert!(!exists(&late));
}

// The original's `/tmp/rustshell/media/{cliphist,screenshot}` are shared: a symlink planted where an
// image goes is replaced, never written through.
#[test]
fn cliphist_decode_does_not_follow_symlinks() {
    let d = tmp("csym");
    let bin = fake_cliphist(&d, 0);
    let victim = d.join("victim");
    std::fs::write(&victim, "keep").unwrap();
    let dir = d.join("cliphist");
    std::fs::create_dir(&dir).unwrap();
    let p = dir.join("7");
    std::os::unix::fs::symlink(&victim, &p).unwrap();
    let job = clip::Job::DecodeTo {
        bin: bin.clone(),
        entry: "7\t[[ binary data 1 KiB png 1x1 ]]".into(),
        path: p.to_string_lossy().into_owned(),
    };
    assert_eq!(job.run(&Io::detached()), 0);
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep");
    assert_eq!(std::fs::read(&p).unwrap(), b"DATA\x00\x01png");
    // A failed decode leaves nothing for the next `-f` check.
    let bad = fake_cliphist(&d, 1);
    let q = dir.join("8");
    let code = clip::Job::DecodeTo {
        bin: bad,
        entry: "8\tx".into(),
        path: q.to_string_lossy().into_owned(),
    }
    .run(&Io::detached());
    assert_eq!(code, 1);
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
}

#[test]
fn temp_screenshot_does_not_follow_symlinks() {
    let d = tmp("ssym");
    let victim = d.join("victim");
    std::fs::write(&victim, "keep").unwrap();
    let dir = d.join("screenshot");
    std::fs::create_dir(&dir).unwrap();
    let p = dir.join("image-DP-1");
    std::os::unix::fs::symlink(&victim, &p).unwrap();
    assert!(shot::save_temp(&p.to_string_lossy(), b"png"));
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep");
    assert_eq!(std::fs::read(&p).unwrap(), b"png");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
}
