//! A value of another type written to a property, by a script (QV4::QObjectWrapper::setProperty)
//! and as the result of a binding (QQmlBinding::doStore, QQmlPropertyPrivate::write): the
//! property takes the value converted to its type, or keeps its value while the assignment
//! throws `Cannot assign <type> to <type>` and the binding logs `Unable to assign <type> to
//! <type>`.
//!
//! `tests/wrong_type_writes/qt.tsv` has what Qt 6.12 gives for each property of `props.tsv`
//! (a label, the path written, a value of its type, the object a script writes and the one
//! with the binding) and each value of `values.tsv` (a label and the expression): PySide6
//! 6.12.0, QQmlComponent, TZ=UTC, written by `tests/wrong_type_writes/qt.py` from `probe.qml`.
//! Of what Qt logs only a binding's `Unable to assign ...` is compared.
//!
//! `tests/wrong_type_writes/leftover.txt` names the lines the engine still gives another
//! answer for; `WRONG_TYPE_WRITES_WRITE=1` writes it again from what the engine gives now.
#![cfg(feature = "interp")]

use qml::runtime::Runtime;
use qml::value::Value;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

const PROBE: &str = include_str!("wrong_type_writes/probe.qml");
const PROPS: &str = include_str!("wrong_type_writes/props.tsv");
const VALUES: &str = include_str!("wrong_type_writes/values.tsv");
const QT: &str = include_str!("wrong_type_writes/qt.tsv");

fn leftover() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/wrong_type_writes/leftover.txt")
}

/// Keeps the `Unable to assign ...` of each warning a binding logs.
#[derive(Clone, Default)]
struct Warnings(Arc<Mutex<Vec<String>>>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Warnings {
    fn enabled(
        &self,
        meta: &tracing::Metadata<'_>,
        _: tracing_subscriber::layer::Context<'_, S>,
    ) -> bool {
        meta.target() == "ri::binding"
    }
    fn on_event(&self, e: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        struct Message(String);
        impl tracing::field::Visit for Message {
            fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
                if f.name() == "message" {
                    self.0 = format!("{v:?}");
                }
            }
        }
        let mut m = Message(String::new());
        e.record(&mut m);
        if let Some(at) = m.0.find("Unable to assign") {
            self.0.lock().unwrap().push(m.0[at..].to_string());
        }
    }
}

/// As qt.py writes an answer: an address is `PTR`, a character outside printable ASCII
/// `\u{hex}`.
fn text(r: &str) -> String {
    let mut out = String::new();
    let mut rest = r;
    while let Some(at) = rest.find("(0x") {
        let hex = rest[at + 3..]
            .find(|c: char| !c.is_ascii_hexdigit() || c.is_ascii_uppercase())
            .unwrap_or(rest.len() - at - 3);
        if hex > 0 && rest[at + 3 + hex..].starts_with(')') {
            out += &rest[..at];
            out += "(PTR)";
            rest = &rest[at + 4 + hex..];
        } else {
            out += &rest[..at + 3];
            rest = &rest[at + 3..];
        }
    }
    out += rest;
    out.chars()
        .map(|c| match c {
            ' '..='~' => c.to_string(),
            c => format!("\\u{{{:x}}}", c as u32),
        })
        .collect()
}

#[test]
fn a_value_of_another_type_written_to_a_property_does_what_qt_does() {
    use tracing_subscriber::layer::SubscriberExt;
    // Qt's answers were written with TZ=UTC: the test runs in a process that has it.
    const TEST: &str = concat!(
        module_path!(),
        "::a_value_of_another_type_written_to_a_property_does_what_qt_does"
    );
    if !testenv::in_own_process(TEST) {
        testenv::run_in_own_process(TEST, [("TZ", "UTC")]);
        return;
    }
    let warnings = Warnings::default();
    let _guard =
        tracing::subscriber::set_default(tracing_subscriber::registry().with(warnings.clone()));
    let values: Vec<(&str, &str)> = (VALUES.lines())
        .map(|l| l.split_once('\t').unwrap())
        .collect();
    let exprs: Vec<String> = (values.iter())
        .map(|(_, e)| format!("        () => {e}"))
        .collect();
    let mut ours = Vec::new();
    for l in PROPS.lines() {
        let [label, path, good, s, b] = l.split('\t').collect::<Vec<_>>()[..] else {
            panic!("{l}")
        };
        let src = PROBE
            .replace("/*S*/", s)
            .replace("/*B*/", b)
            .replace("/*PATH*/", path)
            .replace("/*GOOD*/", good)
            .replace("/*VALUES*/", &exprs.join(",\n"));
        let mut rt = Runtime::new(None);
        let root = rt.load_str(&src).unwrap_or_else(|e| panic!("{label}: {e}"));
        rt.settle();
        for (i, (value, _)) in values.iter().enumerate() {
            for (mode, f) in [("S", "script"), ("B", "bind")] {
                warnings.0.lock().unwrap().clear();
                let r = match rt.call_method(root, f, vec![Value::Num(i as f64)]) {
                    Some(Ok(v)) => text(&v.to_js_string()),
                    r => panic!("{label} {value}: {r:?}"),
                };
                let warn = match mode {
                    "B" => warnings.0.lock().unwrap().join(" ; "),
                    _ => String::new(),
                };
                ours.push(format!("{mode}\t{label}\t{value}\t{r}\t{warn}"));
            }
        }
    }
    let qt: Vec<&str> = QT.lines().collect();
    assert_eq!(qt.len(), ours.len());
    let key = |l: &str| l.rsplitn(3, '\t').last().unwrap().to_string();
    let diff: Vec<(&str, &String)> = (qt.iter().zip(&ours))
        .filter(|(qt, ours)| qt != ours)
        .map(|(qt, ours)| (*qt, ours))
        .collect();
    if std::env::var_os("WRONG_TYPE_WRITES_WRITE").is_some() {
        let lines: Vec<String> = (diff.iter()).map(|(_, ours)| format!("{ours}\n")).collect();
        std::fs::write(leftover(), lines.concat()).unwrap();
        return;
    }
    let text = std::fs::read_to_string(leftover()).unwrap_or_default();
    let known: HashSet<String> = text.lines().map(key).collect();
    let wrong: Vec<String> = (diff.iter())
        .filter(|(qt, _)| !known.contains(&key(qt)))
        .map(|(qt, ours)| format!("qt:   {qt}\nours: {ours}"))
        .collect();
    assert!(
        wrong.is_empty(),
        "{} of {} differ\n{}",
        wrong.len(),
        qt.len(),
        wrong.join("\n")
    );
    // A line the file names is still the answer the engine gives.
    let ours: Vec<&str> = diff.iter().map(|(_, ours)| ours.as_str()).collect();
    assert_eq!(
        text.lines().collect::<Vec<_>>(),
        ours,
        "leftover.txt is out of date"
    );
}
