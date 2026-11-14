//! A/B harness: the same fixture runs in a child test process once on its reference side
//! (`RI_AB_REFERENCE=1`: the fixture runs real bash and real tools) and once on the side under
//! test (the `Impulse` types); the fixture's own state and the outside effects (notifications,
//! files, clipboard, ...) must match.

use crate::host::Host;
use qml::runtime::Runtime;
use qml::value::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

pub fn ri() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../qml"))
}

/// The original's scripts and the QML the shell no longer loads, kept for the comparisons (see its README).
pub fn unshipped() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/original-unshipped"
    ))
}

pub fn require(tools: &[&str]) -> bool {
    tools.iter().all(|t| testenv::require(t))
}

static N: AtomicU32 = AtomicU32::new(0);

/// wl-copy's body in [`Scene::trace`]: wl-copy forks a server that takes the selection after
/// the command exits, so the traced one returns only once a selection is set, which
/// `wl-paste --watch` reports with `CLIPBOARD_STATE` (once for the selection at its start,
/// then once per change; a `nil` is a clear still arriving from before, e.g. a side's exit).
const WL_COPY_SET: &str = r#"p=; for a in "$@"; do case $a in -p|--primary) p=--primary;; esac; done
coproc W { exec wl-paste $p --watch sh -c 'echo "$CLIPBOARD_STATE"' 2>/dev/null; }
read -r _ <&"${W[0]}"
'@REAL@' "$@"; st=$?
if [ "$st" = 0 ]; then
    while read -r s <&"${W[0]}" && [ "$s" = nil ]; do :; done
fi
kill "$W_PID"
exit "$st""#;

pub struct Scene {
    pub dir: PathBuf,
    pub root: PathBuf,
    pub bin: PathBuf,
    pub env: Vec<(String, String)>,
    /// The binary each side runs instead of this one (a link to it, see `probe::SelfExe`),
    /// removed as soon as the side's process has exec'd it.
    pub exe: Option<PathBuf>,
}

impl Scene {
    pub fn new(tag: &str) -> Scene {
        let dir = std::env::temp_dir().join(format!(
            "ri-ab-{}-{}-{tag}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let root = dir.join("root");
        let bin = dir.join("bin");
        let home = dir.join("home");
        for d in [
            root.join("services"),
            root.join("modules/common"),
            bin.clone(),
        ] {
            std::fs::create_dir_all(d).unwrap();
        }
        for d in [".config", ".cache", ".local/state", "Pictures"] {
            std::fs::create_dir_all(home.join(d)).unwrap();
        }
        std::os::unix::fs::symlink(
            ri().join("modules/common/functions"),
            root.join("modules/common/functions"),
        )
        .unwrap();
        let path = format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let env = vec![
            ("HOME".into(), home.display().to_string()),
            (
                "XDG_CONFIG_HOME".into(),
                home.join(".config").display().to_string(),
            ),
            (
                "XDG_CACHE_HOME".into(),
                home.join(".cache").display().to_string(),
            ),
            (
                "XDG_STATE_HOME".into(),
                home.join(".local/state").display().to_string(),
            ),
            ("PATH".into(), path),
        ];
        Scene {
            dir,
            root,
            bin,
            env,
            exe: None,
        }
    }

    pub fn set(&mut self, k: &str, v: &str) {
        self.env.retain(|(n, _)| n != k);
        self.env.push((k.into(), v.into()));
    }

    pub fn write(&self, rel: &str, text: &str) -> PathBuf {
        let p = self.root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, text).unwrap();
        p
    }

    pub fn service(&self, name: &str, body: &str) {
        self.write(
            &format!("services/{name}.qml"),
            &format!("pragma Singleton\nimport QtQuick\nimport Quickshell\n\n{body}\n"),
        );
    }

    pub fn main(&self, text: &str) {
        self.write("Main.qml", text);
    }

    /// An executable shell script; written by `cp` so no fd of ours is open on it while a
    /// sibling test thread forks (ETXTBSY).
    pub fn script(&self, path: &Path, body: &str) -> PathBuf {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let tmp = path.with_extension("ab-tmp");
        std::fs::write(&tmp, format!("#!/bin/bash\n{body}\n")).unwrap();
        assert!(
            Command::new("cp")
                .arg(&tmp)
                .arg(path)
                .status()
                .unwrap()
                .success()
        );
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let _ = std::fs::remove_file(tmp);
        path.to_path_buf()
    }

    pub fn stub(&self, name: &str, body: &str) -> PathBuf {
        self.script(&self.bin.join(name), body)
    }

    /// A stub that appends its argv (one line, NUL-free) to `<dir>/<name>.log` and runs `rest`.
    pub fn logger(&self, name: &str, rest: &str) -> PathBuf {
        let log = self.dir.join(format!("{name}.log"));
        self.stub(
            name,
            &format!(
                "printf '%s|' \"$@\" >> '{}'; echo >> '{}'\n{rest}",
                log.display(),
                log.display()
            ),
        )
    }

    /// Wraps the real `tools` in `bin/` so each run of them is noted in `trace.log` (kept out of
    /// the comparison: it shows which side ran the original pipeline).
    pub fn trace(&self, tools: &[&str]) {
        for t in tools {
            let o = Command::new("bash")
                .args(["-c", &format!("command -v {t}")])
                .output()
                .unwrap();
            let real = String::from_utf8_lossy(&o.stdout).trim().to_string();
            assert!(!real.is_empty(), "{t} not found");
            let log = self.dir.join("trace.log");
            let note = format!("printf '%s\\n' \"{t} $*\" >> '{}'", log.display());
            if *t == "wl-copy" {
                self.stub(
                    t,
                    &format!("{note}\n{WL_COPY_SET}").replace("@REAL@", &real),
                );
            } else {
                self.stub(t, &format!("{note}\nexec '{real}' \"$@\""));
            }
        }
    }

    pub fn take_trace(&self) -> String {
        let t = self.read("trace.log");
        self.rm("trace.log");
        t
    }

    pub fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.dir.join(rel)).unwrap_or_default()
    }

    pub fn rm(&self, rel: &str) {
        let _ = std::fs::remove_file(self.dir.join(rel));
    }
}

impl Drop for Scene {
    fn drop(&mut self) {
        if std::env::var_os("RI_AB_KEEP").is_none() && !std::thread::panicking() {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// Adopted children (see `child_main`) that end by themselves: everything but what serves
/// until replaced or killed, wl-copy's selection server and the daemons scenes start
/// (`testenv::install_daemon`, under a `.daemons` directory).
fn adopted_scripts() -> Vec<i32> {
    let me = std::process::id() as i32;
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<i32>().ok())
        .filter(|&pid| {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
            let rest = stat.rsplit(')').next().unwrap_or_default();
            let mut f = rest.split_whitespace();
            let (state, ppid) = (f.next(), f.next().and_then(|p| p.parse::<i32>().ok()));
            ppid == Some(me)
                && state != Some("Z")
                && std::fs::read_link(format!("/proc/{pid}/exe")).is_ok_and(|x| {
                    !x.file_name().is_some_and(|n| n == "wl-copy")
                        && !x.components().any(|c| c.as_os_str() == ".daemons")
                })
        })
        .collect()
}

/// `Host::idle`, and every adopted script has ended (and been reaped), repeated until both
/// hold at once.
fn quiesce(lp: &mut Host, rt: &mut Runtime) {
    loop {
        lp.idle(rt);
        let left = adopted_scripts();
        if left.is_empty() {
            return;
        }
        for pid in left {
            testenv::wait_gone(pid);
            let mut st = 0;
            // SAFETY: waitpid(2) on an exited child of ours writes only `st`.
            unsafe { libc::waitpid(pid, &mut st, libc::WNOHANG) };
        }
    }
}

/// The child side (`ab_child`): load `$RI_AB_ROOT/Main.qml`, call `go(input, step)` until it
/// returns a falsy value (a number is the ms of the loop's virtual time the step runs, `true`
/// 500), each step until everything it started has ended (`Host::idle`), print `AB-READY`,
/// then once stdin closes print `state()`.
pub fn child_main() {
    let Ok(root) = std::env::var("RI_AB_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let input = std::env::var("RI_AB_INPUT").unwrap_or_default();
    // Commands a side's script leaves behind when it exits (`cmd | tee >(wl-copy)`) are
    // adopted here instead of by init, so `quiesce` can wait for them.
    // SAFETY: prctl(2) PR_SET_CHILD_SUBREAPER takes plain integers.
    assert_eq!(unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1) }, 0);
    let mut rt = Runtime::new(Some(root.clone()));
    let mut lp = Host::new(&mut rt);
    rt.load_singletons_lazy(&[root.join("services"), root.join("modules/common/functions")]);
    let r = rt
        .load_file(&root.join("Main.qml"))
        .unwrap_or_else(|e| panic!("Main.qml: {e}"));
    lp.idle(&mut rt);
    let mut step = 0.0;
    loop {
        let more = rt.call_method(r, "go", vec![Value::str(&input), Value::Num(step)]);
        let wait = match more {
            Some(Ok(Value::Num(ms))) if ms > 0.0 => ms as u64,
            Some(Ok(v)) if v.truthy() => 500,
            Some(Err(e)) => panic!("go({input:?}, {step}): {e}"),
            None => panic!("Main.qml has no go()"),
            _ => 0,
        };
        lp.advance(&mut rt, wait as f64);
        quiesce(&mut lp, &mut rt);
        // The on-screen keyboard's keys and Hyprland requests go out on threads of their own.
        qml::ports::vkbd::drain();
        qml::hypr::drain();
        quiesce(&mut lp, &mut rt);
        if wait == 0 {
            break;
        }
        step += 1.0;
    }
    println!("AB-READY");
    std::io::stdout().flush().unwrap();
    // The parent has read the effects of the steps; nothing is left running here.
    let _ = std::io::stdin().read_to_end(&mut Vec::new());
    lp.idle(&mut rt);
    let state = match rt.call_method(r, "state", vec![]) {
        Some(Ok(v)) => v.to_js_string(),
        Some(Err(e)) => format!("state() threw {e}"),
        None => String::new(),
    };
    let errors: Vec<String> = rt
        .errors()
        .into_iter()
        .map(|e| e.replace(&root.display().to_string(), "<root>"))
        .collect();
    let out = serde_json::json!({ "state": state, "errors": errors });
    println!("AB-STATE {out}");
    std::io::stdout().flush().unwrap();
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// One run of the fixture, on its reference side or on the native one; `(fixture state + QML
/// errors, effects)`.
pub fn side(
    sc: &Scene,
    reference: bool,
    input: &str,
    effects: &mut dyn FnMut() -> String,
) -> (String, String) {
    let tag = if reference { "reference" } else { "native" };
    let log = std::fs::File::create(sc.dir.join(format!("child-{tag}.log"))).unwrap();
    let exe = sc
        .exe
        .clone()
        .unwrap_or_else(|| std::env::current_exe().unwrap());
    let mut c = Command::new(exe);
    c.args([
        "ab_child",
        "--exact",
        "--ignored",
        "--nocapture",
        "--test-threads=1",
        "-q",
    ])
    .env("RI_AB_ROOT", &sc.root)
    .env("RI_AB_INPUT", input)
    .envs(sc.env.iter().map(|(k, v)| (k, v)))
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(log);
    if reference {
        c.env("RI_AB_REFERENCE", "1");
    } else {
        c.env_remove("RI_AB_REFERENCE");
    }
    let mut child = Kill(c.spawn().unwrap());
    // Spawned means exec'd: the side runs the link, which may go now.
    if let Some(link) = &sc.exe {
        std::fs::remove_file(link).unwrap();
    }
    let out = child.0.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for l in BufReader::new(out).lines().map_while(Result::ok) {
            if tx.send(l).is_err() {
                break;
            }
        }
    });
    // Blocks until the child prints the line, or fails once it exited without it.
    let wait_for = |prefix: &str| -> String {
        while let Ok(l) = rx.recv() {
            if let Some(rest) = l.strip_prefix(prefix) {
                return rest.trim().to_string();
            }
        }
        panic!(
            "{tag} side of {input:?}: no {prefix}; child log:\n{}",
            std::fs::read_to_string(sc.dir.join(format!("child-{tag}.log"))).unwrap_or_default()
        );
    };
    // The child is ready once everything its steps started has ended, so the effects are
    // complete; read again after it exited (its exit may end what it served, see `shot::Clip`).
    wait_for("AB-READY");
    effects();
    drop(child.0.stdin.take());
    let state = wait_for("AB-STATE ");
    testenv::wait_exit(&mut child.0);
    let fx = effects();
    (state, fx)
}

/// Runs every input on the reference side and on the native side and asserts equal outcomes;
/// `reset(input)` restores the outside world before each side.
pub fn ab(
    sc: &Scene,
    inputs: &[&str],
    reset: &mut dyn FnMut(&str),
    effects: &mut dyn FnMut() -> String,
) -> Vec<Run> {
    ab_with(sc, inputs, reset, effects, &|s| s.to_string())
}

/// [`ab`] where the native side must give `intended(reference state)`: the result the original means to
/// give where the reference has a flaw the native type does not have.
pub fn ab_with(
    sc: &Scene,
    inputs: &[&str],
    reset: &mut dyn FnMut(&str),
    effects: &mut dyn FnMut() -> String,
    intended: &dyn Fn(&str) -> String,
) -> Vec<Run> {
    ab_with_fx(sc, inputs, reset, effects, intended, &|_, fx| {
        fx.to_string()
    })
}

/// [`ab_with`] where the native side's effects must also be `intended_fx(input, reference
/// effects)`.
pub fn ab_with_fx(
    sc: &Scene,
    inputs: &[&str],
    reset: &mut dyn FnMut(&str),
    effects: &mut dyn FnMut() -> String,
    intended: &dyn Fn(&str) -> String,
    intended_fx: &dyn Fn(&str, &str) -> String,
) -> Vec<Run> {
    let mut all = Vec::new();
    for input in inputs {
        reset(input);
        sc.take_trace();
        let reference = side(sc, true, input, effects);
        let reference_trace = sc.take_trace();
        reset(input);
        let native = side(sc, false, input, effects);
        let native_trace = sc.take_trace();
        let want = (intended(&reference.0), intended_fx(input, &reference.1));
        assert_eq!(
            native, want,
            "the native side differs from the intended reference for {input:?} (left: native, right: reference)"
        );
        all.push(Run {
            input: input.to_string(),
            state: reference.0,
            effects: reference.1,
            reference_trace,
            native_trace,
        });
    }
    all
}

#[derive(Debug)]
pub struct Run {
    pub input: String,
    pub state: String,
    pub effects: String,
    pub reference_trace: String,
    pub native_trace: String,
}

impl Run {
    /// The reference side ran `tool` and the native side did not: the `Impulse` type does the
    /// work itself.
    pub fn only_reference_ran(&self, tool: &str) -> bool {
        let has = |t: &str| t.lines().any(|l| l.split(' ').next() == Some(tool));
        has(&self.reference_trace) && !has(&self.native_trace)
    }
}

/// A private session bus with a recording `org.freedesktop.Notifications` server. Only what
/// the user sees is recorded (`Notify`, `CloseNotification`), not notify-send's capability
/// queries (it asks `GetServerInformation` twice, the native side once).
pub struct Notes {
    pub log: Arc<Mutex<Vec<String>>>,
    next: Arc<AtomicU32>,
    _srv: zbus::blocking::Connection,
    _bus: Kill,
}

struct NoteSrv {
    log: Arc<Mutex<Vec<String>>>,
    /// Index of the action to invoke on a notification with actions (`None`: dismiss it).
    invoke: Arc<Mutex<Option<usize>>>,
    next: Arc<AtomicU32>,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl NoteSrv {
    #[zbus(name = "GetServerInformation")]
    fn get_server_information(&self) -> (String, String, String, String) {
        ("ab".into(), "ri".into(), "1".into(), "1.2".into())
    }
    #[zbus(name = "GetCapabilities")]
    fn get_capabilities(&self) -> Vec<String> {
        vec!["actions".into(), "body".into()]
    }
    #[allow(clippy::too_many_arguments)]
    #[zbus(name = "Notify")]
    async fn notify(
        &mut self,
        #[zbus(signal_emitter)] em: zbus::object_server::SignalEmitter<'_>,
        app: String,
        replaces: u32,
        icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
        timeout: i32,
    ) -> zbus::object_server::ResponseDispatchNotifier<u32> {
        let mut h: Vec<String> = hints
            .iter()
            .map(|(k, v)| {
                if k == "sender-pid" {
                    format!("{k}:{}", v.value_signature())
                } else {
                    format!("{k}={v:?}")
                }
            })
            .collect();
        h.sort();
        self.log.lock().unwrap().push(format!(
            "Notify app={app:?} replaces={replaces} icon={icon:?} summary={summary:?} body={body:?} actions={actions:?} hints={h:?} timeout={timeout}"
        ));
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        // The action (or the dismissal) comes once the reply carrying the id is sent: the bus
        // delivers a sender's messages in order, so the client knows the id the signal names.
        let (reply, sent) = zbus::object_server::ResponseDispatchNotifier::new(id);
        if !actions.is_empty() {
            let pick = *self.invoke.lock().unwrap();
            let key = pick.and_then(|i| actions.get(2 * i).cloned());
            let em = em.to_owned();
            std::thread::spawn(move || {
                zbus::block_on(async {
                    sent.await;
                    if let Some(k) = key {
                        let _ = em
                            .emit("org.freedesktop.Notifications", "ActionInvoked", &(id, k))
                            .await;
                    }
                    let _ = em
                        .emit(
                            "org.freedesktop.Notifications",
                            "NotificationClosed",
                            &(id, 2u32),
                        )
                        .await;
                });
            });
        }
        reply
    }
    #[zbus(name = "CloseNotification")]
    fn close_notification(&self, id: u32) {
        self.log
            .lock()
            .unwrap()
            .push(format!("CloseNotification {id}"));
    }
}

impl Notes {
    pub fn start(sc: &mut Scene) -> Option<Notes> {
        if !require(&["dbus-daemon", "notify-send"]) {
            return None;
        }
        let mut d = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(d.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let addr = line.trim().to_string();
        let log = Arc::new(Mutex::new(Vec::new()));
        let next = Arc::new(AtomicU32::new(0));
        let srv = NoteSrv {
            log: log.clone(),
            invoke: Arc::new(Mutex::new(None)),
            next: next.clone(),
        };
        let conn = zbus::blocking::connection::Builder::address(addr.as_str())
            .unwrap()
            .name("org.freedesktop.Notifications")
            .unwrap()
            .serve_at("/org/freedesktop/Notifications", srv)
            .unwrap()
            .build()
            .unwrap();
        sc.set("DBUS_SESSION_BUS_ADDRESS", &addr);
        Some(Notes {
            log,
            next,
            _srv: conn,
            _bus: Kill(d),
        })
    }

    pub fn calls(&self) -> String {
        self.log.lock().unwrap().join("\n")
    }

    /// Forgets the calls and restarts the notification ids.
    pub fn clear(&self) {
        self.log.lock().unwrap().clear();
        self.next.store(0, Ordering::SeqCst);
    }
}
