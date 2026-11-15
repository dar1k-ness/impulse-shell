#![cfg(feature = "interp")]
//! Equivalence of `qml::jobs_ai::{translate, categorize}` with `scripts/ai/gemini-translate.sh`
//! (+ `manage-translations.sh`) and `gemini-categorize-wallpaper.sh`: the original runs with
//! its Gemini URL pointed at a local server, a private D-Bus with fake Secret Service and
//! Notifications, jq 1.8 (`~/deps/jq/bin`) and `magick` (→ `convert` if only IM6 is there).
use qml::jobs_ai::{categorize, translate};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

/// The original's scripts, which the shell no longer ships.
const REF: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/original-unshipped"
);
const GEMINI: &str = "https://generativelanguage.googleapis.com";
const SECRET: &str = r#"{"apiKeys":{"gemini":"sekrit-key"}}"#;
const ITEM: &str = "/org/freedesktop/secrets/collection/login/1";

#[derive(Clone, Debug, PartialEq)]
struct Seen {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

struct Server {
    port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
}

fn server(reply: &'static str) -> Server {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s2 = seen.clone();
    std::thread::spawn(move || {
        for c in l.incoming() {
            let Ok(mut c) = c else { continue };
            let mut r = BufReader::new(c.try_clone().unwrap());
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            let mut it = line.split_whitespace();
            let (method, path) = (
                it.next().unwrap_or("").to_string(),
                it.next().unwrap_or("").to_string(),
            );
            let mut headers = Vec::new();
            loop {
                let mut l = String::new();
                r.read_line(&mut l).unwrap();
                let l = l.trim_end_matches(['\r', '\n']);
                if l.is_empty() {
                    break;
                }
                let (k, v) = l.split_once(':').unwrap();
                headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
            }
            let hv = |k: &str| headers.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
            if hv("expect").is_some() {
                c.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").unwrap();
            }
            let n: usize = hv("content-length")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let mut body = vec![0; n];
            r.read_exact(&mut body).unwrap();
            headers.retain(|(k, _)| {
                ![
                    "user-agent",
                    "accept",
                    "accept-encoding",
                    "host",
                    "expect",
                    "connection",
                ]
                .contains(&k.as_str())
            });
            headers.sort();
            s2.lock().unwrap().push(Seen {
                method,
                path,
                headers,
                body,
            });
            let _ = c.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{reply}",
                    reply.len()
                )
                .as_bytes(),
            );
        }
    });
    Server { port, seen }
}

struct Notif(Arc<Mutex<Vec<String>>>);

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Notif {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app: String,
        id: u32,
        icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        timeout: i32,
    ) -> u32 {
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
        self.0.lock().unwrap().push(format!(
            "{app}|{id}|{icon}|{summary}|{body}|{actions:?}|{h:?}|{timeout}"
        ));
        1
    }
    fn get_server_information(&self) -> (String, String, String, String) {
        ("fake".into(), "test".into(), "1".into(), "1.2".into())
    }
    fn get_capabilities(&self) -> Vec<String> {
        vec!["body".into()]
    }
}

fn op(p: &str) -> OwnedObjectPath {
    OwnedObjectPath::try_from(p).unwrap()
}

type SecretT = (OwnedObjectPath, Vec<u8>, Vec<u8>, String);

fn secret_value() -> SecretT {
    (
        op("/org/freedesktop/secrets/session/1"),
        vec![],
        SECRET.as_bytes().to_vec(),
        "text/plain".into(),
    )
}

struct Svc;

#[zbus::interface(name = "org.freedesktop.Secret.Service")]
impl Svc {
    fn open_session(
        &self,
        algorithm: String,
        _input: OwnedValue,
    ) -> zbus::fdo::Result<(OwnedValue, OwnedObjectPath)> {
        if algorithm != "plain" {
            return Err(zbus::fdo::Error::NotSupported("plain only".into()));
        }
        let v = zbus::zvariant::Value::from("").try_to_owned().unwrap();
        Ok((v, op("/org/freedesktop/secrets/session/1")))
    }
    fn search_items(
        &self,
        attributes: HashMap<String, String>,
    ) -> (Vec<OwnedObjectPath>, Vec<OwnedObjectPath>) {
        if attributes.get("application").map(String::as_str) == Some("impulse-shell") {
            (vec![op(ITEM)], vec![])
        } else {
            (vec![], vec![])
        }
    }
    fn unlock(&self, objects: Vec<OwnedObjectPath>) -> (Vec<OwnedObjectPath>, OwnedObjectPath) {
        (objects, op("/"))
    }
    fn get_secrets(
        &self,
        items: Vec<OwnedObjectPath>,
        _session: OwnedObjectPath,
    ) -> HashMap<OwnedObjectPath, SecretT> {
        items.into_iter().map(|i| (i, secret_value())).collect()
    }
    #[zbus(property)]
    fn collections(&self) -> Vec<OwnedObjectPath> {
        vec![op("/org/freedesktop/secrets/collection/login")]
    }
}

struct Item;

#[zbus::interface(name = "org.freedesktop.Secret.Item")]
impl Item {
    fn get_secret(&self, _session: OwnedObjectPath) -> SecretT {
        secret_value()
    }
    #[zbus(property)]
    fn locked(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn attributes(&self) -> HashMap<String, String> {
        HashMap::from([("application".into(), "impulse-shell".into())])
    }
    #[zbus(property)]
    fn label(&self) -> String {
        "impulse-shell".into()
    }
    #[zbus(property)]
    fn created(&self) -> u64 {
        0
    }
    #[zbus(property)]
    fn modified(&self) -> u64 {
        0
    }
}

struct Sess;

#[zbus::interface(name = "org.freedesktop.Secret.Session")]
impl Sess {
    fn close(&self) {}
}

/// Whether this is `test`'s own process (see `testenv::run_in_own_process`), which the process
/// running the test starts with the session bus address naming a socket in a fresh directory
/// (`bus` starts the bus there): both sides find it through the environment. The process
/// running the test holds SERIAL meanwhile, as the test does in its own process.
fn own_bus(test: &str) -> bool {
    if testenv::in_own_process(test) {
        return true;
    }
    let dir = tempfile::tempdir().unwrap();
    let addr = format!("unix:path={}", dir.path().join("bus").display());
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    testenv::run_in_own_process(test, [("DBUS_SESSION_BUS_ADDRESS", addr)]);
    false
}

/// The private session bus with the fakes, at the address the environment names (`own_bus`);
/// returns the notification log.
fn bus() -> Arc<Mutex<Vec<String>>> {
    static B: OnceLock<Arc<Mutex<Vec<String>>>> = OnceLock::new();
    B.get_or_init(|| {
        // Spawned from a thread that never exits, so PDEATHSIG fires only with the process.
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut c = Command::new("dbus-daemon");
            c.args(["--session", "--nofork", "--print-address=1", "--nopidfile"])
                .arg(format!(
                    "--address={}",
                    std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap()
                ))
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null());
            let die_with_parent = || {
                // SAFETY: prctl(2) PR_SET_PDEATHSIG takes a signal number, no pointers.
                unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) };
                Ok(())
            };
            // SAFETY: the hook runs between fork and exec; it only makes the prctl(2) syscall, which
            // is async-signal-safe, and allocates nothing.
            unsafe { std::os::unix::process::CommandExt::pre_exec(&mut c, die_with_parent) };
            tx.send(c.spawn().expect("dbus-daemon")).unwrap();
            loop {
                std::thread::park();
            }
        });
        let mut d: std::process::Child = rx.recv().unwrap();
        let mut addr = String::new();
        BufReader::new(d.stdout.take().unwrap())
            .read_line(&mut addr)
            .unwrap();
        let addr = addr.trim().to_string();
        std::mem::forget(d);
        let log = Arc::new(Mutex::new(Vec::new()));
        let c = zbus::blocking::connection::Builder::address(addr.as_str())
            .unwrap()
            .name("org.freedesktop.Notifications")
            .unwrap()
            .name("org.freedesktop.secrets")
            .unwrap()
            .serve_at("/org/freedesktop/Notifications", Notif(log.clone()))
            .unwrap()
            .serve_at("/org/freedesktop/secrets", Svc)
            .unwrap()
            .serve_at(ITEM, Item)
            .unwrap()
            .serve_at("/org/freedesktop/secrets/session/1", Sess)
            .unwrap()
            .build()
            .unwrap();
        std::mem::forget(c);
        log
    })
    .clone()
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ai-scripts-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Tool dir for the original: jq 1.8, `magick`, python3; `None` (skip) if missing.
fn tools(dir: &Path) -> Option<String> {
    let mut jq = format!("{}/deps/jq/bin", std::env::var("HOME").unwrap_or_default());
    if !Path::new(&jq).join("jq").exists()
        && let Ok(o) = Command::new("sh")
            .arg("-c")
            .arg("jq --version | grep -q '^jq-1\\.8' && dirname \"$(command -v jq)\"")
            .output()
        && o.status.success()
    {
        jq = String::from_utf8_lossy(&o.stdout).trim().to_string();
    }
    let mut missing: Vec<&str> = [
        "curl",
        "secret-tool",
        "notify-send",
        "python3",
        "base64",
        "dbus-daemon",
    ]
    .into_iter()
    .filter(|t| {
        !Command::new("sh")
            .arg("-c")
            .arg(format!("command -v {t}"))
            .output()
            .unwrap()
            .status
            .success()
    })
    .collect();
    if !Path::new(&jq).join("jq").exists() {
        missing.push("jq 1.8 (~/deps/jq/bin or PATH)");
    }
    if !missing.is_empty() {
        testenv::unavailable(format!("missing {missing:?}"));
        return None;
    }
    let shim = dir.join("shim");
    std::fs::create_dir_all(&shim).unwrap();
    if !Command::new("sh")
        .arg("-c")
        .arg("command -v magick")
        .output()
        .unwrap()
        .status
        .success()
    {
        let m = shim.join("magick");
        std::fs::write(&m, "#!/bin/sh\nexec convert \"$@\"\n").unwrap();
        std::os::unix::fs::PermissionsExt::set_mode(
            &mut std::fs::metadata(&m).unwrap().permissions(),
            0o755,
        );
        Command::new("chmod").arg("+x").arg(&m).status().unwrap();
    }
    Some(format!(
        "{jq}:{}:/usr/local/bin:/usr/bin:/bin",
        shim.display()
    ))
}

fn script_copy(rel: &str, to: &Path, port: u16) {
    let s = std::fs::read_to_string(Path::new(REF).join(rel)).unwrap();
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::write(to, s.replace(GEMINI, &format!("http://127.0.0.1:{port}"))).unwrap();
    Command::new("chmod").arg("+x").arg(to).status().unwrap();
}

#[derive(Debug, PartialEq)]
struct Run {
    stdout: String,
    code: i32,
    files: Vec<(String, Option<Vec<u8>>)>,
    seen: Vec<Seen>,
    notes: Vec<String>,
}

const TR_FILES: &[&str] = &[
    "rustshell/impulse-shell/translations/en_US.json",
    "rustshell/impulse-shell/translations/en_US.json.bak",
    "home/.config/impulse-shell/translations/ru_RU.json",
];

/// The config the translation edits: not byte-compared with the script (`jq … > f.tmp && mv`
/// left an empty `.tmp` on failure and reformatted the whole file); see `Cfg`.
const TR_CONFIG: &str = "home/.config/impulse-shell/config.json";

/// config.json after a run, and whether a `config.json.tmp` was left.
struct Cfg {
    json: Option<String>,
    tmp: bool,
}

/// A config tree under `d`: `quickshell/impulse-shell` (the scripts, translations, sources) and a
/// sibling `quickshell/other` (manage-translations scans the parent of the tree). `en_US.json`
/// lists the texts of the sources, as the shipped one does (`tests/translation_keys.rs`), so
/// the script's update step finds nothing to change; the native side has no such step.
fn translate_tree(d: &Path, port: u16, config: Option<&str>) -> PathBuf {
    let ri = d.join("rustshell/impulse-shell");
    let script = ri.join("scripts/ai/gemini-translate.sh");
    script_copy("scripts/ai/gemini-translate.sh", &script, port);
    for f in ["manage-translations.sh", "translation-manager.py"] {
        script_copy(
            &format!("translations/tools/{f}"),
            &ri.join("translations/tools").join(f),
            port,
        );
    }
    std::fs::write(
        ri.join("translations/en_US.json"),
        "{\n  \"Hello\": \"Hello\",\n  \"New\\n line\": \"New\\n line\",\n  \"Kept\": \"Kept /*keep*/\",\n  \"café\": \"café\",\n  \"Other %1\": \"Other %1\",\n  \"tick\": \"tick\",\n  \"Say \\\"hi\\\"\": \"Say \\\"hi\\\"\"\n}",
    )
    .unwrap();
    std::fs::create_dir_all(ri.join("modules")).unwrap();
    std::fs::write(
        ri.join("modules/A.qml"),
        "Text { text: Translation.tr(\"Hello\") }\r\nText { text: Translation.tr( 'New\\n line' ) }\nText { text: Translation.tr(\"caf\\u00e9\") }\nText { text: Translation.tr(\"Say \\\"hi\\\"\") }\n",
    )
    .unwrap();
    std::fs::write(
        ri.join("modules/b.js"),
        "const t = Translation.tr(`tick`)\n",
    )
    .unwrap();
    std::fs::create_dir_all(d.join("rustshell/other")).unwrap();
    std::fs::write(
        d.join("rustshell/other/C.qml"),
        "Text { text: Translation.tr(\"  Other %1 \") }\n",
    )
    .unwrap();
    let cfg = d.join("home/.config/impulse-shell");
    std::fs::create_dir_all(&cfg).unwrap();
    if let Some(c) = config {
        std::fs::write(cfg.join("config.json"), c).unwrap();
    }
    std::fs::create_dir_all(d.join("tmp")).unwrap();
    script
}

fn collect(
    d: &Path,
    files: &[&str],
    srv: &Server,
    from: usize,
    notes: usize,
    stdout: String,
    code: i32,
) -> Run {
    let tmp_left: Vec<_> = std::fs::read_dir(d.join("tmp"))
        .map(|r| r.flatten().map(|e| e.file_name()).collect())
        .unwrap_or_default();
    assert!(tmp_left.is_empty(), "temp files left: {tmp_left:?}");
    let created = regex_lite(&stdout);
    Run {
        stdout: created,
        code,
        files: files
            .iter()
            .map(|f| (f.to_string(), std::fs::read(d.join(f)).ok()))
            .collect(),
        seen: srv.seen.lock().unwrap()[from..].to_vec(),
        notes: bus().lock().unwrap()[notes..]
            .iter()
            .map(|n| n.replace(&d.to_string_lossy().into_owned(), "<d>"))
            .collect(),
    }
}

/// The random `tempfile` name.
fn regex_lite(s: &str) -> String {
    s.lines()
        .map(|l| {
            if l.starts_with("Created temporary file: ") {
                "Created temporary file: <tmp>"
            } else {
                l
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Field-by-field, printing only what differs (the bodies are long).
fn same(o: &Run, n: &Run) {
    let s = |b: &Option<Vec<u8>>| b.as_ref().map(|b| String::from_utf8_lossy(b).into_owned());
    let mut bad = Vec::new();
    if o.stdout != n.stdout {
        bad.push(format!("stdout:\n{}\n---\n{}", o.stdout, n.stdout));
    }
    if o.code != n.code {
        bad.push(format!("code {} vs {}", o.code, n.code));
    }
    for ((f, a), (_, b)) in o.files.iter().zip(&n.files) {
        if a != b {
            bad.push(format!("file {f}:\n{:?}\n---\n{:?}", s(a), s(b)));
        }
    }
    if o.seen.len() != n.seen.len() {
        bad.push(format!("requests {} vs {}", o.seen.len(), n.seen.len()));
    }
    for (a, b) in o.seen.iter().zip(&n.seen) {
        if a != b {
            let (x, y) = (
                String::from_utf8_lossy(&a.body),
                String::from_utf8_lossy(&b.body),
            );
            let at = x.chars().zip(y.chars()).take_while(|(p, q)| p == q).count();
            let tail = |t: &str| {
                t.chars()
                    .skip(at.saturating_sub(40))
                    .take(120)
                    .collect::<String>()
            };
            bad.push(format!(
                "request {:?} {:?}\n{:?} {:?}\nbody@{at}: {:?}\n---\n{:?}",
                a.path,
                a.headers,
                b.path,
                b.headers,
                tail(&x),
                tail(&y)
            ));
        }
    }
    // libnotify < 0.8 sends no `sender-pid`; the native side follows 0.8 (Arch).
    let old_libnotify = Command::new("notify-send")
        .arg("--version")
        .output()
        .is_ok_and(|v| String::from_utf8_lossy(&v.stdout).contains(" 0.7."));
    let n_notes: Vec<String> = if old_libnotify {
        n.notes
            .iter()
            .map(|s| s.replace("\"sender-pid:x\", ", ""))
            .collect()
    } else {
        n.notes.clone()
    };
    if o.notes != n_notes {
        bad.push(format!("notes {:?}\n---\n{:?}", o.notes, n_notes));
    }
    assert!(bad.is_empty(), "MISMATCH\n{}", bad.join("\n"));
}

static SERIAL: Mutex<()> = Mutex::new(());

fn translate_both(
    name: &str,
    reply: &'static str,
    locale: &str,
    config: Option<&str>,
) -> Option<(Run, Run, Cfg)> {
    let (mut o, _) = translate_one(name, reply, locale, config, false)?;
    let (n, cfg) = translate_one(name, reply, locale, config, true)?;
    // What the script prints beyond its usage line is the report of its update step: nothing
    // to add or delete (`Kept` is in no source and stays).
    if !locale.is_empty() {
        assert!(
            o.stdout.contains("Missing keys: 0\n  Extra keys: 0\n")
                && o.stdout.contains("No changes made"),
            "{}",
            o.stdout
        );
        o.stdout.clear();
    }
    Some((o, n, cfg))
}

fn translate_one(
    name: &str,
    reply: &'static str,
    locale: &str,
    config: Option<&str>,
    native: bool,
) -> Option<(Run, Cfg)> {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let log = bus();
    let srv = server(reply);
    let mut runs = Vec::new();
    {
        let d = tmp(&format!("{name}-{native}"));
        let path = tools(&d)?;
        let script = translate_tree(&d, srv.port, config);
        let (from, notes) = (srv.seen.lock().unwrap().len(), log.lock().unwrap().len());
        let home = d.join("home").to_string_lossy().into_owned();
        let tmpdir = d.join("tmp").to_string_lossy().into_owned();
        let (stdout, code) = if native {
            let env = |k: &str| match k {
                "HOME" => Some(home.clone()),
                "TMPDIR" => Some(tmpdir.clone()),
                _ => None,
            };
            let (mut o, mut e) = (String::new(), String::new());
            let args: Vec<String> = if locale.is_empty() {
                vec![String::new()]
            } else {
                vec![locale.into()]
            };
            let base = format!("http://127.0.0.1:{}", srv.port);
            let c = translate::run(
                &script.to_string_lossy(),
                &args,
                &env,
                &base,
                &mut |s| o += &s,
                &mut |s| e += &s,
            );
            eprintln!("{name} native stderr: {e}");
            (o, c)
        } else {
            let o = Command::new("bash")
                .arg(&script)
                .arg(locale)
                .env_clear()
                .env("PATH", &path)
                .env("HOME", &home)
                .env("TMPDIR", &tmpdir)
                .env(
                    "DBUS_SESSION_BUS_ADDRESS",
                    std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap(),
                )
                .output()
                .unwrap();
            eprintln!(
                "{name} original stderr: {}",
                String::from_utf8_lossy(&o.stderr)
            );
            (
                String::from_utf8(o.stdout).unwrap(),
                o.status.code().unwrap_or(-1),
            )
        };
        // The script path differs per run; compare relative to the tree.
        let stdout = stdout.replace(&d.to_string_lossy().into_owned(), "<d>");
        let cfg = Cfg {
            json: std::fs::read_to_string(d.join(TR_CONFIG)).ok(),
            tmp: d.join(format!("{TR_CONFIG}.tmp")).exists(),
        };
        runs.push((collect(&d, TR_FILES, &srv, from, notes, stdout, code), cfg));
    }
    runs.pop()
}

const TR_REPLY: &str = "{\n  \"candidates\": [\n    {\n      \"content\": {\n        \"parts\": [\n          {\n            \"text\": \"{\\\"Hello\\\": \\\"Привет\\\"}\"\n          }\n        ]\n      }\n    }\n  ]\n}\n";

#[test]
fn translate_matches_script() {
    if !own_bus(concat!(module_path!(), "::translate_matches_script")) {
        return;
    }
    let config = "{\n  \"language\": {\n    \"ui\": \"auto\",\n    \"translator\": {\"engine\": \"auto\"}\n  },\n  \"n\": [1e-07, 0.10, 3, -0, 2E10]\n}\n";
    let Some((o, n, cfg)) = translate_both("ok", TR_REPLY, "ru_RU", Some(config)) else {
        return;
    };
    // Only `.language.ui` changes; the rest of the file is kept as written.
    let v: serde_json::Value = serde_json::from_str(cfg.json.as_deref().unwrap()).unwrap();
    assert_eq!(v["language"]["ui"], "ru_RU");
    assert_eq!(v["language"]["translator"]["engine"], "auto");
    assert_eq!(v["n"].to_string(), "[1e-07,0.10,3,-0,2e+10]");
    assert!(!cfg.tmp);
    assert_eq!(o.seen.len(), 1);
    assert_eq!(o.notes.len(), 2);
    same(&o, &n);
}

#[test]
fn translate_edge_cases_match_script() {
    if !own_bus(concat!(
        module_path!(),
        "::translate_edge_cases_match_script"
    )) {
        return;
    }
    // No config.json and a Gemini error reply; then a config whose `.language` is not an
    // object and a non-JSON reply. The script saved the reply as `ru_RU.json`, switched the UI
    // to it and announced "Translation complete"; native writes nothing, keeps `language.ui`
    // and says the translation failed (with Gemini's message when it sent one).
    let cases = [
        (
            "nocfg",
            "{\"error\": {\"code\": 429, \"message\": \"Quota exceeded\"}}",
            None,
            "Quota exceeded",
        ),
        (
            "badcfg",
            "<html>nope</html>",
            Some("{\"language\": \"x\"}"),
            "Gemini returned no valid translation",
        ),
    ];
    for (name, reply, config, why) in cases {
        let Some((o, n, cfg)) = translate_both(name, reply, "ru_RU", config) else {
            return;
        };
        let tr = |r: &Run| {
            r.files
                .iter()
                .find(|(f, _)| f.ends_with("ru_RU.json"))
                .unwrap()
                .1
                .clone()
        };
        assert!(
            tr(&o).is_some(),
            "{name}: the script saves the error as a translation"
        );
        assert_eq!(tr(&n), None, "{name}");
        assert_eq!(n.code, 1, "{name}");
        assert_eq!(o.stdout, n.stdout, "{name}");
        assert_eq!(o.seen.len(), n.seen.len(), "{name}");
        let last = n.notes.last().unwrap();
        assert!(
            last.contains("|Translation failed|") && last.contains(why),
            "{name}: {last}"
        );
        assert!(
            !n.notes.iter().any(|m| m.contains("Translation complete")),
            "{name}"
        );
        assert_eq!(cfg.json.as_deref(), config, "{name}");
        assert!(!cfg.tmp, "{name}");
    }
    // No locale: usage.
    let Some((o, n, cfg)) = translate_both("usage", TR_REPLY, "", Some("{}")) else {
        return;
    };
    assert_eq!(o.code, 1);
    same(&o, &n);
    assert_eq!(cfg.json.as_deref(), Some("{}"));
    assert!(!cfg.tmp);
}

fn wallpaper(p: &Path) {
    let img = image::RgbImage::from_fn(640, 360, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
    });
    img.save(p).unwrap();
}

/// Width and height of the base64 JPEG a request carries.
fn jpeg_size(b64: &str) -> (u32, u32) {
    use base64::Engine;
    let jpg = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .unwrap();
    let im = image::load_from_memory_with_format(&jpg, image::ImageFormat::Jpeg).unwrap();
    (im.width(), im.height())
}

/// (stdout, exit code, request with the base64 image swapped for `<jpeg>`, the JPEG's size).
type CatRun = (String, i32, Vec<Seen>, Option<(u32, u32)>);

fn categorize_both(
    reply: &'static str,
    args: &[&str],
    env_prompt: Option<&str>,
) -> Option<Vec<CatRun>> {
    categorize_sides(reply, args, env_prompt, &[false, true])
}

fn categorize_one(reply: &'static str, native: bool) -> Option<()> {
    categorize_sides(reply, &["<img>"], None, &[native]).map(|_| ())
}

fn categorize_sides(
    reply: &'static str,
    args: &[&str],
    env_prompt: Option<&str>,
    sides: &[bool],
) -> Option<Vec<CatRun>> {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    bus();
    let srv = server(reply);
    let d = tmp("cat");
    let path = tools(&d)?;
    let script = d.join("scripts/ai/gemini-categorize-wallpaper.sh");
    script_copy(
        "scripts/ai/gemini-categorize-wallpaper.sh",
        &script,
        srv.port,
    );
    let img = d.join("My Wall.png");
    wallpaper(&img);
    let args: Vec<String> = args
        .iter()
        .map(|a| a.replace("<img>", &img.to_string_lossy()))
        .collect();
    let mut out = Vec::new();
    for &native in sides {
        let from = srv.seen.lock().unwrap().len();
        let (stdout, code) = if native {
            let env = |k: &str| match k {
                "GEMINI_WALLPAPER_PROMPT" => env_prompt.map(String::from),
                _ => None,
            };
            let (mut o, mut e) = (String::new(), String::new());
            let base = format!("http://127.0.0.1:{}", srv.port);
            let c = categorize::run(
                &script.to_string_lossy(),
                &args,
                &env,
                &base,
                &mut |s| o += &s,
                &mut |s| e += &s,
            );
            eprintln!("native stderr: {e}");
            if let [a] = args.as_slice() {
                let n = srv.seen.lock().unwrap().len();
                let cap = categorize::capture(&script, a, &env, &base);
                assert_eq!(cap, o, "switchwall's in-process $(categorize) differs");
                srv.seen.lock().unwrap().truncate(n);
            }
            (o, c)
        } else {
            let mut c = Command::new("bash");
            c.arg(&script)
                .args(&args)
                .env_clear()
                .env("PATH", &path)
                .env("HOME", d.join("home"));
            c.env(
                "DBUS_SESSION_BUS_ADDRESS",
                std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap(),
            );
            if let Some(p) = env_prompt {
                c.env("GEMINI_WALLPAPER_PROMPT", p);
            }
            let o = c.output().unwrap();
            eprintln!("original stderr: {}", String::from_utf8_lossy(&o.stderr));
            (
                String::from_utf8(o.stdout).unwrap(),
                o.status.code().unwrap_or(-1),
            )
        };
        let mut seen = srv.seen.lock().unwrap()[from..].to_vec();
        let mut dims = None;
        for s in &mut seen {
            // Compared as JSON: the original writes its request by hand, the native job
            // with a serializer.
            let Ok(mut body) = serde_json::from_slice::<serde_json::Value>(&s.body) else {
                continue;
            };
            let data = &mut body["contents"][0]["parts"][0]["inline_data"]["data"];
            if let Some(b64) = data.as_str() {
                dims = Some(jpeg_size(b64));
                *data = "<jpeg>".into();
            }
            s.body = serde_json::to_vec_pretty(&body).unwrap();
            s.headers.retain(|(k, _)| k != "content-length");
        }
        out.push((
            stdout.replace(&d.to_string_lossy().into_owned(), "<d>"),
            code,
            seen,
            dims,
        ));
    }
    Some(out)
}

#[test]
fn categorize_matches_script() {
    if !own_bus(concat!(module_path!(), "::categorize_matches_script")) {
        return;
    }
    let reply = "{\n  \"candidates\": [{\"content\": {\"parts\": [{\"text\": \"abstract\"}]}}]\n}";
    let Some(r) = categorize_both(reply, &["<img>"], None) else {
        return;
    };
    assert_eq!(r[0].0, "abstract\n");
    assert_eq!(r[0].3, Some((200, 113)));
    assert_eq!(r[0], r[1]);
    let Some(r) = categorize_both("[1]", &["<img>", "gemini-x", "Pick one"], None) else {
        return;
    };
    assert_eq!(r[0].1, 5);
    assert_eq!(r[0], r[1]);
    let Some(r) = categorize_both(reply, &["<img>"], Some("From env")) else {
        return;
    };
    assert_eq!(r[0], r[1]);
    let Some(r) = categorize_both(reply, &[], None) else {
        return;
    };
    assert_eq!(r[0].1, 1);
    assert_eq!(r[0], r[1]);
}

/// The original resizes into the fixed `/tmp/impulse-shell/ai/wallpaper.jpg`: a symlink planted
/// there (by whoever made `/tmp/impulse-shell` first) gets the JPEG written through it. The
/// native job keeps the resized image in memory and never touches that path.
#[test]
fn categorize_does_not_write_through_tmp_wallpaper_symlink() {
    if !own_bus(concat!(
        module_path!(),
        "::categorize_does_not_write_through_tmp_wallpaper_symlink"
    )) {
        return;
    }
    const FIXED: &str = "/tmp/impulse-shell/ai/wallpaper.jpg";
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    bus();
    let reply = "{\"candidates\": [{\"content\": {\"parts\": [{\"text\": \"city\"}]}}]}";
    let srv = server(reply);
    let d = tmp("cat-symlink");
    let img = d.join("wall.png");
    wallpaper(&img);
    let victim = d.join("victim.txt");
    std::fs::write(&victim, "victim data\n").unwrap();
    std::fs::create_dir_all("/tmp/impulse-shell/ai").unwrap();
    let _ = std::fs::remove_file(FIXED);
    std::os::unix::fs::symlink(&victim, FIXED).unwrap();
    let (mut o, mut e) = (String::new(), String::new());
    let code = categorize::run(
        "gemini-categorize-wallpaper.sh",
        &[img.to_string_lossy().into_owned()],
        &|_| None,
        &format!("http://127.0.0.1:{}", srv.port),
        &mut |s| o += &s,
        &mut |s| e += &s,
    );
    let link = std::fs::read_link(FIXED);
    let _ = std::fs::remove_file(FIXED);
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "victim data\n");
    assert_eq!(link.ok().as_deref(), Some(victim.as_path()));
    assert_eq!((o.as_str(), code), ("city\n", 0), "stderr: {e}");
    let seen = srv.seen.lock().unwrap();
    let body: serde_json::Value = serde_json::from_slice(&seen.last().unwrap().body).unwrap();
    let data = body["contents"][0]["parts"][0]["inline_data"]["data"]
        .as_str()
        .unwrap();
    assert_eq!(jpeg_size(data), (200, 113));
}

/// A wallpaper named with a `"` and a backslash: the request is still JSON, and its text part
/// names the file as it is.
#[test]
fn categorize_request_is_json_for_any_file_name() {
    if !own_bus(concat!(
        module_path!(),
        "::categorize_request_is_json_for_any_file_name"
    )) {
        return;
    }
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    bus();
    let reply = "{\"candidates\": [{\"content\": {\"parts\": [{\"text\": \"city\"}]}}]}";
    let srv = server(reply);
    let d = tmp("cat-name");
    let img = d.join("My \"Wall\" \\ back.png");
    wallpaper(&img);
    let (mut o, mut e) = (String::new(), String::new());
    let code = categorize::run(
        "gemini-categorize-wallpaper.sh",
        &[img.to_string_lossy().into_owned()],
        &|_| None,
        &format!("http://127.0.0.1:{}", srv.port),
        &mut |s| o += &s,
        &mut |s| e += &s,
    );
    assert_eq!((o.as_str(), code), ("city\n", 0), "stderr: {e}");
    let seen = srv.seen.lock().unwrap();
    let request: serde_json::Value = serde_json::from_slice(&seen.last().unwrap().body)
        .unwrap_or_else(|e| panic!("the request is not JSON: {e}"));
    assert_eq!(
        request["contents"][0]["parts"][1]["text"],
        "Categorize the wallpaper. Its file name is My \"Wall\" \\ back.png"
    );
}

fn cpu(who: i32) -> f64 {
    // SAFETY: rusage is plain integers, for which all-zero bytes are a valid value.
    let mut u: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: `u` is a valid, writable rusage for getrusage(2) to fill.
    unsafe { libc::getrusage(who, &mut u) };
    let t = |v: libc::timeval| v.tv_sec as f64 * 1e3 + v.tv_usec as f64 / 1e3;
    t(u.ru_utime) + t(u.ru_stime)
}

/// `cargo test --features live --release -p qml --test ai_scripts bench -- --nocapture`. Original
/// CPU = its process tree (RUSAGE_CHILDREN); native CPU = this process (RUSAGE_SELF), which
/// also includes the in-process fake HTTP/D-Bus servers (their share is printed for the
/// original run as "servers").
#[test]
#[cfg_attr(
    not(feature = "live"),
    ignore = "live: --features live, docs/benchmarks.md"
)]
fn bench_ai_scripts() {
    if !own_bus(concat!(module_path!(), "::bench_ai_scripts")) {
        return;
    }
    const N: usize = 10;
    type Case<'a> = (&'a str, &'a dyn Fn(bool) -> Option<()>);
    let tr = |native: bool| -> Option<()> {
        let _ = translate_one("bench", TR_REPLY, "ru_RU", Some("{}"), native)?;
        Some(())
    };
    let cat = |native: bool| -> Option<()> {
        let reply = "{\"candidates\": [{\"content\": {\"parts\": [{\"text\": \"city\"}]}}]}";
        categorize_one(reply, native)
    };
    let cases: [Case; 2] = [
        ("gemini-translate", &tr),
        ("gemini-categorize-wallpaper", &cat),
    ];
    for (name, f) in cases {
        if f(true).is_none() {
            return;
        }
        let mut r = [[0.0; 3]; 2];
        for (k, native) in [false, true].into_iter().enumerate() {
            for _ in 0..N {
                let (c0, s0, t0) = (
                    cpu(libc::RUSAGE_CHILDREN),
                    cpu(libc::RUSAGE_SELF),
                    std::time::Instant::now(),
                );
                f(native).unwrap();
                r[k][0] += t0.elapsed().as_secs_f64() * 1e3 / N as f64;
                r[k][1] += (cpu(libc::RUSAGE_CHILDREN) - c0) / N as f64;
                r[k][2] += (cpu(libc::RUSAGE_SELF) - s0) / N as f64;
            }
        }
        println!(
            "{name}: original wall {:.1} ms cpu {:.1} ms (servers {:.1} ms) | native wall {:.1} ms cpu {:.1} ms",
            r[0][0], r[0][1], r[0][2], r[1][0], r[1][2]
        );
    }
}
