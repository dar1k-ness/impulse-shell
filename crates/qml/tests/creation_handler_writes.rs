//! A change handler's write while a component is created re-evaluates only the bindings
//! already enabled that depend on it; the component's other pending bindings are enabled by
//! the finalize loop in their turn (QQmlObjectCreator::finalize), as Qt 6.12 orders them.
//! Every scenario runs interpreted and with every script site compiled.
use qml::eval::R;
use qml::jsrt::{Aot, Cx, Entry, Env, FnKind, FnMeta, assign_qml, lookup_qml};
use qml::runtime::{Runtime, Sym};
use qml::value::Value;
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

const META: FnMeta = FnMeta {
    name: "",
    kind: FnKind::Normal,
    nparams: 0,
    nslots: 0,
    strict: false,
};

static RUNS: AtomicUsize = AtomicUsize::new(0);

type Site = (&'static str, qml::jsrt::CompiledFn);

/// Compiles each listed member of `main.qml` (by name) with its function.
struct Compiled(&'static [Site]);

impl Aot for Compiled {
    fn doc_scripts(&self, file: &Path, doc: &qml::Document) -> Vec<(qml::NodeId, &'static Entry)> {
        let mut out = vec![];
        if !file.ends_with("main.qml") {
            return out;
        }
        qml::walk::sites(doc, &mut |s| {
            let Some(&(member, f)) = self.0.iter().find(|(m, _)| *m == s.name) else {
                return;
            };
            let e: &'static Entry = Box::leak(Box::new(Entry {
                file: "main.qml",
                path: Box::leak(s.path.clone().into_boxed_str()),
                member,
                kind: s.kind,
                f,
                meta: META,
                hash: s.hash(),
            }));
            out.push((s.script.id, e));
        });
        out
    }
    fn library(&self, _: &Path) -> Option<&'static Entry> {
        None
    }
    fn script(&self, _: &Path) -> Option<&'static Entry> {
        None
    }
}

fn read(cx: &mut Cx, env: &Env, path: &[&str]) -> R<Value> {
    RUNS.fetch_add(1, Ordering::Relaxed);
    let mut v = lookup_qml(cx, env, Sym::new(path[0]))?;
    for k in &path[1..] {
        v = cx.get(&v, k)?;
    }
    Ok(v)
}

fn write(cx: &mut Cx, env: &Env, path: &[&str], v: Value) -> R<()> {
    match path {
        [name] => assign_qml(cx, env, Sym::new(name), v),
        [base @ .., last] => {
            let o = read(cx, env, base)?;
            cx.put(&o, last, v)
        }
        [] => unreachable!(),
    }
}

fn num(cx: &mut Cx, env: &Env, path: &[&str]) -> R<f64> {
    Ok(read(cx, env, path)?.to_number())
}

fn append(cx: &mut Cx, env: &Env, log: &[&str], s: &str) -> R<Value> {
    let l = read(cx, env, log)?.to_js_string();
    write(cx, env, log, Value::str(&format!("{l}{s}")))?;
    Ok(Value::Undefined)
}

/// The final `log` of `main.qml`, interpreted or with `sites` compiled.
fn log_of(qml: &str, sites: Option<&'static [Site]>) -> String {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("main.qml"), qml).unwrap();
    let mut rt = Runtime::new(Some(d.path().to_path_buf()));
    if let Some(s) = sites {
        rt.eng().set_compiled(Some(Rc::new(Compiled(s))));
    }
    let before = RUNS.load(Ordering::Relaxed);
    let root = rt.load_file(&d.path().join("main.qml")).unwrap();
    rt.settle();
    if sites.is_some() {
        assert!(RUNS.load(Ordering::Relaxed) > before, "compiled code ran");
    }
    rt.get(root, "log").to_js_string()
}

const SAME_OBJECT: &str = r#"import QtQuick
Item {
    property string log: ""
    property int k: 1
    property int a: k + 1
    onAChanged: { zz = 9; log += "a" }
    property int zz: 0
    property int b: k + 2
    onBChanged: log += "b"
    property int c: k + 3
    onCChanged: log += "c"
}
"#;

static SAME_OBJECT_SITES: &[Site] = &[
    ("a", |cx, env, _, _| {
        Ok(Value::Num(num(cx, env, &["k"])? + 1.0))
    }),
    ("b", |cx, env, _, _| {
        Ok(Value::Num(num(cx, env, &["k"])? + 2.0))
    }),
    ("c", |cx, env, _, _| {
        Ok(Value::Num(num(cx, env, &["k"])? + 3.0))
    }),
    ("onAChanged", |cx, env, _, _| {
        write(cx, env, &["zz"], Value::Num(9.0))?;
        append(cx, env, &["log"], "a")
    }),
    ("onBChanged", |cx, env, _, _| append(cx, env, &["log"], "b")),
    ("onCChanged", |cx, env, _, _| append(cx, env, &["log"], "c")),
];

/// Round 7 A1: `zz = 9` inside `onAChanged` does not enable `b` and `c` ahead of their turn.
#[test]
fn handler_write_during_creation_leaves_pending_bindings_pending() {
    assert_eq!(log_of(SAME_OBJECT, None), "abc", "interpreted");
    assert_eq!(
        log_of(SAME_OBJECT, Some(SAME_OBJECT_SITES)),
        "abc",
        "compiled"
    );
}

const CHILD_OBJECT: &str = r#"import QtQuick
Item {
    id: root
    property string log: ""
    property int k: 1
    property int zz: 0
    QtObject {
        property int a: root.k + 1
        onAChanged: { root.zz = 9; root.log += "a" }
        property int b: root.k + 2
        onBChanged: root.log += "b"
        property int c: root.k + 3
        onCChanged: root.log += "c"
    }
}
"#;

static CHILD_OBJECT_SITES: &[Site] = &[
    ("a", |cx, env, _, _| {
        Ok(Value::Num(num(cx, env, &["root", "k"])? + 1.0))
    }),
    ("b", |cx, env, _, _| {
        Ok(Value::Num(num(cx, env, &["root", "k"])? + 2.0))
    }),
    ("c", |cx, env, _, _| {
        Ok(Value::Num(num(cx, env, &["root", "k"])? + 3.0))
    }),
    ("onAChanged", |cx, env, _, _| {
        write(cx, env, &["root", "zz"], Value::Num(9.0))?;
        append(cx, env, &["root", "log"], "a")
    }),
    ("onBChanged", |cx, env, _, _| {
        append(cx, env, &["root", "log"], "b")
    }),
    ("onCChanged", |cx, env, _, _| {
        append(cx, env, &["root", "log"], "c")
    }),
];

/// A1 with the bindings on a child object.
#[test]
fn handler_write_during_creation_leaves_a_childs_pending_bindings_pending() {
    assert_eq!(log_of(CHILD_OBJECT, None), "abc", "interpreted");
    assert_eq!(
        log_of(CHILD_OBJECT, Some(CHILD_OBJECT_SITES)),
        "abc",
        "compiled"
    );
}

const FLUSHED: &str = r#"import QtQuick
Item {
    id: root
    property string log: ""
    property int k: 1
    property int a: b + 1
    onAChanged: log += "a" + a
    property int c: other.d + 1
    onCChanged: log += "c" + c
    property int b: k * 10
    onBChanged: log += "b" + b
    QtObject {
        id: other
        property int d: root.k * 100
        onDChanged: root.log += "d" + d
    }
}
"#;

fn log_with(cx: &mut Cx, env: &Env, log: &[&str], tag: &str, prop: &str) -> R<Value> {
    let v = read(cx, env, &[prop])?.to_js_string();
    append(cx, env, log, &format!("{tag}{v}"))
}

static FLUSHED_SITES: &[Site] = &[
    ("a", |cx, env, _, _| {
        Ok(Value::Num(num(cx, env, &["b"])? + 1.0))
    }),
    ("c", |cx, env, _, _| {
        Ok(Value::Num(num(cx, env, &["other", "d"])? + 1.0))
    }),
    ("b", |cx, env, _, _| {
        Ok(Value::Num(num(cx, env, &["k"])? * 10.0))
    }),
    ("d", |cx, env, _, _| {
        Ok(Value::Num(num(cx, env, &["root", "k"])? * 100.0))
    }),
    ("onAChanged", |cx, env, _, _| {
        log_with(cx, env, &["log"], "a", "a")
    }),
    ("onCChanged", |cx, env, _, _| {
        log_with(cx, env, &["log"], "c", "c")
    }),
    ("onBChanged", |cx, env, _, _| {
        log_with(cx, env, &["log"], "b", "b")
    }),
    ("onDChanged", |cx, env, _, _| {
        log_with(cx, env, &["root", "log"], "d", "d")
    }),
];

/// Round 7 A2: `a` reads the pending `b`, which is enabled first (flushPendingBinding);
/// `onBChanged`'s write to `log` then does not enable `c` ahead of `a`'s change handler.
#[test]
fn handler_write_after_a_pending_binding_flush_keeps_creation_order() {
    assert_eq!(log_of(FLUSHED, None), "d100b10a11c101", "interpreted");
    assert_eq!(
        log_of(FLUSHED, Some(FLUSHED_SITES)),
        "d100b10a11c101",
        "compiled"
    );
}

const ENABLED_FIRST: &str = r#"import QtQuick
Item {
    property string log: ""
    property int k: 1
    property int zz: 0
    property int dz: zz + 1
    onDzChanged: log += "z" + dz
    property int a: k + 1
    onAChanged: { zz = 9; log += "a" }
    property int b: k + 2
    onBChanged: log += "b"
}
"#;

const ENABLED_LAST: &str = r#"import QtQuick
Item {
    property string log: ""
    property int k: 1
    property int zz: 0
    property int a: k + 1
    onAChanged: { zz = 9; log += "a" }
    property int b: k + 2
    onBChanged: log += "b"
    property int dz: zz + 1
    onDzChanged: log += "z" + dz
}
"#;

static ENABLED_SITES: &[Site] = &[
    ("dz", |cx, env, _, _| {
        Ok(Value::Num(num(cx, env, &["zz"])? + 1.0))
    }),
    ("onDzChanged", |cx, env, _, _| {
        log_with(cx, env, &["log"], "z", "dz")
    }),
    ("a", |cx, env, _, _| {
        Ok(Value::Num(num(cx, env, &["k"])? + 1.0))
    }),
    ("b", |cx, env, _, _| {
        Ok(Value::Num(num(cx, env, &["k"])? + 2.0))
    }),
    ("onAChanged", |cx, env, _, _| {
        write(cx, env, &["zz"], Value::Num(9.0))?;
        append(cx, env, &["log"], "a")
    }),
    ("onBChanged", |cx, env, _, _| append(cx, env, &["log"], "b")),
];

/// The write still updates a dependent that was enabled before it, inside the write
/// (`z10` ahead of `a`); one still pending runs once, in its turn, on the written value.
#[test]
fn handler_write_during_creation_updates_only_enabled_dependents() {
    assert_eq!(log_of(ENABLED_FIRST, None), "z1z10ab", "interpreted");
    assert_eq!(
        log_of(ENABLED_FIRST, Some(ENABLED_SITES)),
        "z1z10ab",
        "compiled"
    );
    assert_eq!(log_of(ENABLED_LAST, None), "abz10", "interpreted");
    assert_eq!(
        log_of(ENABLED_LAST, Some(ENABLED_SITES)),
        "abz10",
        "compiled"
    );
}
