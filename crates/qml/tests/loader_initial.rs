//! `Loader.setSource(url, properties)` writes each initial property as a script does
//! (`object->put` in QQmlComponentPrivate::setInitialProperties): a value the property takes
//! from a script is read back the same, and one it refuses leaves the item created, the
//! property as it was, and the script's error printed as a warning. `expected.qt` is what
//! Qt 6.12 says for every property of `Typed.qml` and every value of `Init.qml` (`qt.py`):
//! what a script's write gives, what `setSource` gives, what is warned. `waits.qt` lists the
//! cases where a script's write itself is not Qt's yet: `setSource` follows the script there
//! too, and a listed case that became Qt's fails the test.
use qml::runtime::Runtime;
use qml::value::Value;
use std::path::Path;

#[test]
fn set_source_writes_its_properties_as_a_script_does() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/loader_initial");
    let mut rt = Runtime::new(Some(dir.clone()));
    let root = rt.load_file(&dir.join("Init.qml")).unwrap();
    let call = |rt: &mut Runtime, f: &str, a: Vec<Value>| {
        let said = rt.call_method(root, f, a);
        let said = said.unwrap_or_else(|| panic!("no {f}"));
        said.unwrap_or_else(|e| panic!("{f}: {e} {:?}", rt.errors()))
    };
    let n = call(&mut rt, "count", vec![]).to_number() as usize;
    let want = std::fs::read_to_string(dir.join("expected.qt")).unwrap();
    let want: Vec<&str> = want.lines().collect();
    assert_eq!(n, want.len());
    let waits = std::fs::read_to_string(dir.join("waits.qt")).unwrap();
    let waits: std::collections::HashSet<&str> =
        waits.lines().filter(|l| !l.starts_with('#')).collect();
    let here = format!("file://{}", dir.display());
    let (mut bad, mut waiting, mut unlike) = (0, 0, 0);
    for (at, w) in want.iter().enumerate() {
        let before = rt.errors().len();
        let row = call(&mut rt, "go", vec![Value::Num(at as f64)]);
        let warned: Vec<String> = rt.errors()[before..]
            .iter()
            .map(|e| {
                e.split_once(".qml: ")
                    .map_or(e.as_str(), |x| x.1)
                    .to_string()
            })
            .collect();
        let ours = format!("{} | warns {}", row.to_js_string(), warned.join("; "));
        // The fixture's directory is the shell's here, so that `Typed` is a type.
        let ours = ours.replace(&here, "DIR").replace("qs:@/qs", "DIR");
        let ours = ours.trim_end().to_string();
        // The rule itself, whatever a script's write does with the value here.
        let part = |i: usize| ours.split(" | ").nth(i).unwrap_or("").to_string();
        let (script, set, warns) = (part(1), part(2), part(3));
        let as_script = match script.strip_prefix("script throws ") {
            Some(e) => warns == format!("warns {e}") && set.starts_with("setSource 1 "),
            None => set == script.replacen("script ", "setSource 1 ", 1) && warns == "warns",
        };
        if !as_script {
            unlike += 1;
            eprintln!("NOT AS A SCRIPT: {ours}");
        }
        let case = w.split(" | ").next().unwrap();
        match (ours == *w, waits.contains(case)) {
            (true, false) => {}
            (true, true) => {
                bad += 1;
                eprintln!("MARKED, and no longer differs\n  Qt:   {w}");
            }
            (false, marked) => {
                *(if marked { &mut waiting } else { &mut bad }) += 1;
                let mark = if marked { "WAITS" } else { "DIFF" };
                eprintln!("{mark}\n  Qt:   {w}\n  ours: {ours}");
            }
        }
    }
    eprintln!(
        "{waiting} of {} wait for a script's write to be Qt's",
        want.len()
    );
    assert_eq!(unlike, 0, "setSource and a script's write disagree");
    assert_eq!(
        (bad, waiting),
        (0, waits.len()),
        "see DIFF and MARKED above"
    );
}
