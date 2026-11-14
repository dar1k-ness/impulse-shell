//! The release shell has no JavaScript interpreter (`cargo test -p qml --no-default-features
//! --test no_interp`, run by tools/testenv/ri-gate): QML text that `Qt.createQmlObject`
//! compiles at run time has no compiled code.
#![cfg(not(feature = "interp"))]
use qml::eval::Ex;
use qml::runtime::{Runtime, create_qml_object};
use qml::value::Value;

fn thrown(r: &Result<Value, Ex>) -> String {
    match r {
        Err(Ex::Throw(e)) => e.to_js_string(),
        Err(Ex::Short) => "short".into(),
        Ok(v) => format!("returned {}", v.to_js_string()),
    }
}

/// Whether this is `test`'s own process (see `testenv::run_in_own_process`), which the process
/// running the test starts with a diagnostics switch set, as a user's environment might hold
/// it (they are read once per process).
fn hostile_env(test: &str) -> bool {
    if testenv::in_own_process(test) {
        return true;
    }
    testenv::run_in_own_process(test, vec![("RI_DEBUG", "noplan")]);
    false
}

#[test]
fn create_qml_object_with_scripts_throws_before_creating_anything() {
    if !hostile_env(concat!(
        module_path!(),
        "::create_qml_object_with_scripts_throws_before_creating_anything"
    )) {
        return;
    }
    let mut rt = Runtime::new(None);
    let root = rt.load_str("import QtQuick\nItem {}\n").unwrap();
    rt.settle();
    let n0 = rt.world.borrow().items.len();
    let src =
        Value::str("import QtQuick\nItem { Item { width: 3 } property int n: 2; onNChanged: n++ }");
    let r = create_qml_object(&mut rt.it, &Value::Undefined, &[src, Value::Item(root)]);
    let msg = thrown(&r);
    assert!(
        msg.contains("no JavaScript interpreter") && msg.contains("width, n, onNChanged"),
        "{msg}"
    );
    assert_eq!(
        rt.world.borrow().items.len(),
        n0,
        "a half-built object stayed behind"
    );

    // Text without scripts needs no compiled code and still works.
    let src = Value::str("import QtQuick\nRow { Repeater {} }");
    let r = create_qml_object(&mut rt.it, &Value::Undefined, &[src, Value::Item(root)]);
    assert!(matches!(r, Ok(Value::Item(_))), "{}", thrown(&r));
}

/// The diagnostics switches of `RI_DEBUG` apply in the shipped shell too.
#[test]
fn diagnostics_switches_apply() {
    if !hostile_env(concat!(module_path!(), "::diagnostics_switches_apply")) {
        return;
    }
    assert!(
        qml::debug::on(qml::debug::Switch::NOPLAN),
        "diagnostics switches still apply"
    );
}

/// A document whose scripts have no compiled code (a file outside the compiled tree, or one
/// edited since the build) fails to load, as Qt fails a component it cannot compile, instead of
/// building objects without those bindings and handlers. One without scripts (no binding or
/// handler) still loads.
#[test]
fn document_without_compiled_code_fails_to_load() {
    if !hostile_env(concat!(
        module_path!(),
        "::document_without_compiled_code_fails_to_load"
    )) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let scripted = dir.path().join("Scripted.qml");
    std::fs::write(
        &scripted,
        "import QtQuick\nItem { width: 2 + 3; function f() { return 1 } }\n",
    )
    .unwrap();
    let plain = dir.path().join("Plain.qml");
    std::fs::write(&plain, "import QtQuick\nItem { Item {} }\n").unwrap();

    let mut rt = Runtime::new(None);
    let n0 = rt.world.borrow().items.len();
    let e = rt.load_file(&scripted).unwrap_err();
    assert!(
        e.contains("Scripted.qml: 2/2 scripts have no compiled code"),
        "{e}"
    );
    assert_eq!(
        rt.world.borrow().items.len(),
        n0,
        "objects were built from it"
    );
    // Not cached as loaded: a second attempt fails the same way.
    assert!(rt.load_file(&scripted).is_err());
    rt.load_file(&plain).unwrap();
}

/// A `.js` import with no compiled code (or a missing one) fails the document load, as Qt
/// fails a component whose script import it cannot resolve, instead of loading it with the
/// import's namespace missing and only an entry in `errors`.
#[test]
fn js_import_without_compiled_code_fails_to_load() {
    if !hostile_env(concat!(
        module_path!(),
        "::js_import_without_compiled_code_fails_to_load"
    )) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("lib.js"), ".pragma library\nvar k = 1\n").unwrap();
    let uncompiled = dir.path().join("Uncompiled.qml");
    std::fs::write(
        &uncompiled,
        "import QtQuick\nimport \"lib.js\" as Lib\nItem {}\n",
    )
    .unwrap();
    let missing = dir.path().join("Missing.qml");
    std::fs::write(
        &missing,
        "import QtQuick\nimport \"gone.js\" as Gone\nItem {}\n",
    )
    .unwrap();

    let mut rt = Runtime::new(None);
    let n0 = rt.world.borrow().items.len();
    for (f, want) in [
        (&uncompiled, "\"lib.js\" unavailable"),
        (&missing, "\"gone.js\" unavailable"),
    ] {
        let e = rt.load_file(f).unwrap_err();
        assert!(e.contains(want), "{e}");
        assert_eq!(rt.world.borrow().items.len(), n0, "objects were built: {e}");
        // Not cached as loaded: a second attempt fails the same way.
        assert!(rt.load_file(f).is_err());
    }
    assert!(
        rt.world.borrow().errors.is_empty(),
        "{:?}",
        rt.world.borrow().errors
    );
}

/// Which bindings Qt's QmlIR stores as constants (no NOTIFY while creating) is decided when
/// the document is parsed, so the shipped shell decides it without the interpreter: literals
/// followed by comments and substitution-free templates are constants.
#[test]
fn qmlir_constants_are_decided_without_the_interpreter() {
    let doc = qml::parse(
        "import QtQuick\nText {\n    visible: false // c\n    width: 5 /* c */\n    text: `y`\n    \
         horizontalAlignment: Text.AlignRight // c\n    height: (5)\n    color: `${x}`\n}\n",
    )
    .unwrap();
    let shapes: Vec<(&str, qml::Shape)> = doc
        .root
        .members
        .iter()
        .filter_map(|m| match m {
            qml::Member::Binding {
                name,
                value: qml::Value::Script(s),
            } => Some((name.as_str(), s.shape.clone())),
            _ => None,
        })
        .collect();
    use qml::Shape::*;
    assert_eq!(
        shapes,
        [
            ("visible", Literal),
            ("width", Literal),
            ("text", Literal),
            ("horizontalAlignment", Member("Text.AlignRight".into())),
            ("height", Script),
            ("color", Script),
        ]
    );
}

/// `eval` of a string needs an evaluator, which the shipped shell does not have: it throws
/// EvalError like `Function(...)`; a non-string is returned as is (PerformEval step 2).
#[test]
fn eval_of_text_throws_without_an_interpreter() {
    if !hostile_env(concat!(
        module_path!(),
        "::eval_of_text_throws_without_an_interpreter"
    )) {
        return;
    }
    let mut rt = Runtime::new(None);
    let eval = rt.it.global.borrow().vars.get("eval").cloned().unwrap();
    let r = rt.it.call(&eval, Value::Undefined, &[Value::str("1 + 2")]);
    let Err(Ex::Throw(e)) = &r else {
        panic!("{}", thrown(&r));
    };
    let name = rt.it.get(e, "name").unwrap().to_js_string();
    assert_eq!(name, "EvalError");
    assert!(
        thrown(&r).contains("needs the interpreter"),
        "{}",
        thrown(&r)
    );
    let r = rt.it.call(&eval, Value::Undefined, &[Value::Num(4.0)]);
    assert!(matches!(r, Ok(Value::Num(n)) if n == 4.0), "{}", thrown(&r));
}

/// `Function(params, body)` compiles its text at run time (CreateDynamicFunction): without an
/// interpreter it throws EvalError and returns no function, with or without arguments.
#[test]
fn function_from_text_throws_without_an_interpreter() {
    if !hostile_env(concat!(
        module_path!(),
        "::function_from_text_throws_without_an_interpreter"
    )) {
        return;
    }
    let mut rt = Runtime::new(None);
    let function = rt.it.global.borrow().vars.get("Function").cloned().unwrap();
    for args in [
        vec![Value::str("a"), Value::str("return a + 1")],
        vec![Value::str("return 7")],
        vec![],
    ] {
        let r = rt.it.call(&function, Value::Undefined, &args);
        let Err(Ex::Throw(e)) = &r else {
            panic!("{}", thrown(&r));
        };
        let name = rt.it.get(e, "name").unwrap().to_js_string();
        assert_eq!(name, "EvalError");
        assert!(
            thrown(&r).contains("needs the interpreter"),
            "{}",
            thrown(&r)
        );
    }
}

/// The same constructor reached where no build check can see it, as `.constructor` of a
/// function value (`f.constructor("return 7")`, `new f.constructor(...)`), and the generator
/// function constructor, which has no global name: EvalError, and no function comes back.
#[test]
fn function_constructor_of_a_value_throws_without_an_interpreter() {
    if !hostile_env(concat!(
        module_path!(),
        "::function_constructor_of_a_value_throws_without_an_interpreter"
    )) {
        return;
    }
    let mut rt = Runtime::new(None);
    let global = |rt: &Runtime, n: &str| rt.it.global.borrow().vars.get(n).cloned().unwrap();
    let function = global(&rt, "Function");
    let some_function = global(&rt, "parseInt");
    let ctor = rt.it.get(&some_function, "constructor").unwrap();
    assert!(ctor.strict_eq(&function), "{}", ctor.to_js_string());
    let generator_function = Value::Obj(rt.it.generator_protos.function.clone());
    let generator_ctor = rt.it.get(&generator_function, "constructor").unwrap();
    assert!(!generator_ctor.strict_eq(&function));
    let args = [Value::str("return 7")];
    for r in [
        rt.it.call(&ctor, some_function.clone(), &args),
        rt.it.construct(&ctor, &args),
        rt.it.call(&generator_ctor, Value::Undefined, &args),
    ] {
        let Err(Ex::Throw(e)) = &r else {
            panic!("{}", thrown(&r));
        };
        let name = rt.it.get(e, "name").unwrap().to_js_string();
        assert_eq!(name, "EvalError", "{}", thrown(&r));
        assert!(
            thrown(&r).contains("needs the interpreter"),
            "{}",
            thrown(&r)
        );
    }
}

/// `Qt.createComponent` loads its file as a Loader source does: a document whose scripts have
/// no compiled code gives a Component.Error component (nothing is compiled at run time), one
/// without scripts a Component.Ready one. `Qt.include` runs no file it has no compiled code for.
#[test]
fn create_component_and_include_compile_nothing() {
    if !hostile_env(concat!(
        module_path!(),
        "::create_component_and_include_compile_nothing"
    )) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let scripted = dir.path().join("Scripted.qml");
    std::fs::write(&scripted, "import QtQuick\nItem { width: 2 + 3 }\n").unwrap();
    let plain = dir.path().join("Plain.qml");
    std::fs::write(&plain, "import QtQuick\nItem { Item {} }\n").unwrap();
    std::fs::write(dir.path().join("lib.js"), "var k = 1\n").unwrap();

    let mut rt = Runtime::new(None);
    let root = rt.load_file(&plain).unwrap();
    rt.settle();
    let create = |rt: &mut Runtime, f: &std::path::Path| {
        let url = Value::str(&f.display().to_string());
        let r = qml::runtime::create_component(
            &mut rt.it,
            &Value::Undefined,
            &[url, Value::Item(root)],
        );
        match r {
            Ok(Value::Item(c)) => c,
            r => panic!("{}", thrown(&r)),
        }
    };
    let c = create(&mut rt, &scripted);
    assert_eq!(rt.get(c, "status").to_number(), 3.0);
    let e = rt.get(c, "\x01error").to_js_string();
    assert!(
        e.contains("Scripted.qml: 1/1 scripts have no compiled code"),
        "{e}"
    );
    let c = create(&mut rt, &plain);
    assert_eq!(rt.get(c, "status").to_number(), 1.0);

    let r = qml::qtobj::include(&mut rt.it, &Value::Undefined, &[Value::str("lib.js")]);
    let e = thrown(&r);
    assert!(
        e.ends_with("Qt.include(): Can only be called from JavaScript files"),
        "{e}"
    );

    // From a JS file's context: the build compiled no code for the file, which the include
    // result reports as Qt reports a script that fails to compile.
    std::fs::write(dir.path().join("inc.js"), "var k = 1\n").unwrap();
    let ctx = qml::eval::Interp::child(&rt.it.global);
    ctx.borrow_mut().script = Some(std::rc::Rc::new(qml::eval::ScriptFile {
        file: dir.path().join("main.js"),
        ctx: Some(qml::eval::ScriptCtx::new(None)),
    }));
    rt.it.code = Some(ctx);
    let r = qml::qtobj::include(&mut rt.it, &Value::Undefined, &[Value::str("inc.js")]).unwrap();
    rt.it.code = None;
    assert_eq!(rt.it.get(&r, "status").unwrap().to_number(), 3.0);
    let ex = rt.it.get(&r, "exception").unwrap();
    let m = rt.it.get(&ex, "message").unwrap().to_js_string();
    assert!(m.ends_with("inc.js: no compiled code"), "{m}");
}

fn binding_getter(
    cx: &mut qml::jsrt::Cx,
    _: &qml::jsrt::Env,
    this: &Value,
    _: &[Value],
) -> qml::eval::R<Value> {
    let w = cx.get(this, "w")?;
    Ok(Value::Num(w.to_number() * 2.0))
}

const GETTER: qml::jsrt::FnMeta = qml::jsrt::FnMeta {
    name: "",
    kind: qml::jsrt::FnKind::Normal,
    nparams: 0,
    nslots: 0,
    strict: false,
};

/// `p = Qt.binding(function () { return this.w * 2 })` as compiled code runs it.
fn assign_binding(
    cx: &mut qml::jsrt::Cx,
    env: &qml::jsrt::Env,
    _: &Value,
    _: &[Value],
) -> qml::eval::R<Value> {
    let qt = qml::jsrt::lookup(cx, env, "Qt")?;
    let binding = cx.get(&qt, "binding")?;
    let f = qml::jsrt::closure(binding_getter, env, &GETTER);
    let b = cx.call(&binding, qt, &[f])?;
    qml::jsrt::assign_qml(cx, env, qml::runtime::Sym::new("p"), b)?;
    Ok(Value::Undefined)
}

struct OnCompleted(qml::jsrt::CompiledFn);

impl qml::jsrt::Aot for OnCompleted {
    fn doc_scripts(
        &self,
        file: &std::path::Path,
        doc: &qml::Document,
    ) -> Vec<(qml::NodeId, &'static qml::jsrt::Entry)> {
        let mut out = vec![];
        qml::walk::sites(doc, &mut |s| {
            if s.name == "Component.onCompleted" {
                let e: &'static qml::jsrt::Entry = Box::leak(Box::new(qml::jsrt::Entry {
                    file: Box::leak(file.display().to_string().into_boxed_str()),
                    path: Box::leak(s.path.clone().into_boxed_str()),
                    member: "Component.onCompleted",
                    kind: qml::jsrt::BodyKind::Handler,
                    f: self.0,
                    meta: GETTER,
                    hash: s.hash(),
                }));
                out.push((s.script.id, e));
            }
        });
        out
    }
    fn library(&self, _: &std::path::Path) -> Option<&'static qml::jsrt::Entry> {
        None
    }
    fn script(&self, _: &std::path::Path) -> Option<&'static qml::jsrt::Entry> {
        None
    }
}

/// Compiled code assigning a `Qt.binding` result installs the binding as the interpreter does
/// (Qt 6.12 "Property Binding"): live with `this` the target object, until a plain assignment.
#[test]
fn compiled_qt_binding_assignment_installs_a_binding() {
    if !hostile_env(concat!(
        module_path!(),
        "::compiled_qt_binding_assignment_installs_a_binding"
    )) {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("Main.qml");
    std::fs::write(
        &main,
        "import QtQuick\nItem {\n    property int w\n    property int p\n    \
         Component.onCompleted: p = Qt.binding(function () { return this.w * 2 })\n}\n",
    )
    .unwrap();
    let mut rt = Runtime::new(Some(dir.path().to_path_buf()));
    rt.eng()
        .set_compiled(Some(std::rc::Rc::new(OnCompleted(assign_binding))));
    let root = rt.load_file(&main).unwrap();
    rt.settle();
    assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    assert_eq!(rt.get(root, "p").to_number(), 0.0);
    rt.set(root, "w", Value::Num(3.0));
    assert_eq!(rt.get(root, "p").to_number(), 6.0);
    rt.set(root, "w", Value::Num(5.0));
    assert_eq!(rt.get(root, "p").to_number(), 10.0);
    rt.set(root, "p", Value::Num(1.0));
    rt.set(root, "w", Value::Num(7.0));
    assert_eq!(rt.get(root, "p").to_number(), 1.0);
}
