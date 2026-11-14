//! A/B of GCloudVision.qml and GCloudTranslate.qml against the command lines the original ran there.
//! The reference side is the original's own files (`tests/gcloud_commands`): MultiTurnProcess.qml
//! (`runSequence`/`step` running each command, then the callbacks) driven by GCloudVision
//! (prepMultiproc: bash/base64; lookMultiproc: jq, then curl to Vision) and GCloudTranslate
//! (multiproc: curl to Translation), with the real `bash`, `base64`, `jq` and `curl`. The
//! native side is the tree's files, whose requests are the `Impulse` types `GoogleVision` and
//! `GoogleTranslate`. Both reach the real Google hosts through a mock `https_proxy` that
//! refuses the CONNECT, so no request leaves the machine. Compared: the models' state, errors
//! and `onlineImageReady`, the CONNECT targets, and the files written.
//! The test runs in a process of its own (`testenv::run_in_own_process_env`) whose environment
//! names the proxy and private XDG homes from the start; the proxy runs here, in the process
//! that starts it, and logs the requests to a file that process reads.
//! GoogleCloud's token request is `sound_qalc_gcloud_booru_pin_equiv.rs`.

use qml::runtime::Runtime;
use qml::value::Value;
use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::Duration;
use testenv::Wakeups;

fn ri() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

const SCREENSHOT_TEMP: &str = "/tmp/rustshell/media/screenshot";

/// GCloudVision.qml, GCloudTranslate.qml and MultiTurnProcess.qml as the original had them.
const COMMANDS: [(&str, &str); 3] = [
    (
        "GCloudVisionCommand.qml",
        include_str!("gcloud_commands/GCloudVisionCommand.qml"),
    ),
    (
        "GCloudTranslateCommand.qml",
        include_str!("gcloud_commands/GCloudTranslateCommand.qml"),
    ),
    (
        "MultiTurnProcess.qml",
        include_str!("gcloud_commands/MultiTurnProcess.qml"),
    ),
];

const DOC: &str = r#"import QtQuick
import ri.services
import ri.modules.common.models.gCloud
Item {
    property var s: GoogleCloud
    property var errs: []
    @VISION@ { id: v; onError: m => errs.push("vision: " + m) }
    @TRANSLATE@ { id: t; onError: m => errs.push("translate: " + m) }
    function ready() {
        GoogleCloud.keyContent = ({ project_id: "proj-1" });
        GoogleCloud.keyError = false;
        GoogleCloud.keyReady = true;
        GoogleCloud.tokenError = false;
        GoogleCloud.token = "ya29.test-token";
        GoogleCloud.tokenExpiry = new Date(Date.now() + 3600000);
        GoogleCloud.tokenReady = true;
    }
    function look(u) { v.annotateImage(u) }
    function tr() { t.translateStrings(["it's", "Привет"]) }
    function states() { return [v.state, t.state] }
    // Vision and Translate run concurrently and fail independently: the order of their
    // error signals is unspecified, so they are compared as a set.
    function report() {
        return JSON.stringify([v.state, v.errorMessage, v.onlineImageReady, t.state, t.errorMessage, t.pendingStrings, errs.slice().sort()])
    }
}
"#;

/// An HTTP proxy refusing every request with 403, on a thread of this process; its address. It
/// appends each request line to `log` before it answers, so a client that got its answer
/// finds its request there.
fn refusing_proxy(log: PathBuf) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("http://{}", l.local_addr().unwrap());
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut line = String::new();
            r.read_line(&mut line).unwrap_or(0);
            loop {
                let mut h = String::new();
                if r.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                    break;
                }
            }
            let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
            f.write_all(format!("{}\n", line.trim_end()).as_bytes())
                .unwrap();
            drop(f);
            let _ = s.write_all(
                b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    });
    addr
}

/// The test's host loop on a virtual clock (`Runtime::tick` sees `now`): it sleeps only on the
/// runtime's own wakeups (its waker's ping, its epoll set: process output and exits) and on
/// pool work (`workpool::wait_idle`, which also reaps `execDetached` children), and jumps the
/// clock to the next timer's deadline once nothing is in flight, so a test waits for what it
/// waits for, never for a stretch of wall time.
struct Host {
    wake: Wakeups,
    ep: Option<i32>,
    now: f64,
}

impl Host {
    /// Becomes `rt`'s host loop: installs its waker ping and watches its epoll set.
    fn new(rt: &mut Runtime) -> Host {
        let wake = Wakeups::new();
        rt.world.borrow().waker.set_ping(wake.ping());
        let ep = rt.process_fd();
        Host { wake, ep, now: 0.0 }
    }

    /// Runs loop turns until nothing is ready at `now`: no pool work, no wakeup, no epoll
    /// event, no timer due and nothing a poll changed left to poll.
    fn drain(&mut self, rt: &mut Runtime) {
        loop {
            let rev = rt.revision();
            workpool::wait_idle();
            rt.poll_native();
            rt.tick(self.now);
            rt.poll_native();
            rt.settle();
            let woke = self.wake.wait(self.ep, Some(Duration::ZERO));
            if woke.fd {
                rt.process_fd_ready();
            }
            if !woke.any() && rt.revision() == rev && rt.next_deadline(self.now) > 0.0 {
                return;
            }
        }
    }

    /// A Process (child or in-process job) or a keyring request is running: its end wakes
    /// the loop.
    fn in_flight(rt: &Runtime) -> bool {
        let w = rt.world.borrow();
        !w.procs.is_empty() || w.keyring.as_ref().is_some_and(|k| !k.idle())
    }

    /// Sleeps until the runtime is woken.
    fn block(&mut self, rt: &mut Runtime) {
        if self.wake.wait(self.ep, None).fd {
            rt.process_fd_ready();
        }
    }

    /// Runs the loop until `done`.
    fn until(&mut self, rt: &mut Runtime, what: &str, mut done: impl FnMut(&mut Runtime) -> bool) {
        // Named in the output nextest shows when its slow-timeout ends a hang.
        eprintln!("waiting for {what}");
        loop {
            self.drain(rt);
            if done(rt) {
                return;
            }
            let d = rt.next_deadline(self.now);
            if Self::in_flight(rt) || !d.is_finite() {
                self.block(rt);
            } else {
                self.now += d;
            }
        }
    }

    /// Runs the loop through the next `ms` of virtual time: every Process started meanwhile
    /// runs to its end, each timer fires at its deadline.
    fn advance(&mut self, rt: &mut Runtime, ms: f64) {
        let end = self.now + ms;
        loop {
            self.drain(rt);
            if Self::in_flight(rt) {
                self.block(rt);
                continue;
            }
            let d = rt.next_deadline(self.now);
            if self.now + d <= end {
                self.now += d;
            } else if self.now < end {
                self.now = end;
            } else {
                return;
            }
        }
    }
}

#[derive(Debug, PartialEq)]
struct Got {
    report: String,
    connects: Vec<String>,
    base64: Option<String>,
    payload: Option<String>,
}

fn call(rt: &mut Runtime, r: usize, f: &str, args: Vec<Value>) {
    rt.call_method(r, f, args)
        .unwrap_or_else(|| panic!("no {f}"))
        .unwrap_or_else(|e| panic!("{f}: {e} {:?}", rt.errors()));
}

/// One side; `log` is the proxy's request log.
fn run(original: bool, d: &Path, image: &Path, log: &Path) -> Got {
    for f in ["vision_base64.txt", "vision_payload.json"] {
        let _ = std::fs::remove_file(Path::new(SCREENSHOT_TEMP).join(f));
    }
    std::fs::write(log, "").unwrap();
    let mut rt = Runtime::new(Some(ri()));
    rt.set_screens(&[("DP-1".into(), 0, 0, 1920, 1080)]);
    rt.load_singletons_lazy(&[
        ri(),
        ri().join("services"),
        ri().join("modules/common"),
        ri().join("modules/common/functions"),
    ]);
    for (name, src) in COMMANDS {
        std::fs::write(d.join(name), src).unwrap();
    }
    let (vision, translate) = if original {
        ("GCloudVisionCommand", "GCloudTranslateCommand")
    } else {
        ("GCloudVision", "GCloudTranslate")
    };
    let doc = d.join("CaseMultiTurn.qml");
    let src = DOC
        .replace("@VISION@", vision)
        .replace("@TRANSLATE@", translate);
    std::fs::write(&doc, src).unwrap();
    let r = rt
        .load_file(&doc)
        .unwrap_or_else(|e| panic!("load: {e:?} {:?}", rt.errors()));
    let mut host = Host::new(&mut rt);
    // KeyringStorage's first load (no keyring here) settles before the token is set.
    host.advance(&mut rt, 700.0);
    call(&mut rt, r, "ready", vec![]);
    let uri = format!("file://{}", image.display());
    call(&mut rt, r, "look", vec![Value::str(&uri)]);
    call(&mut rt, r, "tr", vec![]);
    // Error = 3 in GCloudApi.State.
    host.until(&mut rt, "both requests to fail", |rt| {
        let s = rt
            .call_method(r, "states", vec![])
            .and_then(Result::ok)
            .unwrap_or_default();
        s.to_js_string() == "3,3"
    });
    let report = rt
        .call_method(r, "report", vec![])
        .and_then(Result::ok)
        .unwrap_or_default()
        .to_js_string();
    drop(rt);
    let mut connects: Vec<String> = std::fs::read_to_string(log)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect();
    connects.sort();
    let read = |f: &str| std::fs::read_to_string(Path::new(SCREENSHOT_TEMP).join(f)).ok();
    Got {
        report,
        connects,
        base64: read("vision_base64.txt"),
        payload: read("vision_payload.json"),
    }
}

#[test]
fn gcloud_vision_and_translate_match_the_original_commands() {
    const TEST: &str = concat!(
        module_path!(),
        "::gcloud_vision_and_translate_match_the_original_commands"
    );
    for t in ["bash", "base64", "jq", "curl"] {
        if !testenv::require(t) {
            return;
        }
    }
    if !testenv::in_own_process(TEST) {
        let d = tempfile::tempdir().unwrap();
        let conf = d.path().join("config");
        std::fs::create_dir_all(conf.join("rusty-impulse")).unwrap();
        std::fs::write(conf.join("rusty-impulse/config.json"), "{}").unwrap();
        let log = d.path().join("proxy.log");
        std::fs::write(&log, "").unwrap();
        let mut vars: Vec<(&str, OsString)> = [
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_CACHE_HOME", "cache"),
            ("XDG_STATE_HOME", "state"),
            ("XDG_DATA_HOME", "data"),
        ]
        .into_iter()
        .map(|(k, v)| (k, d.path().join(v).into()))
        .collect();
        vars.push(("https_proxy", refusing_proxy(log).into()));
        let unset = [
            "no_proxy",
            "NO_PROXY",
            "all_proxy",
            "ALL_PROXY",
            "HTTPS_PROXY",
        ];
        testenv::run_in_own_process_env(TEST, vars, &unset);
        return;
    }
    let d = PathBuf::from(std::env::var_os("XDG_CONFIG_HOME").unwrap())
        .parent()
        .unwrap()
        .to_path_buf();
    let log = d.join("proxy.log");
    for (k, name) in ["shot.png", "it's a shot.png"].into_iter().enumerate() {
        let image = d.join(name);
        std::fs::write(
            &image,
            (0..=255u8).cycle().take(3000 + k).collect::<Vec<_>>(),
        )
        .unwrap();
        let want = run(true, &d, &image, &log);
        let got = run(false, &d, &image, &log);
        assert_eq!(
            want.connects,
            [
                "CONNECT translation.googleapis.com:443 HTTP/1.1",
                "CONNECT vision.googleapis.com:443 HTTP/1.1"
            ],
            "{name}: the original's requests"
        );
        assert!(
            want.report.contains("Failed to parse API response"),
            "{name}: {}",
            want.report
        );
        assert!(
            want.payload
                .as_deref()
                .is_some_and(|p| p.contains("DOCUMENT_TEXT_DETECTION"))
        );
        // Intended: the native steps report the real cause instead of the original's "Failed to parse
        // API response", and keep the image and request in memory instead of temp files.
        let intended = Got {
            report: want.report.replace(
                "Failed to parse API response",
                "CONNECT proxy failed: proxy server responded 403/403",
            ),
            connects: want.connects.clone(),
            base64: None,
            payload: None,
        };
        assert_eq!(got, intended, "{name}: native vs original");
    }
}
