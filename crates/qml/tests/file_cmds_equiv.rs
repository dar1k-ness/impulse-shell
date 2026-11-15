//! What FirstRunExperience, welcome, Directories, MessageCodeBlock and GeneralConfig do to
//! files through `Impulse.Files` (`qml::files`), in a temporary HOME: the file tree, contents
//! and modes they leave, the programs they start and the notifications they send, also for
//! the inputs the `bash -c`, `rm`, `mkdir` and `sed` lines the original used to run got wrong.

mod event_loop;
use event_loop::Loop;
use qml::runtime::Runtime;
use qml::value::Value;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

fn ri() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../qml")
}

struct Fx {
    root: PathBuf,
    home: PathBuf,
    /// Lines the spawn hook appends: the window launches and the wallpaper switches.
    log: PathBuf,
    /// The `notify-send` command lines `Impulse.Notify` handed to the spawn hook, without the
    /// program name, joined by `|`.
    notes: Arc<Mutex<Vec<String>>>,
}

const TMP_MEDIA: &[&str] = &[
    "/tmp/rustshell/media/images",
    "/tmp/rustshell/media/cliphist",
];

/// The reference tools the original's commands run; a missing one fails the test (`testenv`).
fn tools() -> bool {
    ["bash", "rm", "mkdir", "sed", "dbus-daemon"]
        .iter()
        .all(|t| testenv::require(t))
}

/// A session bus daemon, stopped when dropped.
struct Daemon(std::process::Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Runs `test` in a process of its own (`testenv::run_in_own_process`) whose environment names,
/// from the start, a fresh HOME and its XDG homes, `LC_ALL=C.UTF-8` and a private session bus;
/// true in that process, where the test's body runs (see [`fx`]).
fn own_process(test: &str) -> bool {
    use std::io::BufRead;
    if testenv::in_own_process(test) {
        return true;
    }
    let root = tempfile::tempdir().unwrap();
    let mut child = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let _bus = Daemon(child);
    let mut addr = String::new();
    std::io::BufReader::new(stdout)
        .read_line(&mut addr)
        .unwrap();
    // The original's fixed /tmp/rustshell/media paths (`TMP_MEDIA`): one test at a time.
    static RUN: Mutex<()> = Mutex::new(());
    let _run = RUN.lock().unwrap_or_else(|e| e.into_inner());
    let mut vars = testenv::home_vars(&root.path().join("home"));
    vars.extend([
        ("LC_ALL", "C.UTF-8".into()),
        ("DBUS_SESSION_BUS_ADDRESS", addr.trim().into()),
        ("FILE_CMDS_ROOT", root.path().into()),
    ]);
    testenv::run_in_own_process(test, vars);
    false
}

/// The fixture of this test's [`own_process`]: its root and HOME.
fn fx() -> MutexGuard<'static, &'static Fx> {
    static F: OnceLock<Mutex<&'static Fx>> = OnceLock::new();
    let m = F.get_or_init(|| {
        let var = |k: &str| PathBuf::from(std::env::var_os(k).unwrap());
        let root = var("FILE_CMDS_ROOT");
        Mutex::new(&*Box::leak(Box::new(Fx {
            log: root.join("log"),
            root,
            home: var("HOME"),
            notes: Arc::new(Mutex::new(vec![])),
        })))
    });
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn chmod(p: &Path, mode: u32) {
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// Writable again, so a test tree with read-only directories can be removed.
fn unlock(p: &Path) {
    if let Ok(m) = std::fs::symlink_metadata(p)
        && m.is_dir()
    {
        chmod(p, 0o755);
        for e in std::fs::read_dir(p).unwrap().flatten() {
            unlock(&e.path());
        }
    }
}

fn clear(f: &Fx) {
    for p in [f.home.clone(), f.root.join("outside")]
        .into_iter()
        .chain(TMP_MEDIA.iter().map(PathBuf::from))
    {
        unlock(&p);
        let _ = std::fs::remove_dir_all(&p);
        let _ = std::fs::remove_file(&p);
    }
    let _ = std::fs::remove_file(&f.log);
    f.notes.lock().unwrap().clear();
    std::fs::create_dir_all(f.home.join(".config/impulse-shell")).unwrap();
    std::fs::write(f.home.join(".config/impulse-shell/config.json"), "{}").unwrap();
}

fn walk(p: &Path, rel: &str, out: &mut BTreeMap<String, String>) {
    let Ok(m) = std::fs::symlink_metadata(p) else {
        return;
    };
    let mode = m.permissions().mode() & 0o7777;
    let v = if m.file_type().is_symlink() {
        format!("link {}", std::fs::read_link(p).unwrap().display())
    } else if m.is_dir() {
        let mut names: Vec<_> = std::fs::read_dir(p)
            .map(|r| r.flatten().map(|e| e.file_name()).collect())
            .unwrap_or_default();
        names.sort();
        for n in names {
            walk(&p.join(&n), &format!("{rel}/{}", n.to_string_lossy()), out);
        }
        format!("dir {mode:o}")
    } else {
        format!(
            "file {mode:o} {:?}",
            String::from_utf8_lossy(&std::fs::read(p).unwrap_or_default())
        )
    };
    out.insert(rel.to_string(), v);
}

/// The test trees, the hook's log and the notifications; files the original itself keeps (config,
/// notifications cache) are left out.
fn snapshot(f: &Fx) -> Vec<String> {
    let mut m = BTreeMap::new();
    walk(&f.home, "~", &mut m);
    walk(&f.root.join("outside"), "outside", &mut m);
    for t in TMP_MEDIA {
        walk(Path::new(t), t, &mut m);
    }
    let mut out: Vec<String> = m
        .into_iter()
        .filter(|(k, _)| {
            !k.starts_with("~/.config/impulse-shell/config.json") && !k.contains("/notifications")
        })
        .map(|(k, v)| format!("{k}: {v}"))
        .collect();
    let mut log: Vec<String> = std::fs::read_to_string(&f.log)
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect();
    log.sort();
    out.extend(log.into_iter().map(|l| format!("ran {l}")));
    out.extend(
        f.notes
            .lock()
            .unwrap()
            .iter()
            .map(|n| format!("notify {n}")),
    );
    out
}

thread_local!(static LOOP: RefCell<Option<Loop>> = const { RefCell::new(None) });

/// Until the native commands are done: the pool has run their jobs and reaped the children
/// they detached, and the loop has taken in the events those caused.
fn settle(rt: &mut Runtime) {
    LOOP.with_borrow_mut(|lp| lp.as_mut().expect("a run()").drain(rt));
    assert_eq!(qml::files::running(), 0, "commands still running");
}

/// What an act observed along the way (file contents between steps), added to the snapshot.
type Act<'a> = &'a dyn Fn(&mut Runtime, usize) -> Vec<String>;

/// Whether the spawn hook runs `a` for real: the commands of the members under test, and
/// `rm`/`mkdir` inside the test trees. Everything else the original starts fails, in both runs alike.
fn allowed(f: &Fx, a: &[String]) -> bool {
    let inside =
        |p: &str| p.starts_with(f.root.to_str().unwrap()) || p.starts_with("/tmp/rustshell/media/");
    match a {
        [c, rest @ ..] if c == "rm" || c == "mkdir" => rest
            .iter()
            .filter(|x| !x.starts_with('-'))
            .all(|x| inside(x)),
        [b, c, s] if b == "bash" && c == "-c" => ["echo '", "rm -rf '", "qs -p '", "sed -i '"]
            .iter()
            .any(|p| s.starts_with(p)),
        [n, s, ..] if n == "notify-send" => {
            ["Welcome app", "Code saved to file", "Couldn't save code"].contains(&s.as_str())
        }
        _ => false,
    }
}

/// One run: `setup` the tree, load the original's singletons and `load` the object under test, `act`,
/// and snapshot. The spawn hook sees what the original still starts as a program; `Impulse.Files` does
/// its own work on the disk.
fn run(
    setup: &dyn Fn(&Fx),
    load: &dyn Fn(&mut Runtime) -> usize,
    act: Act,
    props: &[&str],
) -> Vec<String> {
    let f = fx();
    clear(&f);
    setup(&f);
    qml::files::set_live(true);
    let ran = Arc::new(Mutex::new(Vec::<String>::new()));
    let r = ran.clone();
    let fx: &'static Fx = *f;
    qml::native::set_spawn_hook(Some(Box::new(move |a: &[String]| {
        // The welcome window comes as `ShellControl`, `openWindow` and its file, a wallpaper
        // switch as `WallpaperSwitch`, `run` and its arguments.
        if let [ty, m, rest @ ..] = a
            && matches!(
                (ty.as_str(), m.as_str()),
                ("ShellControl", "openWindow") | ("WallpaperSwitch", "run")
            )
        {
            let n = format!("{ty}.{m}");
            use std::io::Write;
            let mut log = std::fs::File::options()
                .append(true)
                .create(true)
                .open(&fx.log)
                .unwrap();
            writeln!(log, "{n}|{}|", rest.join("|")).unwrap();
            return Some(String::new());
        }
        if !allowed(fx, a) {
            return None;
        }
        // `Impulse.Notify` under a spawn hook only hands over its command line (what reaches
        // the bus is `notifications_rusty.rs`).
        if a[0] == "notify-send" {
            fx.notes.lock().unwrap().push(a[1..].join("|"));
            return Some(String::new());
        }
        r.lock().unwrap().push(a.join(" "));
        let _ = Command::new(&a[0])
            .args(&a[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        Some(String::new())
    })));
    // Midday for the engine: the original's night light schedule (Hyprsunset, loaded with the services)
    // decides nothing here from the hour the test runs at.
    qml::builtins::set_clock(Some(testenv::local_time_ms(12, 0)));
    let mut rt = Runtime::new(Some(ri()));
    LOOP.set(Some(Loop::new(&mut rt)));
    rt.load_singletons(&[
        ri().join("services"),
        ri().join("modules/common"),
        ri().join("modules/common/functions"),
    ]);
    let s = load(&mut rt);
    settle(&mut rt);
    let seen = act(&mut rt, s);
    settle(&mut rt);
    qml::native::set_spawn_hook(None);
    qml::files::set_live(false);
    let mut out = snapshot(&f);
    out.extend(seen);
    for p in props {
        out.push(format!("{p}={}", rt.get(s, p).to_js_string()));
    }
    let ran = ran.lock().unwrap();
    let mine = |a: &&String| {
        [
            "bash -c echo '",
            "bash -c rm -rf '",
            "bash -c qs -p '",
            "bash -c sed -i '",
            "rm ",
            "mkdir ",
        ]
        .iter()
        .any(|p| a.starts_with(p))
    };
    // `Impulse.Files` leaves no command line of the original's for the hook.
    assert!(!ran.iter().any(|a| mine(&a)), "{ran:?}");
    out
}

fn has(snap: &[String], line: &str) -> bool {
    snap.iter().any(|l| l == line)
}

/// `file 644 "TEXT\n"`, as the snapshot shows a file holding TEXT and a newline.
fn line_file(text: &str) -> String {
    format!("file 644 {:?}", format!("{text}\n"))
}

fn find(rt: &Runtime, pred: impl Fn(&Runtime, usize) -> bool) -> usize {
    let n = rt.world.borrow().items.len();
    let hits: Vec<usize> = (0..n)
        .filter(|&i| {
            let w = rt.world.borrow();
            w.items.get(i).is_some() && !w.dead.contains(&i)
        })
        .filter(|&i| pred(rt, i))
        .collect();
    if hits.len() != 1 {
        let w = rt.world.borrow();
        let near: Vec<String> = (0..n)
            .filter(|&i| !w.dead.contains(&i))
            .filter_map(|i| {
                let it = w.items.get(i)?;
                let hs: Vec<_> = it
                    .handlers
                    .keys()
                    .filter(|k| {
                        ["onClicked", "onSelected", "onCheckedChanged"].contains(&k.as_str())
                    })
                    .collect();
                (!hs.is_empty()).then(|| format!("{i} {:?} {hs:?}", it.types))
            })
            .collect();
        panic!("{hits:?} of {n}: {near:#?}\n{:#?}", rt.errors());
    }
    hits[0]
}

fn item(rt: &mut Runtime, src: &str) -> usize {
    let r = rt.load_str(src).unwrap_or_else(|e| panic!("{e}"));
    match rt.get(r, "s") {
        Value::Item(s) => s,
        v => panic!("no s ({v:?}): {:?}", rt.errors()),
    }
}

fn singleton(name: &'static str) -> impl Fn(&mut Runtime) -> usize {
    move |rt| {
        item(
            rt,
            &format!(
                "import QtQuick\nimport ri.services\nimport ri.modules.common\nItem {{ property var s: {name} }}"
            ),
        )
    }
}

/// Strings bash quoting, `echo` and the file system treat specially.
const CONTENTS: &[&str] = &[
    "plain",
    "it's \"quoted\" 'twice'",
    "back\\slash \\n \\\\ \\'",
    "line1\nline2\n\n",
    "-n",
    "-e",
    "-neE",
    "--",
    "",
    "$HOME `id` $(id) * ? ~",
    "tab\there é 🌶️",
];

fn first_run_path(f: &Fx) -> PathBuf {
    f.home.join(".local/state/rustshell/user/first_run.txt")
}

fn marker(f: &Fx) {
    let p = first_run_path(f);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, "old\n").unwrap();
}

fn home_of(rt: &Runtime) -> String {
    let d = rt
        .world
        .borrow()
        .singletons
        .get("Directories")
        .copied()
        .unwrap();
    let h = rt.get(d, "home").to_js_string();
    h.trim_start_matches("file://")
        .trim_end_matches('/')
        .to_string()
}

fn read(p: &str) -> String {
    format!(
        "{p}: {:?}",
        std::fs::read(p)
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .ok()
    )
}

#[test]
fn directories_cleanup() {
    if !tools() || !own_process(concat!(module_path!(), "::directories_cleanup")) {
        return;
    }
    let setup = |f: &Fx| {
        let cache = f.home.join(".cache/rustshell/media");
        let c = cache.join("coverart");
        std::fs::create_dir_all(c.join("sub/deeper")).unwrap();
        std::fs::write(c.join("a.png"), "x").unwrap();
        std::fs::write(c.join("sub/deeper/b"), "y").unwrap();
        let out = f.root.join("outside");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("keep"), "keep").unwrap();
        std::os::unix::fs::symlink(&out, c.join("link")).unwrap();
        std::fs::write(cache.join("latex"), "a file where the dir goes").unwrap();
        let clip = Path::new(TMP_MEDIA[1]);
        std::fs::create_dir_all(clip.join("ro")).unwrap();
        std::fs::write(clip.join("ro/stuck"), "z").unwrap();
        std::fs::write(clip.join("gone"), "z").unwrap();
        chmod(&clip.join("ro"), 0o555);
        let img = Path::new(TMP_MEDIA[0]);
        std::fs::create_dir_all(img.join("x")).unwrap();
        std::fs::write(img.join("x/1.png"), "1").unwrap();
    };
    let load = |rt: &mut Runtime| {
        rt.world
            .borrow()
            .singletons
            .get("Directories")
            .copied()
            .unwrap()
    };
    let snap = run(&setup, &load, &|_, _| vec![], &[]);
    let has = |s: &str| snap.iter().any(|l| l.contains(s));
    assert!(
        has("outside/keep: file")
            && has("/ro/stuck: file")
            && has("media/coverart: dir")
            && !has("coverart/a.png"),
        "{snap:#?}"
    );
    assert!(has("media/latex: dir") && !has("media/images"), "{snap:#?}");
}

#[test]
fn first_run_marker() {
    if !tools() || !own_process(concat!(module_path!(), "::first_run_marker")) {
        return;
    }
    let setup = |f: &Fx| {
        marker(f);
        for p in ["it's", "with space"] {
            std::fs::create_dir_all(f.home.join(p)).unwrap();
            std::fs::write(f.home.join(p).join("first run.txt"), "old\n").unwrap();
        }
    };
    let set_path = |rt: &mut Runtime, s: usize, rel: &str| {
        let home = home_of(rt);
        rt.set(s, "firstRunFilePath", Value::str(&format!("{home}/{rel}")));
        // Let the FileView load the new path before acting on it.
        settle(rt);
    };
    let call = |rt: &mut Runtime, s: usize, f: &str| {
        rt.call_method(s, f, vec![]).unwrap().unwrap();
        settle(rt);
    };
    // `rm -f` on a plain path works in the original too.
    run(
        &setup,
        &singleton("FirstRunExperience"),
        &|rt, s| {
            for p in ["it's", "with space"] {
                set_path(rt, s, &format!("{p}/first run.txt"));
                call(rt, s, "enableNextTime");
            }
            vec![]
        },
        &[],
    );
    // The original's path is a file:// URL and its text has a `'`: its commands miss the marker.
    let snap = run(
        &setup,
        &singleton("FirstRunExperience"),
        &|rt, s| {
            let home = home_of(rt);
            let url = rt.get(s, "firstRunFilePath").to_js_string();
            assert!(url.starts_with("file:///"), "{url}");
            let marker = format!("{home}/.local/state/rustshell/user/first_run.txt");
            let mut seen = vec![];
            call(rt, s, "enableNextTime");
            seen.push(read(&marker));
            call(rt, s, "disableNextTime");
            seen.push(read(&marker));
            for (k, c) in CONTENTS.iter().enumerate() {
                set_path(rt, s, &format!("new dir/marker-{k}"));
                rt.set(s, "firstRunFileContent", Value::str(c));
                call(rt, s, "disableNextTime");
            }
            for p in ["it's", "with space"] {
                set_path(rt, s, &format!("{p}/first run.txt"));
                call(rt, s, "enableNextTime");
            }
            seen
        },
        &[],
    );
    let marker = "~/.local/state/rustshell/user/first_run.txt";
    let home = std::env::var("HOME").unwrap();
    assert!(
        has(&snap, &format!("{home}{}: None", &marker[1..]))
            && has(
                &snap,
                &format!(
                    "{home}{}: Some({:?})",
                    &marker[1..],
                    "This file is just here to confirm you've been greeted :>\n"
                )
            ),
        "{snap:#?}"
    );
    for (k, c) in CONTENTS.iter().enumerate() {
        assert!(
            has(&snap, &format!("~/new dir/marker-{k}: {}", line_file(c))),
            "{c:?}: {snap:#?}"
        );
    }
    assert!(
        !snap.iter().any(|l| l.contains("first run.txt")),
        "{snap:#?}"
    );
}

fn launch(welcomes: &'static [&'static str]) -> impl Fn(&mut Runtime, usize) -> Vec<String> {
    move |rt, s| {
        for welcome in welcomes {
            rt.set(s, "welcomeQmlPath", Value::str(welcome));
            rt.call_method(s, "handleFirstRun", vec![])
                .unwrap()
                .unwrap();
            settle(rt);
        }
        vec![]
    }
}

#[test]
fn first_run_launches() {
    if !tools() || !own_process(concat!(module_path!(), "::first_run_launches")) {
        return;
    }
    let snap = run(
        &marker,
        &singleton("FirstRunExperience"),
        &launch(&[
            "/opt/impulse-shell/welcome.qml",
            "/opt/my impulse-shell/welcome.qml",
        ]),
        &[],
    );
    let ran = |snap: &[String], p: &str| snap.iter().filter(|l| l.starts_with(p)).count();
    assert_eq!(
        (
            ran(&snap, "ran WallpaperSwitch.run|"),
            ran(&snap, "ran ShellControl.openWindow|"),
        ),
        (2, 2),
        "{snap:#?}"
    );
    // A `'` in the path broke the original's `bash -c "qs -p '...'"`.
    let snap = run(
        &marker,
        &singleton("FirstRunExperience"),
        &launch(&["/opt/it's/welcome.qml"]),
        &[],
    );
    assert!(
        has(&snap, "ran ShellControl.openWindow|/opt/it's/welcome.qml|"),
        "{snap:#?}"
    );
}

fn welcome(rt: &mut Runtime) -> usize {
    rt.load_file(&ri().join("welcome.qml"))
        .unwrap_or_else(|e| panic!("{e}"))
}

/// Each path gets its text through the switch: off (write), on (`rm`, read back), off.
fn toggle(
    cases: &'static [(&'static str, &'static str)],
) -> impl Fn(&mut Runtime, usize) -> Vec<String> {
    move |rt, w| {
        let sw = find(rt, |rt, i| {
            rt.has_handler(i, "onCheckedChanged") && rt.get(i, "scale").to_number() == 0.6
        });
        let home = home_of(rt);
        let mut seen = vec![];
        for (rel, text) in cases {
            let p = format!("{home}/{rel}");
            rt.set(w, "firstRunFilePath", Value::str(&p));
            rt.set(w, "firstRunFileContent", Value::str(text));
            for checked in [false, true] {
                rt.set(sw, "checked", Value::Bool(checked));
                settle(rt);
            }
            seen.push(read(&p));
            rt.set(sw, "checked", Value::Bool(false));
            settle(rt);
        }
        rt.fire(w, "onClosing", &["close"], vec![Value::Undefined]);
        seen
    }
}

#[test]
fn welcome_switch_and_close() {
    if !tools() || !own_process(concat!(module_path!(), "::welcome_switch_and_close")) {
        return;
    }
    let setup = |f: &Fx| {
        marker(f);
        std::fs::create_dir_all(f.home.join("it's")).unwrap();
    };
    let snap = run(
        &setup,
        &welcome,
        &toggle(&[
            ("m0", "plain"),
            ("m1", "it's \"quoted\" 'twice'"),
            ("m2", "back\\slash \\n \\\\ \\'"),
            ("m3", "line1\nline2\n\n"),
            ("m4", ""),
            ("m5", "$HOME `id` $(id) * ? ~"),
            ("m6", "tab\there é 🌶️"),
            ("it's/first run.txt", "x"),
        ]),
        &[],
    );
    assert!(
        snap.iter()
            .any(|l| l.starts_with("notify Welcome app|") && l.ends_with("|-a|Shell")),
        "{snap:#?}"
    );
    assert!(
        has(&snap, &format!("~/it's/first run.txt: {}", line_file("x"))),
        "{snap:#?}"
    );
    // `echo` took these texts for its flags; a missing directory failed the write.
    let snap = run(
        &setup,
        &welcome,
        &toggle(&[
            ("f0", "-n"),
            ("f1", "-e"),
            ("f2", "-neE"),
            ("missing/first_run.txt", "--"),
        ]),
        &[],
    );
    for (rel, text) in [
        ("f0", "-n"),
        ("f1", "-e"),
        ("f2", "-neE"),
        ("missing/first_run.txt", "--"),
    ] {
        assert!(
            has(&snap, &format!("~/{rel}: {}", line_file(text))),
            "{snap:#?}"
        );
    }
    assert!(
        snap.iter()
            .any(|l| l.starts_with("notify Welcome app|") && l.ends_with("|-a|Shell")),
        "{snap:#?}"
    );
}

fn code_block(rt: &mut Runtime) -> usize {
    rt.load_file(&ri().join("modules/rusty/sidebarLeft/aiChat/MessageCodeBlock.qml"))
        .unwrap_or_else(|e| panic!("{e}"))
}

/// Saves each (code, language) and records the button's check mark.
fn save(
    cases: &'static [(&'static str, Option<&'static str>)],
) -> impl Fn(&mut Runtime, usize) -> Vec<String> {
    move |rt, s| {
        let b = find(rt, |rt, i| {
            rt.has_handler(i, "onClicked") && rt.get(i, "buttonIcon").to_js_string() == "save"
        });
        let mut seen = vec![];
        for &(content, lang) in cases {
            rt.set(s, "segmentContent", Value::str(content));
            rt.set(s, "segmentLang", lang.map_or(Value::Undefined, Value::str));
            rt.emit(b, "clicked", &[], vec![]);
            rt.settle();
            seen.push(format!(
                "activated={}",
                rt.get(b, "activated").to_js_string()
            ));
            settle(rt);
        }
        seen
    }
}

#[test]
fn code_block_save() {
    if !tools() || !own_process(concat!(module_path!(), "::code_block_save")) {
        return;
    }
    let downloads = |f: &Fx| std::fs::create_dir_all(f.home.join("Downloads")).unwrap();
    let snap = run(
        &downloads,
        &code_block,
        &save(&[
            ("print('hi')\n", Some("python")),
            ("echo \"$HOME\" \\\\ 'x'\n\nlast", Some("bash")),
            ("x", Some("")),
        ]),
        &[],
    );
    assert!(has(&snap, "activated=true"), "{snap:#?}");
    assert!(
        has(
            &snap,
            &format!("~/Downloads/code.python: {}", line_file("print('hi')\n"))
        ),
        "{snap:#?}"
    );
    let home = std::env::var("HOME").unwrap();
    let saved = |snap: &[String], name: &str| {
        let n = format!("notify Code saved to file|Saved to {home}/Downloads/{name}|-a|Shell");
        snap.contains(&n)
    };
    assert!(saved(&snap, "code.txt"), "{snap:#?}");
    // The original's `echo` dropped "-n", a `'` in the language broke its quoting, a `/` in it wrote
    // outside Downloads (or failed, notifying "saved" anyway), and notify-send turned a `\t`
    // in the file name into a tab.
    let cases: &[(&str, Option<&str>)] = &[
        ("-n", Some("sh")),
        ("no newline", Some("it's")),
        ("in a dir", Some("../x")),
        ("escapes", Some("a\\tb\\101")),
    ];
    let snap = run(
        &downloads,
        &code_block,
        &save(&[
            ("-n", Some("sh")),
            ("no newline", Some("it's")),
            ("in a dir", Some("../x")),
            ("escapes", Some("a\\tb\\101")),
        ]),
        &[],
    );
    for (content, lang) in cases {
        let name = format!("code.{}", lang.unwrap().replace('/', "_"));
        assert!(
            has(
                &snap,
                &format!("~/Downloads/{name}: {}", line_file(content))
            ) && saved(&snap, &name),
            "{name}: {snap:#?}"
        );
    }
    assert!(!snap.iter().any(|l| l.starts_with("~/x")), "{snap:#?}");
    // No Downloads yet: it is made. Downloads a file: the save fails and says so.
    let snap = run(&|_| {}, &code_block, &save(&[("a", None)]), &[]);
    assert!(
        has(&snap, &format!("~/Downloads/code.txt: {}", line_file("a"))),
        "{snap:#?}"
    );
    let snap = run(
        &|f| std::fs::write(f.home.join("Downloads"), "").unwrap(),
        &code_block,
        &save(&[("a", None)]),
        &[],
    );
    assert!(
        snap.iter()
            .any(|l| l.starts_with("notify Couldn't save code|")),
        "{snap:#?}"
    );
    assert!(!snap.iter().any(|l| l.contains("Code saved")), "{snap:#?}");
}

/// `Config.options.time.format`.
fn time_format(rt: &Runtime) -> String {
    let mut v = Value::Item(rt.world.borrow().singletons.get("Config").copied().unwrap());
    for k in ["options", "time", "format"] {
        v = match v {
            Value::Item(i) => rt.get(i, k),
            Value::Obj(o) => o.borrow().props.get(k).cloned().unwrap_or_default(),
            _ => Value::Undefined,
        };
    }
    v.to_js_string()
}

fn conf_at(rel: &'static str, text: &'static str, mode: u32) -> impl Fn(&Fx) {
    move |f| {
        let p = f.home.join(".config/hypr/hyprlock.conf");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        let real = f.root.join(rel);
        std::fs::create_dir_all(real.parent().unwrap()).unwrap();
        std::fs::write(&real, text).unwrap();
        chmod(&real, mode);
        if real != p {
            std::os::unix::fs::symlink(&real, &p).unwrap();
        }
    }
}

fn general(rt: &mut Runtime) -> usize {
    rt.load_file(&ri().join("modules/settings/GeneralConfig.qml"))
        .unwrap_or_else(|e| panic!("{e}"))
}

/// Picks each format and records the conf and the stored format after it.
fn pick(rt: &mut Runtime, _: usize) -> Vec<String> {
    // The row GeneralConfig declares, not the segmented control inside it (which aliases
    // its currentValue and forwards its own onSelected to the row's).
    let a = find(rt, |rt, i| {
        rt.has_handler(i, "onSelected")
            && rt.world.borrow().items[i]
                .types
                .iter()
                .any(|t| t == "SettingsSegmentedRow")
            && rt.get(i, "currentValue").to_js_string().contains(":mm")
    });
    let conf = format!("{}/.config/hypr/hyprlock.conf", home_of(rt));
    let mut seen = vec![];
    for (k, v) in ["hh:mm", "h:mm ap", "h:mm AP", "hh:mm", "hh:mm"]
        .iter()
        .enumerate()
    {
        rt.fire(a, "onSelected", &["newValue"], vec![Value::str(v)]);
        settle(rt);
        let text = std::fs::read_to_string(&conf).ok();
        seen.push(format!("step{k} {text:?} fmt={}", time_format(rt)));
    }
    seen
}

const HYPRLOCK: &str = "label {\n    text = cmd[update:1000] echo \"$TIME12\"\n    font_size = 64\n}\nlabel {\n    text = $USER\n}\n";

#[test]
fn time_format_sed() {
    if !tools() || !own_process(concat!(module_path!(), "::time_format_sed")) {
        return;
    }
    // A usual hyprlock.conf (one `$TIME` per line), and none at all: as the original.
    let usual = conf_at("home/.config/hypr/hyprlock.conf", HYPRLOCK, 0o640);
    for setup in [&usual as &dyn Fn(&Fx), &|_: &Fx| {}] {
        let snap = run(setup, &general, &pick, &[]);
        assert!(snap.iter().any(|l| l.ends_with("fmt=h:mm AP")), "{snap:#?}");
    }
    // The original's sed changed only a line's first match and any `TIME12` word, and put a copy
    // where the symlink was.
    let tricky = "$TIME12 $TIME12\nlabel { text = cmd[update:1000] echo \"$TIME\" }\n$TIMEé $TIME_ $TIME-x TIME12a\n\\TIME TIME12\nno newline $TIME";
    let as_24 = "$TIME $TIME\nlabel { text = cmd[update:1000] echo \"$TIME\" }\n$TIMEé $TIME_ $TIME-x TIME12a\n\\TIME TIME12\nno newline $TIME";
    let as_12 = "$TIME12 $TIME12\nlabel { text = cmd[update:1000] echo \"$TIME12\" }\n$TIMEé $TIME_ $TIME12-x TIME12a\n\\TIME TIME12\nno newline $TIME12";
    let want = [
        (as_24, "hh:mm"),
        (as_12, "h:mm ap"),
        (as_12, "h:mm AP"),
        (as_24, "hh:mm"),
        (as_24, "hh:mm"),
    ];
    for (real, mode) in [
        ("home/.config/hypr/hyprlock.conf", 0o640),
        ("outside/dots/hyprlock.conf", 0o600),
    ] {
        let snap = run(&conf_at(real, tricky, mode), &general, &pick, &[]);
        for (k, (text, fmt)) in want.iter().enumerate() {
            assert!(
                has(&snap, &format!("step{k} {:?} fmt={fmt}", Some(text))),
                "{k}: {snap:#?}"
            );
        }
        let real_key = real.replacen("home", "~", 1);
        assert!(
            has(&snap, &format!("{real_key}: file {mode:o} {as_24:?}")),
            "{snap:#?}"
        );
        if real.starts_with("outside") {
            let target = PathBuf::from(std::env::var_os("FILE_CMDS_ROOT").unwrap()).join(real);
            assert!(
                has(
                    &snap,
                    &format!("~/.config/hypr/hyprlock.conf: link {}", target.display())
                ),
                "{snap:#?}"
            );
        }
    }
}
