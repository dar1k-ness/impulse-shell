//! A8: `qml::keyring` against the original `secret-tool` and `scripts/keyring/*.sh`, both talking
//! to a real `gnome-keyring-daemon` on a private session bus with a throwaway HOME. Skipped
//! without dbus-daemon, gnome-keyring-daemon or secret-tool.

use qml::keyring::{self, Lookup};
use qml::runtime::Runtime;
use qml::value::Value;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Instant;
use zbus::blocking::{Connection, MessageIterator, fdo::DBusProxy};

const SECRETS: &str = "org.freedesktop.secrets";
const LOGIN: &str = "/org/freedesktop/secrets/collection/login";
const LABEL: &str = "impulse-shell Safe Storage";
/// KeyringStorage.properties.explanation (untranslated).
const EXPLANATION: &str = "For storing API keys and other sensitive information";

fn ri() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../qml"))
}

fn script(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/original-unshipped/scripts/keyring")
        .join(name)
        .display()
        .to_string()
}

fn have(bin: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {bin}")])
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

struct Env {
    bus: Child,
    daemon: Option<Child>,
    home: PathBuf,
}

impl Drop for Env {
    fn drop(&mut self) {
        if let Some(d) = &mut self.daemon {
            let _ = d.kill();
            let _ = d.wait();
        }
        // A daemon unlock.sh restarted exits with the bus.
        let _ = self.bus.kill();
        let _ = self.bus.wait();
    }
}

/// A throwaway HOME (with the XDG data and runtime directories under it) and a session bus
/// address there, for a process of the test's own (`testenv::run_in_own_process`): the
/// scripts, secret-tool and the native side all find them through the environment.
fn keyring_vars(home: &Path) -> Vec<(&'static str, std::ffi::OsString)> {
    std::fs::create_dir_all(home.join("run")).unwrap();
    vec![
        ("HOME", home.into()),
        ("XDG_DATA_HOME", home.join("share").into()),
        ("XDG_RUNTIME_DIR", home.join("run").into()),
        (
            "DBUS_SESSION_BUS_ADDRESS",
            format!("unix:path={}", home.join("bus").display()).into(),
        ),
    ]
}

/// In `test`'s own process: the private session bus + gnome-keyring under the HOME the parent
/// made; `password`: create/unlock the login keyring with it. `None` in the process that ran
/// the test there, or without the tools.
fn env(test: &str, password: Option<&str>) -> Option<Env> {
    if !testenv::in_own_process(test) {
        let home = tempfile::tempdir().unwrap();
        testenv::run_in_own_process(test, keyring_vars(home.path()));
        return None;
    }
    start(password)
}

/// One side of an A/B comparison, on a fresh keyring in a process of its own. In the process
/// that runs `test`: runs `test` again there for `side` and returns what `f` returned (handed
/// back through a file), `None` when that process skipped. In that process: runs `f` for
/// `side` and returns `None`, as for every other side.
fn side<R: serde::Serialize + serde::de::DeserializeOwned>(
    test: &str,
    side: &str,
    password: Option<&str>,
    f: impl FnOnce(&mut Env) -> R,
) -> Option<R> {
    if !testenv::in_own_process(test) {
        let home = tempfile::tempdir().unwrap();
        let out = home.path().join("side.json");
        let mut vars = keyring_vars(home.path());
        vars.push(("KEYRING_SIDE", side.into()));
        vars.push(("KEYRING_SIDE_OUT", out.clone().into()));
        testenv::run_in_own_process(test, vars);
        // Nothing written: the side skipped for a missing tool (RI_EQUIV_SKIP=1).
        let r = std::fs::read(&out).ok()?;
        return Some(serde_json::from_slice(&r).unwrap());
    }
    if std::env::var_os("KEYRING_SIDE").is_none_or(|s| s != side) {
        return None;
    }
    let mut e = start(password)?;
    let r = serde_json::to_vec(&f(&mut e)).unwrap();
    std::fs::write(std::env::var_os("KEYRING_SIDE_OUT").unwrap(), r).unwrap();
    None
}

/// The private session bus + gnome-keyring at the HOME and address the environment names.
fn start(password: Option<&str>) -> Option<Env> {
    if !["dbus-daemon", "gnome-keyring-daemon", "secret-tool"]
        .iter()
        .all(|b| have(b))
    {
        testenv::unavailable("needs dbus-daemon, gnome-keyring-daemon, secret-tool");
        return None;
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let mut bus = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--print-address=1"])
        .arg(format!(
            "--address={}",
            std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap()
        ))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut addr = String::new();
    BufReader::new(bus.stdout.take().unwrap())
        .read_line(&mut addr)
        .ok()?;
    let mut c = Command::new("gnome-keyring-daemon");
    c.args(["--foreground", "--components=secrets"]);
    if password.is_some() {
        c.arg("--unlock");
    }
    let mut d = c
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .ok()?;
    let mut stdin = d.stdin.take().unwrap();
    if let Some(p) = password {
        stdin.write_all(p.as_bytes()).unwrap();
    }
    drop(stdin);
    let owner = SecretsOwner::watch(&Connection::session().unwrap());
    // Whichever comes first: the daemon takes the name, or it exits (its pidfd turns readable).
    let (tx, first) = std::sync::mpsc::channel();
    let up = tx.clone();
    std::thread::spawn(move || {
        owner.until(|o| !o.is_empty());
        let _ = up.send(true);
    });
    let pidfd = Pidfd::open(d.id());
    std::thread::spawn(move || {
        pidfd.wait();
        let _ = tx.send(false);
    });
    if !first.recv().unwrap() {
        // e.g. root in a container: it aborts when it cannot drop capabilities
        let st = testenv::wait_exit(&mut d);
        testenv::unavailable(format!("gnome-keyring-daemon exited ({st})"));
        let _ = bus.kill();
        let _ = bus.wait();
        return None;
    }
    Some(Env {
        bus,
        daemon: Some(d),
        home,
    })
}

fn run(cmd: &str, stdin: &[u8]) -> (String, i32) {
    let mut c = Command::new("bash")
        .args(["-c", cmd])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(stdin).unwrap();
    let o = c.wait_with_output().unwrap();
    (
        String::from_utf8_lossy(&o.stdout).into_owned(),
        o.status.code().unwrap_or(-1),
    )
}

/// KeyringStorage.qml's getData: `try_lookup.sh 2> /dev/null`, stdout and exit code.
fn original_lookup() -> Lookup {
    match run(&format!("{} 2> /dev/null", script("try_lookup.sh")), b"") {
        (o, 0) => Lookup::Found(o.strip_suffix('\n').unwrap_or(&o).to_string()),
        (_, 1) => Lookup::NotFound,
        (_, 2) => Lookup::Locked,
        r => panic!("try_lookup.sh: {r:?}"),
    }
}

/// KeyringStorage.qml's saveData: `secret-tool store --label=… application … explanation …`.
fn original_store(payload: &str) {
    let (_, code) = run(
        &format!(
            "secret-tool store --label='{LABEL}' application impulse-shell explanation '{EXPLANATION}'"
        ),
        payload.as_bytes(),
    );
    assert_eq!(code, 0);
}

fn native_store(payload: &str) {
    let attrs = [
        ("application".to_string(), "impulse-shell".to_string()),
        ("explanation".to_string(), EXPLANATION.to_string()),
    ];
    keyring::secret_store(LABEL, &attrs, payload.as_bytes()).unwrap();
}

/// `secret-tool search --all` minus the per-item path and timestamps, plus content types.
/// `schema` (`xdg:schema`) is left out: gnome-keyring fills it in by itself for some items and
/// not others whoever stores them.
fn snapshot() -> String {
    let (o, _) = run(
        "secret-tool search --all application impulse-shell 2>&1",
        b"",
    );
    let mut s: Vec<String> = o
        .lines()
        .filter(|l| {
            !["[", "created", "modified", "schema"]
                .iter()
                .any(|p| l.starts_with(p))
        })
        .map(String::from)
        .collect();
    let ss = secret_service::blocking::SecretService::connect(secret_service::EncryptionType::Dh)
        .unwrap();
    for it in ss
        .search_items([("application", "impulse-shell")].into())
        .unwrap()
        .unlocked
    {
        s.push(format!(
            "content-type = {}",
            it.get_secret_content_type().unwrap()
        ));
    }
    s.join("\n")
}

fn clear() {
    run("secret-tool clear application impulse-shell", b"");
}

fn service_call(member: &str, paths: &[&str]) {
    let conn = zbus::blocking::Connection::session().unwrap();
    let p: Vec<zbus::zvariant::ObjectPath> =
        paths.iter().map(|p| (*p).try_into().unwrap()).collect();
    conn.call_method(
        Some("org.freedesktop.secrets"),
        "/org/freedesktop/secrets",
        Some("org.freedesktop.Secret.Service"),
        member,
        &(p,),
    )
    .unwrap();
}

#[test]
fn lookup_matches_try_lookup() {
    let Some(_e) = env(
        concat!(module_path!(), "::lookup_matches_try_lookup"),
        Some("pw"),
    ) else {
        return;
    };
    assert_eq!(original_lookup(), Lookup::NotFound);
    assert_eq!(keyring::try_lookup(&[keyring::APP_ATTR]), Lookup::NotFound);
    for payload in [
        r#"{"apiKeys":{"gemini":"k1"}}"#,
        "{}",
        "{}\n\n",
        "not json",
        "{\"a\": \"x\ny\"}\n",
    ] {
        clear();
        original_store(payload);
        let o = original_lookup();
        assert_eq!(keyring::try_lookup(&[keyring::APP_ATTR]), o, "{payload:?}");
        assert!(matches!(o, Lookup::Found(_)));
    }
}

#[test]
fn store_matches_secret_tool() {
    let Some(_e) = env(
        concat!(module_path!(), "::store_matches_secret_tool"),
        Some("pw"),
    ) else {
        return;
    };
    let payload = r#"{"apiKeys":{"openai":"sk-test"},"n":1}"#;
    original_store(payload);
    original_store(payload);
    let o = snapshot();
    clear();
    native_store(payload);
    native_store(payload);
    assert_eq!(snapshot(), o);
    assert_eq!(o.matches("label = ").count(), 1, "{o}");
}

#[test]
fn store_without_default_collection() {
    // No login keyring and no `default` alias: both need a "Default keyring" creation prompt,
    // which the test bus has no prompter for, so both fail and store nothing.
    const TEST: &str = concat!(module_path!(), "::store_without_default_collection");
    let o = side(TEST, "original", None, |_| {
        let ok = run(
            "secret-tool store --label=x application impulse-shell",
            b"{}",
        )
        .1 == 0;
        (ok, snapshot())
    });
    let n = side(TEST, "native", None, |_| {
        let attrs = [("application".to_string(), "impulse-shell".to_string())];
        let n = keyring::secret_store("x", &attrs, b"{}").is_ok();
        (n, snapshot())
    });
    let (Some(o), Some(n)) = (o, n) else { return };
    assert_eq!(n, o);
    assert!(!o.0);
}

#[test]
fn locked_keyring() {
    let Some(_e) = env(concat!(module_path!(), "::locked_keyring"), Some("pw")) else {
        return;
    };
    original_store("{}");
    service_call("Lock", &[LOGIN]);
    assert!(!keyring::login_unlocked());
    // No prompter on the test bus: the Unlock prompt is dismissed in both.
    assert_eq!(original_lookup(), Lookup::Locked);
    assert_eq!(keyring::try_lookup(&[keyring::APP_ATTR]), Lookup::Locked);
}

/// unlock.sh with `killall` limited to the test's daemon (it kills every gnome-keyring-daemon of
/// the user otherwise).
/// Owner changes of `org.freedesktop.secrets`: the NameOwnerChanged match is added before the
/// owner is asked for, so no change in between is missed.
struct SecretsOwner {
    dbus: DBusProxy<'static>,
    changes: MessageIterator,
}

impl SecretsOwner {
    fn watch(conn: &Connection) -> SecretsOwner {
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .sender("org.freedesktop.DBus")
            .unwrap()
            .interface("org.freedesktop.DBus")
            .unwrap()
            .member("NameOwnerChanged")
            .unwrap()
            .add_arg(SECRETS)
            .unwrap()
            .build();
        let changes = MessageIterator::for_match_rule(rule, conn, None).unwrap();
        SecretsOwner {
            dbus: DBusProxy::new(conn).unwrap(),
            changes,
        }
    }

    /// The owner's unique name, empty without one.
    fn current(&self) -> String {
        self.dbus
            .get_name_owner(SECRETS.try_into().unwrap())
            .map(|o| o.to_string())
            .unwrap_or_default()
    }

    /// Blocks until the owner (empty: none) is one `want` accepts.
    fn until(self, want: impl Fn(&str) -> bool) {
        if want(&self.current()) {
            return;
        }
        for m in self.changes.flatten() {
            let (_, _, new): (String, String, String) = m.body().deserialize().unwrap();
            if want(&new) {
                return;
            }
        }
        panic!("the session bus closed while waiting for {SECRETS}'s owner");
    }
}

/// A pidfd of a child not yet reaped: readable once it exited, `wait` leaves the reaping to
/// its `Child`.
struct Pidfd(std::os::fd::OwnedFd);

impl Pidfd {
    fn open(pid: u32) -> Pidfd {
        use std::os::fd::FromRawFd;
        // SAFETY: pidfd_open(2) takes no pointers.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0) };
        assert!(fd >= 0, "pidfd_open: {}", std::io::Error::last_os_error());
        // SAFETY: pidfd_open returned a new descriptor that nothing else owns.
        Pidfd(unsafe { std::os::fd::OwnedFd::from_raw_fd(fd as i32) })
    }

    fn wait(&self) {
        use std::os::fd::AsRawFd;
        let mut p = libc::pollfd {
            fd: self.0.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: `p` is one valid pollfd.
        while unsafe { libc::poll(&mut p, 1, -1) } < 0 {
            let e = std::io::Error::last_os_error();
            assert_eq!(e.kind(), std::io::ErrorKind::Interrupted, "poll: {e}");
        }
    }
}

fn original_unlock(e: &mut Env, password: &str) -> i32 {
    let shim = e.home.join("bin");
    std::fs::create_dir_all(&shim).unwrap();
    // killall asks the test, which SIGTERMs the daemon and answers once it has exited (pidfd):
    // unlock.sh starts the new daemon only after the old one released the name.
    let (req, ack) = (shim.join("killall.req"), shim.join("killall.ack"));
    for f in [&req, &ack] {
        let _ = std::fs::remove_file(f);
        assert!(Command::new("mkfifo").arg(f).status().unwrap().success());
    }
    std::fs::write(
        shim.join("killall"),
        format!(
            "#!/bin/sh\necho kill > '{}'\nread _ < '{}'\n",
            req.display(),
            ack.display()
        ),
    )
    .unwrap();
    let conn = Connection::session().unwrap();
    let owner = SecretsOwner::watch(&conn);
    let old = owner.current();
    let daemon = e.daemon.take();
    let killer = std::thread::spawn(move || {
        // Read and write: the request FIFO never reports EOF between killall runs.
        let r = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&req)
            .unwrap();
        let mut daemon = daemon;
        let mut killed = false;
        for l in BufReader::new(r).lines() {
            match l.unwrap().as_str() {
                "kill" => {
                    if let Some(mut d) = daemon.take() {
                        // SAFETY: kill(2) takes no pointers.
                        unsafe { libc::kill(d.id() as libc::pid_t, libc::SIGTERM) };
                        testenv::wait_exit(&mut d);
                        killed = true;
                    }
                    std::fs::write(&ack, "\n").unwrap();
                }
                _ => break,
            }
        }
        (daemon, killed)
    });
    Command::new("chmod")
        .arg("+x")
        .arg(shim.join("killall"))
        .status()
        .unwrap();
    let path = format!("{}:{}", shim.display(), std::env::var("PATH").unwrap());
    let st = Command::new("bash")
        .arg(script("unlock.sh"))
        .env("PATH", path)
        .env("UNLOCK_PASSWORD", password)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    std::fs::write(shim.join("killall.req"), "done\n").unwrap();
    let (daemon, killed) = killer.join().unwrap();
    // unlock.sh only kills the daemon when the keyring was locked. The bus then drops the
    // killed daemon's name once it saw its connection close; the daemon unlock.sh started
    // takes it when the next call to it starts `--start` through D-Bus activation.
    e.daemon = daemon;
    if killed {
        owner.until(|o| o != old);
    }
    st.code().unwrap_or(-1)
}

#[test]
fn unlock_matches_unlock_sh() {
    const TEST: &str = concat!(module_path!(), "::unlock_matches_unlock_sh");
    for (pw, want) in [("pw", true), ("wrong", false)] {
        let o = side(TEST, &format!("original {pw}"), Some("pw"), |e| {
            service_call("Lock", &[LOGIN]);
            original_unlock(e, pw);
            keyring::login_unlocked()
        });
        let n = side(TEST, &format!("native {pw}"), Some("pw"), |_| {
            service_call("Lock", &[LOGIN]);
            let code = if want {
                keyring::UNLOCKED
            } else {
                keyring::UNLOCK_FAILED
            };
            assert_eq!(keyring::unlock_login(pw), code);
            keyring::login_unlocked()
        });
        let (Some(o), Some(n)) = (o, n) else { continue };
        assert_eq!((o, n), (want, want), "{pw}");
    }
    // Already unlocked: unlock.sh exits 1 and changes nothing.
    side(TEST, "already unlocked", Some("pw"), |e| {
        assert_eq!(original_unlock(e, "wrong"), 1);
        assert!(keyring::login_unlocked());
        assert_eq!(keyring::unlock_login("wrong"), keyring::ALREADY_UNLOCKED);
        assert!(keyring::login_unlocked());
    });
}

/// The Secret Service sessions the daemon exports (children of `/org/freedesktop/secrets/session`).
fn open_sessions() -> Vec<String> {
    let conn = zbus::blocking::Connection::session().unwrap();
    let xml: String = conn
        .call_method(
            Some("org.freedesktop.secrets"),
            "/org/freedesktop/secrets/session",
            Some("org.freedesktop.DBus.Introspectable"),
            "Introspect",
            &(),
        )
        .unwrap()
        .body()
        .deserialize()
        .unwrap();
    xml.split("<node name=\"")
        .skip(1)
        .filter_map(|n| n.split_once('"').map(|(n, _)| n.to_string()))
        .collect()
}

/// B7-9: every unlock attempt closes the encrypted session it opened (Secret Service
/// `Session.Close`), and a rejected password is a failure, not success.
#[test]
fn unlock_closes_its_session() {
    const TEST: &str = concat!(module_path!(), "::unlock_closes_its_session");
    let r = side(TEST, "native", Some("pw"), |_| {
        // The daemon lists an open session: a plain one this test leaves open.
        let conn = zbus::blocking::Connection::session().unwrap();
        let plain: (zbus::zvariant::OwnedValue, zbus::zvariant::OwnedObjectPath) = conn
            .call_method(
                Some("org.freedesktop.secrets"),
                "/org/freedesktop/secrets",
                Some("org.freedesktop.Secret.Service"),
                "OpenSession",
                &("plain", zbus::zvariant::Value::from("")),
            )
            .unwrap()
            .body()
            .deserialize()
            .unwrap();
        let listed = open_sessions();
        let mut attempts = Vec::new();
        for pw in ["wrong", "pw"] {
            service_call("Lock", &[LOGIN]);
            let code = keyring::unlock_login(pw);
            attempts.push((pw.to_string(), code, open_sessions()));
        }
        (plain.1.to_string(), listed, attempts)
    });
    let Some((plain, listed, attempts)) = r else {
        return;
    };
    let plain = plain.rsplit('/').next().unwrap().to_string();
    assert_eq!(listed, std::slice::from_ref(&plain));
    assert_eq!(
        attempts,
        [
            ("wrong".into(), keyring::UNLOCK_FAILED, vec![plain.clone()]),
            ("pw".into(), keyring::UNLOCKED, vec![plain]),
        ]
    );
}

/// Every lookup and store closes the Secret Service session it opened (`Session.Close`): the
/// daemon frees a session only then or when the client leaves the bus, and the shell's
/// session-bus connection lives as long as it does.
#[test]
fn lookup_and_store_close_their_sessions() {
    const TEST: &str = concat!(module_path!(), "::lookup_and_store_close_their_sessions");
    let r = side(TEST, "native", Some("pw"), |_| {
        let mut after = Vec::new();
        after.push((
            "lookup missing".to_string(),
            keyring::try_lookup(&[keyring::APP_ATTR]) == Lookup::NotFound,
            open_sessions(),
        ));
        native_store("{}");
        after.push(("store".to_string(), true, open_sessions()));
        native_store(r#"{"a":1}"#);
        after.push(("replace".to_string(), true, open_sessions()));
        let found = keyring::try_lookup(&[keyring::APP_ATTR]) == Lookup::Found(r#"{"a":1}"#.into());
        after.push(("lookup found".to_string(), found, open_sessions()));
        service_call("Lock", &[LOGIN]);
        // No prompter on the test bus: the unlock fails and the item stays unread.
        let locked = keyring::try_lookup(&[keyring::APP_ATTR]) == Lookup::Locked;
        after.push(("lookup locked".to_string(), locked, open_sessions()));
        after
    });
    let Some(after) = r else {
        return;
    };
    for (what, ok, open) in after {
        assert!(ok, "{what}");
        assert_eq!(open, Vec::<String>::new(), "{what}");
    }
}

#[test]
fn unlock_creates_login_keyring() {
    const TEST: &str = concat!(module_path!(), "::unlock_creates_login_keyring");
    let o = side(TEST, "original", None, |e| {
        original_unlock(e, "pw");
        keyring::login_unlocked()
    });
    let n = side(TEST, "native", None, |_| {
        assert_eq!(keyring::unlock_login("pw"), keyring::UNLOCKED);
        keyring::login_unlocked()
    });
    let (Some(o), Some(n)) = (o, n) else { return };
    assert_eq!((o, n), (true, true));
}

const QML: &str = r#"import QtQuick
import ri.services
Item {
    property var data: KeyringStorage.keyringData
    property bool loaded: KeyringStorage.loaded
    property int changes: 0
    Connections { target: KeyringStorage; function onDataChanged() { changes++ } }
    function fetch() { KeyringStorage.fetchKeyringData() }
    function set(k, v) { KeyringStorage.setNestedField(["apiKeys", k], v) }
    function key(k) { return KeyringStorage.keyringData?.apiKeys?.[k] ?? "" }
}
"#;

fn load() -> (Runtime, usize, Host) {
    let mut rt = Runtime::new(Some(ri()));
    let f = std::env::temp_dir().join(format!("qml-keyring-{}.qml", std::process::id()));
    std::fs::write(&f, QML).unwrap();
    let keep = [
        "KeyringStorage",
        "Translation",
        "Directories",
        "Config",
        "FileUtils",
    ];
    let dirs = ["modules/common", "modules/common/functions", "services"].map(|d| ri().join(d));
    rt.load_singletons_where(&dirs, &|n| keep.contains(&n));
    let r = rt.load_file(&f).unwrap();
    rt.settle();
    let host = Host::new(&mut rt);
    (rt, r, host)
}

/// The shell's host loop for one runtime: poll its services until nothing changes, then block
/// until one of them wakes it (a service channel's ping, or its Process/inotify fd readable).
struct Host {
    wake: testenv::Wakeups,
    fd: Option<i32>,
}

impl Host {
    fn new(rt: &mut Runtime) -> Host {
        let wake = testenv::Wakeups::new();
        rt.world.borrow().waker.set_ping(wake.ping());
        Host {
            wake,
            fd: rt.process_fd(),
        }
    }

    fn until(&self, rt: &mut Runtime, what: &str, f: &dyn Fn(&mut Runtime) -> bool) {
        eprintln!("waiting for {what}");
        loop {
            loop {
                let rev = rt.revision();
                rt.poll_native();
                rt.settle();
                if rt.revision() == rev {
                    break;
                }
            }
            if f(rt) {
                return;
            }
            if self.wake.wait(self.fd, None).fd {
                rt.process_fd_ready();
            }
        }
    }
}

/// Every keyring request finished, after `KeyringStorage`'s lookup and its store ran.
fn saved(rt: &mut Runtime) -> bool {
    let w = rt.world.borrow();
    w.keyring
        .as_ref()
        .is_some_and(|k| k.idle() && k.started() == 2)
}

fn call(rt: &mut Runtime, r: usize, f: &str, a: Vec<Value>) -> String {
    rt.call_method(r, f, a)
        .unwrap()
        .unwrap_or_default()
        .to_js_string()
}

fn idle(rt: &mut Runtime) -> bool {
    rt.world.borrow().keyring.as_ref().is_some_and(|k| k.idle())
}

/// The KeyringStorage singleton end to end, without spawning a process.
#[test]
fn keyring_storage_singleton() {
    let Some(_e) = env(
        concat!(module_path!(), "::keyring_storage_singleton"),
        Some("pw"),
    ) else {
        return;
    };
    original_store(r#"{"apiKeys":{"gemini":"k1"}}"#);
    let (mut rt, r, host) = load();
    call(&mut rt, r, "fetch", vec![]);
    host.until(&mut rt, "loaded", &|rt| rt.get(r, "loaded").truthy());
    assert_eq!(call(&mut rt, r, "key", vec![Value::str("gemini")]), "k1");
    call(
        &mut rt,
        r,
        "set",
        vec![Value::str("openai"), Value::str("sk-2")],
    );
    let want = Lookup::Found(r#"{"apiKeys":{"gemini":"k1","openai":"sk-2"}}"#.into());
    host.until(&mut rt, "the store", &saved);
    assert_eq!(original_lookup(), want);
    assert_eq!(rt.get(r, "changes").to_js_string(), "1");
    // The store replaced the item secret-tool wrote rather than adding a second one.
    let snap = snapshot();
    assert_eq!(
        snap.matches(&format!("label = {LABEL}")).count(),
        1,
        "{snap}"
    );
    assert!(rt.world.borrow().procs.is_empty(), "no process was started");
}

/// Missing entry: `{}` is stored and `loaded` set.
#[test]
fn keyring_storage_missing_entry() {
    let Some(_e) = env(
        concat!(module_path!(), "::keyring_storage_missing_entry"),
        Some("pw"),
    ) else {
        return;
    };
    let (mut rt, r, host) = load();
    call(&mut rt, r, "fetch", vec![]);
    host.until(&mut rt, "loaded", &|rt| rt.get(r, "loaded").truthy());
    host.until(&mut rt, "the store", &saved);
    assert_eq!(original_lookup(), Lookup::Found("{}".into()));
}

/// A second fetchKeyringData() reads the keyring again (the original calls it from GoogleCloud and
/// LockScreen.initIfReady at startup; each runs try_lookup.sh).
#[test]
fn keyring_storage_refetch() {
    let Some(_e) = env(
        concat!(module_path!(), "::keyring_storage_refetch"),
        Some("pw"),
    ) else {
        return;
    };
    original_store(r#"{"apiKeys":{"gemini":"k1"}}"#);
    let (mut rt, r, host) = load();
    call(&mut rt, r, "fetch", vec![]);
    host.until(&mut rt, "loaded", &|rt| rt.get(r, "loaded").truthy());
    assert_eq!(call(&mut rt, r, "key", vec![Value::str("gemini")]), "k1");
    host.until(&mut rt, "the first fetch to finish", &idle);
    original_store(r#"{"apiKeys":{"gemini":"k2"}}"#);
    call(&mut rt, r, "fetch", vec![]);
    host.until(&mut rt, "the second fetch", &|rt| {
        call(rt, r, "key", vec![Value::str("gemini")]) == "k2"
    });
}

/// `cargo test --features live -p qml --test keyring -- --nocapture`: wall time, CPU and processes
/// per operation, original vs native.
#[test]
#[cfg_attr(
    not(feature = "live"),
    ignore = "live: --features live, docs/benchmarks.md"
)]
fn measure() {
    let Some(_e) = env(concat!(module_path!(), "::measure"), Some("pw")) else {
        return;
    };
    let payload = r#"{"apiKeys":{"gemini":"k1"}}"#;
    let n = 50;
    let cpu = |who: i32| {
        // SAFETY: rusage is plain integers, for which all-zero bytes are a valid value.
        let mut u: libc::rusage = unsafe { std::mem::zeroed() };
        // SAFETY: `u` is a valid, writable rusage for getrusage(2) to fill.
        unsafe { libc::getrusage(who, &mut u) };
        let t = |v: libc::timeval| v.tv_sec as f64 * 1e3 + v.tv_usec as f64 / 1e3;
        t(u.ru_utime) + t(u.ru_stime)
    };
    let bench = |name: &str, f: &dyn Fn(), who: i32| {
        let (c0, t) = (cpu(who), Instant::now());
        for _ in 0..n {
            f();
        }
        println!(
            "{name}: {:?}/op wall, {:.2} ms/op CPU",
            t.elapsed() / n,
            (cpu(who) - c0) / n as f64
        );
    };
    bench(
        "store original",
        &|| original_store(payload),
        libc::RUSAGE_CHILDREN,
    );
    bench("store native", &|| native_store(payload), libc::RUSAGE_SELF);
    bench(
        "lookup original",
        &|| drop(original_lookup()),
        libc::RUSAGE_CHILDREN,
    );
    bench(
        "lookup native",
        &|| drop(keyring::try_lookup(&[keyring::APP_ATTR])),
        libc::RUSAGE_SELF,
    );
    clear();
    bench(
        "lookup-missing original",
        &|| drop(original_lookup()),
        libc::RUSAGE_CHILDREN,
    );
    bench(
        "lookup-missing native",
        &|| drop(keyring::try_lookup(&[keyring::APP_ATTR])),
        libc::RUSAGE_SELF,
    );
}

/// LockScreen.qml's keyring unlock, cut out of the file: its `Connections` on `Keyring` (the
/// refetch of KeyringStorage) and `unlockKeyring()`, with `lockContext.currentText` a property.
fn lockscreen() -> String {
    let src =
        std::fs::read_to_string(ri().join("modules/common/panels/lock/LockScreen.qml")).unwrap();
    let from = src
        .find("    Connections {\n        target: Keyring\n")
        .unwrap();
    let f = "    function unlockKeyring() {";
    let to = src.find(f).unwrap();
    let to = to + src[to..].find("\n    }\n").unwrap() + "\n    }\n".len();
    format!(
        "import QtQuick\nimport Impulse\nimport ri.services\nItem {{\n    id: lockContext\n    \
         property string currentText\n    property bool loaded: KeyringStorage.loaded\n{}}}\n",
        &src[from..to]
    )
}

#[test]
fn lockscreen_unlock_keyring() {
    let Some(_e) = env(
        concat!(module_path!(), "::lockscreen_unlock_keyring"),
        Some("pw"),
    ) else {
        return;
    };
    original_store(r#"{"apiKeys":{"gemini":"k1"}}"#);
    service_call("Lock", &[LOGIN]);
    let dir = std::env::temp_dir().join(format!("qml-lockscreen-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("LockScreen.qml"), lockscreen()).unwrap();
    std::fs::write(dir.join("main.qml"), "LockScreen { currentText: \"pw\" }\n").unwrap();
    let mut rt = Runtime::new(Some(ri()));
    let keep = [
        "KeyringStorage",
        "Translation",
        "Directories",
        "Config",
        "FileUtils",
    ];
    let dirs = ["modules/common", "modules/common/functions", "services"].map(|d| ri().join(d));
    rt.load_singletons_where(&dirs, &|n| keep.contains(&n));
    let r = rt.load_file(&dir.join("main.qml")).unwrap();
    rt.settle();
    let host = Host::new(&mut rt);
    call(&mut rt, r, "unlockKeyring", vec![]);
    host.until(&mut rt, "loaded", &|rt| rt.get(r, "loaded").truthy());
    assert!(keyring::login_unlocked());
    let w = rt.world.borrow();
    assert_eq!(
        w.keyring.as_ref().unwrap().started(),
        2,
        "unlock and lookup"
    );
    assert!(w.procs.is_empty(), "no process was started");
    drop(w);
    let _ = std::fs::remove_dir_all(&dir);
}
