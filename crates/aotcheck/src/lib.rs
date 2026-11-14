//! AOT differential checker (docs/aot.md).
//!
//! usage: aotcheck RI_DIR [--cand DIR] [--backend interp|compiled] [--modules a.qml,b.qml]
//!                        [--out DIR] [--rounds N] [--frames N] [--strict]
//!
//! 1. Inventory: every binding, handler and function of every `.qml` under RI_DIR, keyed
//!    `(rel path, object path, name)` as in `qml::walk`, plus lowering failures and
//!    unsupported constructs of every body and of the `.js` libraries.
//! 2. Builtins: every global, namespace member (`Math.x`, `Qt.x`, ...) and ES prototype
//!    method the IR uses, probed in the loaded reference runtime; unimplemented ones are
//!    listed as missing.
//! 3. Differential: a reference runtime (interpreter) and a candidate runtime (compiled code,
//!    or the interpreter on `--cand DIR`) run the same headless scenario in lockstep on a
//!    pinned clock: load the shell like `ri-native`, then open/close every `GlobalStates`
//!    `*Open` flag with animation frames in between. After each step the canonical world
//!    state (every live property, errors, queued effects) is compared; the first differing
//!    step is attributed to the source keys of the differing bindings. Attributed keys are
//!    quarantined (run interpreted in the candidate) and the scenario is rerun until no new
//!    key diverges, so every key's first divergence is listed.

// std's eprintln! panics once stderr's reader is gone; `elog`'s drops the line.
#![warn(clippy::print_stderr)]

use qml::ir::{Expr, Pat, Stmt};
use qml::jsrt::{Aot, Entry};
use qml::runtime::{Runtime, Sym};
use qml::value::Value;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;

mod v4;

const T0: f64 = 1_767_268_800_000.0; // 2026-01-01T12:00:00Z
const FRAME: f64 = 16.0;
/// Where each runtime's `Math.random` starts.
const RANDOM_SEED: u64 = 0x9e37_79b9_7f4a_7c15;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Backend {
    Interp,
    Compiled,
}

struct Opts {
    root: PathBuf,
    cand: PathBuf,
    backend: Backend,
    modules: Option<Vec<String>>,
    scenes: Option<Vec<String>>,
    out: PathBuf,
    rounds: usize,
    isolate: usize,
    frames: usize,
    strict: bool,
    /// `--probe FILE`: load just FILE (interpreted) and settle, to find files that hang.
    probe: Option<String>,
    /// `--v4 PYTHON`: also replay sampled bindings in Qt's QV4 engine through PySide6 6.12
    /// (see `v4.rs`); differences are divergences.
    v4: Option<PathBuf>,
}

fn opts(args: Vec<String>) -> Opts {
    let mut a = args.into_iter();
    let mut o = Opts {
        root: PathBuf::new(),
        cand: PathBuf::new(),
        backend: Backend::Interp,
        modules: None,
        scenes: None,
        out: PathBuf::from("aotcheck-out"),
        rounds: 16,
        isolate: 500,
        frames: 30,
        strict: false,
        probe: None,
        v4: None,
    };
    while let Some(x) = a.next() {
        let mut val = || a.next().unwrap_or_else(|| panic!("{x}: missing value"));
        match x.as_str() {
            "--cand" => o.cand = val().into(),
            "--backend" => {
                o.backend = match val().as_str() {
                    "interp" => Backend::Interp,
                    "compiled" => Backend::Compiled,
                    b => panic!("unknown backend {b}"),
                }
            }
            "--modules" => o.modules = Some(val().split(',').map(String::from).collect()),
            "--out" => o.out = val().into(),
            "--rounds" => o.rounds = val().parse().expect("--rounds"),
            "--scenes" => o.scenes = Some(val().split(',').map(String::from).collect()),
            "--isolate" => o.isolate = val().parse().expect("--isolate"),
            "--frames" => o.frames = val().parse().expect("--frames"),
            "--strict" => o.strict = true,
            "--probe" => o.probe = Some(val()),
            "--v4" => o.v4 = Some(val().into()),
            _ if o.root.as_os_str().is_empty() => o.root = x.into(),
            _ => panic!("unexpected argument {x}"),
        }
    }
    assert!(
        !o.root.as_os_str().is_empty(),
        "usage: aotcheck RI_DIR [options]"
    );
    if o.cand.as_os_str().is_empty() {
        o.cand = o.root.clone();
    }
    o
}

const DEFAULT_MODULES: &str = "background/Background.qml,screenCorners/ScreenCorners.qml,bar/Bar.qml,sidebarRight/SidebarRight.qml,sidebarLeft/SidebarLeft.qml,overview/Overview.qml,mediaControls/MediaControls.qml,onScreenDisplay/OnScreenDisplay.qml,notificationPopup/NotificationPopup.qml,onScreenKeyboard/OnScreenKeyboard.qml,cheatsheet/Cheatsheet.qml,overlay/Overlay.qml,lock/Lock.qml,polkit/Polkit.qml,regionSelector/RegionSelector.qml,screenTranslator/ScreenTranslator.qml,sessionScreen/SessionScreen.qml,wallpaperSelector/WallpaperSelector.qml";

// ---------------------------------------------------------------- inventory

struct Key {
    key: String,
    kind: &'static str,
    line: usize,
    status: String,
    /// Source text and kind of a script key (the QV4 oracle replays it).
    src: Option<(String, qml::ScriptKind)>,
}

/// Why a loaded compiled key never runs headless, by file prefix (first match wins).
const HEADLESS: &[(&str, &str)] = &[
    ("modules/waffle/lock/", LOCK),
    ("modules/rusty/lock/", LOCK),
    ("modules/common/panels/lock/", LOCK),
    (
        "modules/rusty/screenTranslator/",
        "OCR box delegates: model is tesseract output on a grim screen grab (sandboxed, no output)",
    ),
    (
        "modules/rusty/regionSelector/",
        "region selection: needs a grim screen grab (sandboxed) and a pointer drag over it",
    ),
    (
        "modules/rusty/overview/Search",
        "search result delegates: need a typed query against the app/launcher index",
    ),
    (
        "modules/waffle/startMenu/searchPage/",
        "search result delegates: need a typed query against the app/launcher index",
    ),
    (
        "modules/waffle/startMenu/startPage/",
        "app category delegate: no DesktopEntry categories in the sandbox",
    ),
    (
        "modules/rusty/sidebarLeft/",
        "AI chat messages/pickers: populated from HTTP API responses (no network)",
    ),
    (
        "modules/rusty/overlay/notes/",
        "note line delegates: saved notes file is empty in the fixture",
    ),
    (
        "modules/rusty/overlay/recorder/",
        "recorder buttons: act on a running wf-recorder process (sandboxed)",
    ),
    ("modules/rusty/overlay/volumeMixer/", PIPEWIRE),
    ("modules/rusty/sidebarRight/volumeMixer/", PIPEWIRE),
    ("modules/waffle/actionCenter/volumeControl/", PIPEWIRE),
    ("modules/waffle/bar/tray/", TRAY),
    ("modules/rusty/bar/SysTray", TRAY),
    ("modules/rusty/mediaControls/", MPRIS),
    ("services/MprisController.qml", MPRIS),
    ("modules/rusty/sidebarRight/bluetoothDevices/", BLUEZ),
    ("modules/waffle/actionCenter/bluetooth/", BLUEZ),
    ("modules/rusty/sidebarRight/wifiNetworks/", WIFI),
    ("modules/waffle/actionCenter/wifi/", WIFI),
    ("modules/waffle/notification", NOTIFS),
    ("modules/common/widgets/Notification", NOTIFS),
    (
        "modules/rusty/wallpaperSelector/",
        "wallpaper grid delegates: wallpaper folder is empty in the sandbox",
    ),
    (
        "modules/common/widgets/StyledComboBox.qml",
        "combo box popup delegates: popup list only exists after a pointer press opens it",
    ),
    (
        "modules/waffle/notificationCenter/CalendarWidget.qml",
        "calendar flyout cells: only built when the calendar is expanded by a pointer press",
    ),
    ("modules/common/widgets/CalendarView.qml", CALENDAR),
    ("modules/common/widgets/WeekRow.qml", CALENDAR),
    (
        "modules/rusty/dock/",
        "dock app buttons: pinned/running app entries absent in the fixture",
    ),
    (
        "modules/rusty/cheatsheet/",
        "cheatsheet dismissal/keys: needs the open cheatsheet window focused for key input",
    ),
];
const LOCK: &str = "lock surface: WlSessionLockSurface is only created once the compositor grants ext-session-lock (no Wayland headless)";
const PIPEWIRE: &str = "audio node delegates: no PipeWire daemon headless";
const TRAY: &str =
    "system tray items: StatusNotifierItems come over the D-Bus session bus (unset headless)";
const MPRIS: &str =
    "player delegates: MPRIS players come over the D-Bus session bus (unset headless)";
const BLUEZ: &str = "Bluetooth device delegates: BlueZ over the system bus (no adapter headless)";
const WIFI: &str = "Wi-Fi network delegates: nmcli scan results (sandboxed, no output)";
const NOTIFS: &str =
    "notification action/group delegates: need notifications with actions from a D-Bus client";
const CALENDAR: &str = "default day delegate: every use site supplies its own delegate";

fn headless_reason(key: &str) -> &'static str {
    let mut p = key.split('|');
    let (file, obj) = (p.next().unwrap_or(""), p.next().unwrap_or(""));
    if let Some((_, r)) = HEADLESS.iter().find(|(f, _)| file.starts_with(f)) {
        return r;
    }
    if obj == "0" {
        return "root property default: every instantiation in the scenes binds it instead";
    }
    "inner object default: shadowed by the instantiating component's binding or an unused animation/state"
}

/// Per-file `coverage.tsv` and the `uncovered.tsv` list of keys the candidate never executed.
fn coverage(inv: &Inventory, loaded: &[String], executed: &[String]) -> (String, String) {
    let loaded: BTreeSet<&str> = loaded.iter().map(String::as_str).collect();
    let executed: BTreeSet<&str> = executed.iter().map(String::as_str).collect();
    let mut files: BTreeMap<&str, [usize; 3]> = BTreeMap::new();
    let mut unc = String::from("key\tkind\tline\treason\n");
    for k in &inv.keys {
        let file = k.key.split('|').next().unwrap_or("");
        let c = files.entry(file).or_default();
        c[0] += 1;
        let l = loaded.contains(k.key.as_str());
        let e = executed.contains(k.key.as_str());
        c[1] += l as usize;
        c[2] += e as usize;
        if !e {
            let reason = if k.status != "ok" {
                format!("not compiled: {}", k.status)
            } else if l {
                headless_reason(&k.key).to_string()
            } else {
                "file/component never instantiated".to_string()
            };
            writeln!(unc, "{}\t{}\t{}\t{reason}", k.key, k.kind, k.line).unwrap();
        }
    }
    let mut cov = String::from("file\tkeys\tloaded\texecuted\tpercent\n");
    for (f, [n, l, e]) in &files {
        writeln!(
            cov,
            "{f}\t{n}\t{l}\t{e}\t{:.1}",
            *e as f64 * 100.0 / *n as f64
        )
        .unwrap();
    }
    (cov, unc)
}

#[derive(Default)]
struct Inventory {
    keys: Vec<Key>,
    /// Lowered body pointer (shared with the runtime through the lowering cache) -> keys.
    by_body: HashMap<usize, Vec<usize>>,
    /// Builtin use -> (count, first key).
    uses: BTreeMap<String, (usize, String)>,
    unsupported: BTreeMap<String, (usize, String)>,
    js: Vec<(String, String)>,
    /// Bindings whose parse-time QmlIR shape (constant or JS binding, see [`qml::Shape`])
    /// differs from the one their AST gives: (key, token shape, AST shape).
    shapes: Vec<(String, qml::Shape, qml::Shape)>,
}

fn walk(d: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(d) else { return };
    let mut v: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    v.sort();
    for p in v {
        if p.is_dir() {
            walk(&p, ext, out);
        } else if p.extension().is_some_and(|x| x == ext) {
            out.push(p);
        }
    }
}

/// The `.qml` file an [`Inventory`] walk is in: its key and how its types resolve
/// ([`qml::walk::is_script_action`]).
struct InvFile<'a> {
    rel: &'a str,
    script_action: &'a dyn Fn(&str) -> bool,
}

impl Inventory {
    fn build(root: &Path) -> Inventory {
        let mut inv = Inventory::default();
        let mut files = Vec::new();
        walk(root, "qml", &mut files);
        for f in &files {
            let rel = f.strip_prefix(root).unwrap_or(f).display().to_string();
            let Ok(src) = std::fs::read_to_string(f) else {
                continue;
            };
            match qml::parse(&src) {
                Ok(doc) => {
                    let script_action = |t: &str| qml::walk::is_script_action(root, f, &doc, t);
                    let file = InvFile {
                        rel: &rel,
                        script_action: &script_action,
                    };
                    inv.object(&file, "0", &doc.root)
                }
                Err(e) => inv.keys.push(Key {
                    key: format!("{rel}|-|-"),
                    kind: "file",
                    line: 0,
                    status: format!("parse error: {e:?}"),
                    src: None,
                }),
            }
        }
        let mut js = Vec::new();
        walk(root, "js", &mut js);
        for f in &js {
            let rel = f.strip_prefix(root).unwrap_or(f).display().to_string();
            let src = std::fs::read_to_string(f).unwrap_or_default();
            match qml::ir::lower_program(&src) {
                Ok(b) => {
                    inv.scan(&b, &rel);
                    inv.js.push((rel, "ok".into()));
                }
                Err(e) => inv.js.push((rel, format!("lower error: {e}"))),
            }
        }
        inv
    }

    fn object(&mut self, file: &InvFile, path: &str, o: &qml::Object) {
        let mut n = 0;
        self.members(file, path, &o.type_name, "", &o.members, &mut n);
    }

    fn members(
        &mut self,
        file: &InvFile,
        path: &str,
        ty: &str,
        prefix: &str,
        ms: &[qml::Member],
        n: &mut usize,
    ) {
        use qml::Member as M;
        let rel = file.rel;
        let child = |s: &mut Self, o: &qml::Object, n: &mut usize| {
            s.object(file, &format!("{path}.{n}"), o);
            *n += 1;
        };
        for m in ms {
            match m {
                M::Child(o) | M::On { object: o, .. } | M::Component { object: o, .. } => {
                    child(self, o, n)
                }
                M::Group { name, members } => {
                    self.members(file, path, ty, &format!("{prefix}{name}."), members, n)
                }
                M::Function { name, script } => {
                    self.script(rel, path, &format!("{prefix}{name}"), "function", script)
                }
                // An alias target is a property path, not JS.
                M::Property {
                    ty: pty,
                    value: Some(qml::Value::Script(_)),
                    ..
                } if pty == "alias" => {}
                M::Property {
                    name,
                    value: Some(v),
                    ..
                }
                | M::Binding { name, value: v } => {
                    let full = format!("{prefix}{name}");
                    match v {
                        qml::Value::Script(s) => {
                            // A `property` declaration is a binding whatever its name.
                            let kind = match m {
                                M::Binding { .. } => {
                                    qml::walk::binding_kind(&full, || (file.script_action)(ty))
                                }
                                _ => qml::walk::BodyKind::Binding,
                            };
                            let kind = match kind {
                                qml::walk::BodyKind::Handler => "handler",
                                _ => "binding",
                            };
                            self.script(rel, path, &full, kind, s)
                        }
                        qml::Value::Object(o) => child(self, o, n),
                        qml::Value::List(os) => os.iter().for_each(|o| child(self, o, n)),
                    }
                }
                _ => {}
            }
        }
    }

    fn script(&mut self, rel: &str, path: &str, name: &str, kind: &'static str, s: &qml::Script) {
        let key = format!("{rel}|{path}|{name}");
        // A source oxc does not parse as a program is no literal or member chain either.
        let ast = qml::ir::qmlir_shape(s).unwrap_or(qml::Shape::Script);
        if ast != s.shape {
            self.shapes.push((key.clone(), s.shape.clone(), ast));
        }
        let i = self.keys.len();
        let status = match qml::ir::lower_rc(s) {
            Ok(b) => {
                self.by_body.entry(Rc_ptr(&b)).or_default().push(i);
                let mut u = Vec::new();
                qml::ir::unsupported(&b, &mut u);
                for x in &u {
                    let e = self
                        .unsupported
                        .entry(x.clone())
                        .or_insert((0, key.clone()));
                    e.0 += 1;
                }
                self.scan(&b, &key);
                if u.is_empty() {
                    "ok".to_string()
                } else {
                    format!("unsupported: {}", u.join(", "))
                }
            }
            Err(e) => format!("lower error: {e}"),
        };
        self.keys.push(Key {
            key,
            kind,
            line: s.line,
            status,
            src: Some((s.src.to_string(), s.kind)),
        });
    }

    fn scan(&mut self, body: &[Stmt], key: &str) {
        let mut found = Vec::new();
        body.iter().for_each(|s| stmt(s, &mut found));
        for f in found {
            let e = self.uses.entry(f).or_insert((0, key.to_string()));
            e.0 += 1;
        }
    }
}

#[allow(non_snake_case)]
fn Rc_ptr<T>(b: &std::rc::Rc<T>) -> usize {
    std::rc::Rc::as_ptr(b) as *const () as usize
}

// ---------------------------------------------------------------- builtin usage scan

const GLOBALS: &[&str] = &[
    "Math",
    "JSON",
    "Object",
    "Array",
    "Number",
    "String",
    "Boolean",
    "Date",
    "RegExp",
    "Map",
    "Set",
    "WeakMap",
    "WeakSet",
    "Promise",
    "Symbol",
    "Error",
    "TypeError",
    "RangeError",
    "SyntaxError",
    "parseInt",
    "parseFloat",
    "isNaN",
    "isFinite",
    "encodeURIComponent",
    "decodeURIComponent",
    "encodeURI",
    "decodeURI",
    "escape",
    "unescape",
    "setTimeout",
    "clearTimeout",
    "setInterval",
    "clearInterval",
    "console",
    "Qt",
    "qsTr",
    "qsTranslate",
    "Infinity",
    "NaN",
    "Reflect",
    "Proxy",
    "Intl",
    "ArrayBuffer",
    "Uint8Array",
    "Int32Array",
    "Float32Array",
    "globalThis",
    "BigInt",
    "XMLHttpRequest",
    "TextEncoder",
    "TextDecoder",
    "atob",
    "btoa",
    "queueMicrotask",
    "structuredClone",
];
const NAMESPACES: &[&str] = &[
    "Math", "JSON", "Object", "Array", "Number", "String", "Date", "Promise", "Reflect", "Symbol",
    "console", "Qt", "Intl", "RegExp",
];
/// ES prototype methods by sample receiver.
const PROTOS: &[(&str, &[&str])] = &[
    (
        "[]",
        &[
            "at",
            "concat",
            "copyWithin",
            "entries",
            "every",
            "fill",
            "filter",
            "find",
            "findIndex",
            "findLast",
            "findLastIndex",
            "flat",
            "flatMap",
            "forEach",
            "includes",
            "indexOf",
            "join",
            "keys",
            "lastIndexOf",
            "map",
            "pop",
            "push",
            "reduce",
            "reduceRight",
            "reverse",
            "shift",
            "slice",
            "some",
            "sort",
            "splice",
            "toReversed",
            "toSorted",
            "toSpliced",
            "unshift",
            "values",
            "with",
            "toString",
            "toLocaleString",
        ],
    ),
    (
        "''",
        &[
            "at",
            "charAt",
            "charCodeAt",
            "codePointAt",
            "concat",
            "endsWith",
            "includes",
            "indexOf",
            "lastIndexOf",
            "localeCompare",
            "match",
            "matchAll",
            "normalize",
            "padEnd",
            "padStart",
            "repeat",
            "replace",
            "replaceAll",
            "search",
            "slice",
            "split",
            "startsWith",
            "substring",
            "substr",
            "toLowerCase",
            "toUpperCase",
            "toLocaleLowerCase",
            "toLocaleUpperCase",
            "trim",
            "trimStart",
            "trimEnd",
            "arg",
            "toString",
            "valueOf",
        ],
    ),
    (
        "(0)",
        &[
            "toFixed",
            "toPrecision",
            "toString",
            "toLocaleString",
            "toExponential",
            "valueOf",
        ],
    ),
    (
        "new Date(0)",
        &[
            "getDate",
            "getDay",
            "getFullYear",
            "getHours",
            "getMilliseconds",
            "getMinutes",
            "getMonth",
            "getSeconds",
            "getTime",
            "getTimezoneOffset",
            "getUTCDate",
            "getUTCDay",
            "getUTCFullYear",
            "getUTCHours",
            "getUTCMinutes",
            "getUTCMonth",
            "getUTCSeconds",
            "setDate",
            "setFullYear",
            "setHours",
            "setMilliseconds",
            "setMinutes",
            "setMonth",
            "setSeconds",
            "setTime",
            "toISOString",
            "toJSON",
            "toLocaleDateString",
            "toLocaleTimeString",
            "toLocaleString",
            "toDateString",
            "toTimeString",
            "toUTCString",
            "valueOf",
        ],
    ),
    ("/x/", &["exec", "test"]),
    (
        "new Map()",
        &[
            "get", "set", "has", "delete", "clear", "forEach", "keys", "values", "entries",
        ],
    ),
    (
        "new Set()",
        &[
            "add", "has", "delete", "clear", "forEach", "values", "keys", "entries",
        ],
    ),
    ("Promise.resolve()", &["then", "catch", "finally"]),
    ("(function(){})", &["call", "apply", "bind"]),
    (
        "({})",
        &[
            "hasOwnProperty",
            "isPrototypeOf",
            "propertyIsEnumerable",
            "toString",
            "valueOf",
        ],
    ),
];

fn expr(x: &Expr, out: &mut Vec<String>) {
    match x.shown() {
        Expr::Ident(n) if GLOBALS.contains(&n.as_str()) => out.push(n.clone()),
        Expr::Member(a, m, _) => {
            if let Expr::Ident(g) = a.shown()
                && NAMESPACES.contains(&g.as_str())
            {
                out.push(format!("{g}.{}", m.as_str()));
            }
            expr(a, out)
        }
        Expr::Template(_, v) | Expr::Array(v) | Expr::Seq(v) => v.iter().for_each(|x| expr(x, out)),
        Expr::Yield(a, _) => a.iter().for_each(|x| expr(x, out)),
        Expr::Chain(a) | Expr::Unary(_, a) | Expr::Spread(a) => expr(a, out),
        Expr::Index(a, b, _) | Expr::Binary(_, a, b) | Expr::Logical(_, a, b) => {
            expr(a, out);
            expr(b, out)
        }
        Expr::Call(a, v, _) | Expr::New(a, v) => {
            if let Expr::Member(r, m, _) = &**a {
                let m = m.as_str();
                let global_ns =
                    matches!(r.shown(), Expr::Ident(g) if NAMESPACES.contains(&g.as_str()));
                if !global_ns && PROTOS.iter().any(|(_, ms)| ms.contains(&m)) {
                    out.push(format!(".{m}"));
                }
            }
            expr(a, out);
            v.iter().for_each(|x| expr(x, out))
        }
        Expr::Cond(a, b, c) => [a, b, c].iter().for_each(|x| expr(x, out)),
        Expr::Assign(_, p, b) => {
            pat(p, out);
            expr(b, out)
        }
        Expr::Update(_, _, p) => pat(p, out),
        Expr::Object(v) => v.iter().for_each(|(k, x, _)| {
            if let qml::ir::Key::Computed(k) = k {
                expr(k, out)
            }
            expr(x, out)
        }),
        Expr::Func(f) => f.body.iter().for_each(|s| stmt(s, out)),
        Expr::Class(c) => class(c, out),
        _ => {}
    }
}

fn class(c: &qml::ir::Class, out: &mut Vec<String>) {
    if let Some(s) = &c.extends {
        expr(s, out)
    }
    if let Some(f) = &c.ctor {
        f.body.iter().for_each(|s| stmt(s, out))
    }
    for m in &c.methods {
        m.func.body.iter().for_each(|s| stmt(s, out))
    }
    for (_, x, _) in &c.fields {
        if let Some(x) = x {
            expr(x, out)
        }
    }
    c.blocks.iter().for_each(|(_, x)| expr(x, out));
}

fn pat(p: &Pat, out: &mut Vec<String>) {
    match p.shown() {
        Pat::Member(x) | Pat::Init(x) => expr(x, out),
        Pat::Default(p, x) => {
            pat(p, out);
            expr(x, out)
        }
        _ => {}
    }
}

fn stmt(s: &Stmt, out: &mut Vec<String>) {
    let blk = |b: &[Stmt], out: &mut Vec<String>| b.iter().for_each(|s| stmt(s, out));
    match s {
        Stmt::Expr(x) | Stmt::Throw(x) | Stmt::Return(Some(x)) => expr(x, out),
        Stmt::Var(_, d) => d.iter().for_each(|(p, x)| {
            pat(p, out);
            if let Some(x) = x {
                expr(x, out)
            }
        }),
        Stmt::Func(f) => blk(&f.body, out),
        Stmt::If(c, a, b) => {
            expr(c, out);
            stmt(a, out);
            if let Some(b) = b {
                stmt(b, out)
            }
        }
        Stmt::Block(b) => blk(b, out),
        Stmt::Labeled(_, b) => stmt(b, out),
        Stmt::Class(c) => class(c, out),
        Stmt::For(i, t, u, b) => {
            if let Some(i) = i {
                stmt(i, out)
            }
            t.iter().chain(u.iter()).for_each(|x| expr(x, out));
            stmt(b, out)
        }
        Stmt::ForOf(_, p, x, b) | Stmt::ForIn(_, p, x, b) => {
            pat(p, out);
            expr(x, out);
            stmt(b, out)
        }
        Stmt::While(x, b) | Stmt::DoWhile(b, x) | Stmt::With(x, b) => {
            expr(x, out);
            stmt(b, out)
        }
        Stmt::Switch(x, cs) => {
            expr(x, out);
            cs.iter().for_each(|(t, b)| {
                if let Some(t) = t {
                    expr(t, out)
                }
                blk(b, out)
            })
        }
        Stmt::Try(a, h, f) => {
            blk(a, out);
            if let Some((p, h)) = h {
                if let Some(p) = p {
                    pat(p, out)
                }
                blk(h, out)
            }
            if let Some(f) = f {
                blk(f, out)
            }
        }
        _ => {}
    }
}

/// JS expression(s) whose `typeof` tells whether a recorded use is implemented.
fn probes(u: &str) -> Vec<String> {
    if let Some(m) = u.strip_prefix('.') {
        PROTOS
            .iter()
            .filter(|(_, ms)| ms.contains(&m))
            .map(|(r, _)| format!("{r}.{m}"))
            .collect()
    } else {
        vec![u.to_string()]
    }
}

// ---------------------------------------------------------------- runtime workers

struct St {
    rt: Runtime,
    root: PathBuf,
    inv: Inventory,
    now: f64,
    /// Keys the candidate loads compiled.
    used: Rc<RefCell<BTreeSet<String>>>,
    /// `FnMeta` address of every admitted entry -> its key (see `jsrt::track_runs`).
    metas: Rc<RefCell<HashMap<usize, String>>>,
    /// `file\tmissing/total` of documents whose scripts the compiled backend lacks: the
    /// candidate silently interprets those, so they void the comparison.
    interpreted: Rc<RefCell<BTreeSet<String>>>,
    /// Root objects of the scene's files, in load order (`@0` in [`Act::Set`]).
    loaded: Vec<usize>,
    /// `FnMeta` addresses of compiled bodies run so far (see `jsrt::track_runs`).
    ran: std::collections::HashSet<usize>,
    /// Drive targets already tried.
    tried: BTreeSet<String>,
    /// Whether the never-instantiated files were loaded standalone yet.
    standalone: bool,
    /// Runtime errors when the first generic drive step starts: the scene steps' errors.
    drive_mark: Option<Vec<String>>,
    /// Keys earlier scenes of the round already executed / loaded: not driven again.
    done_keys: std::collections::HashSet<String>,
    done_files: std::collections::HashSet<String>,
    /// Commands the scene ran (sandboxed), with counts.
    cmds: Rc<RefCell<BTreeMap<String, usize>>>,
    /// Bindings sampled for the QV4 oracle (`--v4`).
    v4: v4::Sink,
}

/// FileView inputs `(path, nth read)` the reference recorded; the candidate replays them.
type Tape = std::sync::Arc<std::sync::Mutex<HashMap<(String, usize), Option<String>>>>;

type Job = Box<dyn FnOnce(&mut St) -> Vec<String> + Send>;

struct Worker {
    tx: mpsc::Sender<Job>,
    rx: mpsc::Receiver<Vec<String>>,
}

impl Worker {
    fn spawn(
        root: PathBuf,
        backend: Backend,
        sel: Select,
        compiled: Option<AotFactory>,
        tape: Tape,
        record: bool,
    ) -> Worker {
        let (tx, jobs) = mpsc::channel::<Job>();
        let (done, rx) = mpsc::channel();
        std::thread::Builder::new()
            .stack_size(1 << 29)
            .spawn(move || {
                qml::builtins::set_clock(Some(T0));
                // Both runtimes draw the same `Math.random` numbers.
                qml::builtins::set_random_seed(RANDOM_SEED);
                // Wall time must not decide a step's outcome: evaluations are bounded by a
                // step budget instead of the watchdog's timeout, and DesktopEntries scans land
                // at the poll that started them.
                qml::watchdog::set_timeout(None);
                qml::watchdog::set_step_budget(Some(STEP_BUDGET));
                qml::desktop::set_inline_scans(true);
                let mut seen: HashMap<String, usize> = HashMap::new();
                qml::native::set_read_hook(Some(Box::new(move |p: &str| {
                    let n = seen.entry(p.to_string()).or_default();
                    let k = (p.to_string(), *n);
                    *n += 1;
                    let mut t = tape.lock().unwrap();
                    if !record && let Some(v) = t.get(&k) {
                        return v.clone();
                    }
                    let v = std::fs::read_to_string(p).ok();
                    t.insert(k, v.clone());
                    v
                })));
                let inv = Inventory::build(&root);
                let mut st = St {
                    rt: Runtime::new(Some(root.clone())),
                    root,
                    inv,
                    now: 0.0,
                    used: Default::default(),
                    metas: Default::default(),
                    interpreted: Default::default(),
                    loaded: Vec::new(),
                    ran: Default::default(),
                    tried: Default::default(),
                    standalone: false,
                    drive_mark: None,
                    done_keys: Default::default(),
                    done_files: Default::default(),
                    cmds: Default::default(),
                    v4: Default::default(),
                };
                qml::jsrt::track_runs(backend == Backend::Compiled);
                let cmds = st.cmds.clone();
                qml::native::set_spawn_hook(Some(Box::new(move |argv: &[String]| {
                    // A command's first three words, or an `Impulse` call's type, method and
                    // first argument.
                    let k = argv.iter().take(3).cloned().collect::<Vec<_>>().join(" ");
                    *cmds.borrow_mut().entry(k).or_default() += 1;
                    command_tape(argv)
                })));
                qml::native::set_no_writes(true);
                if backend == Backend::Compiled
                    && let Some(f) = compiled
                {
                    install_compiled(&mut st, f, &sel);
                }
                for j in jobs {
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| j(&mut st)));
                    HANGS.fetch_add(qml::watchdog::take_exhausted(), Ordering::Relaxed);
                    let (v, dead) = match r {
                        Ok(v) => (v, false),
                        Err(p) => (
                            vec![PANIC.into(), format!("{backend:?}"), panic_text(&*p)],
                            true,
                        ),
                    };
                    if done.send(v).is_err() || dead {
                        break;
                    }
                }
            })
            .expect("spawn worker");
        Worker { tx, rx }
    }

    fn ask(&self, j: impl FnOnce(&mut St) -> Vec<String> + Send + 'static) -> Vec<String> {
        self.tx.send(Box::new(j)).expect("worker gone");
        let v = self.rx.recv().expect("worker gone");
        if v.first().map(String::as_str) == Some(PANIC) {
            std::panic::panic_any(Panicked {
                backend: v[1].clone(),
                msg: v[2].clone(),
            });
        }
        v
    }
}

const PANIC: &str = "\0panic";

/// Interrupt checks (loop back-edges and calls) one evaluation may pass. The slowest
/// legitimate evaluation of `qml` passes ~143k (the welcome language switch rebuilding every
/// translated delegate); 10M leaves 70x headroom while a runaway loop still fails in about a
/// second of CPU time, even interpreted.
const STEP_BUDGET: u64 = 10_000_000;

/// Evaluations, on any worker, that exhausted [`STEP_BUDGET`]: hangs, which fail the check.
static HANGS: AtomicU64 = AtomicU64::new(0);

/// A worker's runtime panicked; the round ends there.
struct Panicked {
    backend: String,
    msg: String,
}

fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    p.downcast_ref::<String>()
        .cloned()
        .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_else(|| "?".into())
}

/// Root causes among the quarantined keys: each one that diverges when it is the only compiled
/// key (the rest of the quarantine are downstream of those). None when not run (interp backend,
/// nothing quarantined, more than `--isolate` keys).
fn isolate(
    o: &Opts,
    quarantine: &BTreeSet<String>,
    compiled: Option<AotFactory>,
) -> Option<BTreeSet<String>> {
    if o.backend != Backend::Compiled || quarantine.is_empty() || quarantine.len() > o.isolate {
        return None;
    }
    let queue = std::sync::Mutex::new(quarantine.iter().cloned().collect::<Vec<_>>());
    let roots = std::sync::Mutex::new(BTreeSet::new());
    let par = std::thread::available_parallelism()
        .map_or(2, |n| n.get() / 2)
        .max(1);
    std::thread::scope(|s| {
        for _ in 0..par {
            s.spawn(|| {
                while let Some(k) = queue.lock().unwrap().pop() {
                    let sel = Select {
                        keys: [k.clone()].into(),
                        only: true,
                    };
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        run_round(o, 0, &sel, compiled, false)
                    }));
                    if r.map_or(true, |r| !r.divergences.is_empty()) {
                        roots.lock().unwrap().insert(k);
                    }
                }
            });
        }
    });
    Some(roots.into_inner().unwrap())
}

/// Report section of a key file: `rusty/bar` for `modules/rusty/bar/...`, `common` for
/// `modules/common/...`, else its first directory (`services`) or `(root)`.
fn section(file: &str) -> String {
    let parts: Vec<&str> = file.split('/').collect();
    match parts.as_slice() {
        ["modules", "rusty", sub, _, ..] => format!("rusty/{sub}"),
        ["modules", m, _, ..] => (*m).to_string(),
        [d, _, ..] => (*d).to_string(),
        _ => "(root)".into(),
    }
}

/// The `file|objpath|member` key a panic message names, if any.
fn panic_key(msg: &str) -> Option<String> {
    msg.split_whitespace()
        .find(|w| w.matches('|').count() == 2)
        .map(str::to_string)
}

/// Builds the compiled code of a candidate runtime for the QML tree `root` (ricompiled).
pub type AotFactory = fn(root: &Path) -> Rc<dyn Aot>;

/// Which compiled keys a candidate runs: all but `keys` (quarantine), or `only` those.
#[derive(Clone, Default)]
struct Select {
    keys: BTreeSet<String>,
    only: bool,
}

impl Select {
    fn admits(&self, key: &str) -> bool {
        self.keys.contains(key) == self.only
    }
}

/// Compiled code filtered by a [`Select`]; records the keys it hands out.
struct Filter {
    inner: Rc<dyn Aot>,
    sel: Select,
    used: Rc<RefCell<BTreeSet<String>>>,
    metas: Rc<RefCell<HashMap<usize, String>>>,
    interpreted: Rc<RefCell<BTreeSet<String>>>,
    root: PathBuf,
}

impl Filter {
    fn admit(&self, key: String, e: &'static Entry) -> bool {
        if !self.sel.admits(&key) {
            return false;
        }
        self.metas
            .borrow_mut()
            .insert(&e.meta as *const _ as usize, key.clone());
        self.used.borrow_mut().insert(key);
        true
    }
}

impl Aot for Filter {
    fn doc_scripts(&self, file: &Path, doc: &qml::Document) -> Vec<(qml::NodeId, &'static Entry)> {
        let mut v = self.inner.doc_scripts(file, doc);
        let mut total = 0;
        qml::walk::sites(doc, &mut |_| total += 1);
        if v.len() < total {
            let f = file.strip_prefix(&self.root).unwrap_or(file);
            self.interpreted.borrow_mut().insert(format!(
                "{}\t{}/{total}",
                f.display(),
                total - v.len()
            ));
        }
        v.retain(|(_, e)| self.admit(format!("{}|{}|{}", e.file, e.path, e.member), e));
        v
    }

    fn library(&self, file: &Path) -> Option<&'static Entry> {
        self.inner
            .library(file)
            .filter(|e| self.admit(e.file.to_string(), e))
    }

    fn script(&self, file: &Path) -> Option<&'static Entry> {
        self.inner
            .script(file)
            .filter(|e| self.admit(e.file.to_string(), e))
    }
}

/// Run the keys `sel` admits compiled (`Runtime::set_compiled`, before anything loads).
fn install_compiled(st: &mut St, f: AotFactory, sel: &Select) {
    let aot = Rc::new(Filter {
        inner: f(&st.root),
        sel: sel.clone(),
        used: st.used.clone(),
        metas: st.metas.clone(),
        interpreted: st.interpreted.clone(),
        root: st.root.clone(),
    });
    st.rt.eng().set_compiled(Some(aot));
}

#[derive(Clone)]
enum Act {
    Load(Load),
    Frames(usize),
    /// `(singleton name or @n for the nth loaded file, dotted property path, value)`.
    Set(String, String, Lit),
    /// `item\tkind\tname` targets [`plan_drive`] picked: fire handler (`h`), call function
    /// (`f`), activate Loader (`a`), load a file standalone (`l`, item unused).
    Drive(Vec<String>),
}

#[derive(Clone)]
enum Lit {
    Bool(bool),
    Num(f64),
    Str(String),
}

impl Lit {
    fn value(&self) -> Value {
        match self {
            Lit::Bool(b) => Value::Bool(*b),
            Lit::Num(n) => Value::Num(*n),
            Lit::Str(s) => Value::Str(s.as_str().into()),
        }
    }
}

/// What a scene loads: files (root-relative), extra singleton dirs (`dir/*` = its subdirs),
/// properties set after the singletons load, and whether `shell.qml` loads too.
#[derive(Clone, Default)]
struct Load {
    files: Vec<String>,
    dirs: Vec<String>,
    sets: Vec<(String, String, Lit)>,
    shell: bool,
}

/// One fresh pair of runtimes: a load, extra steps, and (with `flags`) every GlobalStates
/// bool toggled and restored.
struct Scene {
    name: &'static str,
    load: Load,
    steps: Vec<(String, Act)>,
    flags: bool,
}

const DEFAULT_SCENES: &[&str] = &["rusty", "rusty-vertical", "waffle", "settings"];

const WAFFLE_MODULES: &str = "actionCenter/WaffleActionCenter.qml,bar/WaffleBar.qml,background/WaffleBackground.qml,lock/WaffleLock.qml,notificationCenter/WaffleNotificationCenter.qml,notificationPopup/WaffleNotificationPopup.qml,onScreenDisplay/WaffleOSD.qml,polkit/WafflePolkit.qml,screenSnip/WScreenSnip.qml,startMenu/WaffleStartMenu.qml,sessionScreen/WaffleSessionScreen.qml,taskView/WaffleTaskView.qml";

/// The panel families and standalone windows of the tree: the original, the original with the vertical bar, waffle,
/// and the settings/welcome windows. `--modules` runs just those QML modules.
/// `GlobalStates` panel flags the panel-state steps open.
const PANELS: &[&str] = &[
    "sidebarLeftOpen",
    "sidebarRightOpen",
    "overviewOpen",
    "searchOpen",
    "sessionOpen",
    "mediaControlsOpen",
    "osdVolumeOpen",
    "osdBrightnessOpen",
    "oskOpen",
    "overlayOpen",
    "crosshairOpen",
    "regionSelectorOpen",
    "screenTranslatorOpen",
    "wallpaperSelectorOpen",
    "workspaceShowNumbers",
    "superDown",
];

fn pages_set(steps: &mut Vec<(String, Act)>, o: &Opts, flag: &str, v: bool) {
    steps.push((
        format!("{flag} = {v}"),
        Act::Set("GlobalStates".into(), flag.into(), Lit::Bool(v)),
    ));
    steps.push((format!("{flag} settled"), Act::Frames(o.frames)));
}

fn scenes(o: &Opts) -> Vec<Scene> {
    let under = |dir: &str, list: &str| -> Vec<String> {
        list.split(',').map(|m| format!("{dir}/{m}")).collect()
    };
    let shell = |files: Vec<String>, sets: Vec<(String, String, Lit)>, dirs: Vec<String>| Load {
        files,
        dirs,
        sets,
        shell: true,
    };
    if let Some(m) = &o.modules {
        return vec![Scene {
            name: "rusty",
            load: shell(under("modules/rusty", &m.join(",")), vec![], vec![]),
            steps: vec![],
            flags: true,
        }];
    }
    let cfg = |p: &str, v: Lit| ("Config".to_string(), format!("options.{p}"), v);
    let mut pages = Vec::new();
    for p in 1..8 {
        pages.push((
            format!("page {p}"),
            Act::Set("@0".into(), "currentPage".into(), Lit::Num(p as f64)),
        ));
        pages.push((format!("page {p} settled"), Act::Frames(o.frames)));
    }
    // Every panel opened in turn (all stay open), the lock screen last.
    let mut panels = Vec::new();
    for p in PANELS {
        pages_set(&mut panels, o, p, true);
    }
    pages_set(&mut panels, o, "screenLocked", true);
    let v = vec![
        Scene {
            name: "rusty",
            load: shell(under("modules/rusty", DEFAULT_MODULES), vec![], vec![]),
            steps: panels.clone(),
            flags: true,
        },
        Scene {
            name: "rusty-vertical",
            load: shell(
                vec!["modules/rusty/verticalBar/VerticalBar.qml".into()],
                vec![cfg("bar.vertical", Lit::Bool(true))],
                vec![],
            ),
            steps: vec![],
            flags: true,
        },
        Scene {
            name: "waffle",
            load: shell(
                under("modules/waffle", WAFFLE_MODULES),
                vec![cfg("panelFamily", Lit::Str("waffle".into()))],
                vec!["modules/waffle/*".into()],
            ),
            steps: panels,
            flags: true,
        },
        Scene {
            name: "settings",
            load: Load {
                files: vec!["settings.qml".into(), "welcome.qml".into()],
                ..Load::default()
            },
            steps: pages,
            flags: false,
        },
    ];
    v.into_iter()
        .filter(|sc| match &o.scenes {
            Some(v) => v.iter().any(|n| n == sc.name),
            None => DEFAULT_SCENES.contains(&sc.name),
        })
        .filter_map(|mut sc| {
            sc.load.files.retain(|f| o.root.join(f).exists());
            (!sc.load.files.is_empty()).then_some(sc)
        })
        .collect()
}

/// Set `path` (`options.bar.vertical`) on a singleton or `@n` loaded root; false if unresolved.
fn set_path(st: &mut St, target: &str, path: &str, v: Value) -> bool {
    let start = match target.strip_prefix('@') {
        Some(n) => n
            .parse::<usize>()
            .ok()
            .and_then(|n| st.loaded.get(n).copied()),
        None => st.rt.world.borrow().singletons.get(target).copied(),
    };
    let Some(mut id) = start else {
        return false;
    };
    let mut segs: Vec<&str> = path.split('.').collect();
    let last = segs.pop().unwrap_or_default();
    for seg in segs {
        match st.rt.get(id, seg) {
            Value::Item(n) => id = n,
            _ => return false,
        }
    }
    st.rt.set(id, last, v);
    true
}

fn act(st: &mut St, a: &Act) -> Vec<String> {
    match a {
        Act::Load(l) => {
            let root = st.root.clone();
            let mut dirs = vec![
                root.join("modules/common"),
                root.join("modules/common/functions"),
                root.join("modules/common/utils"),
                root.join("services"),
                root.clone(),
            ];
            if let Ok(rd) = std::fs::read_dir(root.join("modules/rusty")) {
                let mut d: Vec<_> = rd
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .collect();
                d.sort();
                dirs.extend(d);
            }
            for d in &l.dirs {
                match d.strip_suffix("/*") {
                    Some(p) => {
                        let mut sub: Vec<_> = std::fs::read_dir(root.join(p))
                            .into_iter()
                            .flatten()
                            .flatten()
                            .map(|e| e.path())
                            .filter(|p| p.is_dir())
                            .collect();
                        sub.sort();
                        dirs.extend(sub);
                    }
                    None => dirs.push(root.join(d)),
                }
            }
            st.rt.load_singletons(&dirs);
            st.rt.set_screens(&[("DP-1".into(), 0, 0, 1920, 1080)]);
            let mut log = Vec::new();
            for (t, p, v) in &l.sets {
                if !set_path(st, t, p, v.value()) {
                    log.push(format!("set {t}.{p}: not found"));
                }
            }
            for m in &l.files {
                match st.rt.load_file(&root.join(m)) {
                    Ok(id) => st.loaded.push(id),
                    Err(e) => log.push(format!("load {m}: {e}")),
                }
            }
            let shell = root.join("shell.qml");
            if l.shell
                && shell.exists()
                && let Err(e) = st.rt.load_file_without(&shell, &["PanelFamilyLoader"])
            {
                log.push(format!("load shell.qml: {e}"));
            }
            log
        }
        Act::Frames(n) => {
            for _ in 0..*n {
                st.now += FRAME;
                qml::builtins::set_clock(Some(T0 + st.now));
                // Sandboxed process output and the fake compositor, as the event loop would.
                st.rt.poll_hyprland(Vec::new());
                qml::native::poll_processes(&mut st.rt.world.borrow_mut());
                st.rt.tick(st.now);
            }
            Vec::new()
        }
        Act::Set(target, path, v) => {
            set_path(st, target, path, v.value());
            Vec::new()
        }
        Act::Drive(ts) => {
            if st.drive_mark.is_none() {
                st.drive_mark = Some(st.rt.errors());
            }
            for t in ts {
                let mut p = t.splitn(3, '\t');
                let (Some(i), Some(kind), Some(name)) = (p.next(), p.next(), p.next()) else {
                    continue;
                };
                let i: usize = i.parse().unwrap_or(0);
                match kind {
                    "h" => {
                        let ev = input_event();
                        st.rt
                            .fire(i, name, EVENT_PARAMS, vec![ev; EVENT_PARAMS.len()]);
                    }
                    "f" => {
                        let _ = st.rt.call_method(i, name, vec![]);
                    }
                    "a" => st.rt.set(i, "active", Value::Bool(true)),
                    "l" => {
                        if std::env::var_os("AOTCHECK_TRACE").is_some() {
                            elog::eprintln!("aotcheck: standalone {name}");
                        }
                        // A file no one instantiates has no creator to set its required
                        // properties: they keep their type's default, set explicitly.
                        let file = st.root.join(name);
                        if let Ok(id) = st.rt.load_file_init(&file, &|_, v| Some(v.clone())) {
                            st.loaded.push(id);
                        }
                    }
                    _ => {}
                }
            }
            Vec::new()
        }
    }
}

/// Drive passes after the scene's own steps; each fires what the previous one revealed.
const DRIVE_ROUNDS: usize = 16;
/// Drive targets per snapshot step.
const DRIVE_CHUNK: usize = 64;

/// Names a fired handler sees its argument under (`mouse`, `event`, ...); all get the same
/// [`input_event`], which covers pointer, wheel and key events.
const EVENT_PARAMS: &[&str] = &["mouse", "event", "wheel", "drag", "eventPoint"];

fn input_event() -> Value {
    let pt = |x: f64, y: f64| Runtime::js_object(&[("x", Value::Num(x)), ("y", Value::Num(y))]);
    Runtime::js_object(&[
        ("button", Value::Num(1.0)),
        ("buttons", Value::Num(1.0)),
        ("x", Value::Num(4.0)),
        ("y", Value::Num(4.0)),
        ("modifiers", Value::Num(0.0)),
        ("key", Value::Num(0x0100_0000 as f64)),
        ("text", Value::str("a")),
        ("accepted", Value::Bool(false)),
        ("isAutoRepeat", Value::Bool(false)),
        ("wasHeld", Value::Bool(false)),
        ("angleDelta", pt(0.0, 120.0)),
        ("pixelDelta", pt(0.0, 0.0)),
        ("position", pt(4.0, 4.0)),
    ])
}

/// Deterministic compositor state served to both workers (`hyprctl -j`, Hyprland IPC).
fn hypr_json(what: &str) -> Option<&'static str> {
    Some(match what {
        "monitors" => {
            r#"[{"id":0,"name":"DP-1","description":"Virtual DP-1","make":"","model":"","serial":"","width":1920,"height":1080,"refreshRate":60.0,"x":0,"y":0,"activeWorkspace":{"id":1,"name":"1"},"specialWorkspace":{"id":0,"name":""},"reserved":[0,0,0,0],"scale":1.0,"transform":0,"focused":true,"dpmsStatus":true,"vrr":false,"disabled":false,"availableModes":["1920x1080@60.00Hz"]}]"#
        }
        "workspaces" => {
            r#"[{"id":1,"name":"1","monitor":"DP-1","monitorID":0,"windows":2,"hasfullscreen":false,"lastwindow":"0xa1","lastwindowtitle":"foot"},{"id":2,"name":"2","monitor":"DP-1","monitorID":0,"windows":1,"hasfullscreen":false,"lastwindow":"0xa3","lastwindowtitle":"Files"}]"#
        }
        "activeworkspace" => {
            r#"{"id":1,"name":"1","monitor":"DP-1","monitorID":0,"windows":2,"hasfullscreen":false,"lastwindow":"0xa1","lastwindowtitle":"foot"}"#
        }
        "clients" => concat!(
            r#"[{"address":"0xa1","mapped":true,"hidden":false,"at":[10,40],"size":[900,600],"workspace":{"id":1,"name":"1"},"floating":false,"pseudo":false,"monitor":0,"class":"foot","title":"foot","initialClass":"foot","initialTitle":"foot","pid":101,"xwayland":false,"pinned":false,"fullscreen":0,"fullscreenClient":0,"grouped":[],"tags":[],"swallowing":"0x0","focusHistoryID":0,"inhibitingIdle":false},"#,
            r#"{"address":"0xa2","mapped":true,"hidden":false,"at":[920,40],"size":[990,1030],"workspace":{"id":1,"name":"1"},"floating":true,"pseudo":false,"monitor":0,"class":"firefox","title":"Mozilla Firefox","initialClass":"firefox","initialTitle":"Mozilla Firefox","pid":102,"xwayland":false,"pinned":false,"fullscreen":0,"fullscreenClient":0,"grouped":[],"tags":[],"swallowing":"0x0","focusHistoryID":1,"inhibitingIdle":false},"#,
            r#"{"address":"0xa3","mapped":true,"hidden":false,"at":[0,0],"size":[1920,1080],"workspace":{"id":2,"name":"2"},"floating":false,"pseudo":false,"monitor":0,"class":"org.kde.dolphin","title":"Files","initialClass":"org.kde.dolphin","initialTitle":"Files","pid":103,"xwayland":true,"pinned":false,"fullscreen":2,"fullscreenClient":2,"grouped":[],"tags":[],"swallowing":"0x0","focusHistoryID":2,"inhibitingIdle":false}]"#
        ),
        "activewindow" => {
            r#"{"address":"0xa1","mapped":true,"hidden":false,"at":[10,40],"size":[900,600],"workspace":{"id":1,"name":"1"},"floating":false,"pseudo":false,"monitor":0,"class":"foot","title":"foot","initialClass":"foot","initialTitle":"foot","pid":101,"xwayland":false,"pinned":false,"fullscreen":0,"fullscreenClient":0,"grouped":[],"tags":[],"swallowing":"0x0","focusHistoryID":0,"inhibitingIdle":false}"#
        }
        "layers" => {
            r#"{"DP-1":{"levels":{"0":[],"1":[],"2":[{"address":"0xb1","x":0,"y":0,"w":1920,"h":40,"namespace":"rustshell:bar","pid":1}],"3":[]}}}"#
        }
        "binds" => {
            r#"[{"locked":false,"mouse":false,"release":false,"repeat":false,"longPress":false,"non_consuming":false,"has_description":true,"modmask":64,"submap":"","key":"Return","keycode":0,"catch_all":false,"description":"Apps: Terminal","dispatcher":"exec","arg":"foot"},{"locked":false,"mouse":false,"release":false,"repeat":true,"longPress":false,"non_consuming":false,"has_description":true,"modmask":65,"submap":"","key":"Q","keycode":0,"catch_all":false,"description":"Window: Close","dispatcher":"killactive","arg":""},{"locked":false,"mouse":false,"release":false,"repeat":false,"longPress":false,"non_consuming":false,"has_description":false,"modmask":8,"submap":"","key":"Tab","keycode":0,"catch_all":false,"description":"","dispatcher":"cyclenext","arg":""}]"#
        }
        "devices" => {
            r#"{"mice":[{"address":"0xc1","name":"virtual-mouse","defaultSpeed":0.0}],"keyboards":[{"address":"0xc2","name":"virtual-keyboard","rules":"","model":"","layout":"us,ru","variant":",","options":"grp:alt_shift_toggle","active_keymap":"English (US)","active_layout_index":0,"capsLock":false,"numLock":true,"main":true}],"tablets":[],"touch":[],"switches":[]}"#
        }
        "getoption" => r#"{"option":"general:gaps_out","int":1,"float":1.0,"set":true}"#,
        "version" => r#"{"branch":"","commit":"","version":"0.50.0","tag":"v0.50.0","flags":[]}"#,
        "cursorpos" => r#"{"x":960,"y":540}"#,
        _ => return None,
    })
}

/// Sandboxed command output (see `native::SpawnHook`): commands never really run.
fn command_tape(argv: &[String]) -> Option<String> {
    match argv.first().map(String::as_str) {
        Some("hyprctl") => {
            let args: Vec<&str> = argv[1..].iter().map(String::as_str).collect();
            let what = args.iter().find(|a| !a.starts_with('-'))?;
            if args.contains(&"-j") {
                hypr_json(what).map(String::from)
            } else {
                Some("ok".into())
            }
        }
        Some("bash" | "sh") if argv.get(1).map(String::as_str) == Some("-c") => {
            let script = argv.get(2).map(String::as_str).unwrap_or("");
            let out = match script {
                s if s.starts_with("echo \"$(pidof ") => ";",
                "echo $XDG_CURRENT_DESKTOP,$WAYLAND_DISPLAY" => "Hyprland,wayland-1",
                s if s.starts_with("echo \"a b c $(brightnessctl g)") => "a b c 96000 96000",
                s if s.starts_with("if [ -d ") && s.ends_with("else echo invalid; fi") => "dir",
                s if s.contains("/images/find-regions-venv.sh ") => "[]",
                _ => return None,
            };
            Some(format!("{out}\n"))
        }
        _ => None,
    }
}

/// A Hyprland IPC stand-in under `dir`: requests get [`hypr_json`], dispatches "ok", the
/// event socket stays silent.
fn fake_hyprland(dir: &Path) -> std::io::Result<()> {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;
    std::fs::create_dir_all(dir)?;
    for f in [".socket.sock", ".socket2.sock", "hyprland.lock"] {
        let _ = std::fs::remove_file(dir.join(f));
    }
    let req = UnixListener::bind(dir.join(".socket.sock"))?;
    let ev = UnixListener::bind(dir.join(".socket2.sock"))?;
    // Hyprland writes its lock once its sockets listen.
    std::fs::write(dir.join("hyprland.lock"), "1\nwayland-1\n")?;
    std::thread::spawn(move || {
        for mut s in req.incoming().flatten() {
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            let cmd = String::from_utf8_lossy(&buf[..n]).to_string();
            let out = match cmd.strip_prefix("j/") {
                Some(w) => hypr_json(w.split_whitespace().next().unwrap_or("")).unwrap_or("{}"),
                None => "ok",
            };
            let _ = s.write_all(out.as_bytes());
        }
    });
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for s in ev.incoming().flatten() {
            held.push(s);
        }
    });
    Ok(())
}

/// Candidate side (it knows which compiled bodies ran): one target per compiled handler or
/// function whose body never ran, and every inactive Loader, for [`Act::Drive`].
fn plan_drive(st: &mut St) -> Vec<String> {
    st.ran.extend(qml::jsrt::take_runs());
    let out = plan_targets(st);
    if !out.is_empty() || st.standalone {
        return out;
    }
    st.standalone = true;
    let used = st.used.borrow();
    let loaded: BTreeSet<&str> = used.iter().filter_map(|k| k.split('|').next()).collect();
    let files: BTreeSet<&str> = st
        .inv
        .keys
        .iter()
        .filter_map(|k| k.key.split('|').next())
        .filter(|f| !loaded.contains(f) && !st.done_files.contains(*f))
        .collect();
    files.into_iter().map(|f| format!("0\tl\t{f}")).collect()
}

fn plan_targets(st: &mut St) -> Vec<String> {
    use qml::eval::Callable;
    let metas = st.metas.borrow();
    let w = st.rt.world.borrow();
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for i in 0..w.items.len() {
        if w.dead.contains(&i) {
            continue;
        }
        let it = &w.items[i];
        let mut t: Vec<(String, usize, char)> = it
            .handlers
            .iter()
            .flat_map(|(h, cs)| cs.iter().map(move |c| (h, c)))
            .filter_map(|(h, c)| Some((h.to_string(), &c.compiled?.meta as *const _ as usize, 'h')))
            .collect();
        for (k, p) in it.props.iter() {
            if let Value::Obj(o) = &p.value
                && let Some(Callable::Compiled(_, _, m)) = &o.borrow().call
            {
                t.push((k.as_str().to_string(), *m as *const _ as usize, 'f'));
            }
        }
        t.sort();
        for (name, m, kind) in t {
            if metas.get(&m).is_some_and(|k| !st.done_keys.contains(k))
                && !st.ran.contains(&m)
                && seen.insert(m)
            {
                let key = format!("{i}\t{kind}\t{name}");
                if st.tried.insert(key.clone()) {
                    out.push(key);
                }
            }
        }
        let loader = it.types.iter().any(|t| t == "Loader");
        if (loader || it.types.iter().any(|t| t == "LazyLoader"))
            && !it.props.get("active").map_or(loader, |p| p.value.truthy())
        {
            let key = format!("{i}\ta\tactive");
            if st.tried.insert(key.clone()) {
                out.push(key);
            }
        }
    }
    out
}

fn canon(v: &Value, depth: u32, seen: &mut Vec<usize>, out: &mut String) {
    match v {
        Value::Undefined | Value::Empty => out.push_str("undefined"),
        Value::Null => out.push_str("null"),
        Value::Bool(b) => write!(out, "{b}").unwrap(),
        Value::Num(n) if *n == 0.0 && n.is_sign_negative() => out.push_str("-0"),
        Value::Num(n) => write!(out, "{n}").unwrap(),
        Value::Str(s) => write!(out, "{:?}", &**s).unwrap(),
        Value::Str16(u) => write!(out, "str16{:x?}", &**u).unwrap(),
        Value::Item(i) => write!(out, "#{i}").unwrap(),
        Value::Native(n) => write!(out, "native:{n}").unwrap(),
        Value::Color(c) => write!(out, "{c:?}").unwrap(),
        Value::Symbol(s) => out.push_str(&s.descriptive()),
        Value::Obj(o) => {
            let p = Rc_ptr(o);
            if seen.contains(&p) {
                return out.push_str("<cycle>");
            }
            if depth > 4 {
                return out.push_str("{..}");
            }
            let b = o.borrow();
            if b.call.is_some() {
                return out.push_str("<function>");
            }
            seen.push(p);
            if let Some(a) = &b.array {
                out.push('[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    canon(x, depth + 1, seen, out);
                }
                out.push(']');
            } else {
                out.push('{');
                for (i, k) in b.keys.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write!(out, "{k:?}:").unwrap();
                    canon(
                        b.props.get(k).unwrap_or(&Value::Undefined),
                        depth + 1,
                        seen,
                        out,
                    );
                }
                out.push('}');
            }
            seen.pop();
        }
    }
}

/// Shortens a snapshot value longer than 4000 bytes to its first characters that fit.
fn clip(s: &mut String) {
    if s.len() > 4000 {
        s.truncate(s.floor_char_boundary(4000));
        s.push_str("...");
    }
}

/// Canonical world state: `item\tprop\tvalue` per live property, then errors and effects.
fn snapshot(st: &mut St) -> Vec<String> {
    let w = st.rt.world.borrow();
    let mut lines = Vec::new();
    for id in 0..w.items.len() {
        if w.dead.contains(&id) {
            continue;
        }
        let Some(it) = w.items.get(id) else { continue };
        let mut props: Vec<_> = it.props.iter().collect();
        props.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
        for (k, p) in props {
            let mut s = String::new();
            canon(&p.value, 0, &mut Vec::new(), &mut s);
            clip(&mut s);
            lines.push(format!("{id}\t{}\t{s}", k.as_str()));
        }
    }
    drop(w);
    for e in st.rt.errors() {
        lines.push(format!("!error\t{e}\t"));
    }
    let w = st.rt.world.borrow();
    for (i, e) in w.effects.iter().enumerate() {
        let (name, args) = (e.name(), e.args());
        let mut s = String::new();
        for a in args {
            canon(a, 0, &mut Vec::new(), &mut s);
            s.push(' ');
        }
        lines.push(format!("!effect\t{i} {name}\t{s}"));
    }
    lines
}

/// Source keys of the binding currently driving `item.prop` (empty: written imperatively).
fn explain(st: &mut St, item: usize, prop: &str) -> Vec<String> {
    let w = st.rt.world.borrow();
    let Some(it) = w.items.get(item) else {
        return vec![];
    };
    let ty = qml::runtime::join_syms(&it.types, "<");
    let code = it
        .props
        .get_sym(Sym::new(prop))
        .and_then(|p| p.binding.as_ref());
    let keys: Vec<String> = code
        .and_then(|c| st.inv.by_body.get(&Rc_ptr(&c.body)))
        .map(|v| v.iter().map(|&i| st.inv.keys[i].key.clone()).collect())
        .unwrap_or_default();
    let mut out = vec![ty];
    if code.is_none() {
        out.push(String::new());
    } else if keys.is_empty() {
        out.push("<binding without source key>".into());
    }
    out.extend(keys);
    out
}

// ---------------------------------------------------------------- differential driver

struct Divergence {
    round: usize,
    step: String,
    item: usize,
    prop: String,
    ty: String,
    keys: Vec<String>,
    reference: String,
    candidate: String,
    total: usize,
}

fn split(l: &str) -> (&str, &str) {
    let mut it = l.splitn(3, '\t');
    let k = it.next().unwrap_or("");
    let p = it.next().unwrap_or("");
    let at = k.len() + p.len() + 1;
    (&l[..at.min(l.len())], l.get(at + 1..).unwrap_or(""))
}

fn diff(a: &[String], b: &[String]) -> Vec<(String, String, String)> {
    let ma: BTreeMap<_, _> = a.iter().map(|l| split(l)).collect();
    let mb: BTreeMap<_, _> = b.iter().map(|l| split(l)).collect();
    let mut out = Vec::new();
    for (k, va) in &ma {
        match mb.get(k) {
            Some(vb) if vb == va => {}
            Some(vb) => out.push((k.to_string(), va.to_string(), vb.to_string())),
            None => out.push((k.to_string(), va.to_string(), "<absent>".into())),
        }
    }
    for (k, vb) in &mb {
        if !ma.contains_key(k) {
            out.push((k.to_string(), "<absent>".into(), vb.to_string()));
        }
    }
    out.sort_by_key(|(k, ..)| {
        let (i, p) = k.split_once('\t').unwrap_or((k, ""));
        (i.parse::<usize>().unwrap_or(usize::MAX), p.to_string())
    });
    out
}

struct RunResult {
    steps: usize,
    divergences: Vec<Divergence>,
    builtins: Vec<(String, usize, String, String)>,
    errors: Vec<String>,
    /// Runtime errors raised by generic drive (handlers/functions called out of context).
    drive_errors: usize,
    load_log: Vec<String>,
    compiled: Vec<String>,
    /// Compiled keys whose body actually ran.
    executed: Vec<String>,
    /// `key\tcount` of the commands and `Impulse` calls the candidate made in the sandbox.
    commands: Vec<String>,
    /// See [`St::interpreted`].
    interpreted: Vec<String>,
    /// Reference samples for the QV4 oracle (see [`v4::drain`]).
    v4: Vec<String>,
}

fn run_round(
    o: &Opts,
    round: usize,
    sel: &Select,
    compiled: Option<AotFactory>,
    probe: bool,
) -> RunResult {
    let mut res = RunResult {
        steps: 0,
        divergences: Vec::new(),
        builtins: Vec::new(),
        errors: Vec::new(),
        drive_errors: 0,
        load_log: Vec::new(),
        compiled: Vec::new(),
        executed: Vec::new(),
        commands: Vec::new(),
        interpreted: Vec::new(),
        v4: Vec::new(),
    };
    let sample_v4 = probe && o.v4.is_some();
    for (si, scene) in scenes(o).into_iter().enumerate() {
        let pre = if si == 0 {
            String::new()
        } else {
            format!("{}: ", scene.name)
        };
        let tape = Tape::default();
        let a = Worker::spawn(
            o.root.clone(),
            Backend::Interp,
            Select::default(),
            None,
            tape.clone(),
            true,
        );
        let b = Worker::spawn(
            o.cand.clone(),
            o.backend,
            sel.clone(),
            compiled,
            tape,
            false,
        );
        let mut plan = vec![
            (format!("{pre}load"), Act::Load(scene.load)),
            (format!("{pre}settle"), Act::Frames(o.frames)),
        ];
        plan.extend(
            scene
                .steps
                .into_iter()
                .map(|(n, a)| (format!("{pre}{n}"), a)),
        );
        let mut i = 0;
        let mut drives = 0;
        loop {
            if i == plan.len() && drives < DRIVE_ROUNDS {
                drives += 1;
                if drives == 1 {
                    let keys = res.executed.clone();
                    let files: Vec<String> = res
                        .compiled
                        .iter()
                        .filter_map(|k| k.split('|').next().map(String::from))
                        .collect();
                    b.ask(move |st| {
                        st.done_keys.extend(keys);
                        st.done_files.extend(files);
                        Vec::new()
                    });
                }
                let t = b.ask(plan_drive);
                for (n, c) in t.chunks(DRIVE_CHUNK).enumerate() {
                    plan.push((format!("{pre}drive {drives}.{n}"), Act::Drive(c.to_vec())));
                    plan.push((
                        format!("{pre}drive {drives}.{n} settled"),
                        Act::Frames(o.frames),
                    ));
                }
            }
            if i >= plan.len() {
                break;
            }
            let (name, step) = plan[i].clone();
            let (s1, s2) = (step.clone(), step.clone());
            let la = a.ask(move |st| act(st, &s1));
            b.ask(move |st| act(st, &s2));
            if i == 0 {
                res.load_log
                    .extend(la.into_iter().map(|l| format!("{pre}{l}")));
            }
            if i == 0 && scene.flags {
                let flags = a.ask(|st| {
                    let w = st.rt.world.borrow();
                    let Some(&id) = w.singletons.get("GlobalStates") else {
                        return vec![];
                    };
                    let mut v: Vec<String> = w.items[id]
                        .props
                        .iter()
                        .filter_map(|(k, p)| match p.value {
                            Value::Bool(b) => Some(format!("{}\t{b}", k.as_str())),
                            _ => None,
                        })
                        .collect();
                    v.sort();
                    v
                });
                for f in flags {
                    let (f, on) = f.split_once('\t').unwrap_or((&f, "false"));
                    let on = on == "true";
                    for (val, tag) in [
                        (!on, if on { "close" } else { "open" }),
                        (on, if on { "open" } else { "close" }),
                    ] {
                        plan.push((
                            format!("{pre}{tag} {f}"),
                            Act::Set("GlobalStates".into(), f.to_string(), Lit::Bool(val)),
                        ));
                        plan.push((format!("{pre}{tag} {f} +2f"), Act::Frames(2)));
                        plan.push((format!("{pre}{tag} {f} settled"), Act::Frames(o.frames)));
                    }
                }
            }
            res.steps += 1;
            let sa = a.ask(snapshot);
            let sb = b.ask(snapshot);
            let d = diff(&sa, &sb);
            if !d.is_empty() {
                let total = d.len();
                let mut seen = BTreeSet::new();
                for (k, ra, rb) in d {
                    let (item, prop) = k.split_once('\t').unwrap_or((&k, ""));
                    let (item_n, prop_s) = (
                        item.parse::<usize>().unwrap_or(usize::MAX),
                        prop.to_string(),
                    );
                    let p2 = prop_s.clone();
                    let ex = if item_n == usize::MAX {
                        vec![item.to_string()]
                    } else {
                        a.ask(move |st| explain(st, item_n, &p2))
                    };
                    let ty = ex.first().cloned().unwrap_or_default();
                    let keys: Vec<String> =
                        ex.into_iter().skip(1).filter(|k| !k.is_empty()).collect();
                    let tag = keys
                        .first()
                        .cloned()
                        .unwrap_or_else(|| format!("{ty}.{prop_s}"));
                    if !seen.insert(tag) || seen.len() > 200 {
                        continue;
                    }
                    res.divergences.push(Divergence {
                        round,
                        step: name.clone(),
                        item: item_n,
                        prop: prop_s,
                        ty,
                        keys,
                        reference: ra,
                        candidate: rb,
                        total,
                    });
                }
                // Binding-driven differences first: imperatively written ones follow from them.
                res.divergences.sort_by_key(|d| d.keys.is_empty());
                break;
            }
            i += 1;
        }
        if std::env::var_os("AOTCHECK_DUMP").is_some() {
            for (tag, wk) in [("ref", &a), ("cand", &b)] {
                let items = wk.ask(|st| {
                    let w = st.rt.world.borrow();
                    (0..w.items.len())
                        .filter(|i| !w.dead.contains(i))
                        .filter_map(|i| w.items.get(i).map(|it| (i, it)))
                        .map(|(i, it)| {
                            let mut ks: Vec<String> = it
                                .props
                                .iter()
                                .map(|(k, p)| match &p.binding {
                                    Some(c) if c.compiled.is_some() => format!("{}:c", k.as_str()),
                                    Some(_) => format!("{}:i", k.as_str()),
                                    None => k.as_str().to_string(),
                                })
                                .collect();
                            ks.sort();
                            format!(
                                "{i}\t{:?}\t{}\t{}",
                                it.parent,
                                qml::runtime::join_syms(&it.types, "/"),
                                ks.join(",")
                            )
                        })
                        .collect()
                });
                let _ = std::fs::write(
                    o.out.join(format!("items-{}-{tag}.tsv", scene.name)),
                    items.join("\n"),
                );
            }
        }
        let mut errs = a.ask(|st| st.rt.errors());
        let mark = a.ask(|st| match &st.drive_mark {
            Some(v) => std::iter::once(String::new())
                .chain(v.iter().cloned())
                .collect(),
            None => Vec::new(),
        });
        if !mark.is_empty() {
            let scene: std::collections::HashSet<&String> = mark[1..].iter().collect();
            res.drive_errors += errs.iter().filter(|e| !scene.contains(e)).count();
            errs = mark[1..].to_vec();
        }
        res.errors
            .extend(errs.into_iter().map(|e| format!("{pre}{e}")));
        // Sampling reads binding inputs, which can instantiate lazy singletons and so shift
        // the reference's item ids: only once the scene is compared.
        if sample_v4 {
            res.v4.extend(a.ask(|st| {
                v4::sample(st);
                v4::drain(st)
            }));
        }
        if probe && si == 0 {
            res.builtins = a.ask(|st| {
            let uses: Vec<_> = st.inv.uses.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            // Probe through a real QML binding so host-provided names (qsTr, Qt, ids) resolve.
            let mut src = String::from("import QtQuick\nItem {\n");
            for (i, (u, _)) in uses.iter().enumerate() {
                for (j, p) in probes(u).iter().enumerate() {
                    let (call, ret) = if u.starts_with('.') { (format!("{p}('x');"), "\"callable\"") } else { (String::new(), "t") };
                    writeln!(src, "    property var p{i}_{j}: {{ try {{ var t = typeof ({p}); if (t !== \"undefined\") return t; {call} return {ret} }} catch (e) {{ return \"error: \" + e }} }}").unwrap();
                }
            }
            src.push_str("}\n");
            let id = match st.rt.load_str(&src) {
                Ok(id) => id,
                Err(e) => return vec![format!("<probe load failed: {e}>\t0\tprobe error\t")],
            };
            let mut out = Vec::new();
            for (i, (u, (n, first))) in uses.into_iter().enumerate() {
                let mut status = "missing".to_string();
                for j in 0..probes(&u).len() {
                    let r = match st.rt.get(id, &format!("p{i}_{j}")) {
                        Value::Str(t) => t.to_string(),
                        v => format!("{v:?}"),
                    };
                    let missing = r == "undefined"
                        || (r.starts_with("error: ")
                            && ["not a function", "not defined", "undefined"].iter().any(|m| r.contains(m)));
                    if !missing {
                        status = "ok".into();
                        break;
                    }
                    status = format!("missing ({})", r.replace('\t', " "));
                }
                out.push(format!("{u}\t{n}\t{status}\t{first}"));
            }
            out
        })
        .into_iter()
        .map(|l| {
            let v: Vec<&str> = l.splitn(4, '\t').collect();
            (v[0].to_string(), v[1].parse().unwrap_or(0), v[2].to_string(), v[3].to_string())
        })
        .collect();
        }
        res.compiled
            .extend(b.ask(|st| st.used.borrow().iter().cloned().collect()));
        res.interpreted
            .extend(b.ask(|st| st.interpreted.borrow().iter().cloned().collect()));
        res.executed.extend(b.ask(|st| {
            st.ran.extend(qml::jsrt::take_runs());
            let m = st.metas.borrow();
            st.ran.iter().filter_map(|p| m.get(p).cloned()).collect()
        }));
        res.commands.extend(b.ask(|st| {
            st.cmds
                .borrow()
                .iter()
                .map(|(k, n)| format!("{k}\t{n}"))
                .collect()
        }));
        if !res.divergences.is_empty() {
            break;
        }
    }
    res.compiled.sort();
    res.compiled.dedup();
    res.executed.sort();
    res.executed.dedup();
    res
}

/// Run aotcheck with command line `args` (without the program name); `compiled` builds the
/// candidate's compiled code for `--backend compiled`. Returns the exit status.
pub fn run(args: Vec<String>, compiled: Option<AotFactory>) -> i32 {
    qml::ir::KEEP_IR.store(true, std::sync::atomic::Ordering::Relaxed);
    let hook = std::panic::take_hook();
    // A worker's panic is printed where it happens; its re-raise on the main thread is not.
    std::panic::set_hook(Box::new(move |i| {
        if !i.payload().is::<Panicked>() {
            hook(i)
        }
    }));
    let o = opts(args);
    // Driven handlers must not reach the session's compositor or media players: both
    // workers talk to a fake Hyprland under the output directory instead.
    let rt_dir =
        std::path::absolute(o.out.join("xdg-runtime")).unwrap_or(o.out.join("xdg-runtime"));
    if let Err(e) = fake_hyprland(&rt_dir.join("hypr/aotcheck")) {
        elog::eprintln!("aotcheck: fake Hyprland: {e}");
    }
    #[expect(
        clippy::disallowed_methods,
        reason = "before the threads that read the environment start"
    )]
    // SAFETY: no worker has started yet; the only other threads are the fake
    // Hyprland's two socket loops, which never touch the environment.
    unsafe {
        std::env::remove_var("DBUS_SESSION_BUS_ADDRESS")
    };
    #[expect(
        clippy::disallowed_methods,
        reason = "before the threads that read the environment start"
    )]
    // SAFETY: see above.
    unsafe {
        std::env::set_var("HYPRLAND_INSTANCE_SIGNATURE", "aotcheck")
    };
    #[expect(
        clippy::disallowed_methods,
        reason = "before the threads that read the environment start"
    )]
    // SAFETY: see above.
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", &rt_dir)
    };
    // Both workers must see each Hyprland reply at the same step.
    qml::hypr::set_sync(true);
    if let Some(f) = o.probe.clone() {
        let w = Worker::spawn(
            o.root.clone(),
            Backend::Interp,
            Select::default(),
            None,
            Tape::default(),
            true,
        );
        let load = Act::Load(Load {
            files: vec![f],
            ..Load::default()
        });
        let log = w.ask(move |st| act(st, &load));
        let frames = Act::Frames(o.frames);
        w.ask(move |st| act(st, &frames));
        for l in log {
            println!("{l}");
        }
        return 0;
    }
    if o.backend == Backend::Compiled && compiled.is_none() {
        elog::eprintln!(
            "aotcheck: this build has no compiled code; run the ricompiled aotcheck binary"
        );
        return 2;
    }
    std::fs::create_dir_all(&o.out).expect("create --out");
    let inv = Inventory::build(&o.root);
    let dynamic = qml::runtime::uncompiled_create_qml_object(&o.root);
    let evals = qml::runtime::uncompiled_eval(&o.root);
    let type_errors = qml::typecheck::check(&o.root).errors();
    let unwritten = qml::unwritten::errors(&o.root);
    let count = |k: &str| inv.keys.iter().filter(|x| x.kind == k).count();
    let bad: Vec<_> = inv.keys.iter().filter(|k| k.status != "ok").collect();
    let js_bad = inv.js.iter().filter(|(_, s)| s != "ok").count();

    let mut quarantine = BTreeSet::new();
    let mut all = Vec::new();
    let mut first = None;
    let mut last_steps = 0;
    let mut converged = false;
    let mut callee_rounds = 0;
    let mut interpreted = BTreeSet::new();
    let mut v4_samples = Vec::new();
    let fn_keys: BTreeSet<&str> = inv
        .keys
        .iter()
        .filter(|k| k.kind == "function")
        .map(|k| k.key.as_str())
        .collect();
    for round in 0..o.rounds {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let sel = Select {
                keys: quarantine.clone(),
                only: false,
            };
            run_round(&o, round, &sel, compiled, round == 0)
        }));
        let r = match r {
            Ok(r) => r,
            Err(p) => {
                let Some(pm) = p.downcast_ref::<Panicked>() else {
                    std::panic::resume_unwind(p)
                };
                let key = panic_key(&pm.msg).filter(|k| !quarantine.contains(k));
                elog::eprintln!(
                    "PANIC round {round} {} [{}]: {}",
                    pm.backend,
                    key.as_deref().unwrap_or(""),
                    pm.msg
                );
                all.push(Divergence {
                    round,
                    step: "panic".into(),
                    item: 0,
                    prop: String::new(),
                    ty: pm.backend.clone(),
                    keys: key.iter().cloned().collect(),
                    reference: String::new(),
                    candidate: pm.msg.clone(),
                    total: 1,
                });
                // Only a compiled body can be quarantined away.
                match key {
                    Some(k)
                        if o.backend == Backend::Compiled
                            && pm.backend == format!("{:?}", Backend::Compiled) =>
                    {
                        quarantine.insert(k);
                        continue;
                    }
                    _ => break,
                }
            }
        };
        interpreted.extend(r.interpreted.iter().cloned());
        let ran_compiled: BTreeSet<&str> = r.compiled.iter().map(String::as_str).collect();
        last_steps = r.steps;
        let new: Vec<String> = r
            .divergences
            .iter()
            .flat_map(|d| d.keys.clone())
            .filter(|k| !quarantine.contains(k))
            // Interpreted bindings that differ only follow a compiled one.
            .filter(|k| ran_compiled.contains(k.as_str()))
            .collect();
        let n = r.divergences.len();
        all.extend(r.divergences);
        if first.is_none() {
            v4_samples = r.v4;
            first = Some((
                r.builtins,
                r.errors,
                r.drive_errors,
                r.load_log,
                r.compiled.clone(),
                r.executed.clone(),
                r.commands.clone(),
            ));
        }
        if n == 0 {
            converged = true;
            break;
        }
        if o.backend == Backend::Interp {
            break;
        }
        if new.is_empty() {
            // Bindings already interpreted still diverge: the cause is a compiled function
            // or library they call, which a binding key does not name. Quarantine those.
            let callees: Vec<String> = r
                .compiled
                .iter()
                .filter(|k| !quarantine.contains(*k))
                .filter(|k| !k.contains('|') || fn_keys.contains(k.as_str()))
                .cloned()
                .collect();
            if callees.is_empty() {
                break;
            }
            callee_rounds += 1;
            quarantine.extend(callees);
            continue;
        }
        quarantine.extend(new);
    }
    let roots = isolate(&o, &quarantine, compiled);
    let v4 = o.v4.as_ref().map(|py| v4::check(py, &o.out, &v4_samples));
    for d in v4.iter().flat_map(|v| &v.divergences) {
        all.push(Divergence {
            round: 0,
            step: "QV4 oracle".into(),
            item: 0,
            prop: String::new(),
            ty: "QV4".into(),
            keys: d.keys.clone(),
            reference: d.v4.clone(),
            candidate: d.native.clone(),
            total: 1,
        });
    }
    let (builtins, errors, drive_errors, load_log, compiled, executed, commands) =
        first.unwrap_or_default();
    let missing: Vec<_> = builtins.iter().filter(|b| b.2 != "ok").collect();

    let mut keys_tsv = String::from("key\tkind\tline\tstatus\n");
    for k in &inv.keys {
        writeln!(keys_tsv, "{}\t{}\t{}\t{}", k.key, k.kind, k.line, k.status).unwrap();
    }
    for (f, s) in &inv.js {
        writeln!(keys_tsv, "{f}\tjs\t0\t{s}").unwrap();
    }
    let mut b_tsv = String::from("use\tcount\tstatus\tfirst_key\n");
    for (u, n, s, f) in &builtins {
        writeln!(b_tsv, "{u}\t{n}\t{s}\t{}", f.replace('\t', " ")).unwrap();
    }
    let mut d_tsv =
        String::from("round\tstep\titem\ttype\tprop\tkeys\treference\tcandidate\tstep_diffs\n");
    for d in &all {
        writeln!(
            d_tsv,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            d.round,
            d.step,
            d.item,
            d.ty,
            d.prop,
            d.keys.join(" | ").replace('\t', " "),
            d.reference.replace('\t', " "),
            d.candidate.replace('\t', " "),
            d.total
        )
        .unwrap();
    }
    let mut report = String::new();
    writeln!(report, "# aotcheck report\n").unwrap();
    writeln!(
        report,
        "- tree: `{}` candidate: `{}` backend: {}",
        o.root.display(),
        o.cand.display(),
        if o.backend == Backend::Interp {
            "interp"
        } else {
            "compiled"
        }
    )
    .unwrap();
    writeln!(
        report,
        "- keys: {} (bindings {}, handlers {}, functions {}), not lowerable/unsupported: {}",
        inv.keys.len(),
        count("binding"),
        count("handler"),
        count("function"),
        bad.len()
    )
    .unwrap();
    writeln!(report, "- js libraries: {} (failed {js_bad})", inv.js.len()).unwrap();
    writeln!(
        report,
        "- builtins used: {} missing: {}",
        builtins.len(),
        missing.len()
    )
    .unwrap();
    writeln!(report, "- scenario steps: {last_steps}, divergences: {}, quarantined keys: {}, converged: {converged}", all.len(), quarantine.len()).unwrap();
    writeln!(
        report,
        "- interpreted for lack of compiled code (not compared): {}",
        interpreted.len()
    )
    .unwrap();
    for e in &type_errors {
        println!("TYPE {e}");
    }
    for e in &unwritten {
        println!("UNWRITTEN {e}");
    }
    for i in &interpreted {
        writeln!(report, "  - `{}`", i.replace('\t', "` ")).unwrap();
    }
    if let Some(r) = &roots {
        writeln!(
            report,
            "- isolation (each quarantined key alone compiled): {} root causes, {} downstream",
            r.len(),
            quarantine.len() - r.len()
        )
        .unwrap();
    }
    if o.backend == Backend::Compiled {
        writeln!(
            report,
            "- compiled keys run by the candidate: {} of {} (+{} libraries); callee quarantine rounds: {callee_rounds}",
            compiled.iter().filter(|k| k.contains('|')).count(),
            inv.keys.len(),
            compiled.iter().filter(|k| !k.contains('|')).count()
        )
        .unwrap();
        writeln!(
            report,
            "- compiled keys executed by the candidate: {} of {} (+{} libraries); per file: coverage.tsv, not executed: uncovered.tsv",
            executed.iter().filter(|k| k.contains('|')).count(),
            inv.keys.len(),
            executed.iter().filter(|k| !k.contains('|')).count()
        )
        .unwrap();
    }
    writeln!(
        report,
        "- reference runtime errors (scene steps): {}",
        errors.len()
    )
    .unwrap();
    writeln!(
        report,
        "- reference runtime errors from generic drive (out-of-context calls, informational): {drive_errors}\n"
    )
    .unwrap();
    if let Some(v) = &v4 {
        match &v.error {
            Some(e) => writeln!(report, "- QV4 oracle failed to run: {e}").unwrap(),
            None => writeln!(
                report,
                "- QV4 oracle (reference: Qt's V4, candidate: the native interpreter): {} programs from {} of {} live binding keys, {} divergences; per program: v4.tsv",
                v.programs, v.keys, v.seen, v.divergences.len()
            )
            .unwrap(),
        }
        for (r, n) in &v.skipped {
            writeln!(report, "  - not replayable: {r}: {n} keys").unwrap();
        }
        writeln!(report).unwrap();
    }
    if !missing.is_empty() {
        writeln!(report, "## Missing builtins\n").unwrap();
        for (u, n, s, f) in &missing {
            writeln!(
                report,
                "- `{u}` x{n} {s}, first in `{}`",
                f.replace('\t', " ")
            )
            .unwrap();
        }
        writeln!(report).unwrap();
    }
    if !inv.unsupported.is_empty() {
        writeln!(report, "## Unsupported constructs\n").unwrap();
        for (u, (n, f)) in &inv.unsupported {
            writeln!(report, "- `{u}` x{n}, first in `{}`", f.replace('\t', " ")).unwrap();
        }
        writeln!(report).unwrap();
    }
    if !bad.is_empty() {
        writeln!(report, "## Keys that do not lower\n").unwrap();
        for k in bad.iter().take(200) {
            writeln!(report, "- `{}`: {}", k.key.replace('\t', " "), k.status).unwrap();
        }
        writeln!(report).unwrap();
    }
    if !all.is_empty() {
        writeln!(report, "## First divergences\n").unwrap();
        for d in &all {
            writeln!(report, "- round {} step `{}`: #{} {} `{}` keys [{}]: reference `{}` candidate `{}` ({} diffs in step)", d.round, d.step, d.item, d.ty, d.prop, d.keys.join(", ").replace('\t', " "), d.reference, d.candidate, d.total).unwrap();
        }
        writeln!(report).unwrap();
    }
    // Root causes: the quarantined keys, by section and file, with the first divergence naming each.
    let mut q_tsv =
        String::from("section\tfile\tkey\tcause\tround\tstep\tprop\treference\tcandidate\n");
    let mut by_section: BTreeMap<String, BTreeMap<String, Vec<(u8, String)>>> = BTreeMap::new();
    for k in &quarantine {
        // root: diverges as the only compiled key; downstream: only follows a root.
        let cause = match &roots {
            Some(r) if r.contains(k) => "root",
            Some(r) if r.is_empty() => "unresolved",
            Some(_) => "downstream",
            None => "unresolved",
        };
        let file = k.split('|').next().unwrap_or(k);
        let sec = section(file);
        let d = all.iter().find(|d| d.keys.contains(k));
        let (round, step, prop, r, c) = match d {
            Some(d) => (
                d.round.to_string(),
                d.step.as_str(),
                d.prop.as_str(),
                d.reference.as_str(),
                d.candidate.as_str(),
            ),
            None => ("-".into(), "callee", "", "", ""),
        };
        writeln!(
            q_tsv,
            "{sec}\t{file}\t{k}\t{cause}\t{round}\t{step}\t{prop}\t{r}\t{c}"
        )
        .unwrap();
        let line = match d {
            Some(_) => format!(
                "{cause} `{k}` round {round} step `{step}` `{prop}`: reference `{r}` candidate `{c}`"
            ),
            None => format!(
                "{cause} `{k}` (callee quarantine: a compiled function/library behind a divergence)"
            ),
        };
        let rank = match cause {
            "root" => 0,
            "unresolved" => 1,
            _ => 2,
        };
        by_section
            .entry(sec)
            .or_default()
            .entry(file.to_string())
            .or_default()
            .push((rank, line));
    }
    if !by_section.is_empty() {
        writeln!(report, "## Root causes by section\n").unwrap();
        for (sec, files) in &by_section {
            let n: usize = files.values().map(Vec::len).sum();
            let nr = files.values().flatten().filter(|l| l.0 == 0).count();
            writeln!(report, "### {sec} ({n} keys, {nr} root)\n").unwrap();
            let mut files: Vec<_> = files.iter().collect();
            files.sort_by_key(|(f, ls)| (ls.iter().map(|l| l.0).min(), *f));
            for (f, ls) in files {
                writeln!(report, "- {f}").unwrap();
                let mut ls = ls.clone();
                ls.sort();
                for (_, l) in &ls {
                    writeln!(report, "  - {l}").unwrap();
                }
            }
            writeln!(report).unwrap();
        }
    }
    std::fs::write(o.out.join("quarantine.tsv"), q_tsv).unwrap();
    if !load_log.is_empty() || !errors.is_empty() {
        writeln!(report, "## Reference load log and errors\n").unwrap();
        for e in load_log.iter().chain(errors.iter()) {
            writeln!(report, "- {e}").unwrap();
        }
    }
    std::fs::write(o.out.join("keys.tsv"), keys_tsv).unwrap();
    std::fs::write(o.out.join("builtins.tsv"), b_tsv).unwrap();
    std::fs::write(o.out.join("divergences.tsv"), d_tsv).unwrap();
    std::fs::write(o.out.join("report.md"), &report).unwrap();
    let mut c_tsv = String::from("key\tquarantined\n");
    for k in &compiled {
        writeln!(c_tsv, "{k}\t{}", quarantine.contains(k)).unwrap();
    }
    std::fs::write(o.out.join("compiled.tsv"), c_tsv).unwrap();
    if o.backend == Backend::Compiled {
        let (cov, unc) = coverage(&inv, &compiled, &executed);
        std::fs::write(o.out.join("coverage.tsv"), cov).unwrap();
        std::fs::write(o.out.join("uncovered.tsv"), unc).unwrap();
        let mut c = String::from("command\tcount\n");
        for l in &commands {
            writeln!(c, "{l}").unwrap();
        }
        std::fs::write(o.out.join("commands.tsv"), c).unwrap();
    }
    let hangs = HANGS.load(Ordering::Relaxed);
    println!(
        "keys {} bindings {} handlers {} functions {} unlowerable {} js {} js_failed {js_bad} builtins {} missing {} steps {last_steps} divergences {} errors {} compiled {} quarantined {} interpreted {} type_errors {} hangs {hangs}",
        inv.keys.len(),
        count("binding"),
        count("handler"),
        count("function"),
        bad.len(),
        inv.js.len(),
        builtins.len(),
        missing.len(),
        all.len(),
        errors.len(),
        compiled.len(),
        quarantine.len(),
        interpreted.len(),
        type_errors.len()
    );
    for d in all.iter().take(20) {
        println!(
            "DIVERGE step `{}` #{} {} {} [{}]: {} != {}",
            d.step,
            d.item,
            d.ty,
            d.prop,
            d.keys.join(", ").replace('\t', " "),
            d.reference,
            d.candidate
        );
    }
    for (u, n, s, _) in &missing {
        println!("MISSING {u} x{n} {s}");
    }
    if let Some(v) = &v4 {
        match &v.error {
            Some(e) => println!("V4 ERROR {e}"),
            None => println!(
                "v4 programs {} keys {} live {} divergences {}",
                v.programs,
                v.keys,
                v.seen,
                v.divergences.len()
            ),
        }
    }
    for d in &dynamic {
        println!(
            "DYNAMIC {d}: Qt.createQmlObject compiles QML text at run time, which has no compiled code"
        );
    }
    for e in &evals {
        println!(
            "EVAL {e}: eval evaluates JavaScript text at run time, which has no compiled code"
        );
    }
    for i in &interpreted {
        println!(
            "INTERPRETED {}: scripts without compiled code ran in the interpreter, so the comparison does not cover them",
            i.replace('\t', " ")
        );
    }
    if hangs > 0 {
        println!(
            "HANG: {hangs} evaluation(s) exhausted the step budget of {STEP_BUDGET} interrupt checks"
        );
    }
    for (key, tokens, ast) in &inv.shapes {
        println!(
            "SHAPE {key}: parsed as {tokens:?}, its AST is {ast:?}: creation NOTIFY would differ from Qt's QmlIR"
        );
    }
    for e in errors.iter().collect::<BTreeSet<_>>() {
        println!(
            "ERROR {e}: a script raised this while a scene loaded or its panels opened and closed"
        );
    }
    if o.strict
        && (hangs > 0
            || !inv.shapes.is_empty()
            || !all.is_empty()
            || !errors.is_empty()
            || !missing.is_empty()
            || !dynamic.is_empty()
            || !evals.is_empty()
            || !interpreted.is_empty()
            || !type_errors.is_empty()
            || !unwritten.is_empty()
            || v4.as_ref().is_some_and(|v| v.error.is_some()))
    {
        return 1;
    }
    0
}
