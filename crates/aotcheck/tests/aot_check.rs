//! aotcheck (docs/aot.md) on a small ri-shaped tree.
use std::path::Path;
use std::process::Command;

const STATES: &str =
    "pragma Singleton\nimport Quickshell\nSingleton {\n    property bool panelOpen: false\n}\n";

const TEST: &str = r#"import QtQuick
import ri.modules.common
Item {
    id: root
    property int w: GlobalStates.panelOpen ? 5 * 2 : 3
    property string s: Math.max(1, 2) + "x" + new Date().getFullYear()
    property string s2: ""
    property var probe: typeof Math.notABuiltin
    onWChanged: s2 = "changed " + w
    function twice(x) { return x * 2 }
    Item { width: root.twice(root.w) }
}
"#;

/// A tree holding `test` as `modules/rusty/test/Test.qml`, removed when dropped.
fn tree(test: &str) -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    let d = t.path();
    std::fs::create_dir_all(d.join("modules/common")).unwrap();
    std::fs::create_dir_all(d.join("modules/rusty/test")).unwrap();
    std::fs::write(d.join("modules/common/GlobalStates.qml"), STATES).unwrap();
    std::fs::write(d.join("modules/rusty/test/Test.qml"), test).unwrap();
    t
}

fn run(root: &Path, cand: &Path, out: &Path) -> (String, String, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_aotcheck"))
        .arg(root)
        .args(["--cand".as_ref(), cand.as_os_str()])
        .args(["--modules", "test/Test.qml", "--frames", "3"])
        .args(["--out".as_ref(), out.as_os_str()])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let rd = |f: &str| std::fs::read_to_string(out.join(f)).unwrap();
    (
        String::from_utf8_lossy(&o.stdout).into(),
        rd("keys.tsv"),
        rd("builtins.tsv"),
        rd("divergences.tsv"),
    )
}

#[test]
fn identical_trees_do_not_diverge() {
    let t = tree(TEST);
    let t = t.path();
    let (stdout, keys, builtins, div) = run(t, t, &t.join("out"));
    assert!(stdout.contains("divergences 0"), "{stdout}");
    assert_eq!(div.lines().count(), 1, "{div}");
    for k in [
        "Test.qml|0|w\tbinding",
        "Test.qml|0|onWChanged\thandler",
        "Test.qml|0|twice\tfunction",
        "Test.qml|0.0|width\tbinding",
    ] {
        assert!(keys.contains(k), "{k} not in\n{keys}");
    }
    assert!(
        builtins
            .lines()
            .any(|l| l.starts_with("Math.max\t") && l.contains("\tok\t")),
        "{builtins}"
    );
    assert!(
        builtins
            .lines()
            .any(|l| l.starts_with(".getFullYear\t") && l.contains("\tok\t")),
        "{builtins}"
    );
    assert!(
        builtins
            .lines()
            .any(|l| l.starts_with("Math.notABuiltin\t") && l.contains("\tmissing")),
        "{builtins}"
    );
}

/// A snapshot value longer than the 4000 bytes kept is cut between characters: a cut inside
/// one panicked the worker, which reads as a divergence.
#[test]
fn long_non_ascii_value_does_not_diverge() {
    let t = tree(&TEST.replace(
        "property string s2: \"\"",
        "property string s2: \"\"\n    property string long: \"я\".repeat(3000)",
    ));
    let t = t.path();
    let (stdout, _, _, div) = run(t, t, &t.join("out"));
    assert!(stdout.contains("divergences 0"), "{stdout}");
    assert_eq!(div.lines().count(), 1, "{div}");
}

#[test]
fn first_divergence_is_attributed_to_its_binding() {
    let a = tree(TEST);
    let b = tree(&TEST.replace("5 * 2", "5 * 2 + 1"));
    let (stdout, _, _, div) = run(a.path(), b.path(), &a.path().join("out"));
    assert!(!stdout.contains("divergences 0"), "{stdout}");
    let first = div.lines().nth(1).expect("a divergence");
    assert!(first.contains("\topen panelOpen\t"), "{first}");
    assert!(
        first.contains("\tw\tmodules/rusty/test/Test.qml|0|w\t10\t11\t"),
        "{div}"
    );
}

/// A tree `--strict` has nothing against: no missing builtin, every step the same on both sides.
const CLEAN: &str = r#"import QtQuick
import ri.modules.common
Item {
    id: root
    property int w: GlobalStates.panelOpen ? 5 * 2 : 3
    property string s2: ""
    onWChanged: s2 = "changed " + w
}
"#;

fn strict(root: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_aotcheck"))
        .arg(root)
        .args(["--modules", "test/Test.qml", "--frames", "3", "--strict"])
        .args(["--out".as_ref(), root.join("out").as_os_str()])
        .output()
        .unwrap()
}

#[test]
fn strict_passes_a_tree_without_script_errors() {
    let o = strict(tree(CLEAN).path());
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{stdout}");
    assert!(stdout.contains(" errors 0 "), "{stdout}");
    assert!(!stdout.contains("\nERROR "), "{stdout}");
}

/// The handler throws once the panel's flag opens it: the same on both sides, so no step
/// diverges, and the error is the only thing `--strict` has to fail on.
#[test]
fn strict_fails_on_a_script_error_raised_while_a_panel_opens() {
    let t = tree(&CLEAN.replace(r#"s2 = "changed " + w"#, "s2 = root.noSuchObject.title"));
    let t = t.path();
    let o = strict(t);
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert_eq!(o.status.code(), Some(1), "{stdout}");
    // Three steps change `w` (the load and the flag set and cleared), one distinct message.
    assert!(stdout.contains(" divergences 0 errors 3 "), "{stdout}");
    let lines: Vec<&str> = stdout.lines().filter(|l| l.starts_with("ERROR ")).collect();
    assert_eq!(lines.len(), 1, "{stdout}");
    assert!(
        lines[0].contains("onWChanged: Cannot read property 'title' of undefined"),
        "{stdout}"
    );
    // Without --strict the error is reported and the exit code stays 0, as before.
    let (stdout, ..) = run(t, t, &t.join("out"));
    assert!(stdout.contains(" errors 3 "), "{stdout}");
}

mod compiled {
    use qml::jsrt::{Aot, BodyKind, Cx, Entry, Env, FnKind, FnMeta};
    use qml::value::Value;
    use std::path::Path;
    use std::rc::Rc;

    const META: FnMeta = FnMeta {
        name: "",
        kind: FnKind::Normal,
        nparams: 0,
        nslots: 0,
        strict: false,
    };
    const FILE: &str = "modules/rusty/test/Test.qml";

    // `w` compiled wrong (always 11), `probe` compiled right.
    fn w(_: &mut Cx, _: &Env, _: &Value, _: &[Value]) -> qml::eval::R<Value> {
        Ok(Value::Num(11.0))
    }
    fn probe(_: &mut Cx, _: &Env, _: &Value, _: &[Value]) -> qml::eval::R<Value> {
        Ok(Value::str("undefined"))
    }
    static W: Entry = Entry {
        file: FILE,
        path: "0",
        member: "w",
        kind: BodyKind::Binding,
        f: w,
        meta: META,
        hash: 0,
    };
    static PROBE: Entry = Entry {
        file: FILE,
        path: "0",
        member: "probe",
        kind: BodyKind::Binding,
        f: probe,
        meta: META,
        hash: 0,
    };

    struct Fake;
    impl Aot for Fake {
        fn doc_scripts(
            &self,
            file: &Path,
            doc: &qml::Document,
        ) -> Vec<(qml::NodeId, &'static Entry)> {
            if !file.ends_with(FILE) {
                return vec![];
            }
            let mut out = vec![];
            for m in &doc.root.members {
                if let qml::Member::Property {
                    name,
                    value: Some(qml::Value::Script(s)),
                    ..
                } = m
                {
                    match name.as_str() {
                        "w" => out.push((s.id, &W)),
                        "probe" => out.push((s.id, &PROBE)),
                        _ => {}
                    }
                }
            }
            out
        }
        fn library(&self, _: &Path) -> Option<&'static Entry> {
            None
        }
        fn script(&self, _: &Path) -> Option<&'static Entry> {
            None
        }
    }

    fn factory(_: &Path) -> Rc<dyn Aot> {
        Rc::new(Fake)
    }

    fn boom(_: &mut Cx, _: &Env, _: &Value, _: &[Value]) -> qml::eval::R<Value> {
        panic!("ricompiled: {FILE}|0|w has no compiled body")
    }
    static BOOM: Entry = Entry {
        file: FILE,
        path: "0",
        member: "w",
        kind: BodyKind::Binding,
        f: boom,
        meta: META,
        hash: 0,
    };

    struct Panics;
    impl Aot for Panics {
        fn doc_scripts(
            &self,
            file: &Path,
            doc: &qml::Document,
        ) -> Vec<(qml::NodeId, &'static Entry)> {
            Fake.doc_scripts(file, doc)
                .into_iter()
                .map(|(s, e)| (s, if e.member == "w" { &BOOM } else { e }))
                .collect()
        }
        fn library(&self, _: &Path) -> Option<&'static Entry> {
            None
        }
        fn script(&self, _: &Path) -> Option<&'static Entry> {
            None
        }
    }

    #[test]
    fn panicking_compiled_body_is_reported_and_quarantined() {
        let t = super::tree(super::TEST);
        let t = t.path();
        let out = t.join("out");
        let args = [
            t.to_str().unwrap(),
            "--backend",
            "compiled",
            "--modules",
            "test/Test.qml",
            "--frames",
            "3",
            "--out",
            out.to_str().unwrap(),
        ];
        let code = aotcheck::run(
            args.iter().map(|s| s.to_string()).collect(),
            Some(|_| Rc::new(Panics)),
        );
        assert_eq!(code, 0);
        let div = std::fs::read_to_string(out.join("divergences.tsv")).unwrap();
        let report = std::fs::read_to_string(out.join("report.md")).unwrap();
        let first = div.lines().nth(1).expect("a divergence");
        assert!(first.starts_with("0\tpanic\t"), "{div}");
        assert!(first.contains(&format!("{FILE}|0|w")), "{div}");
        assert!(
            report.contains("quarantined keys: 1, converged: true"),
            "{report}"
        );
        // The fake compiles 2 of Test.qml's 7 scripts: the rest ran interpreted, not compared.
        assert!(
            report.contains("interpreted for lack of compiled code (not compared): 2"),
            "{report}"
        );
        assert!(report.contains(&format!("`{FILE}` 5/7")), "{report}");
    }

    #[test]
    fn wrong_compiled_binding_is_found_and_quarantined() {
        let t = super::tree(super::TEST);
        let t = t.path();
        let out = t.join("out");
        let args = [
            t.to_str().unwrap(),
            "--backend",
            "compiled",
            "--modules",
            "test/Test.qml",
            "--frames",
            "3",
            "--out",
            out.to_str().unwrap(),
        ];
        let code = aotcheck::run(args.iter().map(|s| s.to_string()).collect(), Some(factory));
        assert_eq!(code, 0);
        let rd = |f: &str| std::fs::read_to_string(out.join(f)).unwrap();
        let (div, report, comp) = (rd("divergences.tsv"), rd("report.md"), rd("compiled.tsv"));
        let first = div.lines().nth(1).expect("a divergence");
        assert!(first.starts_with("0\tload\t"), "{div}");
        assert!(
            first.contains(&format!("\tw\t{FILE}|0|w\t3\t11\t")),
            "{div}"
        );
        assert!(div.lines().skip(1).all(|l| l.starts_with("0\t")), "{div}");
        assert!(
            report.contains("quarantined keys: 1, converged: true"),
            "{report}"
        );
        assert!(comp.contains(&format!("{FILE}|0|w\ttrue")), "{comp}");
        assert!(comp.contains(&format!("{FILE}|0|probe\tfalse")), "{comp}");
        assert!(
            report.contains("### rusty/test (1 keys, 1 root)"),
            "{report}"
        );
        let q = rd("quarantine.tsv");
        assert!(
            q.lines().nth(1).unwrap().starts_with(&format!(
                "rusty/test\t{FILE}\t{FILE}|0|w\troot\t0\tload\tw\t3\t11"
            )),
            "{q}"
        );
    }
}
