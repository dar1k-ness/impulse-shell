//! Compiled code is tied to its script site by the site's parse identity
//! ([`qml::NodeId`]) and must be compiled as the kind the runtime runs the site as.
use qml::jsrt::{Aot, BodyKind, Cx, Entry, Env, FnKind, FnMeta};
use qml::runtime::Runtime;
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

/// Compiles every site named `member` of `Main.qml` as `kind`, running `f`.
struct Fake {
    member: &'static str,
    kind: BodyKind,
    f: qml::jsrt::CompiledFn,
}

impl Aot for Fake {
    fn doc_scripts(&self, file: &Path, doc: &qml::Document) -> Vec<(qml::NodeId, &'static Entry)> {
        let mut out = vec![];
        if !file.ends_with("Main.qml") {
            return out;
        }
        qml::walk::sites(doc, &mut |s| {
            if s.name == self.member {
                let e: &'static Entry = Box::leak(Box::new(Entry {
                    file: "Main.qml",
                    path: Box::leak(s.path.clone().into_boxed_str()),
                    member: self.member,
                    kind: self.kind,
                    f: self.f,
                    meta: META,
                    hash: s.hash(),
                }));
                out.push((s.script.id, e));
            }
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

fn undefined(_: &mut Cx, _: &Env, _: &Value, _: &[Value]) -> qml::eval::R<Value> {
    Ok(Value::Undefined)
}

fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for (name, src) in files {
        std::fs::write(d.path().join(name), src).unwrap();
    }
    d
}

const ACTION: (&str, &str) = ("MyAction.qml", "import QtQuick\nScriptAction {}\n");
const MAIN: (&str, &str) = (
    "Main.qml",
    "import QtQuick\nItem {\n    SequentialAnimation { MyAction { script: console.log(1) } }\n}\n",
);

fn load(d: &tempfile::TempDir, aot: Fake) -> Result<usize, String> {
    let mut rt = Runtime::new(Some(d.path().to_path_buf()));
    rt.eng().set_compiled(Some(Rc::new(aot)));
    rt.load_file(&d.path().join("Main.qml"))
}

#[test]
fn script_of_a_derived_script_action_compiled_as_a_binding_is_rejected() {
    let d = tree(&[ACTION, MAIN]);
    let err = load(
        &d,
        Fake {
            member: "script",
            kind: BodyKind::Binding,
            f: undefined,
        },
    )
    .expect_err("a handler site ran code compiled as a binding");
    assert!(
        err.contains("compiled as Binding, runs as Handler"),
        "{err}"
    );
}

#[test]
fn script_of_a_derived_script_action_compiled_as_a_handler_loads() {
    let d = tree(&[ACTION, MAIN]);
    let r = load(
        &d,
        Fake {
            member: "script",
            kind: BodyKind::Handler,
            f: undefined,
        },
    );
    assert!(r.is_ok(), "{r:?}");
}

static RUNS: AtomicUsize = AtomicUsize::new(0);

fn counted(_: &mut Cx, _: &Env, _: &Value, _: &[Value]) -> qml::eval::R<Value> {
    RUNS.fetch_add(1, Ordering::Relaxed);
    Ok(Value::Num(7.0))
}

#[test]
fn delegate_instances_run_the_code_compiled_for_their_template() {
    let d = tree(&[(
        "Main.qml",
        "import QtQuick\nItem {\n    Repeater {\n        model: 3\n        Item { property int probe: 1 + 1 }\n    }\n}\n",
    )]);
    let mut rt = Runtime::new(Some(d.path().to_path_buf()));
    rt.eng().set_compiled(Some(Rc::new(Fake {
        member: "probe",
        kind: BodyKind::Binding,
        f: counted,
    })));
    rt.load_file(&d.path().join("Main.qml")).unwrap();
    rt.settle();
    let n = rt.world.borrow().items.len();
    let probes: Vec<String> = (0..n)
        .map(|i| rt.get(i, "probe"))
        .filter(|v| !matches!(v, Value::Undefined))
        .map(|v| v.to_js_string())
        .collect();
    assert_eq!(probes, ["7", "7", "7"]);
}

fn falsy(_: &mut Cx, _: &Env, _: &Value, _: &[Value]) -> qml::eval::R<Value> {
    Ok(Value::Bool(false))
}

#[test]
fn compiled_literal_is_assigned_while_populating_without_notify() {
    // Qt 6.12: a literal is a constant assigned while populating, also when its site has
    // compiled code; a binding with the same value emits activeChanged in finalize().
    let d = tree(&[(
        "Main.qml",
        "import QtQuick\nItem {\n    id: root\n    property bool on: false\n    property string out: \"\"\n    Loader { active: false; onActiveChanged: root.out += \"lit;\"; sourceComponent: Item {} }\n    Loader { active: root.on; onActiveChanged: root.out += \"bind;\"; sourceComponent: Item {} }\n}\n",
    )]);
    let mut rt = Runtime::new(Some(d.path().to_path_buf()));
    rt.eng().set_compiled(Some(Rc::new(Fake {
        member: "active",
        kind: BodyKind::Binding,
        f: falsy,
    })));
    let root = rt.load_file(&d.path().join("Main.qml")).unwrap();
    rt.settle();
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    assert_eq!(rt.get(root, "out").to_js_string(), "bind;");
}
