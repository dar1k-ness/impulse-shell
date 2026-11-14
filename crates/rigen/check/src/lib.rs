#[allow(dead_code, unused, clippy::all)]
pub mod rt {
    include!(concat!(env!("OUT_DIR"), "/rt.rs"));
}
/// Names the compiled QML cases are written against, as in ricompiled.
#[allow(unused_imports)]
mod abi {
    pub use qml::jsrt::{BodyKind, Entry};
}
/// The code rigen generated for the cases; clippy holds the hand-written code only.
#[allow(clippy::all)]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/gen.rs"));
}
pub use generated::*;
#[cfg(test)]
mod host;

use qml::eval::{Ex, Interp, ScriptCtx, ScriptFile};
use qml::value::Value;
use std::path::Path;
use std::rc::Rc;

/// The compiled code of `cases/included/` (`INCLUDES`), for the `Qt.include`s of the cases.
struct Includes;

impl qml::jsrt::Aot for Includes {
    fn doc_scripts(
        &self,
        _: &Path,
        _: &qml::Document,
    ) -> Vec<(qml::NodeId, &'static qml::jsrt::Entry)> {
        Vec::new()
    }
    fn library(&self, _: &Path) -> Option<&'static qml::jsrt::Entry> {
        None
    }
    fn script(&self, file: &Path) -> Option<&'static qml::jsrt::Entry> {
        INCLUDES.iter().find(|e| Path::new(e.file) == file)
    }
}

/// Makes the global scope of `it` the context of the JS file `path`, as a `.js` import's is.
fn file_context(it: &mut Interp, path: &str, aot: Option<Rc<dyn qml::jsrt::Aot>>) {
    let g = it.global.clone();
    g.borrow_mut().script = Some(Rc::new(ScriptFile {
        file: path.into(),
        ctx: Some(ScriptCtx::new(aot)),
    }));
    it.code = Some(g);
}

fn msg(it: &mut Interp, e: Ex) -> String {
    match e {
        Ex::Throw(v) => {
            let m = it
                .get(&v, "message")
                .map(|m| m.to_js_string())
                .unwrap_or_default();
            format!("throw {} {m}", v.to_js_string())
        }
        Ex::Short => "short".into(),
    }
}

/// `result` of a case program: (interpreter, compiled).
pub fn run_case(src: &str, path: &str, f: qml::jsrt::CompiledFn) -> (String, String) {
    let mut it = Interp::default();
    file_context(&mut it, path, None);
    let g = it.global.clone();
    // As `qml::eval::load_library` runs a `.js` file: `this` is the global object.
    g.borrow_mut().this = Some(Value::Obj(it.global_object.clone()));
    let body = qml::ir::lower_program(src).unwrap();
    let a = match it.run(&body, &g) {
        Ok(_) => {
            let v = g.borrow().vars.get("result").cloned().unwrap_or_default();
            it.to_string(&v).unwrap_or_else(|e| msg(&mut it, e))
        }
        Err(e) => msg(&mut it, e),
    };
    let mut cx = Interp::default();
    file_context(&mut cx, path, Some(Rc::new(Includes)));
    let env = qml::jsrt::Env::from_scope(cx.global.clone());
    // A library body is a top-level call from the engine: its promise jobs run as it returns.
    let r = cx.enter().and_then(|frame| {
        let this = Value::Obj(cx.global_object.clone());
        let r = f(&mut cx, &env, &this, &[]);
        cx.leave(frame, r)
    });
    let b = match r {
        Ok(_) => {
            // The compiled body keeps its top-level names in its frame, or in its context's
            // scope when it includes files: the namespace object of the file reads both.
            let script = cx.global.borrow().script.clone();
            let ctx = script.as_ref().and_then(|s| s.ctx.as_ref());
            if let Some(ctx) = ctx {
                ctx.export_vars(&cx.global);
            }
            let exports = ctx
                .map(|c| Value::Obj(c.exports.clone()))
                .unwrap_or_default();
            let v = cx.get(&exports, "result");
            v.and_then(|v| cx.to_string(&v))
                .unwrap_or_else(|e| msg(&mut cx, e))
        }
        Err(e) => msg(&mut cx, e),
    };
    (a, b)
}

/// The compiled code of one QML case document (`QML_CASES`).
struct QmlCase {
    file: &'static str,
    entries: &'static [qml::jsrt::Entry],
    layouts: &'static [qml::jsrt::Layout],
}

impl qml::jsrt::Aot for QmlCase {
    fn doc_scripts(
        &self,
        file: &Path,
        doc: &qml::Document,
    ) -> Vec<(qml::NodeId, &'static qml::jsrt::Entry)> {
        if file != Path::new(self.file) {
            return Vec::new();
        }
        let mut out = Vec::new();
        qml::walk::sites(doc, &mut |s| {
            let h = s.hash();
            let e = self
                .entries
                .iter()
                .find(|e| e.path == s.path && e.member == s.name && e.hash == h)
                .unwrap_or_else(|| panic!("{}: {}.{} not compiled", self.file, s.path, s.name));
            out.push((s.script.id, e));
        });
        out
    }
    fn doc_layouts(
        &self,
        file: &Path,
        doc: &qml::Document,
    ) -> Vec<(qml::NodeId, &'static [&'static str])> {
        if file != Path::new(self.file) {
            return Vec::new();
        }
        let mut out = Vec::new();
        qml::walk::decls(doc, &mut |path, o, names| {
            if let Some(l) = self.layouts.iter().find(|l| l.0 == path && l.1 == names) {
                out.push((o.decl, l.1));
            }
        });
        out
    }
    fn library(&self, file: &Path) -> Option<&'static qml::jsrt::Entry> {
        let lib = QML_LIBS.iter().find(|(f, _)| Path::new(f) == file);
        lib.map(|(_, entry)| *entry)
    }
    fn script(&self, _: &Path) -> Option<&'static qml::jsrt::Entry> {
        None
    }
}

/// `result` of the root of a QML case once loaded as the shell starts (`load_singletons`,
/// `load_file`, settle, the `Qt.callLater` calls it queued): (interpreter, compiled). A case
/// that calls `Qt.quit()` is read when it has: the loop turns on a virtual clock, 16 ms a
/// frame, until then.
pub fn run_qml_case(
    path: &'static str,
    entries: &'static [qml::jsrt::Entry],
    layouts: &'static [qml::jsrt::Layout],
) -> (String, String) {
    let file = Path::new(path);
    let dir = file.parent().unwrap();
    // `// errors: <n>` pins how many errors the case leaves in the engine: the warnings Qt
    // prints for it.
    let allowed = std::fs::read_to_string(file)
        .ok()
        .and_then(|s| {
            let pin = s.lines().find_map(|l| l.strip_prefix("// errors: "))?;
            pin.split(' ').next()?.parse::<usize>().ok()
        })
        .unwrap_or(0);
    let quits = std::fs::read_to_string(file).is_ok_and(|s| s.contains("Qt.quit()"));
    let run = |aot: Option<Rc<dyn qml::jsrt::Aot>>| {
        let mut rt = qml::runtime::Runtime::new(Some(dir.to_path_buf()));
        rt.eng().set_compiled(aot);
        rt.load_singletons(&[dir.to_path_buf()]);
        let root = match rt.load_file(file) {
            Ok(r) => r,
            Err(e) => return format!("load error {e}"),
        };
        rt.settle();
        // What the loop brings: the listing of a FolderListModel.
        rt.poll_native();
        rt.settle();
        if quits {
            let mut now = 0.0;
            while rt.world.borrow().quit.is_none() && now < 60_000.0 {
                now += 16.0;
                rt.tick(now);
            }
        }
        // The next turns of the loop: what the document left to `Qt.callLater`.
        while !quits && rt.later_pending() {
            rt.run_later();
            rt.settle();
        }
        let errors = rt.errors();
        let v = rt.get(root, "result").to_js_string();
        if errors.len() == allowed {
            v
        } else {
            format!("{v} errors {errors:?}")
        }
    };
    let case = QmlCase {
        file: path,
        entries,
        layouts,
    };
    (run(None), run(Some(Rc::new(case))))
}

#[cfg(test)]
mod tests {
    /// Names a QML case to the process that runs it in an environment of its own.
    const CASE: &str = "RIGEN_CHECK_QML_CASE";

    /// The environments a QML case is run in, and Qt's answer in each: a `// env: NAME=value
    /// ...` line and the `// qt: ` line under it. A case without one runs once, in this
    /// process, and its one `// qt: ` line is the answer.
    ///
    /// A case with an `// answers: <file>` line (the path from the case) takes Qt's answers
    /// from that file, the one a fixture of crates/qml reads, so both engines answer to the
    /// same lines: a block of it begins with `@` and an environment, then come an expression
    /// and, behind `=> `, what Qt answered. The case runs in the environment of each block
    /// and its `result` is the answers, one a line, to the lines of the case that are
    /// expressions of the block (less the comma after them).
    fn environments(src: &str, path: &str) -> Vec<(Option<String>, Option<String>)> {
        fn qt(line: Option<&&str>) -> Option<String> {
            line?.strip_prefix("// qt: ").map(str::to_string)
        }
        let lines: Vec<&str> = src.lines().collect();
        if let Some(file) = lines.iter().find_map(|l| l.strip_prefix("// answers: ")) {
            let file = std::path::Path::new(path).parent().unwrap().join(file);
            let answers = std::fs::read_to_string(&file).unwrap();
            let asked = |l: &&str| l.trim().trim_end_matches(',').to_string();
            let asked: Vec<String> = lines.iter().map(asked).collect();
            let mut envs: Vec<(Option<String>, Vec<&str>)> = Vec::new();
            let mut all = answers.lines().filter(|l| !l.is_empty()).peekable();
            while let Some(line) = all.next() {
                if let Some(vars) = line.strip_prefix("@ ") {
                    envs.push((Some(vars.to_string()), vec![""; asked.len()]));
                } else if let Some(at) = asked.iter().position(|a| a == line) {
                    envs.last_mut().unwrap().1[at] =
                        all.peek().unwrap().strip_prefix("=> ").unwrap();
                }
            }
            let answered = |qt: Vec<&str>| {
                let qt: Vec<&str> = qt.into_iter().filter(|a| !a.is_empty()).collect();
                assert!(
                    !qt.is_empty(),
                    "{path}: no expression of {}",
                    file.display()
                );
                Some(qt.join("\n"))
            };
            return envs
                .into_iter()
                .map(|(vars, qt)| (vars, answered(qt)))
                .collect();
        }
        let envs: Vec<_> = lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| Some((l.strip_prefix("// env:")?, qt(lines.get(i + 1)))))
            .map(|(vars, qt)| (Some(vars.trim().to_string()), qt))
            .collect();
        if envs.is_empty() {
            vec![(None, lines.iter().find_map(|l| qt(Some(l))))]
        } else {
            envs
        }
    }

    /// [`crate::run_qml_case`] of the case `name` in a process that has the variables of
    /// `vars` (`NAME=value ...`) and no others: what the machine's environment says does not
    /// reach the case.
    fn run_qml_case_in(name: &str, vars: &str) -> (String, String) {
        let mut process = std::process::Command::new(std::env::current_exe().unwrap());
        process
            .args([
                "tests::qml_cases_match_interpreter",
                "--exact",
                "--nocapture",
            ])
            .env_clear()
            .envs(vars.split(' ').filter_map(|v| v.split_once('=')))
            .env(CASE, name);
        let ran = process.output().unwrap();
        let out = String::from_utf8_lossy(&ran.stdout);
        let part = |mode: &str| {
            let part = out
                .split_once(&format!("{mode}<<<"))
                .and_then(|(_, rest)| rest.split_once(">>>"));
            part.map_or_else(|| format!("no answer: {out}"), |(part, _)| part.to_string())
        };
        (part("interp"), part("compiled"))
    }

    /// The first lines of two answers that differ, with their number.
    fn first_difference(a: &str, b: &str) -> String {
        let differ = a
            .lines()
            .zip(b.lines())
            .enumerate()
            .find(|(_, (a, b))| a != b);
        match differ {
            Some((n, (a, b))) => format!("line {}:\n    {a}\n    {b}", n + 1),
            None => format!("{} lines and {}", a.lines().count(), b.lines().count()),
        }
    }

    #[test]
    fn qml_cases_match_interpreter() {
        if let Ok(case) = std::env::var(CASE) {
            let (_, _, path, entries, layouts) =
                crate::QML_CASES.iter().find(|c| c.0 == case).unwrap();
            let (a, b) = crate::run_qml_case(path, entries, layouts);
            println!("\ninterp<<<{a}>>>\ncompiled<<<{b}>>>");
            return;
        }
        let mut bad = Vec::new();
        for (name, src, path, entries, layouts) in crate::QML_CASES {
            // `// qt: <result>` pins what Qt 6.12's QQmlEngine leaves in the root's `result`
            // (`load error <line>:<column>: <message>` when it does not load the file).
            // `// ours: <result>` beside it pins a difference of the engine that is known and
            // not fixed: what we leave there instead.
            let ours = src.lines().find_map(|l| l.strip_prefix("// ours: "));
            for (vars, qt) in environments(src, path) {
                let (a, b) = match &vars {
                    Some(vars) => run_qml_case_in(name, vars),
                    None => crate::run_qml_case(path, entries, layouts),
                };
                let name = vars.map_or(name.to_string(), |vars| format!("{name} [{vars}]"));
                if a != b {
                    bad.push(format!(
                        "{name}:\n  interp, compiled: {}",
                        first_difference(&a, &b)
                    ));
                } else if let Some(ours) =
                    ours.filter(|ours| *ours != a || qt.as_deref() == Some(*ours))
                {
                    bad.push(format!(
                        "{name}:\n  ours:   {ours}\n  native: {a}\n  qt:     {qt:?}"
                    ));
                } else if let Some(qt) = qt.filter(|qt| ours.is_none() && *qt != a) {
                    bad.push(format!(
                        "{name}:\n  qt, native: {}",
                        first_difference(&qt, &a)
                    ));
                } else if a.contains('\n') {
                    println!("{name}: {} lines as Qt's", a.lines().count());
                } else {
                    println!("{name}: {a}");
                }
            }
        }
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }

    /// The fixtures of `crates/qml/tests` in `QML_FIXTURES` give Qt 6.12's lines with and
    /// without their compiled code. Their own tests run the interpreter only.
    #[test]
    fn qml_fixtures_match_qt_in_both_modes() {
        use std::path::Path;
        use std::rc::Rc;
        // What Qt 6.12 printed, and the cases the fixture's own test lists as known to differ
        // (`KNOWN` in builtins_b1_qt_equiv.rs).
        let pins: [(&str, &str, &[&str]); 3] = [
            (
                "fn",
                include_str!("../../../qml/tests/data/builtins_b1/fn.qt612"),
                &["N1 ", "N9 "],
            ),
            (
                "n3",
                include_str!("../../../qml/tests/data/builtins_b1/n3.qt612"),
                &["N3h "],
            ),
            (
                "R06MethodValue",
                include_str!("../../../qml/tests/runtime_audit_r3/R06MethodValue.qt"),
                &[],
            ),
        ];
        assert_eq!(crate::QML_FIXTURES.len(), pins.len());
        // The engine keeps no console lines: the test takes them as they are printed.
        thread_local! {
            static PRINTED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
        }
        let _ = qml::builtins::CONSOLE_SINK.set(|_, line| {
            PRINTED.with_borrow_mut(|p| p.push(line.to_string()));
        });
        let mut bad = Vec::new();
        for (name, qt, known) in pins {
            let (_, _, path, entries, layouts) =
                (crate::QML_FIXTURES.iter().find(|c| c.0 == name)).expect(name);
            let file = Path::new(path);
            for compiled in [false, true] {
                let aot = compiled.then(|| {
                    let case = crate::QmlCase {
                        file: path,
                        entries,
                        layouts,
                    };
                    Rc::new(case) as Rc<dyn qml::jsrt::Aot>
                });
                PRINTED.with_borrow_mut(Vec::clear);
                let mut rt = qml::runtime::Runtime::new(file.parent().map(Path::to_path_buf));
                rt.eng().set_compiled(aot);
                let root = rt.load_file(file).unwrap();
                rt.settle();
                // The fixture's timers run out on the engine's clock.
                for t in 0..=700 {
                    rt.tick(f64::from(t));
                }
                // A fixture keeps its lines in `log`, or prints them.
                let got = match rt.get(root, "log") {
                    qml::value::Value::Undefined => PRINTED.take().join("\n"),
                    log => log.to_js_string(),
                };
                let lines = |s: &str| -> Vec<String> {
                    let known = |l: &&str| known.iter().any(|k| l.starts_with(k));
                    let kept = s.trim_end().lines().filter(|l| !known(l));
                    kept.map(str::to_string).collect()
                };
                let errors = rt.errors();
                if lines(&got) != lines(qt) || !errors.is_empty() {
                    bad.push(format!(
                        "{name} compiled={compiled} errors {errors:?}:\n{got}"
                    ));
                }
            }
        }
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }

    #[test]
    fn cases_match_interpreter() {
        let mut bad = Vec::new();
        for (name, src, path, f) in crate::CASES {
            let (a, b) = crate::run_case(src, path, *f);
            // `// v4: <result>` pins the output Qt's V4 engine produces for the case;
            // `// expect: <result>` ECMA-262's, for a case V4 cannot run.
            let v4 = src.lines().find_map(|l| l.strip_prefix("// v4: "));
            let expect = src.lines().find_map(|l| l.strip_prefix("// expect: "));
            if a != b {
                bad.push(format!("{name}:\n  interp:   {a}\n  compiled: {b}"));
            } else if let Some(v4) = v4.filter(|v4| *v4 != a) {
                bad.push(format!("{name}:\n  v4:     {v4}\n  native: {a}"));
            } else if let Some(expect) = expect.filter(|e| *e != a) {
                bad.push(format!("{name}:\n  expect: {expect}\n  native: {a}"));
            } else {
                println!("{name}: {a}");
            }
        }
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }

    #[test]
    fn ri_counts() {
        println!(
            "QML scripts compiled: qml {} js {} unlowerable {}",
            crate::RI_COUNTS.0,
            crate::RI_COUNTS.1,
            crate::RI_COUNTS.2
        );
        assert_eq!(
            crate::RI_COUNTS.2,
            0,
            "QML scripts rigen could not parse or lower (build.rs prints each as a cargo warning)"
        );
    }
}
