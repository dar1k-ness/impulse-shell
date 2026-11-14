//! Emits a Quickshell QML file that prints every enum value of every exported type as
//! `module\ttype\tvalue\tnumber`; run it with `qs -p` on the reference system to refresh
//! `enums.tsv`.
fn main() {
    let r = qml::types::Registry::load();
    let mut modules: Vec<String> = Vec::new();
    let mut exprs = Vec::new();
    let mut names: Vec<_> = r.by_name.iter().collect();
    names.sort_by(|a, b| a.0.cmp(b.0));
    for (name, cands) in names {
        for (module, cpp) in cands {
            let mut vals = Vec::new();
            for t in r.chain(cpp) {
                for e in &t.enums {
                    vals.extend(e.values.iter().cloned());
                }
            }
            if vals.is_empty() || !name.chars().next().is_some_and(char::is_uppercase) {
                continue;
            }
            let idx = match modules.iter().position(|m| m == module) {
                Some(i) => i,
                None => {
                    modules.push(module.clone());
                    modules.len() - 1
                }
            };
            for v in vals {
                exprs.push(format!(
                    "        g({module:?}, {name:?}, {v:?}, () => M{idx}.{name}.{v})"
                ));
            }
        }
    }
    println!("import QtQuick");
    for (i, m) in modules.iter().enumerate() {
        println!("import {m} as M{i}");
    }
    println!(
        "Item {{\n    function g(m, t, v, f) {{ try {{ const n = f(); if (typeof n === \"number\") console.log(\"ENUM\\t\" + m + \"\\t\" + t + \"\\t\" + v + \"\\t\" + n) }} catch (e) {{}} }}\n    Component.onCompleted: {{"
    );
    for e in exprs {
        println!("{e}");
    }
    println!("        console.log(\"ENUMDONE\");\n        Qt.quit()\n    }}\n}}");
}
