//! The `Keyring` singleton of the `Impulse` module (`qmltypes/Impulse/keyring.qmltypes`): the
//! Secret Service over D-Bus, the calls `secret-tool` and gnome-keyring's unlock make.
//!
//! `services/KeyringStorage.qml` looks its JSON up and stores it through `Keyring.lookup` /
//! `Keyring.store`, and LockScreen.qml unlocks the login keyring with `Keyring.unlock`. Each
//! request runs on its own worker thread and answers with a signal.

use crate::runtime::World;
use crate::value::Value;
use std::collections::HashMap;
use std::sync::mpsc::Receiver;

/// `try_lookup.sh` looks up exactly this attribute.
pub const APP_ATTR: (&str, &str) = ("application", "rusty-impulse");
const LOGIN: &str = "/org/freedesktop/secrets/collection/login";

/// `try_lookup.sh`'s three outcomes (exit 0 / 1 / 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    Found(String),
    NotFound,
    Locked,
}

const SECRETS: &str = "org.freedesktop.secrets";
const SERVICE: &str = "/org/freedesktop/secrets";

/// Why a Secret Service lookup or store failed.
#[derive(Debug)]
pub enum Error {
    /// No session bus.
    Unavailable,
    /// The unlock or create Prompt completed with `dismissed`.
    Dismissed,
    /// The bus, or the Secret Service on it, went away before the Prompt sent `Completed`.
    PromptLost,
    /// The secret did not decrypt with the session key.
    Crypto,
    /// No session key: no randomness for ours, or the service's DH value outside `2..p-1`.
    Key,
    Bus(zbus::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Unavailable => f.write_str("Secret Service unavailable"),
            Error::Dismissed => f.write_str("prompt dismissed"),
            Error::PromptLost => f.write_str("prompt lost with the Secret Service"),
            Error::Crypto => f.write_str("secret does not decrypt with the session key"),
            Error::Key => f.write_str("no usable DH session key"),
            Error::Bus(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {}

impl From<zbus::Error> for Error {
    fn from(e: zbus::Error) -> Error {
        Error::Bus(e)
    }
}

impl From<zbus::fdo::Error> for Error {
    fn from(e: zbus::fdo::Error) -> Error {
        Error::Bus(e.into())
    }
}

/// A method call on the Secret Service, its reply body deserialized as `R`.
fn call<R>(
    conn: &zbus::blocking::Connection,
    path: &str,
    iface: &str,
    member: &str,
    body: &(impl serde::Serialize + zbus::zvariant::DynamicType),
) -> Result<R, Error>
where
    R: for<'d> serde::Deserialize<'d> + zbus::zvariant::Type,
{
    let m = conn.call_method(Some(SECRETS), path, Some(iface), member, body)?;
    Ok(m.body().deserialize()?)
}

/// `Prompt.Prompt` and its `Completed (dismissed, result)`: `result` unless dismissed. The
/// signal match is in place before the prompt can complete, and so is the watch of the
/// service's name: a Secret Service that exits with its prompt open never sends `Completed`,
/// and its name losing that owner ends the wait instead.
fn run_prompt(
    conn: &zbus::blocking::Connection,
    path: &zbus::zvariant::OwnedObjectPath,
) -> Result<zbus::zvariant::OwnedValue, Error> {
    use futures_lite::StreamExt;
    zbus::block_on(async {
        let conn = conn.inner();
        let p: zbus::Proxy = zbus::proxy::Builder::new(conn)
            .destination(SECRETS)?
            .path(path.as_ref())?
            .interface("org.freedesktop.Secret.Prompt")?
            .cache_properties(zbus::proxy::CacheProperties::No)
            .build()
            .await?;
        let mut completed = p.receive_signal("Completed").await?;
        let mut owners = zbus::fdo::DBusProxy::new(conn)
            .await?
            .receive_name_owner_changed_with_args(&[(0, SECRETS)])
            .await?;
        p.call_method("Prompt", &("",)).await?;
        let owner_left = async {
            // The owner that had the name: the prompt was its object.
            while let Some(s) = owners.next().await {
                if s.args().is_ok_and(|a| a.old_owner().is_some()) {
                    break;
                }
            }
            None
        };
        let m = futures_lite::future::or(completed.next(), owner_left)
            .await
            .ok_or(Error::PromptLost)?;
        let (dismissed, result): (bool, zbus::zvariant::OwnedValue) = m.body().deserialize()?;
        if dismissed {
            return Err(Error::Dismissed);
        }
        Ok(result)
    })
}

/// `Service.Unlock` of one object, following its Prompt when the service asks for one.
fn unlock(
    conn: &zbus::blocking::Connection,
    path: &zbus::zvariant::OwnedObjectPath,
) -> Result<(), Error> {
    let (_, prompt): (
        Vec<zbus::zvariant::OwnedObjectPath>,
        zbus::zvariant::OwnedObjectPath,
    ) = call(
        conn,
        SERVICE,
        "org.freedesktop.Secret.Service",
        "Unlock",
        &(vec![path.as_ref()],),
    )?;
    if prompt.as_str() != "/" {
        run_prompt(conn, &prompt)?;
    }
    Ok(())
}

/// The collection behind the `default` alias, `None` when the alias is unset.
fn default_collection(
    conn: &zbus::blocking::Connection,
) -> Result<Option<zbus::zvariant::OwnedObjectPath>, Error> {
    let c: zbus::zvariant::OwnedObjectPath = call(
        conn,
        SERVICE,
        "org.freedesktop.Secret.Service",
        "ReadAlias",
        &("default",),
    )?;
    Ok((c.as_str() != "/").then_some(c))
}

/// A collection's `Locked` property.
fn locked(conn: &zbus::blocking::Connection, path: &str) -> Result<bool, Error> {
    let p = zbus::blocking::fdo::PropertiesProxy::builder(conn)
        .destination(SECRETS)?
        .path(path)?
        .build()?;
    let iface =
        zbus::names::InterfaceName::from_static_str_unchecked("org.freedesktop.Secret.Collection");
    Ok(bool::try_from(p.get(iface, "Locked")?).map_err(zbus::Error::from)?)
}

/// `secret-tool lookup KEY VALUE…`: the first unlocked match, else the first locked one after
/// `Service.Unlock` (following its Prompt to `Completed`); `Ok(None)` when nothing matches. The
/// session the secret crosses the bus in is closed before returning.
pub fn secret_lookup(attrs: &[(&str, &str)]) -> Result<Option<Vec<u8>>, Error> {
    let conn = crate::dbus::session().ok_or(Error::Unavailable)?;
    let a: HashMap<&str, &str> = attrs.iter().copied().collect();
    type Paths = Vec<zbus::zvariant::OwnedObjectPath>;
    let (unlocked, locked): (Paths, Paths) = call(
        &conn,
        SERVICE,
        "org.freedesktop.Secret.Service",
        "SearchItems",
        &(a,),
    )?;
    let item = match (unlocked.first(), locked.first()) {
        (Some(it), _) => it,
        (None, Some(it)) => {
            unlock(&conn, it)?;
            it
        }
        (None, None) => return Ok(None),
    };
    let session = Session::open(&conn)?;
    let (_, parameters, value, _): (zbus::zvariant::OwnedObjectPath, Vec<u8>, Vec<u8>, String) =
        call(
            &conn,
            item.as_str(),
            "org.freedesktop.Secret.Item",
            "GetSecret",
            &(session.path.as_ref(),),
        )?;
    session.decrypt(&parameters, &value).map(Some)
}

/// `is_unlocked.sh`: the login collection's `Locked` property is `false`.
pub fn login_unlocked() -> bool {
    let Some(conn) = crate::dbus::session() else {
        return false;
    };
    login_locked(&conn) == Some(false)
}

fn login_locked(conn: &zbus::blocking::Connection) -> Option<bool> {
    locked(conn, LOGIN).ok()
}

/// `try_lookup.sh`'s outcome, decided by what the Secret Service answered rather than by the
/// login collection's lock: an item that lives in another collection (the `default` alias) is
/// not "missing" because `login` happens to be unlocked. A failed or dismissed unlock is
/// `Locked`, so `apply_lookup` never resets (and overwrites) data it could not read.
pub fn try_lookup(attrs: &[(&str, &str)]) -> Lookup {
    lookup_outcome(secret_lookup(attrs))
}

/// [`try_lookup`] for a caller that is about to write: when nothing matches while the `default`
/// collection is locked, unlocks it (following its Prompt) and searches again, so that the
/// answer is `Found` or `NotFound` and the write goes on top of what is stored. A failed or
/// dismissed unlock is `Locked`.
pub fn try_lookup_unlocking(attrs: &[(&str, &str)]) -> Lookup {
    lookup_outcome(secret_lookup(attrs).and_then(|found| match found {
        None if unlock_default()? => secret_lookup(attrs),
        found => Ok(found),
    }))
}

/// Unlocks the `default` collection when it is locked: whether it was.
fn unlock_default() -> Result<bool, Error> {
    let conn = crate::dbus::session().ok_or(Error::Unavailable)?;
    let Some(col) = default_collection(&conn)? else {
        return Ok(false);
    };
    if !locked(&conn, col.as_str())? {
        return Ok(false);
    }
    unlock(&conn, &col)?;
    Ok(true)
}

fn lookup_outcome(found: Result<Option<Vec<u8>>, Error>) -> Lookup {
    match found {
        Ok(Some(v)) => Lookup::Found(
            String::from_utf8_lossy(&v)
                .trim_end_matches('\n')
                .to_string(),
        ),
        Ok(None) if default_locked() => Lookup::Locked,
        Ok(None) => Lookup::NotFound,
        Err(e) => {
            elog::eprintln!("Keyring: lookup failed: {e}");
            Lookup::Locked
        }
    }
}

/// Whether the `default` alias, where `secret_store` writes, is locked: a service that hides a
/// locked collection's items cannot tell missing from locked. Unknown counts as locked.
fn default_locked() -> bool {
    let Some(conn) = crate::dbus::session() else {
        return true;
    };
    match default_collection(&conn) {
        Ok(Some(c)) => locked(&conn, c.as_str()).unwrap_or(true),
        Ok(None) => false,
        Err(_) => true,
    }
}

/// `secret-tool store --label=LABEL KEY VALUE…` with `secret` on stdin: replaces the matching
/// item in the `default` collection (created as "Default keyring" when missing, unlocked when
/// locked), content type `text/plain`. The session the secret crosses the bus in is closed
/// before returning.
pub fn secret_store(label: &str, attrs: &[(String, String)], secret: &[u8]) -> Result<(), String> {
    store_item(label, attrs, secret).map_err(|e| e.to_string())
}

fn store_item(label: &str, attrs: &[(String, String)], secret: &[u8]) -> Result<(), Error> {
    use zbus::zvariant::{OwnedObjectPath, Value};
    let conn = crate::dbus::session().ok_or(Error::Unavailable)?;
    let col = match default_collection(&conn)? {
        Some(c) => c,
        None => {
            let label = HashMap::from([(
                "org.freedesktop.Secret.Collection.Label",
                Value::from("Default keyring"),
            )]);
            let (c, prompt): (OwnedObjectPath, OwnedObjectPath) = call(
                &conn,
                SERVICE,
                "org.freedesktop.Secret.Service",
                "CreateCollection",
                &(label, "default"),
            )?;
            if c.as_str() != "/" {
                c
            } else {
                OwnedObjectPath::try_from(run_prompt(&conn, &prompt)?).map_err(zbus::Error::from)?
            }
        }
    };
    if locked(&conn, col.as_str())? {
        unlock(&conn, &col)?;
    }
    let a: HashMap<&str, &str> = attrs
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let props = HashMap::from([
        ("org.freedesktop.Secret.Item.Label", Value::from(label)),
        ("org.freedesktop.Secret.Item.Attributes", Value::from(a)),
    ]);
    let session = Session::open(&conn)?;
    let (parameters, value) = session.encrypt(secret);
    let item = (session.path.as_ref(), parameters, value, "text/plain");
    let (created, prompt): (OwnedObjectPath, OwnedObjectPath) = call(
        &conn,
        col.as_str(),
        "org.freedesktop.Secret.Collection",
        "CreateItem",
        &(props, item, true),
    )?;
    if created.as_str() == "/" {
        run_prompt(&conn, &prompt)?;
    }
    Ok(())
}

/// [`unlock_login`]'s exit code: the login keyring was unlocked (or created) with the password.
pub const UNLOCKED: i32 = 0;
/// `unlock.sh`'s exit 1: the login keyring was already unlocked; nothing changed.
pub const ALREADY_UNLOCKED: i32 = 1;
/// The keyring stays locked: no Secret Service, no encrypted session, or gnome-keyring
/// answered the request with an error (a wrong password).
pub const UNLOCK_FAILED: i32 = 2;

/// `unlock.sh`, as its exit code: [`ALREADY_UNLOCKED`] when the login keyring is unlocked;
/// otherwise unlocks it with `password` — gnome-keyring's `UnlockWithMasterPassword`, or
/// `CreateWithMasterPassword` when there is no login keyring yet, as
/// `gnome-keyring-daemon --login` does — without restarting the daemon: [`UNLOCKED`], or
/// [`UNLOCK_FAILED`] when that call returned an error.
pub fn unlock_login(password: &str) -> i32 {
    let Some(conn) = crate::dbus::session() else {
        return UNLOCK_FAILED;
    };
    let locked = login_locked(&conn);
    if locked == Some(false) {
        return ALREADY_UNLOCKED;
    }
    // The password never crosses the bus in the clear: no fallback to a `plain` session.
    let session = match Session::open_dh(&conn) {
        Ok(s) => s,
        Err(e) => {
            elog::eprintln!("KeyringStorage: cannot open an encrypted Secret Service session: {e}");
            return UNLOCK_FAILED;
        }
    };
    let (iv, value) = session.encrypt(password.as_bytes());
    let secret = (session.path.as_ref(), iv, value, "text/plain");
    let internal = |member: &str| {
        (
            Some("org.freedesktop.secrets"),
            "/org/freedesktop/secrets",
            Some("org.gnome.keyring.InternalUnsupportedGuiltRiddenInterface"),
            member.to_string(),
        )
    };
    let r = if locked.is_some() {
        let (d, path, i, m) = internal("UnlockWithMasterPassword");
        let login = zbus::zvariant::ObjectPath::from_static_str_unchecked(LOGIN);
        conn.call_method(d, path, i, m.as_str(), &(login, secret))
    } else {
        let (d, path, i, m) = internal("CreateWithMasterPassword");
        let mut props: HashMap<&str, zbus::zvariant::Value> = HashMap::new();
        // gnome-keyring names the file and object path after the label; "login" like `--login`.
        props.insert("org.freedesktop.Secret.Collection.Label", "login".into());
        conn.call_method(d, path, i, m.as_str(), &(props, secret))
    };
    drop(session);
    match r {
        Ok(_) => UNLOCKED,
        Err(e) => {
            elog::eprintln!("KeyringStorage: unlock failed: {e}");
            UNLOCK_FAILED
        }
    }
}

/// RFC 2409 §6.2 Second Oakley Group (1024-bit MODP), generator 2: the group of
/// `dh-ietf1024-sha256-aes128-cbc-pkcs7`.
const DH_PRIME: [u8; 128] = [
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xC9, 0x0F, 0xDA, 0xA2, 0x21, 0x68, 0xC2, 0x34,
    0xC4, 0xC6, 0x62, 0x8B, 0x80, 0xDC, 0x1C, 0xD1, 0x29, 0x02, 0x4E, 0x08, 0x8A, 0x67, 0xCC, 0x74,
    0x02, 0x0B, 0xBE, 0xA6, 0x3B, 0x13, 0x9B, 0x22, 0x51, 0x4A, 0x08, 0x79, 0x8E, 0x34, 0x04, 0xDD,
    0xEF, 0x95, 0x19, 0xB3, 0xCD, 0x3A, 0x43, 0x1B, 0x30, 0x2B, 0x0A, 0x6D, 0xF2, 0x5F, 0x14, 0x37,
    0x4F, 0xE1, 0x35, 0x6D, 0x6D, 0x51, 0xC2, 0x45, 0xE4, 0x85, 0xB5, 0x76, 0x62, 0x5E, 0x7E, 0xC6,
    0xF4, 0x4C, 0x42, 0xE9, 0xA6, 0x37, 0xED, 0x6B, 0x0B, 0xFF, 0x5C, 0xB6, 0xF4, 0x06, 0xB7, 0xED,
    0xEE, 0x38, 0x6B, 0xFB, 0x5A, 0x89, 0x9F, 0xA5, 0xAE, 0x9F, 0x24, 0x11, 0x7C, 0x4B, 0x1F, 0xE6,
    0x49, 0x28, 0x66, 0x51, 0xEC, 0xE6, 0x53, 0x81, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
];

/// A private exponent and its public value `2^x mod p`.
fn dh_keypair() -> Option<(num_bigint::BigUint, num_bigint::BigUint)> {
    let mut x = [0u8; 128];
    getrandom::fill(&mut x).ok()?;
    let x = num_bigint::BigUint::from_bytes_be(&x);
    let p = num_bigint::BigUint::from_bytes_be(&DH_PRIME);
    let public = num_bigint::BigUint::from(2u32).modpow(&x, &p);
    Some((x, public))
}

/// The session's AES-128 key: HKDF-SHA256 (no salt, empty info) over the shared secret, padded
/// to the prime's 128 bytes. `None` for a peer value outside `2..p-1` (a degenerate key).
fn dh_key(private: &num_bigint::BigUint, peer: &[u8]) -> Option<[u8; 16]> {
    let p = num_bigint::BigUint::from_bytes_be(&DH_PRIME);
    let y = num_bigint::BigUint::from_bytes_be(peer);
    let one = num_bigint::BigUint::from(1u32);
    if y <= one || y >= &p - &one {
        return None;
    }
    let s = y.modpow(private, &p).to_bytes_be();
    let mut ikm = [0u8; 128];
    ikm[128 - s.len()..].copy_from_slice(&s);
    let mut key = [0u8; 16];
    hkdf::Hkdf::<sha2::Sha256>::new(None, &ikm)
        .expand(&[], &mut key)
        .ok()?;
    Some(key)
}

fn aes_encrypt(key: &[u8; 16], iv: &[u8; 16], data: &[u8]) -> Vec<u8> {
    use aes::cipher::{BlockModeEncrypt, KeyIvInit, block_padding::Pkcs7};
    cbc::Encryptor::<aes::Aes128>::new(key.into(), iv.into()).encrypt_padded_vec::<Pkcs7>(data)
}

fn aes_decrypt(key: &[u8; 16], iv: &[u8; 16], data: &[u8]) -> Option<Vec<u8>> {
    use aes::cipher::{BlockModeDecrypt, KeyIvInit, block_padding::Pkcs7};
    cbc::Decryptor::<aes::Aes128>::new(key.into(), iv.into())
        .decrypt_padded_vec::<Pkcs7>(data)
        .ok()
}

/// A Secret Service session (Secret Service API, "Transferring Secrets"). Negotiated with
/// `dh-ietf1024-sha256-aes128-cbc-pkcs7`, a secret's value crosses the bus AES-128-CBC
/// encrypted with the key both ends derive, the random IV as its `parameters`; with `plain`
/// (`key` `None`) as is. Dropping it calls `Session.Close`: the service frees a session only
/// then or when the client leaves the bus, and the session connection lives as long as the
/// process.
struct Session {
    conn: zbus::blocking::Connection,
    path: zbus::zvariant::OwnedObjectPath,
    key: Option<[u8; 16]>,
}

impl Session {
    /// libsecret's negotiation (`secret-session.c`, `_secret_service_open_session_sync`):
    /// `dh-ietf1024-sha256-aes128-cbc-pkcs7`, and `plain` only when the service answers that
    /// with `org.freedesktop.DBus.Error.NotSupported`. Any other failure of the encrypted
    /// session (another error, a reply of the wrong type, a degenerate DH value) fails the
    /// request: the secret does not cross the bus in the clear because of it.
    fn open(conn: &zbus::blocking::Connection) -> Result<Session, Error> {
        match Session::open_dh(conn) {
            Err(Error::Bus(zbus::Error::MethodError(name, ..)))
                if name.as_str() == "org.freedesktop.DBus.Error.NotSupported" =>
            {
                Session::open_plain(conn)
            }
            r => r,
        }
    }

    fn open_dh(conn: &zbus::blocking::Connection) -> Result<Session, Error> {
        let (private, public) = dh_keypair().ok_or(Error::Key)?;
        let (output, mut session) = Session::negotiate(
            conn,
            "dh-ietf1024-sha256-aes128-cbc-pkcs7",
            public.to_bytes_be().into(),
        )?;
        // Owned before the key is checked, so a degenerate peer value still closes it.
        let peer = Vec::<u8>::try_from(output).map_err(zbus::Error::Variant)?;
        session.key = Some(dh_key(&private, &peer).ok_or(Error::Key)?);
        Ok(session)
    }

    fn open_plain(conn: &zbus::blocking::Connection) -> Result<Session, Error> {
        Session::negotiate(conn, "plain", "".into()).map(|(_, s)| s)
    }

    /// `Service.OpenSession`: the service's `output` and the session, its key not yet set.
    fn negotiate(
        conn: &zbus::blocking::Connection,
        algorithm: &str,
        input: zbus::zvariant::Value<'_>,
    ) -> Result<(zbus::zvariant::OwnedValue, Session), Error> {
        let (output, path) = call::<(zbus::zvariant::OwnedValue, zbus::zvariant::OwnedObjectPath)>(
            conn,
            SERVICE,
            "org.freedesktop.Secret.Service",
            "OpenSession",
            &(algorithm, input),
        )?;
        let session = Session {
            conn: conn.clone(),
            path,
            key: None,
        };
        Ok((output, session))
    }

    /// `(parameters, value)` of a `Secret` carrying `data`.
    fn encrypt(&self, data: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let Some(key) = &self.key else {
            return (Vec::new(), data.to_vec());
        };
        let mut iv = [0u8; 16];
        getrandom::fill(&mut iv).expect("platform RNG failed");
        (iv.to_vec(), aes_encrypt(key, &iv, data))
    }

    /// The data a `Secret`'s `(parameters, value)` carries.
    fn decrypt(&self, parameters: &[u8], value: &[u8]) -> Result<Vec<u8>, Error> {
        let Some(key) = &self.key else {
            return Ok(value.to_vec());
        };
        let iv: &[u8; 16] = parameters.try_into().map_err(|_| Error::Crypto)?;
        aes_decrypt(key, iv, value).ok_or(Error::Crypto)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let r = self.conn.call_method(
            Some(SECRETS),
            &self.path,
            Some("org.freedesktop.Secret.Session"),
            "Close",
            &(),
        );
        if let Err(e) = r {
            elog::eprintln!("KeyringStorage: Session.Close failed: {e}");
        }
    }
}

enum Done {
    Lookup(Lookup),
    Stored,
    /// [`unlock_login`]'s exit code.
    Unlocked(i32),
}

/// `Keyring.store`'s arguments: label, attributes, secret.
type Store = (String, Vec<(String, String)>, String);

/// The `Keyring` singleton's requests (`Impulse/keyring.qmltypes`).
pub struct Keyring {
    tx: crate::svc::Sender<Done>,
    rx: Receiver<Done>,
    looking: bool,
    /// The lookup in flight may unlock the `default` collection ([`try_lookup_unlocking`]).
    lookup_unlocks: bool,
    /// A `lookup` that may unlock it, called while one that may not was in flight: it runs
    /// when that one answers `Locked`.
    unlocking_lookup: Option<Vec<(String, String)>>,
    unlocking: bool,
    storing: bool,
    /// The newest `store` called while one was in flight: it runs next.
    next_store: Option<Store>,
    /// Requests started so far.
    started: usize,
}

impl Keyring {
    /// The `lookup`, `store` and `unlock` requests started so far.
    pub fn started(&self) -> usize {
        self.started
    }

    /// No request in flight or queued.
    pub fn idle(&self) -> bool {
        !self.looking && !self.unlocking && !self.storing
    }
}

fn state(w: &mut World) -> &mut Keyring {
    let h = w.waker.handle(crate::svc::Svc::Keyring);
    w.keyring.get_or_insert_with(|| {
        let (tx, rx) = h.channel();
        Keyring {
            tx,
            rx,
            looking: false,
            lookup_unlocks: false,
            unlocking_lookup: None,
            unlocking: false,
            storing: false,
            next_store: None,
            started: 0,
        }
    })
}

/// Runs `f` on a thread of its own and answers with what it returns. When the system refuses
/// the thread the request ends at once with `refused`, the answer of a request that failed, so
/// its signal still comes and the next request starts.
fn spawn(
    f: impl FnOnce() -> Done + Send + 'static,
    tx: crate::svc::Sender<Done>,
    what: &str,
    refused: Done,
) {
    let t = tx.clone();
    let r = std::thread::Builder::new()
        .name("ri-keyring".into())
        .spawn(move || {
            let _ = t.send(f());
        });
    if let Err(e) = r {
        elog::eprintln!("Keyring: cannot start the thread for {what}: {e}");
        let _ = tx.send(refused);
    }
}

fn string_pairs(v: &Value) -> Vec<(String, String)> {
    let Value::Obj(o) = v else { return vec![] };
    let o = o.borrow();
    o.keys
        .iter()
        .filter_map(|k| o.props.get(k).map(|v| (k.to_string(), v.to_js_string())))
        .collect()
}

fn with_state(it: &mut crate::eval::Interp, f: impl FnOnce(&mut Keyring)) {
    if let Some(w) = crate::runtime::world_of(it) {
        f(state(&mut w.borrow_mut()));
    }
}

/// [`crate::impulse::hooked`] for `method` of the singleton: under a spawn hook no Secret
/// Service is called.
fn hooked(method: &str, args: &[String]) -> bool {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    crate::impulse::hooked("Keyring", method, &args)
}

/// Attributes as the hook is told them: `name=value`.
fn shown(attrs: &[(String, String)]) -> Vec<String> {
    attrs.iter().map(|(k, v)| format!("{k}={v}")).collect()
}

fn arg_string(a: &[Value], i: usize) -> String {
    a.get(i).map(|v| v.to_js_string()).unwrap_or_default()
}

fn run_lookup(k: &mut Keyring, attrs: Vec<(String, String)>, unlock: bool) {
    k.looking = true;
    k.lookup_unlocks = unlock;
    k.started += 1;
    spawn(
        move || {
            let attrs: Vec<_> = attrs
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            Done::Lookup(if unlock {
                try_lookup_unlocking(&attrs)
            } else {
                try_lookup(&attrs)
            })
        },
        k.tx.clone(),
        "lookup",
        // As a lookup that failed: nothing was read, so nothing is reset.
        Done::Lookup(Lookup::Locked),
    );
}

fn run_store(k: &mut Keyring, (label, attrs, secret): Store) {
    k.storing = true;
    k.started += 1;
    spawn(
        move || {
            if let Err(e) = secret_store(&label, &attrs, secret.as_bytes()) {
                elog::eprintln!("Keyring: store failed: {e}");
            }
            Done::Stored
        },
        k.tx.clone(),
        "store",
        Done::Stored,
    );
}

/// The methods of the `Keyring` singleton. Each answers with its signal once the Secret
/// Service did; a `lookup` or `unlock` called while one is in flight is that one. `lookup`'s
/// second argument, `unlock`, is for a caller about to write ([`try_lookup_unlocking`]).
const METHODS: &[(&str, crate::eval::NativeFn)] = &[
    ("lookup", |it, _, a| {
        let attrs = string_pairs(a.first().unwrap_or(&Value::Undefined));
        let unlock = a.get(1).is_some_and(Value::truthy);
        let sandboxed = hooked("lookup", &shown(&attrs));
        with_state(it, |k| {
            if sandboxed {
                // No Secret Service is asked: the answer of one that has no such item.
                if !std::mem::replace(&mut k.looking, true) {
                    k.started += 1;
                    let _ = k.tx.send(Done::Lookup(Lookup::NotFound));
                }
            } else if !k.looking {
                run_lookup(k, attrs, unlock);
            } else if unlock && !k.lookup_unlocks {
                k.unlocking_lookup = Some(attrs);
            }
        });
        Ok(Value::Undefined)
    }),
    ("store", |it, _, a| {
        let job = (
            arg_string(a, 0),
            string_pairs(a.get(1).unwrap_or(&Value::Undefined)),
            arg_string(a, 2),
        );
        // The secret is not for the hook's log.
        let mut told = vec![job.0.clone()];
        told.extend(shown(&job.1));
        if hooked("store", &told) {
            return Ok(Value::Undefined);
        }
        with_state(it, |k| {
            if k.storing {
                k.next_store = Some(job);
            } else {
                run_store(k, job);
            }
        });
        Ok(Value::Undefined)
    }),
    ("unlock", |it, _, a| {
        let pw = arg_string(a, 0);
        // The password is not for the hook's log.
        let sandboxed = hooked("unlock", &[]);
        with_state(it, |k| {
            if !std::mem::replace(&mut k.unlocking, true) {
                k.started += 1;
                if sandboxed {
                    let _ = k.tx.send(Done::Unlocked(UNLOCK_FAILED));
                    return;
                }
                spawn(
                    move || Done::Unlocked(unlock_login(&pw)),
                    k.tx.clone(),
                    "unlock",
                    Done::Unlocked(UNLOCK_FAILED),
                );
            }
        });
        Ok(Value::Undefined)
    }),
];

pub static TYPE: crate::impulse::Type = crate::impulse::Type {
    name: "Keyring",
    methods: METHODS,
    init: None,
};

/// `Keyring.LookupResult`.
fn lookup_result(r: Lookup) -> (f64, String) {
    match r {
        Lookup::Found(s) => (0.0, s),
        Lookup::NotFound => (1.0, String::new()),
        Lookup::Locked => (2.0, String::new()),
    }
}

/// Finished requests as the `Keyring` singleton's signals.
pub(crate) fn poll(w: &mut World) -> bool {
    let Some(k) = w.keyring.as_mut() else {
        return false;
    };
    let done: Vec<Done> = k.rx.try_iter().collect();
    let changed = !done.is_empty();
    let me = w.singletons.get("Keyring").copied();
    for d in done {
        let k = state(w);
        match d {
            Done::Lookup(r) => {
                k.looking = false;
                if let Some(attrs) = k.unlocking_lookup.take()
                    && r == Lookup::Locked
                {
                    run_lookup(k, attrs, true);
                    continue;
                }
                let (code, secret) = lookup_result(r);
                if let Some(me) = me {
                    crate::native::emit(
                        w,
                        me,
                        "lookupFinished",
                        &["result", "secret"],
                        vec![Value::Num(code), Value::str(&secret)],
                    );
                }
            }
            Done::Stored => {
                k.storing = false;
                if let Some(job) = k.next_store.take() {
                    run_store(k, job);
                }
            }
            Done::Unlocked(code) => {
                k.unlocking = false;
                if let Some(me) = me {
                    crate::native::emit(
                        w,
                        me,
                        "unlockFinished",
                        &["result"],
                        vec![Value::Num(f64::from(code))],
                    );
                }
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A request whose thread the system refuses ends at once as a failed one: its signal
    /// comes, the refusal is in the log, and the next request of its kind starts instead of
    /// being taken for the one "in flight". In a process of its own: the test reads what that
    /// process wrote to its stderr.
    #[test]
    fn a_refused_thread_ends_the_request_with_its_signal() {
        const TEST: &str = concat!(
            module_path!(),
            "::a_refused_thread_ends_the_request_with_its_signal"
        );
        if !crate::testenv::in_own_process(TEST) {
            crate::testenv::run_in_own_process(TEST, [("LC_ALL", "C")]);
            return;
        }
        let mut rt = crate::runtime::Runtime::new(None);
        let r = rt
            .load_str(
                "import QtQuick\nimport Impulse\nItem { id: r\n\
                 property var log: []\n\
                 function go() { Keyring.lookup({ application: 'a' }); Keyring.store('l', { application: 'a' }, 's'); Keyring.unlock('pw') }\n\
                 Connections { target: Keyring\n\
                   function onLookupFinished(result, secret) { r.log = [...r.log, 'lookup:' + result + ':' + secret] }\n\
                   function onUnlockFinished(result) { r.log = [...r.log, 'unlock:' + result] } } }",
            )
            .unwrap();
        rt.settle();
        crate::testenv::refuse_new_threads();
        let text = crate::testenv::stderr_of(|| {
            for round in 1..=2 {
                let called = rt.call_method(r, "go", vec![]);
                assert!(matches!(called, Some(Ok(_))), "{called:?}");
                rt.poll_native();
                rt.settle();
                let k = rt.world.borrow();
                let k = k.keyring.as_ref().expect("Keyring was used");
                assert!(k.idle(), "a request is still in flight");
                assert_eq!(k.started(), 3 * round);
            }
        });
        assert_eq!(
            rt.get(r, "log").to_js_string(),
            "lookup:2:,unlock:2,lookup:2:,unlock:2"
        );
        for what in ["lookup", "store", "unlock"] {
            let line = format!("Keyring: cannot start the thread for {what}:");
            assert_eq!(text.matches(&line).count(), 2, "stderr: {text:?}");
        }
        assert_eq!(rt.errors(), Vec::<String>::new());
    }

    /// Both ends of the exchange derive the same key from each other's public value, and what
    /// one encrypts the other decrypts (as gnome-keyring's server side does).
    #[test]
    fn dh_session_key_agrees_and_decrypts() {
        let (a, pub_a) = dh_keypair().unwrap();
        let (b, pub_b) = dh_keypair().unwrap();
        let ka = dh_key(&a, &pub_b.to_bytes_be()).unwrap();
        let kb = dh_key(&b, &pub_a.to_bytes_be()).unwrap();
        assert_eq!(ka, kb);
        let iv = [7u8; 16];
        let ct = aes_encrypt(&ka, &iv, b"hunter2");
        assert_eq!(ct.len(), 16, "PKCS#7 pads to one block");
        assert_ne!(&ct[..7], b"hunter2");
        use aes::cipher::{BlockModeDecrypt, KeyIvInit, block_padding::Pkcs7};
        let pt = cbc::Decryptor::<aes::Aes128>::new((&kb).into(), (&iv).into())
            .decrypt_padded_vec::<Pkcs7>(&ct)
            .unwrap();
        assert_eq!(pt, b"hunter2");
    }

    /// Degenerate peer values (which would make the shared secret predictable) are refused.
    #[test]
    fn dh_rejects_degenerate_peer() {
        let (a, _) = dh_keypair().unwrap();
        let p = num_bigint::BigUint::from_bytes_be(&DH_PRIME);
        let one = num_bigint::BigUint::from(1u32);
        for y in [
            num_bigint::BigUint::from(0u32),
            one.clone(),
            &p - &one,
            p.clone(),
        ] {
            assert!(dh_key(&a, &y.to_bytes_be()).is_none(), "{y}");
        }
    }

    /// HKDF-SHA256 with no salt and empty info (RFC 5869 test case 3, truncated to 16 bytes).
    #[test]
    fn hkdf_matches_rfc5869() {
        let mut okm = [0u8; 16];
        hkdf::Hkdf::<sha2::Sha256>::new(None, &[0x0b; 22])
            .expand(&[], &mut okm)
            .unwrap();
        assert_eq!(
            okm,
            [
                0x8d, 0xa4, 0xe7, 0x75, 0xa5, 0x63, 0xc1, 0x8f, 0x71, 0x5f, 0x80, 0x2a, 0x06, 0x3c,
                0x5a, 0x31
            ]
        );
    }
}
