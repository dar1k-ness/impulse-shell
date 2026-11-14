use qml::runtime::Runtime;
use qml::value::Value;
use std::time::Instant;

/// Free names resolved through `depth` nested component contexts: every lookup misses
/// `depth - 1` context roots before it reaches `top`.
fn run(depth: usize) -> std::time::Duration {
    let mut rows = String::new();
    for i in 0..200 {
        rows.push_str(&format!(
            "Item {{ property real v{i}: foo + bar * {i} + baz + qux }}\n"
        ));
    }
    let mut inner = format!("Item {{\n{rows}\n}}");
    for d in (0..depth).rev() {
        inner = format!(
            "Item {{ property var m{d}: null\n Component {{ id: c{d}\n {inner} }}\n Component.onCompleted: m{d} = c{d}.createObject(this) }}"
        );
    }
    let src = format!(
        "import QtQuick\nItem {{\n id: top\n property real foo: 1\n property real bar: 2\n property real baz: 3\n property real qux: 4\n {inner}\n}}"
    );
    let mut rt = Runtime::new(None);
    let root = rt.load_str(&src).unwrap();
    rt.settle();
    let t = Instant::now();
    for n in 0..500 {
        rt.set(root, "foo", Value::Num(n as f64));
        rt.settle();
    }
    t.elapsed()
}

/// A measurement, printed and not asserted:
/// `cargo test --release -p qml --features live --test scope_lookup_bench -- --nocapture`.
#[test]
#[cfg_attr(
    not(feature = "live"),
    ignore = "live: --features live, docs/benchmarks.md"
)]
fn free_identifier_lookup_bench() {
    for depth in [1, 4, 8] {
        let best = (0..5).map(|_| run(depth)).min().unwrap();
        eprintln!(
            "BENCH depth {depth}: 500 settles x 200 bindings x 4 free names: best of 5 {best:?}"
        );
    }
}
