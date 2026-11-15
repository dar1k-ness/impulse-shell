//! PipeWire (pipewire-pulse for the script's `pactl`), a private session bus with a recording
//! notification server, a fake Hyprland socket (and `hyprctl` for the script), and recording
//! `songrec` / `wf-recorder` / `slurp` leaf stubs. The leaf invocations (argv + cwd), stdout,
//! exit code, notifications and files must match.
use qml::runtime::Runtime;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixListener;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

const MONITORS: &str = r#"[{"id":0,"name":"DP-1","focused":false,"width":1920,"height":1080},{"id":1,"name":"HDMI-A-1","focused":true,"width":2560,"height":1440}]"#;
const MATCH: &str = r#"{"matches": [{"id": "1", "offset": 2.5}], "track": {"title": "Song é", "subtitle": "Artist"}}"#;

fn ri() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn ctl(&self, name: &str) -> PathBuf {
        self.root.join("ctl").join(name)
    }
    fn leaves(&self) -> PathBuf {
        self.root.join("leaves.log")
    }
    fn notes(&self) -> PathBuf {
        self.root.join("notes.log")
    }
    fn home(&self) -> PathBuf {
        self.root.join("home")
    }
}

/// A recording leaf; `#!/bin/bash` (not `env bash`) keeps its comm, so `pgrep NAME` sees it.
fn stub(dir: &Path, name: &str, body: &str) {
    let p = dir.join(name);
    testenv::install_script(
        &p,
        &format!(
            "#!/bin/bash\n{{ printf '%s' {name}; printf '\\t%s' \"$@\" \"cwd=$PWD\"; echo; }} >> \"$RI_LEAF_LOG\"\n{body}\n"
        ),
    );
}

fn pactl(args: &[&str]) -> Option<String> {
    let o = Command::new("pactl")
        .args(args)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Starts a daemon that runs until the test process exits ([`stop_daemons`]), with
/// `listen` as its socket-activation fds 3.. (sd_listen_fds(3): LISTEN_FDS, and LISTEN_PID set
/// by the shell that `exec`s it so that it is the daemon's pid). Returns its stdout if `stdout`
/// is piped.
fn daemon(
    argv: &[&str],
    listen: &[UnixListener],
    stdout: Stdio,
) -> Option<BufReader<std::process::ChildStdout>> {
    let mut c = Command::new("sh");
    c.args(["-c", r#"LISTEN_PID=$$ exec "$0" "$@""#])
        .args(argv)
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(Stdio::null());
    if listen.is_empty() {
        c.env_remove("LISTEN_FDS");
    } else {
        c.env("LISTEN_FDS", listen.len().to_string());
    }
    let fds: Vec<RawFd> = listen.iter().map(|l| l.as_raw_fd()).collect();
    let mut moved = fds.clone();
    let first = 3 + fds.len() as RawFd;
    // Runs in the forked child before exec: it only calls fcntl(2) and dup2(2), which are
    // async-signal-safe, and allocates nothing (`moved` was allocated before the fork).
    let hook = move || {
        // Moved above 3..first first, so that placing one cannot overwrite another.
        for (m, &fd) in moved.iter_mut().zip(&fds) {
            // SAFETY: F_DUPFD_CLOEXEC duplicates the open `fd`; the copy closes at exec.
            *m = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, first) };
            if *m < 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
        for (i, &m) in moved.iter().enumerate() {
            // SAFETY: dup2 onto 3 + i of the open `m`; the target is left without FD_CLOEXEC,
            // so the daemon inherits it.
            if unsafe { libc::dup2(m, 3 + i as RawFd) } < 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok(())
    };
    // SAFETY: `hook` is async-signal-safe and allocation-free, as pre_exec requires.
    unsafe { c.pre_exec(hook) };
    let mut child = c.spawn().unwrap_or_else(|e| panic!("{argv:?}: {e}"));
    let out = child.stdout.take().map(BufReader::new);
    DAEMONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(child);
    out
}

/// Every daemon [`daemon`] started, oldest first.
static DAEMONS: Mutex<Vec<std::process::Child>> = Mutex::new(Vec::new());

/// Registered with atexit(3), which runs whether the test passed or panicked: the harness ends
/// through `exit` in both cases. Each daemon gets SIGTERM, newest first, and is reaped. A test
/// that nextest kills on timeout takes them with it, because nextest signals the test's
/// process group, which the daemons share.
extern "C" fn stop_daemons() {
    let mut ds = DAEMONS.lock().unwrap_or_else(|e| e.into_inner());
    for c in ds.iter().rev() {
        // SAFETY: kill(2) on a child this process has not reaped yet, so its pid is not reused.
        unsafe { libc::kill(c.id() as libc::pid_t, libc::SIGTERM) };
    }
    for c in ds.iter_mut().rev() {
        let _ = c.wait();
    }
}

/// Waits for every process left in process group `pgid` (a script run's own group, so what it
/// left running in the background, reparented here by PR_SET_CHILD_SUBREAPER) to exit: waitid(2)
/// blocks on their exits and fails with ECHILD once none is left in the group.
fn reap_group(pgid: u32) {
    loop {
        // SAFETY: waitid(2) writes only the zeroed siginfo_t it is given.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: as above; the group's members are children of this process that nothing
        // else waits for.
        let r = unsafe { libc::waitid(libc::P_PGID, pgid as libc::id_t, &mut info, libc::WEXITED) };
        if r == 0 {
            continue;
        }
        let e = std::io::Error::last_os_error();
        match e.raw_os_error() {
            Some(libc::EINTR) => continue,
            Some(libc::ECHILD) => return,
            _ => panic!("waitid: {e}"),
        }
    }
}

const NOTIFY_SRV: &str = r#"
import dbus, dbus.service, dbus.mainloop.glib, json, sys
from gi.repository import GLib
dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
log = sys.argv[1]
class N(dbus.service.Object):
    @dbus.service.method("org.freedesktop.Notifications", in_signature="", out_signature="ssss")
    def GetServerInformation(self): return ("ri-test", "ri", "1", "1.2")
    @dbus.service.method("org.freedesktop.Notifications", in_signature="", out_signature="as")
    def GetCapabilities(self): return ["body", "actions"]
    @dbus.service.method("org.freedesktop.Notifications", in_signature="susssasa{sv}i", out_signature="u")
    def Notify(self, app, rid, icon, summary, body, actions, hints, timeout):
        with open(log, "a") as f:
            f.write(json.dumps([str(app), int(rid), str(icon), str(summary), str(body), [str(a) for a in actions], int(hints.get("urgency", -1)), int(timeout)]) + "\n")
        return 1
    @dbus.service.method("org.freedesktop.Notifications", in_signature="u", out_signature="")
    def CloseNotification(self, i): pass
bus = dbus.SessionBus()
name = dbus.service.BusName("org.freedesktop.Notifications", bus)
N(bus, "/org/freedesktop/Notifications")
print("ready", flush=True)
GLib.MainLoop().run()
"#;

/// In `test`'s own process: the started sandbox, whose environment every run (script or native)
/// sees. The process that ran it there (`testenv::run_in_own_process_env`) gets `None`, as
/// without the tools. That process runs without PULSE_SERVER and PIPEWIRE_REMOTE, so pactl and
/// libpipewire reach only the sandbox's daemons.
fn sandbox(test: &str) -> Option<Sandbox> {
    if testenv::in_own_process(test) {
        let s = Sandbox {
            root: std::env::var_os("SCRIPT_PORTS_ROOT").unwrap().into(),
        };
        s.start();
        return Some(s);
    }
    for t in [
        "bash",
        "jq",
        "pgrep",
        "pkill",
        "pactl",
        "pipewire",
        "pipewire-pulse",
        "wireplumber",
        "dbus-daemon",
        "notify-send",
        "python3",
        "mkfifo",
        "awk",
    ] {
        if !testenv::require(t) {
            return None;
        }
    }
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let (xdg, bin) = (root.join("xdg"), root.join("bin"));
    for d in [&xdg, &bin, &root.join("ctl")] {
        std::fs::create_dir_all(d).unwrap();
    }
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&xdg, std::fs::Permissions::from_mode(0o700)).unwrap();
    let s = Sandbox {
        root: root.to_path_buf(),
    };
    // HOME exists from the start: the daemons and `pactl` keep their state under it.
    std::fs::create_dir_all(s.home()).unwrap();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut bus = std::ffi::OsString::from("unix:path=");
    bus.push(xdg.join("bus"));
    testenv::run_in_own_process_env(
        test,
        [
            ("SCRIPT_PORTS_ROOT", root.into()),
            ("XDG_RUNTIME_DIR", xdg.clone().into()),
            ("DBUS_SESSION_BUS_ADDRESS", bus),
            ("HYPRLAND_INSTANCE_SIGNATURE", "iitest".into()),
            ("RI_LEAF_LOG", s.leaves().into()),
            ("RI_CTL", root.join("ctl").into()),
            ("PATH", path.into()),
            // Every run in the process shares `home`: libpipewire reads HOME with getenv(3).
            ("HOME", s.home().into()),
        ],
        &["PULSE_SERVER", "PIPEWIRE_REMOTE"],
    );
    None
}

impl Sandbox {
    /// Starts the daemons and leaf stubs in the directories `sandbox` made.
    fn start(&self) {
        let s = self;
        let xdg = s.root.join("xdg");
        let bin = s.root.join("bin");
        let bus = std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap();
        // Orphans of a script run (`notify-send … & disown`) become this process's children, so
        // [`reap_group`] can wait for them (prctl(2) PR_SET_CHILD_SUBREAPER).
        // SAFETY: PR_SET_CHILD_SUBREAPER takes one integer argument and touches no memory.
        let r = unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1) };
        assert_eq!(r, 0, "PR_SET_CHILD_SUBREAPER");
        // SAFETY: atexit(3) takes a function with no arguments or result, as `stop_daemons` is.
        let r = unsafe { libc::atexit(stop_daemons) };
        assert_eq!(r, 0, "atexit");
        // Each daemon is waited for through a readiness signal it documents, not a deadline:
        // dbus-daemon prints its address once it listens (`--print-address`), the notification
        // server a line once it owns its name, and PipeWire / pipewire-pulse are handed their
        // listening sockets (systemd socket activation, as pipewire.socket and
        // pipewire-pulse.socket do), so a client that connects before its daemon is up waits in
        // the socket's backlog instead of failing.
        let mut bus_out = daemon(
            &[
                "dbus-daemon",
                "--session",
                "--nofork",
                "--print-address",
                &format!("--address={bus}"),
            ],
            &[],
            Stdio::piped(),
        )
        .unwrap();
        let mut addr = String::new();
        bus_out.read_line(&mut addr).unwrap();
        assert!(!addr.is_empty(), "dbus-daemon exited");
        let mut notify_out = daemon(
            &["python3", "-c", NOTIFY_SRV, s.notes().to_str().unwrap()],
            &[],
            Stdio::piped(),
        )
        .unwrap();
        let mut ready = String::new();
        notify_out.read_line(&mut ready).unwrap();
        assert_eq!(ready, "ready\n", "notification server exited");
        let listen = |p: PathBuf| UnixListener::bind(p).unwrap();
        daemon(
            &["pipewire"],
            &[
                listen(xdg.join("pipewire-0")),
                listen(xdg.join("pipewire-0-manager")),
            ],
            Stdio::null(),
        );
        daemon(&["wireplumber"], &[], Stdio::null());
        std::fs::create_dir_all(xdg.join("pulse")).unwrap();
        daemon(
            &["pipewire-pulse"],
            &[listen(xdg.join("pulse/native"))],
            Stdio::null(),
        );
        let mut events = Command::new("pactl")
            .arg("subscribe")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut events_out = BufReader::new(events.stdout.take().unwrap()).lines();
        pactl(&["load-module", "module-null-sink", "sink_name=ri_sink"]).unwrap();
        pactl(&[
            "load-module",
            "module-null-sink",
            "sink_name=ri_mic",
            "media.class=Audio/Source/Virtual",
        ])
        .unwrap();
        // The defaults stick once WirePlumber runs its default-nodes policy, which changes the
        // server's defaults itself and so emits a server event: retry on each event, not on a
        // clock.
        loop {
            let set = pactl(&["set-default-sink", "ri_sink"]).is_some()
                & pactl(&["set-default-source", "ri_mic"]).is_some();
            let src = pactl(&["info"]).unwrap_or_default();
            if set
                && pactl(&["get-default-sink"]).as_deref() == Some("ri_sink")
                && src.contains("Default Source: ri_mic")
            {
                break;
            }
            events_out.next().expect("pactl subscribe ended").unwrap();
        }
        let _ = events.kill();
        let _ = events.wait();
        // Hyprland's request socket for the port; `hyprctl` for the script.
        let hd = xdg.join("hypr/iitest");
        std::fs::create_dir_all(&hd).unwrap();
        let l = std::os::unix::net::UnixListener::bind(hd.join(".socket.sock")).unwrap();
        // Hyprland writes its lock once its sockets listen.
        std::fs::write(hd.join("hyprland.lock"), "1\nwayland-1\n").unwrap();
        std::thread::spawn(move || {
            for mut c in l.incoming().flatten() {
                let mut b = [0u8; 256];
                let n = c.read(&mut b).unwrap_or(0);
                let req = String::from_utf8_lossy(&b[..n]).into_owned();
                let _ = c.write_all(if req == "j/monitors" {
                    MONITORS.as_bytes()
                } else {
                    b"unknown request"
                });
            }
        });
        testenv::install_script(
            &bin.join("hyprctl"),
            &format!(
                "#!/usr/bin/env bash\n[[ \"$*\" == 'monitors -j' ]] && printf '%s' '{MONITORS}'\n"
            ),
        );
        // songrec: prints $RI_CTL/songrec.out, then exits, or (mode `hang`) runs until a TERM,
        // which it logs. A TERM racing a plain exit is not logged, so both sides log the same.
        stub(
            &bin,
            "songrec",
            r#"hang=; [[ $(cat "$RI_CTL/songrec.mode") == hang ]] && hang=1
[[ -n $hang ]] && trap '[[ -z $! ]] || kill "$!"; echo "songrec TERM" >> "$RI_LEAF_LOG"; exit 143' TERM
cat "$RI_CTL/songrec.out"
# A trapped signal ends `wait` at once (bash(1), SIGNALS): it blocks until the TERM.
[[ -n $hang ]] && { sleep infinity & wait "$!"; }
exit 0"#,
        );
        stub(
            &bin,
            "wf-recorder",
            r#"[[ $1 == --already-running ]] && { echo; read -r _; exit 0; }
f=; while [[ $# -gt 0 ]]; do [[ $1 == -f ]] && f=$2; shift; done
[[ -n $f ]] && echo recorded > "$f"
exit "$(cat "$RI_CTL/wf.exit")""#,
        );
        stub(
            &bin,
            "slurp",
            r#"[[ -s $RI_CTL/slurp.region ]] || { echo "selection cancelled" >&2; exit 1; }
cat "$RI_CTL/slurp.region"; echo "slurp note" >&2"#,
        );
    }
}

#[derive(Debug, PartialEq)]
struct Outcome {
    code: i32,
    out: String,
    leaves: Vec<String>,
    notes: Vec<String>,
    files: Vec<String>,
    stopped_recorder: Option<Option<i32>>,
}

fn norm(s: &str, roots: &[(&Path, &str)]) -> String {
    let mut s = s.to_string();
    for (p, r) in roots {
        s = s.replace(p.to_str().unwrap(), r);
    }
    regex_lite(&s)
}

/// `recording_<date>` → `recording_@DATE@` (the two runs start seconds apart).
fn regex_lite(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("recording_") {
        out.push_str(&rest[..i + "recording_".len()]);
        rest = &rest[i + "recording_".len()..];
        let stamp: String = rest.chars().take(19).collect();
        let shape = stamp.chars().enumerate().all(|(k, c)| match k {
            4 | 7 => c == '-',
            10 => c == '_',
            13 | 16 => c == '.',
            _ => c.is_ascii_digit(),
        });
        if stamp.len() == 19 && shape {
            out.push_str("@DATE@");
            rest = &rest[19..];
        }
    }
    out.push_str(rest);
    out
}

fn tree(d: &Path) -> Vec<(String, String)> {
    let mut v = Vec::new();
    let mut stack = vec![d.to_path_buf()];
    while let Some(p) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&p) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            let rel = p.strip_prefix(d).unwrap().to_string_lossy().into_owned();
            // libpulse's client cookie, written by the script's `pactl`; the port asks PipeWire.
            if rel == ".config/pulse" {
                continue;
            }
            if p.is_dir() {
                v.push((format!("{rel}/"), String::new()));
                stack.push(p);
            } else {
                v.push((
                    rel,
                    String::from_utf8_lossy(&std::fs::read(&p).unwrap()).into_owned(),
                ));
            }
        }
    }
    // `.config` itself, when the cookie was all it held.
    if !v
        .iter()
        .any(|(n, _)| n.starts_with(".config/") && n != ".config/")
    {
        v.retain(|(n, _)| n != ".config/");
    }
    v.sort();
    v
}

/// One run of `script args` in `home` (also the cwd): as bash (`native == false`) or as the
/// native job behind a QML `Process`, which also drives the `Pipewire` singleton.
/// What runs the script's work: the original's script, the shell's QML (`ScreenRecorder`,
/// `MusicRecognizer`), or the shell binary run in the script's place (`record::serve`, what
/// the tree's `scripts/videos/record.sh` starts for a Hyprland keybind).
#[derive(Clone, Copy, PartialEq)]
enum Side {
    Script,
    Qml,
    Program,
}

fn run(
    s: &Sandbox,
    script: &str,
    args: &[&str],
    home: &Path,
    native: bool,
    recorder: bool,
) -> Outcome {
    let side = if native { Side::Qml } else { Side::Script };
    run_side(s, script, args, home, side, recorder).0
}

/// The run's outcome as the original sees it, and the exit status of what ran.
fn run_side(
    s: &Sandbox,
    script: &str,
    args: &[&str],
    home: &Path,
    side: Side,
    recorder: bool,
) -> (Outcome, i32) {
    let _ = std::fs::remove_dir_all(home);
    std::fs::create_dir_all(home).unwrap();
    let _ = std::fs::remove_file(s.leaves());
    std::fs::write(s.notes(), "").unwrap();
    assert_eq!(
        std::env::var_os("HOME").as_deref(),
        Some(home.as_os_str()),
        "the sandbox's HOME"
    );
    std::env::set_current_dir(home).unwrap();
    std::fs::write(home.join("seed"), "").unwrap();
    let seed = s.ctl("home-seed");
    if let Ok(cfg) = std::fs::read_to_string(&seed)
        && !cfg.is_empty()
    {
        let c = home.join(".config/impulse-shell");
        std::fs::create_dir_all(&c).unwrap();
        std::fs::write(
            c.join("config.json"),
            cfg.replace("@HOME@", home.to_str().unwrap()),
        )
        .unwrap();
    }
    let rec = recorder.then(|| {
        let mut r = Command::new(s.root.join("bin/wf-recorder"))
            .args(["--already-running"])
            .env("RI_LEAF_LOG", s.root.join("pre.log"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        // spawn returns when the exec closes std's CLOEXEC pipe, which the kernel does before it
        // renames the process: `pgrep wf-recorder` is sure to see it only once its script runs.
        let mut ready = String::new();
        BufReader::new(r.stdout.take().unwrap())
            .read_line(&mut ready)
            .unwrap();
        assert_eq!(ready, "\n", "the running recorder did not start");
        r
    });
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/original-unshipped")
        .join(script);
    let argv: Vec<String> = std::iter::once(path.to_string_lossy().into_owned())
        .chain(args.iter().map(|a| a.to_string()))
        .collect();
    let songrec = script == SONGREC;
    let (code, out) = if side == Side::Program {
        // The sandbox's sink is the default one by now, as in a session that runs.
        qml::pw::wait_snapshot_until(|s| s.defaults[0] == "ri_sink");
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        (qml::ports::record::serve(&args), String::new())
    } else if side == Side::Qml {
        let tree = s.root.join("shell");
        let (root_dir, f, body) = if songrec {
            let arg = |k: &str| {
                let i = args.iter().position(|a| *a == k).unwrap();
                args[i + 1].to_string()
            };
            let body = format!(
                r#"onGoChanged: MusicRecognizer.start({}, {}, {})
    Connections {{
        target: MusicRecognizer
        function onFinished(text, exitCode) {{ root.out = text; root.done = true; root.code = exitCode }}
    }}"#,
                arg("-i"),
                arg("-t"),
                serde_json::to_string(&arg("-s")).unwrap(),
            );
            (tree.clone(), tree.join("services/SongRec.qml"), body)
        } else {
            // The original's launches of the script are the calls its QML makes now. The region selector's
            // two (`--region R` without sound, `--region R --sound` with it) are the two it
            // names; the script records sound in both (its `SOUND_FLAG` starts at 1).
            let region = |r: &str| serde_json::to_string(r).unwrap();
            // The save directory is the caller's to give (`Config.options.screenRecord
            // .savePath`): what the config file says, as the script reads it.
            let saved = std::fs::read_to_string(home.join(".config/impulse-shell/config.json"))
                .ok()
                .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
                .and_then(|c| Some(c.get("screenRecord")?.get("savePath")?.as_str()?.to_owned()))
                .unwrap_or_default();
            let saved = serde_json::to_string(&saved).unwrap();
            let call = match args {
                [] => format!("toggle({saved})"),
                ["--fullscreen", "--sound"] => format!("toggleFullscreen({saved})"),
                ["--region", r] => format!("toggleRegion({}, false, {saved})", region(r)),
                ["--region", r, "--sound"] => {
                    format!("toggleRegion({}, true, {saved})", region(r))
                }
                _ => panic!("{args:?} is not one of the original's record.sh launches"),
            };
            let body = format!(
                "onGoChanged: {{ ScreenRecorder.{call}; root.code = 0; root.done = true }}"
            );
            (ri(), home.join("Ab.qml"), body)
        };
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(
            &f,
            format!(
                r#"import QtQuick
import Rustshell
import Rustshell.Io
import Rustshell.Services.Pipewire
import Impulse
Item {{
    id: root
    property int code: -1
    property string out: ""
    property bool done: false
    property bool go: false
    property string sink: Pipewire.defaultAudioSink?.name ?? ""
    PwObjectTracker {{ objects: [Pipewire.defaultAudioSink, Pipewire.defaultAudioSource] }}
    {body}
}}
"#
            ),
        )
        .unwrap();
        let mut rt = Runtime::new(Some(root_dir));
        let (ping, woke) = std::sync::mpsc::channel();
        rt.world.borrow().waker.set_ping(move || {
            let _ = ping.send(());
        });
        let root = rt.load_file(&f).unwrap();
        std::fs::remove_file(&f).unwrap();
        rt.settle();
        // The PipeWire service starts on a loop turn and wakes the runtime with each snapshot
        // it publishes.
        rt.poll_native();
        while qml::pw::snapshot()
            .is_none_or(|s| s.defaults[0] != "ri_sink" || s.defaults[1] != "ri_mic")
        {
            woke.recv().expect("runtime waker dropped");
            rt.poll_native();
        }
        rt.settle();
        rt.world
            .borrow_mut()
            .write(root, "go", qml::value::Value::Bool(true));
        rt.settle();
        let read = |rt: &Runtime, k: &str| rt.world.borrow_mut().read(root, k).unwrap();
        // The job's output and exit reach the runtime through its waker: run a loop turn per
        // ping, as the shell's loop does.
        while read(&rt, "code").to_number() == -1.0 || !read(&rt, "done").truthy() {
            woke.recv().expect("runtime waker dropped");
            rt.poll_native();
            rt.settle();
        }
        (
            read(&rt, "code").to_number() as i32,
            read(&rt, "out").to_js_string(),
        )
    } else {
        // Its own process group: what it leaves running in the background keeps that group.
        let o = Command::new("bash")
            .args(&argv)
            .process_group(0)
            .current_dir(home)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let pgid = o.id();
        let o = o.wait_with_output().unwrap();
        // Notifications are sent in the background (`notify-send … & disown`): the script's
        // orphans, reparented here, are waited for by its process group.
        reap_group(pgid);
        (
            o.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&o.stdout).into_owned(),
        )
    };
    // The native jobs (the port's notifications are pool jobs) finish too.
    workpool::wait_idle();
    // The run is over: whatever was to stop the already-running recorder has signalled it. A
    // fatal signal leaves it no user code to run, so closing its stdin now ends it with exit 0
    // only if it was not stopped; its termination signal is the run's.
    let stopped_recorder = rec.map(|mut r| {
        drop(r.stdin.take());
        r.wait().unwrap().signal()
    });
    let status = code;
    // The original launches record.sh detached and never sees its exit status.
    let code = if songrec { code } else { 0 };
    let roots = [(home, "@HOME@"), (s.root.as_path(), "@SANDBOX@")];
    let lines = |p: &Path| -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_to_string(p)
            .unwrap_or_default()
            .lines()
            .map(|l| norm(l, &roots))
            .collect();
        v.sort();
        v
    };
    let outcome = Outcome {
        code,
        out: norm(&out, &roots),
        leaves: std::fs::read_to_string(s.leaves())
            .unwrap_or_default()
            .lines()
            .map(|l| norm(l, &roots))
            .collect(),
        notes: lines(&s.notes()),
        files: tree(home)
            .into_iter()
            .map(|(n, c)| norm(&format!("{n}={c}"), &roots))
            .filter(|l| !l.starts_with("Ab.qml"))
            .collect(),
        stopped_recorder,
    };
    (outcome, status)
}

fn ab(s: &Sandbox, tag: &str, script: &str, args: &[&str], recorder: bool) -> Outcome {
    let home = s.home();
    let want = run(s, script, args, &home, false, recorder);
    let got = run(s, script, args, &home, true, recorder);
    assert_eq!(got, want, "{tag}: native port vs {script}");
    std::env::set_current_dir(env!("CARGO_MANIFEST_DIR")).unwrap();
    want
}

const SONGREC: &str = "scripts/musicRecognition/recognize-music.sh";
const RECORD: &str = "scripts/videos/record.sh";

/// The region selector's "record" and "record with sound" (ScreenshotAction.qml): only the
/// second records the default sink's monitor. The original's script recorded it in both, whatever the
/// action was called.
#[test]
fn region_recording_has_sound_only_when_asked_for() {
    let Some(s) = sandbox("region_recording_has_sound_only_when_asked_for") else {
        return;
    };
    std::fs::write(s.ctl("home-seed"), "").unwrap();
    std::fs::write(s.ctl("slurp.region"), "").unwrap();
    std::fs::write(s.ctl("wf.exit"), "0").unwrap();
    let recorded = |tag: &str, args: &[&str]| {
        let o = run(&s, RECORD, args, &s.home(), true, false);
        std::env::set_current_dir(env!("CARGO_MANIFEST_DIR")).unwrap();
        let wf: Vec<String> = o
            .leaves
            .into_iter()
            .filter(|l| l.starts_with("wf-recorder\t"))
            .collect();
        assert_eq!(wf.len(), 1, "{tag}: {wf:?}");
        assert!(wf[0].contains("\t--geometry\t5,6 70x80"), "{tag}: {wf:?}");
        wf[0].contains("--audio")
    };
    assert!(
        !recorded("silent", &["--region", "5,6 70x80"]),
        "a recording without sound records sound"
    );
    assert!(
        recorded("sound", &["--region", "5,6 70x80", "--sound"]),
        "a recording with sound records none"
    );
}

#[test]
fn recognize_music_script_matches_native() {
    let Some(s) = sandbox("recognize_music_script_matches_native") else {
        return;
    };
    std::fs::write(s.ctl("home-seed"), "").unwrap();
    let set = |out: &str, mode: &str| {
        std::fs::write(s.ctl("songrec.out"), out).unwrap();
        std::fs::write(s.ctl("songrec.mode"), mode).unwrap();
    };
    // SongRec.qml always passes `-i timeoutInterval -t timeoutDuration -s monitorSourceString`.
    let cases: &[(&str, &[&str], String, &str)] = &[
        (
            "match",
            &["-i", "2", "-t", "30", "-s", "monitor"],
            format!("{{\"no\": 1}}\nnot json\n{MATCH}\n{{\"after\": true}}\n"),
            "hang",
        ),
        (
            "input source",
            &["-i", "5", "-t", "10", "-s", "input"],
            format!("{MATCH}\n"),
            "exit",
        ),
        (
            "monitor flags",
            &["-i", "1", "-t", "20", "-s", "monitor"],
            format!("{MATCH}\n"),
            "exit",
        ),
        (
            "no match then eof",
            &["-i", "2", "-t", "30", "-s", "monitor"],
            "{\"matches\":[]}\n{\"matches\": []}x\n".into(),
            "exit",
        ),
        (
            "timeout",
            &["-i", "2", "-t", "1", "-s", "monitor"],
            "listening\n".into(),
            "hang",
        ),
        (
            "invalid source",
            &["-i", "2", "-t", "30", "-s", "bogus"],
            format!("{MATCH}\n"),
            "exit",
        ),
    ];
    for (tag, args, out, mode) in cases {
        set(out, mode);
        let o = ab(&s, tag, SONGREC, args, false);
        if *tag == "match" {
            assert_eq!(o.out, format!("{MATCH}\n"));
        }
    }
    // Without songrec on PATH both end with 1 before touching anything.
    let bin = s.root.join("bin");
    std::fs::rename(bin.join("songrec"), s.root.join("songrec.off")).unwrap();
    let o = ab(
        &s,
        "no songrec",
        SONGREC,
        &["-i", "2", "-t", "30", "-s", "monitor"],
        false,
    );
    std::fs::rename(s.root.join("songrec.off"), bin.join("songrec")).unwrap();
    assert_eq!((o.code, o.leaves.len()), (1, 0));
}

#[test]
fn record_script_matches_native() {
    let Some(s) = sandbox("record_script_matches_native") else {
        return;
    };
    let cfg = |c: &str| std::fs::write(s.ctl("home-seed"), c).unwrap();
    let slurp = |r: &str| std::fs::write(s.ctl("slurp.region"), r).unwrap();
    let wf_exit = |c: &str| std::fs::write(s.ctl("wf.exit"), c).unwrap();
    let saved = r#"{"screenRecord": {"savePath": "@HOME@/Caps tures"}}"#;
    // The script's two launches: no arguments (`ScreenRecorder.toggle()`: UtilButtons,
    // RegionSelection) and `--fullscreen --sound` (`toggleFullscreen()`: Recorder.qml).
    let fs: &[&str] = &["--fullscreen", "--sound"];
    /// (tag, config, script args, slurp region, wf-recorder exit, recorder launched).
    type Case<'a> = (&'a str, &'a str, &'a [&'a str], &'a str, &'a str, bool);
    let cases: &[Case] = &[
        ("fullscreen sound", saved, fs, "", "0", false),
        ("slurp", saved, &[], "5,6 70x80", "0", false),
        ("slurp cancelled", saved, &[], "", "0", false),
        ("recorder fails", saved, fs, "", "3", false),
        ("stop running", saved, fs, "", "0", true),
        ("stop running from bar", saved, &[], "5,6 70x80", "0", true),
        (
            "empty save path",
            r#"{"screenRecord": {"savePath": ""}}"#,
            fs,
            "",
            "0",
            false,
        ),
        ("no config", "", fs, "", "0", false),
    ];
    for (tag, c, args, region, exit, recorder) in cases {
        cfg(c);
        slurp(region);
        wf_exit(exit);
        let o = ab(&s, tag, RECORD, args, *recorder);
        // Both record the default sink's monitor (the port gives no `--audio` only when
        // there is no default sink).
        for l in o.leaves.iter().filter(|l| l.starts_with("wf-recorder\t")) {
            assert!(l.contains("\t--audio=ri_sink.monitor\t"), "{tag}: {l}");
        }
        if *recorder {
            assert!(
                o.stopped_recorder.is_some_and(|sig| sig.is_some()),
                "{tag}: the running recorder was not stopped: {:?}",
                o.stopped_recorder
            );
        }
    }
}

/// The shell binary run in the script's place (`impulse-shell __ri-record ARGS`, what the tree's
/// `scripts/videos/record.sh` runs for the original's Hyprland keybinds) against the original's script with the
/// same arguments: the keybinds' two launches (`--fullscreen`, `--fullscreen --sound`), the
/// ones for a region, and a second launch while a recording runs. Sound is recorded in all
/// of them, as the script records it, and the exit status is the script's.
#[test]
fn record_program_matches_script() {
    let Some(s) = sandbox("record_program_matches_script") else {
        return;
    };
    let cfg = |c: &str| std::fs::write(s.ctl("home-seed"), c).unwrap();
    let slurp = |r: &str| std::fs::write(s.ctl("slurp.region"), r).unwrap();
    let wf_exit = |c: &str| std::fs::write(s.ctl("wf.exit"), c).unwrap();
    let saved = r#"{"screenRecord": {"savePath": "@HOME@/Caps tures"}}"#;
    let region = "5,6 70x80";
    /// (tag, config, script args, slurp region, wf-recorder exit, recorder launched).
    type Case<'a> = (&'a str, &'a str, &'a [&'a str], &'a str, &'a str, bool);
    let cases: &[Case] = &[
        ("Ctrl+Alt+R", saved, &["--fullscreen"], "", "0", false),
        (
            "Super+Shift+Alt+R",
            saved,
            &["--fullscreen", "--sound"],
            "",
            "0",
            false,
        ),
        ("Ctrl+Alt+R again", saved, &["--fullscreen"], "", "0", true),
        ("no config", "", &["--fullscreen"], "", "0", false),
        (
            "empty save path",
            r#"{"screenRecord": {"savePath": ""}}"#,
            &["--fullscreen"],
            "",
            "0",
            false,
        ),
        ("recorder fails", saved, &["--fullscreen"], "", "3", false),
        ("slurp", saved, &[], region, "0", false),
        ("slurp cancelled", saved, &[], "", "0", false),
        ("slurp while recording", saved, &[], region, "0", true),
        ("region", saved, &["--region", region], "", "0", false),
        (
            "region sound",
            saved,
            &["--region", region, "--sound"],
            "",
            "0",
            false,
        ),
        (
            "region and fullscreen",
            saved,
            &["--region", region, "--fullscreen"],
            "",
            "0",
            false,
        ),
        ("region not given", saved, &["--region"], "", "0", false),
        (
            "region not given while recording",
            saved,
            &["--region"],
            "",
            "0",
            true,
        ),
    ];
    let home = s.home();
    for (tag, c, args, r, exit, recorder) in cases {
        cfg(c);
        slurp(r);
        wf_exit(exit);
        let want = run_side(&s, RECORD, args, &home, Side::Script, *recorder);
        let got = run_side(&s, RECORD, args, &home, Side::Program, *recorder);
        assert_eq!(
            got,
            want,
            "{tag}: impulse-shell {} vs {RECORD}",
            qml::ports::record::ARG
        );
        std::env::set_current_dir(env!("CARGO_MANIFEST_DIR")).unwrap();
        let wf: Vec<&String> = want
            .0
            .leaves
            .iter()
            .filter(|l| l.starts_with("wf-recorder\t"))
            .collect();
        let started = !*recorder && *tag != "slurp cancelled" && *tag != "region not given";
        assert_eq!(wf.len(), usize::from(started), "{tag}: {wf:?}");
        for l in wf {
            assert!(l.contains("\t--audio=ri_sink.monitor\t"), "{tag}: {l}");
            assert_eq!(
                l.contains("\t-o\tHDMI-A-1\t"),
                args.contains(&"--fullscreen"),
                "{tag}: {l}"
            );
        }
    }
}
