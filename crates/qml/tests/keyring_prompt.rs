//! `qml::keyring` unlocking through the Secret Service prompt, against a fake
//! `org.freedesktop.secrets` on a private session bus: a locked collection or item gets
//! `Service.Unlock`, whose Prompt is prompted once and followed to its `Completed` signal; a
//! dismissed prompt fails the request once, and so does a service that leaves the bus with its
//! prompt open. The session a secret crosses the bus in is `plain` only when the service
//! answers the encrypted one with `NotSupported`, and KeyringStorage writes on top of what is
//! stored, never before it read it. Skipped without dbus-daemon.

use qml::keyring::{self, Lookup};
use qml::runtime::Runtime;
use qml::value::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value as Variant};

const SERVICE: &str = "/org/freedesktop/secrets";
/// The `default` alias: a collection of its own, as when the item lives outside `login`.
const COLLECTION: &str = "/org/freedesktop/secrets/collection/Default_5fkeyring";
const LOGIN: &str = "/org/freedesktop/secrets/collection/login";
const LABEL: &str = "impulse-shell Safe Storage";
const SESSION: &str = "/org/freedesktop/secrets/session/plain";
const DH: &str = "dh-ietf1024-sha256-aes128-cbc-pkcs7";

type Secret = (OwnedObjectPath, Vec<u8>, Vec<u8>, String);

#[derive(Clone, Copy, PartialEq)]
enum Answer {
    Unlock,
    Dismiss,
    /// The service gives up its name with the prompt open, as a keyring daemon that exits
    /// does: no `Completed` follows.
    Vanish,
}

/// What the service answers `OpenSession` of the encrypted algorithm with.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Encrypted {
    /// `org.freedesktop.DBus.Error.NotSupported`: the one answer libsecret goes on to `plain`
    /// after.
    NotSupported,
    /// Another error.
    Failed,
    /// A reply whose `output` is not a byte array.
    WrongType,
    /// A DH public value of 1, which would make the shared key predictable.
    Degenerate,
}

struct Stored {
    label: String,
    attrs: HashMap<String, String>,
    secret: Vec<u8>,
    content_type: String,
}

struct State {
    locked: bool,
    answer: Answer,
    unlocks: usize,
    prompts: usize,
    items: Vec<Stored>,
    encrypted: Encrypted,
    /// The algorithm of every `OpenSession` call.
    sessions: Vec<String>,
    /// `SearchItems` leaves a locked collection's items out, instead of listing them as locked.
    hides_locked: bool,
}

type Shared = Arc<Mutex<State>>;

fn st(s: &Shared) -> MutexGuard<'_, State> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

fn path(p: String) -> OwnedObjectPath {
    OwnedObjectPath::try_from(p).unwrap()
}

fn item_path(i: usize) -> OwnedObjectPath {
    path(format!("{COLLECTION}/{i}"))
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.Secret.Error")]
enum SecretError {
    #[zbus(error)]
    ZBus(zbus::Error),
    IsLocked(String),
}

struct Service(Shared);

#[zbus::interface(name = "org.freedesktop.Secret.Service")]
impl Service {
    fn open_session(
        &self,
        algorithm: &str,
        _input: OwnedValue,
    ) -> zbus::fdo::Result<(OwnedValue, OwnedObjectPath)> {
        let encrypted = {
            let mut s = st(&self.0);
            s.sessions.push(algorithm.into());
            s.encrypted
        };
        if algorithm == "plain" {
            return Ok((OwnedValue::from(0u8), path(SESSION.into())));
        }
        let output = match encrypted {
            Encrypted::NotSupported => {
                return Err(zbus::fdo::Error::NotSupported(algorithm.into()));
            }
            Encrypted::Failed => return Err(zbus::fdo::Error::Failed(algorithm.into())),
            Encrypted::WrongType => OwnedValue::from(0u8),
            Encrypted::Degenerate => Variant::from(vec![1u8]).try_to_owned().unwrap(),
        };
        Ok((output, path(format!("{SERVICE}/session/encrypted"))))
    }

    fn search_items(
        &self,
        attributes: HashMap<String, String>,
    ) -> (Vec<OwnedObjectPath>, Vec<OwnedObjectPath>) {
        let s = st(&self.0);
        let found = s
            .items
            .iter()
            .enumerate()
            .filter(|(_, it)| attributes.iter().all(|(k, v)| it.attrs.get(k) == Some(v)))
            .map(|(i, _)| item_path(i))
            .collect();
        if s.locked && s.hides_locked {
            (vec![], vec![])
        } else if s.locked {
            (vec![], found)
        } else {
            (found, vec![])
        }
    }

    async fn unlock(
        &self,
        objects: Vec<OwnedObjectPath>,
        #[zbus(object_server)] server: &zbus::ObjectServer,
    ) -> zbus::fdo::Result<(Vec<OwnedObjectPath>, OwnedObjectPath)> {
        let n = {
            let mut s = st(&self.0);
            s.unlocks += 1;
            if !s.locked {
                return Ok((objects, path("/".into())));
            }
            s.unlocks
        };
        let p = path(format!("{SERVICE}/prompt/u{n}"));
        let prompt = Prompt {
            state: self.0.clone(),
            objects,
        };
        server.at(&p, prompt).await?;
        Ok((vec![], p))
    }

    fn read_alias(&self, name: &str) -> OwnedObjectPath {
        path(if name == "default" { COLLECTION } else { "/" }.into())
    }

    #[zbus(property)]
    fn collections(&self) -> Vec<OwnedObjectPath> {
        vec![path(LOGIN.into()), path(COLLECTION.into())]
    }
}

/// The login collection: unlocked and empty, so its lock state says nothing about the item.
struct Login;

#[zbus::interface(name = "org.freedesktop.Secret.Collection")]
impl Login {
    #[zbus(property)]
    fn locked(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn label(&self) -> String {
        "Login".into()
    }

    #[zbus(property)]
    fn items(&self) -> Vec<OwnedObjectPath> {
        vec![]
    }
}

struct Collection(Shared);

#[zbus::interface(name = "org.freedesktop.Secret.Collection")]
impl Collection {
    async fn create_item(
        &self,
        properties: HashMap<String, OwnedValue>,
        secret: Secret,
        replace: bool,
        #[zbus(object_server)] server: &zbus::ObjectServer,
    ) -> Result<(OwnedObjectPath, OwnedObjectPath), SecretError> {
        let prop = |k: &str| properties.get(&format!("org.freedesktop.Secret.Item.{k}"));
        let label = prop("Label")
            .and_then(|v| String::try_from(v.try_clone().ok()?).ok())
            .unwrap_or_default();
        let attrs = prop("Attributes")
            .and_then(|v| HashMap::<String, String>::try_from(v.try_clone().ok()?).ok())
            .unwrap_or_default();
        let stored = Stored {
            label,
            attrs,
            secret: secret.2,
            content_type: secret.3,
        };
        let (i, new) = {
            let mut s = st(&self.0);
            if s.locked {
                return Err(SecretError::IsLocked(COLLECTION.into()));
            }
            match s
                .items
                .iter()
                .position(|it| replace && it.attrs == stored.attrs)
            {
                Some(i) => {
                    s.items[i] = stored;
                    (i, false)
                }
                None => {
                    s.items.push(stored);
                    (s.items.len() - 1, true)
                }
            }
        };
        if new {
            server.at(item_path(i), Item(self.0.clone(), i)).await?;
        }
        Ok((item_path(i), path("/".into())))
    }

    #[zbus(property)]
    fn locked(&self) -> bool {
        st(&self.0).locked
    }

    #[zbus(property)]
    fn label(&self) -> String {
        "Default keyring".into()
    }
}

struct Item(Shared, usize);

#[zbus::interface(name = "org.freedesktop.Secret.Item")]
impl Item {
    fn get_secret(&self, _session: OwnedObjectPath) -> Result<Secret, SecretError> {
        let s = st(&self.0);
        if s.locked {
            return Err(SecretError::IsLocked(item_path(self.1).to_string()));
        }
        let it = &s.items[self.1];
        Ok((
            path(SESSION.into()),
            vec![],
            it.secret.clone(),
            it.content_type.clone(),
        ))
    }

    #[zbus(property)]
    fn locked(&self) -> bool {
        st(&self.0).locked
    }

    #[zbus(property)]
    fn label(&self) -> String {
        st(&self.0).items[self.1].label.clone()
    }

    #[zbus(property)]
    fn attributes(&self) -> HashMap<String, String> {
        st(&self.0).items[self.1].attrs.clone()
    }
}

/// The unlock prompt: answers `Prompt(window-id)` as the user would, then emits `Completed`
/// (result: the unlocked object paths) the way gnome-keyring does, after the call returned.
struct Prompt {
    state: Shared,
    objects: Vec<OwnedObjectPath>,
}

#[zbus::interface(name = "org.freedesktop.Secret.Prompt")]
impl Prompt {
    async fn prompt(
        &self,
        _window_id: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        #[zbus(connection)] conn: &zbus::Connection,
    ) -> zbus::fdo::Result<()> {
        let (dismissed, vanish) = {
            let mut s = st(&self.state);
            s.prompts += 1;
            if s.answer == Answer::Unlock {
                s.locked = false;
            }
            (s.answer == Answer::Dismiss, s.answer == Answer::Vanish)
        };
        if vanish {
            // The connection stays, so this call is still answered: whichever of the reply and
            // the name's loss the client sees first, its prompt never completes.
            let conn = conn.clone();
            std::thread::spawn(move || {
                zbus::block_on(conn.release_name("org.freedesktop.secrets")).unwrap();
            });
            return Ok(());
        }
        let result = if dismissed {
            Variant::from("")
        } else {
            Variant::from(self.objects.clone())
        };
        let emitter = emitter.into_owned();
        let result = result.try_to_owned().map_err(zbus::Error::from)?;
        std::thread::spawn(move || {
            zbus::block_on(Self::completed(&emitter, dismissed, result.into()))
        });
        Ok(())
    }

    #[zbus(signal)]
    async fn completed(
        emitter: &SignalEmitter<'_>,
        dismissed: bool,
        result: Variant<'_>,
    ) -> zbus::Result<()>;
}

struct Fake {
    bus: Child,
    conn: zbus::blocking::Connection,
    state: Shared,
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = self.bus.kill();
        let _ = self.bus.wait();
    }
}

impl Fake {
    /// (Service.Unlock calls, Prompt.Prompt calls)
    fn calls(&self) -> (usize, usize) {
        let s = st(&self.state);
        (s.unlocks, s.prompts)
    }

    fn locked(&self) -> bool {
        st(&self.state).locked
    }

    fn lock(&self) {
        st(&self.state).locked = true;
    }

    fn answer(&self, a: Answer) {
        st(&self.state).answer = a;
    }

    fn encrypted(&self, e: Encrypted) {
        st(&self.state).encrypted = e;
    }

    /// The algorithms of the `OpenSession` calls since the last call of this.
    fn sessions(&self) -> Vec<String> {
        std::mem::take(&mut st(&self.state).sessions)
    }

    /// The stored secrets, in creation order.
    fn secrets(&self) -> Vec<String> {
        let s = st(&self.state);
        s.items
            .iter()
            .map(|it| String::from_utf8_lossy(&it.secret).into_owned())
            .collect()
    }

    fn seed(&self, secret: &str) {
        let i = {
            let mut s = st(&self.state);
            s.items.push(Stored {
                label: LABEL.into(),
                attrs: attrs().into_iter().collect(),
                secret: secret.into(),
                content_type: "text/plain".into(),
            });
            s.items.len() - 1
        };
        self.conn
            .object_server()
            .at(item_path(i), Item(self.state.clone(), i))
            .unwrap();
    }
}

/// In a process of `test`'s own (`testenv::run_in_own_process`): a private session bus whose
/// `org.freedesktop.secrets` is the fake above, at the address the environment names (a socket
/// in a directory the parent made), as the native side finds the bus through the environment.
/// A process is started for one `locked` (KEYRING_FAKE_LOCKED); `None` for any other, in the
/// process that ran the test there, or without dbus-daemon.
fn fake(test: &str, locked: bool, answer: Answer) -> Option<Fake> {
    if !testenv::in_own_process(test) {
        let dir = tempfile::tempdir().unwrap();
        let addr = format!("unix:path={}", dir.path().join("bus").display());
        testenv::run_in_own_process(
            test,
            [
                ("DBUS_SESSION_BUS_ADDRESS", addr),
                ("KEYRING_FAKE_LOCKED", locked.to_string()),
            ],
        );
        return None;
    }
    if std::env::var("KEYRING_FAKE_LOCKED").ok() != Some(locked.to_string()) {
        return None;
    }
    if !testenv::require("dbus-daemon") {
        return None;
    }
    let mut bus = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--print-address=1"])
        .arg(format!(
            "--address={}",
            std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap()
        ))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut addr = String::new();
    BufReader::new(bus.stdout.take().unwrap())
        .read_line(&mut addr)
        .unwrap();
    let state = Arc::new(Mutex::new(State {
        locked,
        answer,
        unlocks: 0,
        prompts: 0,
        items: vec![],
        encrypted: Encrypted::NotSupported,
        sessions: vec![],
        hides_locked: false,
    }));
    // Name requested last: clients see the service only once every object is exported.
    let conn = zbus::blocking::connection::Builder::address(addr.trim())
        .unwrap()
        .serve_at(SERVICE, Service(state.clone()))
        .unwrap()
        .serve_at(COLLECTION, Collection(state.clone()))
        .unwrap()
        .serve_at(LOGIN, Login)
        .unwrap()
        .name("org.freedesktop.secrets")
        .unwrap()
        .build()
        .unwrap();
    Some(Fake { bus, conn, state })
}

fn attrs() -> Vec<(String, String)> {
    [
        ("application", "impulse-shell"),
        (
            "explanation",
            "For storing API keys and other sensitive information",
        ),
    ]
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .to_vec()
}

fn lookup() -> Result<Option<Vec<u8>>, keyring::Error> {
    keyring::secret_lookup(&[keyring::APP_ATTR])
}

#[test]
fn unlocked_collection_is_not_unlocked_again() {
    let Some(f) = fake(
        concat!(
            module_path!(),
            "::unlocked_collection_is_not_unlocked_again"
        ),
        false,
        Answer::Dismiss,
    ) else {
        return;
    };
    keyring::secret_store(LABEL, &attrs(), b"{}").unwrap();
    assert_eq!(lookup().unwrap().as_deref(), Some(&b"{}"[..]));
    assert_eq!(f.calls(), (0, 0));
}

#[test]
fn store_unlocks_locked_collection() {
    let Some(f) = fake(
        concat!(module_path!(), "::store_unlocks_locked_collection"),
        true,
        Answer::Unlock,
    ) else {
        return;
    };
    keyring::secret_store(LABEL, &attrs(), br#"{"apiKeys":{}}"#).unwrap();
    assert_eq!(f.calls(), (1, 1));
    assert!(!f.locked());
    let s = st(&f.state);
    assert_eq!(s.items.len(), 1);
    let it = &s.items[0];
    assert_eq!(
        (
            it.label.as_str(),
            it.secret.as_slice(),
            it.content_type.as_str()
        ),
        (LABEL, &br#"{"apiKeys":{}}"#[..], "text/plain")
    );
    assert_eq!(it.attrs, attrs().into_iter().collect());
}

#[test]
fn lookup_unlocks_locked_item() {
    let Some(f) = fake(
        concat!(module_path!(), "::lookup_unlocks_locked_item"),
        true,
        Answer::Unlock,
    ) else {
        return;
    };
    f.seed(r#"{"apiKeys":{"gemini":"k1"}}"#);
    assert_eq!(
        lookup().unwrap().as_deref(),
        Some(&br#"{"apiKeys":{"gemini":"k1"}}"#[..])
    );
    assert_eq!(f.calls(), (1, 1));
    f.lock();
    assert_eq!(
        keyring::try_lookup(&[keyring::APP_ATTR]),
        Lookup::Found(r#"{"apiKeys":{"gemini":"k1"}}"#.into())
    );
    assert_eq!(f.calls(), (2, 2));
}

#[test]
fn missing_entry_needs_no_prompt() {
    let Some(f) = fake(
        concat!(module_path!(), "::missing_entry_needs_no_prompt"),
        true,
        Answer::Unlock,
    ) else {
        return;
    };
    assert_eq!(lookup().unwrap(), None);
    assert_eq!(f.calls(), (0, 0));
}

#[test]
fn dismissed_prompt_fails_store_once() {
    let Some(f) = fake(
        concat!(module_path!(), "::dismissed_prompt_fails_store_once"),
        true,
        Answer::Dismiss,
    ) else {
        return;
    };
    let e = keyring::secret_store(LABEL, &attrs(), b"{}").unwrap_err();
    assert_eq!(e, keyring::Error::Dismissed.to_string());
    assert_eq!(f.calls(), (1, 1));
    assert!(f.locked());
    assert!(st(&f.state).items.is_empty());
}

#[test]
fn dismissed_prompt_fails_lookup_once() {
    let Some(f) = fake(
        concat!(module_path!(), "::dismissed_prompt_fails_lookup_once"),
        true,
        Answer::Dismiss,
    ) else {
        return;
    };
    f.seed("{}");
    assert!(matches!(lookup(), Err(keyring::Error::Dismissed)));
    assert_eq!(f.calls(), (1, 1));
    assert_eq!(keyring::try_lookup(&[keyring::APP_ATTR]), Lookup::Locked);
    assert_eq!(f.calls(), (2, 2));
}

/// A Secret Service that leaves the bus while its prompt is open sends no `Completed`: the
/// request ends with the service's name losing its owner instead of waiting for good.
#[test]
fn service_leaving_with_its_prompt_open_fails_lookup() {
    let Some(f) = fake(
        concat!(
            module_path!(),
            "::service_leaving_with_its_prompt_open_fails_lookup"
        ),
        true,
        Answer::Vanish,
    ) else {
        return;
    };
    f.seed("{}");
    let r = lookup();
    assert!(matches!(r, Err(keyring::Error::PromptLost)), "{r:?}");
    assert_eq!(f.calls(), (1, 1));
    assert!(f.locked());
}

/// libsecret (`secret-session.c`, "Other errors result in a failure") opens a `plain` session
/// only after `NotSupported`. Any other failure of the encrypted session fails the request,
/// and the secret is neither sent nor asked for in the clear.
#[test]
fn only_not_supported_falls_back_to_a_plain_session() {
    const DATA: &str = r#"{"apiKeys":{"gemini":"k1"}}"#;
    let Some(f) = fake(
        concat!(
            module_path!(),
            "::only_not_supported_falls_back_to_a_plain_session"
        ),
        false,
        Answer::Dismiss,
    ) else {
        return;
    };
    keyring::secret_store(LABEL, &attrs(), DATA.as_bytes()).unwrap();
    assert_eq!(f.sessions(), [DH, "plain"]);
    assert_eq!(lookup().unwrap().as_deref(), Some(DATA.as_bytes()));
    assert_eq!(f.sessions(), [DH, "plain"]);

    for e in [
        Encrypted::Failed,
        Encrypted::WrongType,
        Encrypted::Degenerate,
    ] {
        f.encrypted(e);
        let stored = keyring::secret_store(LABEL, &attrs(), b"not for the bus");
        assert!(stored.is_err(), "{e:?}: stored through {:?}", f.sessions());
        let found = lookup();
        match e {
            Encrypted::Degenerate => assert!(matches!(found, Err(keyring::Error::Key)), "{e:?}"),
            _ => assert!(matches!(found, Err(keyring::Error::Bus(_))), "{e:?}"),
        }
        // Not `NotFound`: KeyringStorage must not take the entry for missing and reset it.
        assert_eq!(keyring::try_lookup(&[keyring::APP_ATTR]), Lookup::Locked);
        assert_eq!(f.sessions(), [DH, DH, DH], "{e:?}");
        assert_eq!(f.secrets(), [DATA], "{e:?}");
    }
}

/// KeyringStorage with a service whose encrypted session fails: nothing is loaded, and a key
/// set then is not written over the entry.
#[test]
fn keyring_storage_without_encrypted_session_writes_nothing() {
    const DATA: &str = r#"{"apiKeys":{"gemini":"k1"}}"#;
    let Some(f) = fake(
        concat!(
            module_path!(),
            "::keyring_storage_without_encrypted_session_writes_nothing"
        ),
        false,
        Answer::Dismiss,
    ) else {
        return;
    };
    f.seed(DATA);
    f.encrypted(Encrypted::Failed);
    let (mut rt, r, host) = load();
    rt.call_method(r, "fetch", vec![]).unwrap().unwrap();
    host.settle(&mut rt);
    assert!(!rt.get(r, "loaded").truthy());
    set(&mut rt, r, "openai", "k2");
    host.settle(&mut rt);
    assert!(!rt.get(r, "loaded").truthy());
    assert_eq!(f.sessions(), [DH, DH]);
    assert_eq!(f.secrets(), [DATA]);
}

fn ri() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../qml"))
}

const QML: &str = r#"import QtQuick
import ri.services
Item {
    property bool loaded: KeyringStorage.loaded
    function fetch() { KeyringStorage.fetchKeyringData() }
    function save() { KeyringStorage.saveKeyringData() }
    function key(k) { return KeyringStorage.keyringData?.apiKeys?.[k] ?? "" }
    function set(k, v) { KeyringStorage.setNestedField(["apiKeys", k], v) }
}
"#;

fn load() -> (Runtime, usize, Host) {
    let mut rt = Runtime::new(Some(ri()));
    let f = std::env::temp_dir().join(format!("qml-keyring-prompt-{}.qml", std::process::id()));
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

impl Host {
    /// Runs the loop until every keyring request finished.
    fn settle(&self, rt: &mut Runtime) {
        self.until(rt, "the keyring requests", &|rt| {
            rt.world.borrow().keyring.as_ref().is_some_and(|k| k.idle())
        });
    }
}

fn set(rt: &mut Runtime, r: usize, key: &str, value: &str) {
    rt.call_method(r, "set", vec![Value::str(key), Value::str(value)])
        .unwrap()
        .unwrap();
}

fn key(rt: &mut Runtime, r: usize, key: &str) -> String {
    rt.call_method(r, "key", vec![Value::str(key)])
        .unwrap()
        .unwrap_or_default()
        .to_js_string()
}

/// Requests the `Keyring` singleton started so far.
fn started(rt: &Runtime) -> usize {
    rt.world
        .borrow()
        .keyring
        .as_ref()
        .map_or(0, |k| k.started())
}

/// Two API keys and the Google Cloud key, as saved by an earlier run.
const SAVED: &str = r#"{"apiKeys":{"gemini":"k1","openai":"k2"},"googleCloud":{"serviceAccountKey":{"project_id":"p"}}}"#;
/// [`SAVED`] with the `mistral` key set to `k3`.
const SAVED_AND_MISTRAL: &str = r#"{"apiKeys":{"gemini":"k1","openai":"k2","mistral":"k3"},"googleCloud":{"serviceAccountKey":{"project_id":"p"}}}"#;

/// A key set before the stored data was read (`/key …` typed first) is written on top of that
/// data: the read comes first, and the keys saved earlier stay.
#[test]
fn write_before_read_keeps_saved_keys() {
    let Some(f) = fake(
        concat!(module_path!(), "::write_before_read_keeps_saved_keys"),
        false,
        Answer::Dismiss,
    ) else {
        return;
    };
    f.seed(SAVED);
    let (mut rt, r, host) = load();
    set(&mut rt, r, "mistral", "k3");
    assert_eq!(key(&mut rt, r, "mistral"), "k3", "usable at once");
    host.settle(&mut rt);
    assert!(rt.get(r, "loaded").truthy());
    assert_eq!(f.secrets(), [SAVED_AND_MISTRAL]);
    assert_eq!(key(&mut rt, r, "gemini"), "k1");
    assert_eq!(key(&mut rt, r, "mistral"), "k3");
    assert_eq!(started(&rt), 2, "one lookup, one store");
    // Loaded: the next key is stored right away.
    set(&mut rt, r, "gemini", "k4");
    host.settle(&mut rt);
    assert_eq!(f.secrets(), [SAVED_AND_MISTRAL.replace("k1", "k4")]);
    assert_eq!(started(&rt), 3);
}

/// The same while the read is in flight: its answer is what the key goes on top of.
#[test]
fn write_while_reading_keeps_saved_keys() {
    let Some(f) = fake(
        concat!(module_path!(), "::write_while_reading_keeps_saved_keys"),
        false,
        Answer::Dismiss,
    ) else {
        return;
    };
    f.seed(SAVED);
    let (mut rt, r, host) = load();
    rt.call_method(r, "fetch", vec![]).unwrap().unwrap();
    set(&mut rt, r, "mistral", "k3");
    host.settle(&mut rt);
    assert!(rt.get(r, "loaded").truthy());
    assert_eq!(f.secrets(), [SAVED_AND_MISTRAL]);
    assert_eq!(started(&rt), 2, "one lookup, one store");
}

/// The read was cancelled at its prompt (`Locked`), then a key is set: the write asks to
/// unlock again, and writes nothing while that is refused. Once the keyring is unlocked the
/// key goes on top of the saved ones.
#[test]
fn write_after_cancelled_read_keeps_saved_keys() {
    let Some(f) = fake(
        concat!(
            module_path!(),
            "::write_after_cancelled_read_keeps_saved_keys"
        ),
        true,
        Answer::Dismiss,
    ) else {
        return;
    };
    f.seed(SAVED);
    let (mut rt, r, host) = load();
    rt.call_method(r, "fetch", vec![]).unwrap().unwrap();
    host.settle(&mut rt);
    set(&mut rt, r, "mistral", "k3");
    host.settle(&mut rt);
    assert!(!rt.get(r, "loaded").truthy());
    assert_eq!(
        f.calls(),
        (2, 2),
        "one prompt for the fetch, one for the key"
    );
    assert_eq!(f.secrets(), [SAVED]);
    assert!(f.locked());

    f.answer(Answer::Unlock);
    rt.call_method(r, "fetch", vec![]).unwrap().unwrap();
    host.settle(&mut rt);
    assert!(rt.get(r, "loaded").truthy());
    assert_eq!(f.calls(), (3, 3));
    assert_eq!(f.secrets(), [SAVED_AND_MISTRAL]);
}

/// Nothing stored yet and the `default` collection locked (the read says `Locked` without a
/// prompt): setting the first key unlocks the collection, finds it empty and stores the key.
#[test]
fn first_write_unlocks_an_empty_locked_keyring() {
    let Some(f) = fake(
        concat!(
            module_path!(),
            "::first_write_unlocks_an_empty_locked_keyring"
        ),
        true,
        Answer::Unlock,
    ) else {
        return;
    };
    let (mut rt, r, host) = load();
    rt.call_method(r, "fetch", vec![]).unwrap().unwrap();
    host.settle(&mut rt);
    assert!(!rt.get(r, "loaded").truthy());
    assert_eq!(f.calls(), (0, 0));
    set(&mut rt, r, "mistral", "k3");
    host.settle(&mut rt);
    assert!(rt.get(r, "loaded").truthy());
    assert_eq!(f.calls(), (1, 1));
    assert_eq!(f.secrets(), [r#"{"apiKeys":{"mistral":"k3"}}"#]);
}

/// The same with the key set while that read is still in flight: the read's `Locked` is not
/// the write's answer, the lookup runs again and unlocks.
#[test]
fn first_write_while_reading_unlocks_an_empty_locked_keyring() {
    let Some(f) = fake(
        concat!(
            module_path!(),
            "::first_write_while_reading_unlocks_an_empty_locked_keyring"
        ),
        true,
        Answer::Unlock,
    ) else {
        return;
    };
    let (mut rt, r, host) = load();
    rt.call_method(r, "fetch", vec![]).unwrap().unwrap();
    set(&mut rt, r, "mistral", "k3");
    host.settle(&mut rt);
    assert!(rt.get(r, "loaded").truthy());
    assert_eq!(f.calls(), (1, 1));
    assert_eq!(f.secrets(), [r#"{"apiKeys":{"mistral":"k3"}}"#]);
    assert_eq!(started(&rt), 3, "the read, the unlocking read, the store");
}

/// A service that leaves a locked collection's items out of `SearchItems`: the write unlocks
/// the collection and reads the entry it could not see, instead of replacing it.
#[test]
fn write_keeps_keys_a_locked_collection_hid() {
    let Some(f) = fake(
        concat!(module_path!(), "::write_keeps_keys_a_locked_collection_hid"),
        true,
        Answer::Unlock,
    ) else {
        return;
    };
    f.seed(SAVED);
    st(&f.state).hides_locked = true;
    let (mut rt, r, host) = load();
    rt.call_method(r, "fetch", vec![]).unwrap().unwrap();
    host.settle(&mut rt);
    assert!(!rt.get(r, "loaded").truthy());
    set(&mut rt, r, "mistral", "k3");
    host.settle(&mut rt);
    assert!(rt.get(r, "loaded").truthy());
    assert_eq!(f.calls(), (1, 1));
    assert_eq!(f.secrets(), [SAVED_AND_MISTRAL]);
}

/// KeyringStorage end to end: an unlocked prompt loads the stored data.
#[test]
fn keyring_storage_unlocks() {
    let Some(f) = fake(
        concat!(module_path!(), "::keyring_storage_unlocks"),
        true,
        Answer::Unlock,
    ) else {
        return;
    };
    f.seed(r#"{"apiKeys":{"gemini":"k1"}}"#);
    let (mut rt, r, host) = load();
    rt.call_method(r, "fetch", vec![]).unwrap().unwrap();
    host.settle(&mut rt);
    assert!(rt.get(r, "loaded").truthy());
    let k = rt
        .call_method(r, "key", vec![Value::str("gemini")])
        .unwrap();
    assert_eq!(k.unwrap_or_default().to_js_string(), "k1");
    assert_eq!(f.calls(), (1, 1));
}

/// KeyringStorage end to end: a dismissed prompt leaves it not loaded after one prompt per
/// request, with no retry behind the user's back.
#[test]
fn keyring_storage_dismissed() {
    let Some(f) = fake(
        concat!(module_path!(), "::keyring_storage_dismissed"),
        true,
        Answer::Dismiss,
    ) else {
        return;
    };
    f.seed("{}");
    let (mut rt, r, host) = load();
    rt.call_method(r, "fetch", vec![]).unwrap().unwrap();
    host.settle(&mut rt);
    rt.call_method(r, "save", vec![]).unwrap().unwrap();
    host.settle(&mut rt);
    assert!(!rt.get(r, "loaded").truthy());
    assert_eq!(
        f.calls(),
        (2, 2),
        "one prompt for the fetch, one for the save"
    );
    assert!(f.locked());
}

/// `login` unlocked, the item in the locked `default` collection, Cancel: one prompt, the
/// lookup is `Locked` (not `NotFound`), and nothing is reset or written.
#[test]
fn cancel_in_other_collection_writes_nothing() {
    const DATA: &str = r#"{"apiKeys":{"gemini":"k1"}}"#;
    let Some(f) = fake(
        concat!(
            module_path!(),
            "::cancel_in_other_collection_writes_nothing"
        ),
        true,
        Answer::Dismiss,
    ) else {
        return;
    };
    f.seed(DATA);
    let (mut rt, r, host) = load();
    rt.call_method(r, "fetch", vec![]).unwrap().unwrap();
    host.settle(&mut rt);
    assert!(!rt.get(r, "loaded").truthy());
    assert_eq!(f.calls(), (1, 1), "exactly one prompt, no save behind it");
    assert_eq!(f.secrets(), [DATA]);
    assert!(f.locked());
}

/// Cancel, then Unlock on the next fetch: the data comes back intact and is never rewritten.
#[test]
fn cancel_then_unlock_keeps_data() {
    const DATA: &str = r#"{"apiKeys":{"gemini":"k1"}}"#;
    let Some(f) = fake(
        concat!(module_path!(), "::cancel_then_unlock_keeps_data"),
        true,
        Answer::Dismiss,
    ) else {
        return;
    };
    f.seed(DATA);
    let (mut rt, r, host) = load();
    rt.call_method(r, "fetch", vec![]).unwrap().unwrap();
    host.settle(&mut rt);
    assert!(!rt.get(r, "loaded").truthy());
    f.answer(Answer::Unlock);
    rt.call_method(r, "fetch", vec![]).unwrap().unwrap();
    host.settle(&mut rt);
    assert!(rt.get(r, "loaded").truthy());
    let k = rt
        .call_method(r, "key", vec![Value::str("gemini")])
        .unwrap();
    assert_eq!(k.unwrap_or_default().to_js_string(), "k1");
    assert_eq!(f.calls(), (2, 2));
    assert_eq!(f.secrets(), [DATA]);
}

/// Nothing found while the `default` collection is locked is `Locked`: a service may hide a
/// locked collection's items. Unlocked and empty is `NotFound`.
#[test]
fn missing_entry_follows_default_collection_lock() {
    const TEST: &str = concat!(
        module_path!(),
        "::missing_entry_follows_default_collection_lock"
    );
    // Each collection state on a bus of its own, in a process of its own.
    if let Some(f) = fake(TEST, true, Answer::Dismiss) {
        assert_eq!(keyring::try_lookup(&[keyring::APP_ATTR]), Lookup::Locked);
        assert_eq!(f.calls(), (0, 0));
    }
    if let Some(f) = fake(TEST, false, Answer::Dismiss) {
        assert_eq!(keyring::try_lookup(&[keyring::APP_ATTR]), Lookup::NotFound);
        assert_eq!(f.calls(), (0, 0));
    }
}
