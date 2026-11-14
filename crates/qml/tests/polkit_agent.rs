//! The PolkitAgent's AuthenticationAgent object against a fake polkitd + logind
//! (tests/fake_sysbus.py `polkitd`) on a private `dbus-daemon` used as the system bus. Each test
//! owns its daemon and runs in a process of its own (`testenv::run_in_own_process`) whose
//! `DBUS_SYSTEM_BUS_ADDRESS` names it. Fails when dbus-daemon or python3 with dbus-python + gi
//! is unavailable (`testenv`).

use qml::runtime::Runtime;
use qml::value::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixListener;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use testenv::Wakeups;

const PATH: &str = "/org/rustshell/PolkitAgent";
const CANCELLED: &str = "org.freedesktop.PolicyKit1.Error.Cancelled";

fn python() -> Option<&'static str> {
    let p = ["/usr/bin/python3", "python3"].into_iter().find(|p| {
        Command::new(p)
            .args([
                "-c",
                "import dbus, dbus.mainloop.glib; from gi.repository import GLib",
            ])
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    });
    if p.is_none() {
        testenv::unavailable("python3 with dbus-python and gi not found");
    }
    p
}

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The system bus: a socket at a fixed path that outlives its daemon, as systemd's
/// dbus.socket does, so a restarted daemon answers at the same address.
struct SystemBus {
    listener: UnixListener,
    config: PathBuf,
    daemon: Option<Daemon>,
}

impl SystemBus {
    /// In `test`'s own process: the bus at the address the environment names, a socket in a
    /// directory the parent made. `None` in the process that ran the test there.
    fn new(test: &str) -> Option<SystemBus> {
        SystemBus::with_config(test, "")
    }

    /// [`SystemBus::new`] whose config also has `extra`.
    fn with_config(test: &str, extra: &str) -> Option<SystemBus> {
        SystemBus::start_in(test, extra, testenv::run_in_own_process)
    }

    /// [`SystemBus::new`] in a process with a `/run` of its own
    /// (`testenv::run_in_own_process_with_own_run`), where the test serves polkit's helper
    /// socket.
    fn with_own_run(test: &str) -> Option<SystemBus> {
        SystemBus::start_in(test, "", testenv::run_in_own_process_with_own_run)
    }

    fn start_in(
        test: &str,
        extra: &str,
        run: fn(&str, [(&'static str, String); 1]),
    ) -> Option<SystemBus> {
        if !testenv::in_own_process(test) {
            let dir = tempfile::tempdir().unwrap();
            let socket = dir.path().join("system_bus_socket");
            let addr = format!("unix:path={}", socket.display());
            run(test, [("DBUS_SYSTEM_BUS_ADDRESS", addr)]);
            return None;
        }
        let addr = std::env::var("DBUS_SYSTEM_BUS_ADDRESS").unwrap();
        let socket = PathBuf::from(addr.strip_prefix("unix:path=").unwrap());
        let config = socket.with_file_name("bus.conf");
        std::fs::write(
            &config,
            format!("<busconfig><type>system</type><listen>systemd:</listen><auth>EXTERNAL</auth>\
             <policy context=\"default\"><allow send_destination=\"*\" eavesdrop=\"true\"/>\
             <allow eavesdrop=\"true\"/><allow own=\"*\"/><allow user=\"*\"/></policy>{extra}</busconfig>"),
        )
        .expect("the bus config next to the bus socket");
        let listener = UnixListener::bind(&socket).expect("the bus socket");
        let mut bus = SystemBus {
            listener,
            config,
            daemon: None,
        };
        if !bus.start() {
            testenv::unavailable("dbus-daemon not available");
            return None;
        }
        Some(bus)
    }

    /// A daemon on the socket, passed as fd 3 the way systemd activates dbus.service.
    fn start(&mut self) -> bool {
        let fd = self.listener.as_raw_fd();
        let mut c = Command::new("sh");
        // sd_listen_fds(3) takes the fds only when LISTEN_PID is the daemon's own pid.
        c.args(["-c", r#"LISTEN_PID=$$ exec "$0" "$@""#, "dbus-daemon"])
            .arg(format!("--config-file={}", self.config.display()))
            .args(["--nofork", "--nopidfile", "--print-address=1"])
            .env("LISTEN_FDS", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        // Runs in the forked child before exec: fcntl(2) and dup2(2) are async-signal-safe and
        // nothing is allocated. Copied above 3 first so that `fd` being 3 still works.
        let hook = move || {
            // SAFETY: F_DUPFD_CLOEXEC duplicates the open `fd`; the copy closes at exec.
            let m = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 4) };
            // SAFETY: dup2 of the open `m` onto 3 leaves 3 without FD_CLOEXEC, so the daemon
            // inherits it.
            if m < 0 || unsafe { libc::dup2(m, 3) } < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        };
        // SAFETY: `hook` is async-signal-safe and allocation-free, as pre_exec requires.
        unsafe { c.pre_exec(hook) };
        let Ok(mut child) = c.spawn() else {
            return false;
        };
        let out = child.stdout.take();
        let d = Daemon(child);
        // Printed once it accepts connections.
        let mut addr = String::new();
        if out.is_none_or(|o| BufReader::new(o).read_line(&mut addr).is_err()) || addr.is_empty() {
            return false;
        }
        self.daemon = Some(d);
        true
    }

    /// dbus-daemon exits (crash, `systemctl restart dbus`): every connection to it closes.
    fn stop(&mut self) {
        self.daemon = None;
    }
}

/// A test's runtime with its host loop: every wait blocks until the runtime is woken (its
/// waker's ping) or a fake prints a line, with the test's deadline as the hang guard only.
struct Host {
    rt: Runtime,
    r: usize,
    wake: Wakeups,
}

impl Host {
    fn new(src: &str) -> Host {
        let mut rt = Runtime::new(None);
        let wake = Wakeups::new();
        rt.world.borrow().waker.set_ping(wake.ping());
        let r = rt.load_str(src).unwrap();
        rt.settle();
        Host { rt, r, wake }
    }

    /// Runs the loop until `done`.
    fn until(&mut self, what: &str, mut done: impl FnMut(&mut Runtime, usize) -> bool) {
        // Named in the output nextest shows when its slow-timeout ends a hang.
        eprintln!("waiting for {what}");
        loop {
            self.rt.poll_native();
            self.rt.settle();
            if done(&mut self.rt, self.r) {
                return;
            }
            self.wake.wait(None, None);
        }
    }

    fn get(&self, prop: &str) -> String {
        self.rt.get(self.r, prop).to_js_string()
    }

    fn set(&mut self, prop: &str, v: Value) {
        self.rt.world.borrow_mut().write(self.r, prop, v);
        self.rt.settle();
    }

    /// The next line of `f` containing `s`, running the loop meanwhile (the line may answer
    /// something only the World decides).
    fn expect(&mut self, f: &Fake, s: &str) -> String {
        let mut got = None;
        self.until(&format!("{s:?} from the fake"), |_, _| {
            while let Ok(l) = f.out.try_recv() {
                if l.contains(s) {
                    got = Some(l);
                    return true;
                }
            }
            false
        });
        got.unwrap()
    }

    /// Lines of `f` containing each of `all`, in any order (replies to concurrent calls).
    fn expect_all(&mut self, f: &Fake, all: &[&str]) {
        let mut left: Vec<&str> = all.to_vec();
        self.until(&format!("{all:?} from the fake"), |_, _| {
            while let Ok(l) = f.out.try_recv() {
                left.retain(|s| !l.contains(s));
            }
            left.is_empty()
        });
    }

    /// Runs the loop until `log` has `s`; clears it.
    fn logged(&mut self, s: &str) {
        self.until(&format!("{s:?} in the log"), |rt, r| {
            rt.get(r, "log").to_js_string().contains(s)
        });
        self.set("log", Value::str(""));
    }

    fn fake(&mut self, python: &str, mode: &str, init: &str) -> Fake {
        let f = Fake::start(python, mode, init, self.wake.ping());
        self.expect(&f, "READY");
        f
    }

    fn cmd(&mut self, f: &mut Fake, json: &str) {
        writeln!(f.stdin, "{json}").unwrap();
        f.stdin.flush().unwrap();
        self.expect(f, "DONE");
    }

    fn op(&mut self, f: &mut Fake, op: &str, cookie: &str) {
        self.cmd(f, &format!(r#"{{"op":"{op}","cookie":"{cookie}"}}"#));
    }

    /// polkitd took the agent's registration and the agent says so.
    fn registered(&mut self, f: &Fake) {
        self.expect(f, &format!("POLKIT Register {PATH} unix-session c1"));
        self.until("isRegistered", |rt, r| {
            rt.get(r, "registered").to_js_string() == "true"
        });
    }

    /// One request: started on BeginAuthentication, then dismissed by the user
    /// (cancelAuthenticationRequest()), which polkitd sees as `Cancelled`.
    fn round(&mut self, f: &mut Fake, cookie: &str) {
        self.op(f, "begin", cookie);
        self.logged("started org.example.act;");
        let n = self.rt.get(self.r, "seq").to_number();
        self.set("seq", Value::Num(n + 1.0));
        self.expect(f, &format!("BEGIN-ERROR {cookie} {CANCELLED}"));
    }
}

struct Fake {
    /// None for a fake the bus started (D-Bus activation): it ends with its stdin.
    child: Option<Child>,
    stdin: Box<dyn Write + Send>,
    out: Receiver<String>,
}

impl Fake {
    /// Each output line wakes the host through `ping`.
    fn start(python: &str, mode: &str, init: &str, ping: impl Fn() + Send + 'static) -> Fake {
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fake_sysbus.py");
        let mut child = Command::new(python)
            .args([script, mode, init])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn fake");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        Fake {
            child: Some(child),
            stdin: Box::new(stdin),
            out: lines(stdout, ping),
        }
    }

    /// The fake polkitd the bus starts by D-Bus activation of `org.freedesktop.PolicyKit1`
    /// ([`activatable_polkitd`] in `dir`), talking through its FIFOs. Each is opened read-write,
    /// which does not block, so the fake's own opens go through whenever the bus starts it.
    fn activated(dir: &std::path::Path, ping: impl Fn() + Send + 'static) -> Fake {
        let fifo = |n: &str| {
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(dir.join(n))
                .unwrap()
        };
        Fake {
            child: None,
            stdin: Box::new(fifo("in")),
            out: lines(fifo("out"), ping),
        }
    }
}

/// Each line of `r`, waking the host through `ping`.
fn lines(r: impl Read + Send + 'static, ping: impl Fn() + Send + 'static) -> Receiver<String> {
    let (tx, out) = channel();
    std::thread::spawn(move || {
        for l in BufReader::new(r).lines().map_while(Result::ok) {
            if tx.send(l).is_err() {
                return;
            }
            ping();
        }
    });
    out
}

/// A service directory in `dir` (`<servicedir>`) whose `org.freedesktop.PolicyKit1` is the fake
/// polkitd, reading commands from the FIFO `dir/in` and printing to `dir/out`.
fn activatable_polkitd(dir: &std::path::Path, python: &str) {
    for n in ["in", "out"] {
        let p = std::ffi::CString::new(dir.join(n).into_os_string().into_encoded_bytes()).unwrap();
        // SAFETY: `p` is a NUL-terminated path that outlives the call.
        assert_eq!(unsafe { libc::mkfifo(p.as_ptr(), 0o600) }, 0, "mkfifo {n}");
    }
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fake_sysbus.py");
    let exec = dir.join("polkitd.sh");
    testenv::install_script(
        &exec,
        &format!(
            "#!/bin/sh\nexec '{python}' '{script}' polkitd '{{}}' <'{d}/in' >'{d}/out'\n",
            d = dir.display()
        ),
    );
    let services = dir.join("services");
    std::fs::create_dir_all(&services).unwrap();
    std::fs::write(
        services.join("org.freedesktop.PolicyKit1.service"),
        format!(
            "[D-BUS Service]\nName=org.freedesktop.PolicyKit1\nExec={}\n",
            exec.display()
        ),
    )
    .unwrap();
}

impl Drop for Fake {
    fn drop(&mut self) {
        if let Some(c) = &mut self.child {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

const QML: &str = r#"
import QtQuick
import Rustshell.Bluetooth
import Rustshell.Services.Polkit
Item { id: r
    property int nd: Bluetooth.devices.values.length
    property bool registered: agent.isRegistered
    property string log: ""
    property int seq
    PolkitAgent { id: agent
        onAuthenticationRequestStarted: r.log += "started " + agent.flow.actionId + ";"
    }
    onSeqChanged: agent.flow.cancelAuthenticationRequest()
}
"#;

const HCI0: &str = "/org/bluez/hci0";

fn bluez_init() -> String {
    format!(
        r#"{{"objects":{{"{HCI0}":{{"org.bluez.Adapter1":{{"Address":"00:1A:7D:DA:71:13","Alias":"arch","Name":"arch","Powered":true,"PowerState":"on","Discoverable":false,"DiscoverableTimeout":180,"Discovering":false,"Pairable":true,"PairableTimeout":0}}}},"{HCI0}/dev_11_22_33_44_55_66":{{"org.bluez.Device1":{{"Address":"11:22:33:44:55:66","Name":"WH-1000XM4","Alias":"Headphones","Icon":"audio-headset","Paired":false,"Bonded":false,"Trusted":false,"Blocked":false,"Connected":false,"Adapter":"{HCI0}"}}}}}}}}"#
    )
}

/// bluetoothd running: the BlueZ pairing agent is served by the shared system connection's
/// ObjectServer, and polkitd's calls to the polkit agent on that connection still reach it
/// (the ObjectServer answered them `UnknownObject` while the agent read raw MethodCalls).
#[test]
fn polkit_agent_beside_the_bluetooth_agent() {
    let Some(py) = python() else { return };
    let Some(_bus) = SystemBus::new(concat!(
        module_path!(),
        "::polkit_agent_beside_the_bluetooth_agent"
    )) else {
        return;
    };
    let mut h = Host::new(QML);
    let bluez = h.fake(py, "bluez", &bluez_init());
    h.expect(
        &bluez,
        "AGENT RequestDefaultAgent /org/rustshell/BluetoothAgent",
    );
    let mut f = h.fake(py, "polkitd", "{}");
    h.registered(&f);
    h.round(&mut f, "c1");

    // polkitd cancelling (the client went away) ends the request as cancelled.
    h.op(&mut f, "begin", "c2");
    h.logged("started org.example.act;");
    h.op(&mut f, "cancel", "c2");
    h.expect_all(
        &f,
        &["CANCEL-REPLY c2", &format!("BEGIN-ERROR c2 {CANCELLED}")],
    );
    h.op(&mut f, "cancel", "c9");
    h.expect(
        &f,
        "CANCEL-ERROR c9 org.freedesktop.PolicyKit1.Error.Failed",
    );

    // Only polkitd may start a request.
    h.op(&mut f, "begin-forged", "c3");
    h.expect(
        &f,
        "BEGIN-ERROR c3 org.freedesktop.PolicyKit1.Error.NotAuthorized",
    );
    assert_eq!(h.get("log"), "");
    drop(bluez);
}

/// The system bus restarting (dbus-daemon crash, `systemctl restart dbus`) closes the
/// connection the agent was served and registered on: it comes back on the new connection,
/// registered with the new polkitd, as the other system-bus services reconnect.
#[test]
fn polkit_agent_survives_a_bus_restart() {
    let Some(py) = python() else { return };
    let Some(mut bus) = SystemBus::new(concat!(
        module_path!(),
        "::polkit_agent_survives_a_bus_restart"
    )) else {
        return;
    };
    let mut h = Host::new(QML);
    let mut f = h.fake(py, "polkitd", "{}");
    h.registered(&f);
    h.round(&mut f, "c1");

    drop(f);
    bus.stop();
    assert!(bus.start(), "dbus-daemon did not restart");
    let mut f = h.fake(py, "polkitd", "{}");
    h.expect(&f, &format!("POLKIT Register {PATH} unix-session c1"));
    h.round(&mut f, "c2");
}

/// polkitd replaced directly (`polkitd --replace`: one NameOwnerChanged from the old owner to
/// the new) starts with no agents: the agent registers with the new one, which can then use it.
#[test]
fn polkit_agent_registers_with_a_replacing_polkitd() {
    let Some(py) = python() else { return };
    let Some(_bus) = SystemBus::new(concat!(
        module_path!(),
        "::polkit_agent_registers_with_a_replacing_polkitd"
    )) else {
        return;
    };
    let mut h = Host::new(QML);
    let mut f = h.fake(py, "polkitd", "{}");
    h.registered(&f);
    h.cmd(&mut f, r#"{"op":"replace-polkitd"}"#);
    h.expect(&f, &format!("POLKIT-NEXT Register {PATH} unix-session c1"));
    assert_eq!(h.get("registered"), "true");
    h.round(&mut f, "c1");
}

/// polkitd not running but D-Bus-activatable (as it ships: polkit.service is started by its
/// first client): the agent starts it and registers, as libpolkit-agent's authority proxy does,
/// so the first CheckAuthorization already finds an agent.
#[test]
fn polkit_agent_activates_polkitd() {
    let Some(py) = python() else { return };
    let test = concat!(module_path!(), "::polkit_agent_activates_polkitd");
    let dir = std::env::var("DBUS_SYSTEM_BUS_ADDRESS").ok().and_then(|a| {
        let socket = PathBuf::from(a.strip_prefix("unix:path=")?);
        Some(socket.parent()?.to_path_buf())
    });
    let services = match dir.as_deref().filter(|_| testenv::in_own_process(test)) {
        Some(d) => {
            activatable_polkitd(d, py);
            format!("<servicedir>{}</servicedir>", d.join("services").display())
        }
        None => String::new(),
    };
    let Some(_bus) = SystemBus::with_config(test, &services) else {
        return;
    };
    let mut h = Host::new(QML);
    let mut f = Fake::activated(dir.as_deref().unwrap(), h.wake.ping());
    h.expect(&f, "READY");
    h.registered(&f);
    h.round(&mut f, "c1");
}

/// polkitd exiting drops the registration: isRegistered follows it to false and back once a
/// polkitd is there again.
#[test]
fn polkit_agent_is_unregistered_while_polkitd_is_gone() {
    let Some(py) = python() else { return };
    let Some(_bus) = SystemBus::new(concat!(
        module_path!(),
        "::polkit_agent_is_unregistered_while_polkitd_is_gone"
    )) else {
        return;
    };
    let mut h = Host::new(QML);
    let f = h.fake(py, "polkitd", "{}");
    h.registered(&f);
    drop(f);
    h.until("isRegistered false", |rt, r| {
        rt.get(r, "registered").to_js_string() == "false"
    });
    let mut f = h.fake(py, "polkitd", "{}");
    h.registered(&f);
    h.round(&mut f, "c1");
}

/// A destroyed agent's service thread lets go of polkitd at once, not at its next owner change:
/// its NameOwnerChanged match leaves the bus while polkitd runs on unchanged.
#[test]
fn polkit_agent_unsubscribes_when_destroyed() {
    let Some(py) = python() else { return };
    let Some(_bus) = SystemBus::new(concat!(
        module_path!(),
        "::polkit_agent_unsubscribes_when_destroyed"
    )) else {
        return;
    };
    let mut h = Host::new(LOADED);
    let mut f = h.fake(py, "polkitd", "{}");
    h.registered(&f);
    h.cmd(&mut f, r#"{"op":"watch-removematch"}"#);
    h.set("on", Value::Bool(false));
    let l = h.expect(&f, "REMOVEMATCH agent");
    assert!(
        l.contains("NameOwnerChanged") && l.contains("org.freedesktop.PolicyKit1"),
        "{l}"
    );
}

const LOADED: &str = "import QtQuick\nimport Rustshell.Services.Polkit\nItem { id: r\n\
     property bool on: true\n\
     property bool registered: l.item?.isRegistered ?? false\n\
     Loader { id: l; active: r.on; sourceComponent: PolkitAgent {} } }";

/// The first answer to BeginAuthentication `cookie`: a destroyed agent has none, its object is
/// gone.
fn begin_finds_no_agent(h: &mut Host, f: &Fake, cookie: &str) {
    let l = h.expect(f, &format!("BEGIN-ERROR {cookie} "));
    assert!(
        l.ends_with(" org.freedesktop.DBus.Error.UnknownObject"),
        "{l}"
    );
}

/// The agent goes away: it unregisters from polkitd and its object leaves the bus.
#[test]
fn polkit_agent_withdraws_when_destroyed() {
    let Some(py) = python() else { return };
    let Some(_bus) = SystemBus::new(concat!(
        module_path!(),
        "::polkit_agent_withdraws_when_destroyed"
    )) else {
        return;
    };
    let mut h = Host::new(LOADED);
    let mut f = h.fake(py, "polkitd", "{}");
    h.registered(&f);
    h.set("on", Value::Bool(false));
    h.expect(&f, &format!("POLKIT Unregister {PATH}"));
    h.op(&mut f, "begin", "c1");
    begin_finds_no_agent(&mut h, &f, "c1");
}

/// polkitd starting a request while the agent unregisters (before it answered) already finds
/// the agent's object gone.
#[test]
fn polkit_agent_is_gone_before_it_unregisters() {
    let Some(py) = python() else { return };
    let Some(_bus) = SystemBus::new(concat!(
        module_path!(),
        "::polkit_agent_is_gone_before_it_unregisters"
    )) else {
        return;
    };
    let mut h = Host::new(LOADED);
    let mut f = h.fake(py, "polkitd", "{}");
    h.registered(&f);
    h.op(&mut f, "begin-on-unregister", "c1");
    h.set("on", Value::Bool(false));
    h.expect(&f, &format!("POLKIT Unregister {PATH}"));
    begin_finds_no_agent(&mut h, &f, "c1");
}

/// The agent's next answer from polkitd after its predecessor at the path went away: polkitd
/// keeps one agent per session and refuses a second.
fn next_registration(h: &mut Host, f: &Fake) {
    let mut got = None;
    h.until("the next RegisterAuthenticationAgent", |_, _| {
        while let Ok(l) = f.out.try_recv() {
            if l.starts_with("POLKIT Register ") || l.starts_with("POLKIT Refused ") {
                got = Some(l);
                return true;
            }
        }
        false
    });
    let l = got.unwrap();
    assert!(l.starts_with("POLKIT Register "), "{l}");
    h.until("isRegistered", |rt, r| {
        rt.get(r, "registered").to_js_string() == "true"
    });
}

/// An agent destroyed before the main loop heard from its service thread (which had already
/// served it and registered it with polkitd) still withdraws: a later agent can register.
#[test]
fn polkit_agent_destroyed_before_its_bus_events_withdraws() {
    let Some(py) = python() else { return };
    let Some(_bus) = SystemBus::new(concat!(
        module_path!(),
        "::polkit_agent_destroyed_before_its_bus_events_withdraws"
    )) else {
        return;
    };
    let mut h = Host::new(LOADED);
    h.set("on", Value::Bool(false));
    let f = h.fake(py, "polkitd", "{}");
    h.set("on", Value::Bool(true));
    // One turn starts the agent; its thread's events are left unread from here on.
    h.rt.poll_native();
    h.rt.settle();
    let reg = format!("POLKIT Register {PATH} unix-session c1");
    while !f.out.recv().unwrap().contains(&reg) {}
    h.set("on", Value::Bool(false));
    h.rt.poll_native();
    h.rt.settle();
    h.set("on", Value::Bool(true));
    next_registration(&mut h, &f);
}

const SWAPPED: &str = "import QtQuick\nimport Rustshell.Services.Polkit\nItem { id: r\n\
     property bool second\n\
     property bool registered: l.item?.isRegistered ?? false\n\
     Component { id: a; PolkitAgent {} }\n\
     Component { id: b; PolkitAgent {} }\n\
     Loader { id: l; sourceComponent: r.second ? b : a } }";

/// One agent replacing another at the same path in one turn registers only once its
/// predecessor's registration is gone (polkitd answered its UnregisterAuthenticationAgent, which
/// it handles late here), and that predecessor's withdrawal leaves it registered.
#[test]
fn polkit_agent_successor_registers_after_its_predecessor_withdrew() {
    let Some(py) = python() else { return };
    let Some(_bus) = SystemBus::new(concat!(
        module_path!(),
        "::polkit_agent_successor_registers_after_its_predecessor_withdrew"
    )) else {
        return;
    };
    let mut h = Host::new(SWAPPED);
    let mut f = h.fake(py, "polkitd", "{}");
    h.registered(&f);
    h.op(&mut f, "hold-unregister", "");
    h.set("second", Value::Bool(true));
    // Every line from here: a refused registration may come before the held call.
    let mut seen = Vec::new();
    let mut until_line = |h: &mut Host, what: &str, end: &dyn Fn(&str) -> bool| {
        h.until(what, |_, _| {
            while let Ok(l) = f.out.try_recv() {
                let done = end(&l) || l.starts_with("POLKIT Refused ");
                seen.push(l);
                if done {
                    return true;
                }
            }
            false
        });
    };
    until_line(&mut h, "the held UnregisterAuthenticationAgent", &|l| {
        l.contains("POLKIT Unregister-held ")
    });
    writeln!(f.stdin, r#"{{"op":"release-unregister","cookie":""}}"#).unwrap();
    f.stdin.flush().unwrap();
    until_line(
        &mut h,
        "the successor's RegisterAuthenticationAgent",
        &|l| l.starts_with("POLKIT Register "),
    );
    assert!(
        seen.last().unwrap().starts_with("POLKIT Register "),
        "{seen:?}"
    );
    h.until("isRegistered", |rt, r| {
        rt.get(r, "registered").to_js_string() == "true"
    });
    // polkitd's agent is the successor: a request reaches it.
    h.op(&mut f, "begin", "c1");
    h.op(&mut f, "cancel", "c1");
    h.expect_all(
        &f,
        &["CANCEL-REPLY c1", &format!("BEGIN-ERROR c1 {CANCELLED}")],
    );
}

/// The helper takes the user and the cookie, then ends without asking anything (PAM refused
/// before any prompt, as pam_faillock does for a locked account): the request fails once and
/// waits for the user, with no new helper started (quickshell starts one again at once, every
/// time, as long as the request lasts). The user can still dismiss it.
#[test]
fn polkit_helper_that_ends_without_asking_is_not_restarted() {
    const SOCKET: &str = "/run/polkit/agent-helper.socket";
    let Some(py) = python() else { return };
    let Some(_bus) = SystemBus::with_own_run(concat!(
        module_path!(),
        "::polkit_helper_that_ends_without_asking_is_not_restarted"
    )) else {
        return;
    };
    std::fs::create_dir_all("/run/polkit").unwrap();
    let l = UnixListener::bind(SOCKET).unwrap();
    let helper = {
        let l = l.try_clone().unwrap();
        std::thread::spawn(move || {
            let (s, _) = l.accept().unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let (mut user, mut cookie) = (String::new(), String::new());
            r.read_line(&mut user).unwrap();
            r.read_line(&mut cookie).unwrap();
            let mut s = s;
            s.write_all(b"PAM_ERROR_MSG The account is locked\nFAILURE\n")
                .unwrap();
            (user, cookie)
        })
    };
    let mut h = Host::new(
        "import QtQuick\nimport Rustshell.Services.Polkit\nItem { id: r\n\
         property bool registered: agent.isRegistered\n\
         property string log: \"\"\n\
         property int seq\n\
         property string failure: agent.flow && agent.flow.failed ? agent.flow.supplementaryMessage : \"\"\n\
         onSeqChanged: agent.flow.cancelAuthenticationRequest()\n\
         PolkitAgent { id: agent\n\
           onAuthenticationRequestStarted: r.log += \"started;\" } }",
    );
    let mut f = h.fake(py, "polkitd", "{}");
    h.registered(&f);
    h.cmd(
        &mut f,
        r#"{"op":"begin","cookie":"c1","ids":[["unix-user","uid",0]]}"#,
    );
    h.until("the failure", |rt, r| {
        !rt.get(r, "failure").to_js_string().is_empty()
    });
    let (user, cookie) = helper.join().unwrap();
    assert_eq!((user.as_str(), cookie.as_str()), ("root\n", "c1\n"));
    assert_eq!(h.get("failure"), "The account is locked");
    // A helper started again connects while the failure is handled, before the loop returns.
    l.set_nonblocking(true).unwrap();
    match l.accept() {
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
        r => panic!("a second helper session was started: {r:?}"),
    }
    let n = h.rt.get(h.r, "seq").to_number();
    h.set("seq", Value::Num(n + 1.0));
    h.expect(&f, &format!("BEGIN-ERROR c1 {CANCELLED}"));
    assert_eq!(h.get("log"), "started;");
}
