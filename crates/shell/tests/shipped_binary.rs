//! What the built `impulse-shell` holds: no JavaScript parser, IR or tree-walking evaluator unless qml
//! was built with `interp`. release_without_interp.rs checks what cargo resolves; this reads
//! the linked binary (its symbol names and panic locations) and runs it.
//! `RI_COMMIT=<word> cargo test --release -p shell --no-default-features --test shipped_binary`
//! checks a binary built as tools/build.sh builds the shipped one (which does not compile
//! without a commit to name); tools/testenv/ri-gate runs it with the gate profile. Any other build of the shell links the interpreter (`--features shell/interp`, or
//! feature unification in a workspace build), and the same marks must then all be found, so
//! a renamed mark fails there instead of passing here.

const BIN: &str = env!("CARGO_BIN_EXE_impulse-shell");

/// Names only the interpreter puts into the binary: the oxc crates (parser, AST, arena), the
/// module that uses them (`qml::ir`, the JS IR lowered from the oxc AST, as a mangled path
/// component and as a source path) and the evaluator's walk over that IR
/// (crates/qml/src/eval.rs: statements, expressions, `eval(text)`).
const INTERPRETER: &[&str] = &[
    "oxc_parser",
    "oxc_ast",
    "oxc_allocator",
    "3qml2ir",
    "qml/src/ir.rs",
    "exec_cold",
    "eval_cold",
    "run_eval",
];

/// Other JavaScript engines, by crate, library or class name. No build has them, so nothing
/// shows these names would be found: they catch an engine added under its usual name.
const ENGINES: &[&str] = &[
    "boa_engine",
    "quickjs",
    "duktape",
    "mozjs",
    "rusty_v8",
    "libv8",
    "deno_core",
    "libnode",
    "javascriptcore",
    "JavaScriptCore",
    "Qt6Qml",
    "QJSEngine",
    "swc_ecma",
];

/// The `names` that occur in `hay`, in one pass.
fn occurring<'a>(hay: &[u8], names: &[&'a str]) -> Vec<&'a str> {
    let mut first = [false; 256];
    for n in names {
        first[n.as_bytes()[0] as usize] = true;
    }
    let mut hit = vec![false; names.len()];
    let mut i = 0;
    while i < hay.len() {
        if first[hay[i] as usize] {
            for (k, n) in names.iter().enumerate() {
                if !hit[k] && hay[i..].starts_with(n.as_bytes()) {
                    hit[k] = true;
                }
            }
        }
        i += 1;
    }
    names
        .iter()
        .zip(hit)
        .filter_map(|(n, h)| h.then_some(*n))
        .collect()
}

#[test]
fn interpreter_is_in_the_binary_only_when_qml_is_built_with_it() {
    let bin = std::fs::read(BIN).unwrap();
    let found = occurring(&bin, INTERPRETER);
    if qml::INTERP {
        assert_eq!(
            found, INTERPRETER,
            "{BIN} links the interpreter, and a mark of it is gone: update INTERPRETER"
        );
    } else {
        assert!(
            found.is_empty(),
            "{BIN} is built without `interp` and holds interpreter code: {found:?}"
        );
    }
}

#[test]
fn binary_has_no_other_javascript_engine() {
    let bin = std::fs::read(BIN).unwrap();
    let found = occurring(&bin, ENGINES);
    assert!(found.is_empty(), "{BIN} holds {found:?}");
}

#[test]
fn occurring_finds_overlapping_and_last_names() {
    let hay = b"..3qml2ir5lower..qml/src/ir.rs";
    assert_eq!(
        occurring(hay, &["3qml2ir", "qml2", "oxc_parser", "ir.rs"]),
        ["3qml2ir", "qml2", "ir.rs"]
    );
}

/// A script the binary was not compiled from is never run, also when the environment asks for
/// the interpreter: on a `shell.qml` whose handler would print `js-ran-42` the shell names the
/// file and exits.
#[cfg(not(feature = "interp"))]
#[test]
fn script_without_compiled_code_is_refused_not_run() {
    let dir = std::env::temp_dir().join(format!("ri-shipped-refused-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("shell.qml"),
        "import QtQuick\nItem { Component.onCompleted: console.log(\"js-\" + \"ran-\" + (40 + 2)) }\n",
    )
    .unwrap();
    let out = std::process::Command::new(BIN)
        .arg("-p")
        .arg(&dir)
        .env("RI_INTERP", "1")
        .env("XDG_RUNTIME_DIR", &dir)
        .env("WAYLAND_DISPLAY", "no-such-compositor")
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env_remove("WAYLAND_SOCKET")
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let said = String::from_utf8_lossy(&out.stdout) + String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{said}");
    assert!(
        said.contains("differs from the tree this binary was compiled from"),
        "the tree was not refused:\n{said}"
    );
    assert!(said.contains("shell.qml"), "{said}");
    assert!(!said.contains("js-ran-42"), "the script ran:\n{said}");
}
