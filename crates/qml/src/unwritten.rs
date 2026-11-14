//! Methods Qt declares for a type (`.qmltypes`) that the engine has not written.
use crate::eval::{Host, HostError};
use crate::runtime::{Runtime, World, WorldHost};

/// `close()`, `show()`, `hide()` and `showNormal()` of a window of its own: the shell runs
/// them from the effects queue (`App::run_window_methods`).
pub fn window_method(w: &World, item: usize, name: &str) -> bool {
    let has = |ty: &str| w.items[item].types.iter().any(|t| t == ty);
    matches!(name, "close" | "show" | "hide" | "showNormal")
        && (has("FloatingWindow")
            || (has("ApplicationWindow") || has("Window")) && !has("ProxyWindowBase"))
}

/// What a call of `name` on `item` throws when Qt declares the method and the engine has not
/// written it; `None` when Qt declares no such method for the item.
pub fn error(w: &World, item: usize, name: &str) -> Option<HostError> {
    let inst = w.items.get(item)?;
    if inst.props.contains_key(name) {
        return None;
    }
    let types = crate::runtime::declared::types_of(w, item);
    let class = crate::types::declarer(&types, name)?;
    Some(HostError::new("Error", message(class, name)))
}

/// The text of the error a call of an unwritten method throws.
pub fn message(class: &str, name: &str) -> String {
    format!("{class}.{name}() is declared by Qt and not written in this engine")
}

/// Gives `o`, the script object the engine has for the QML type `name` (a singleton or a
/// global that is no QML item), a function for every method Qt declares for the type that
/// `o` has no member for: `typeof` says `function` as in Qt and a call throws [`message`].
pub fn fill(o: &crate::value::Obj, name: &str) {
    let r = crate::types::registry();
    let Some((_, ti)) = r.resolve(name) else {
        return;
    };
    for c in r.chain(&ti.cpp) {
        let class = c
            .exports
            .first()
            .map_or(c.cpp.as_str(), |(_, n)| n.as_str());
        for m in &c.methods {
            // `toString` every script object has from %Object.prototype%.
            if m != "toString" && !crate::eval::has_own(o, m) {
                let f = crate::eval::Callable::Unwritten(class, m.as_str());
                crate::eval::set_hidden(o, m, crate::eval::fn_value(f));
            }
        }
    }
}

/// How a method Qt declares answers a script in the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// The engine has it.
    Written,
    /// A QML object's method no branch of the engine answers: calling it throws [`error`].
    Unwritten,
    /// The object has no such member: calling it throws Qt's "is not a function".
    Absent,
}

/// What a script that holds an object of the QML type `name` gets for each method Qt
/// declares for it. The object is the one a document creates when the type can be created,
/// what its name reads as in a document for a singleton, a global or an attaching type, and
/// else a bare object of the type, as the engine makes them for another object's property.
pub fn probe(
    module: &str,
    name: &str,
    ti: &crate::types::TypeInfo,
) -> Vec<(&'static str, &'static str, Kind)> {
    use crate::value::Value;
    let r = crate::types::registry();
    let mut rt = Runtime::new(None);
    const GET: &str = "function get(m) { return o[m] }";
    // A type a document cannot create and that is no singleton reads as its name's type
    // object there, which has none of the methods: the engine's own object of the type is
    // probed.
    let root = if ti.creatable {
        rt.load_str(&format!(
            "import QtQuick\nimport {module}\nQtObject {{ property var o: {name} {{}}\n{GET} }}"
        ))
        .ok()
    } else if ti.singleton {
        rt.load_str(&format!(
            "import QtQuick\nimport {module}\nQtObject {{ property var o: {name}\n{GET} }}"
        ))
        .ok()
    } else {
        None
    };
    let root = match root {
        Some(root) if !matches!(rt.get(root, "o"), Value::Undefined | Value::Null) => root,
        _ => {
            let root = rt
                .load_str(&format!(
                    "import QtQuick\nQtObject {{ property var o\n{GET} }}"
                ))
                .unwrap();
            let o = crate::types::native_singleton(&mut rt.world.borrow_mut(), name);
            rt.set(root, "o", Value::Item(o));
            root
        }
    };
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for c in r.chain(&ti.cpp) {
        for m in &c.methods {
            if !seen.insert(m.as_str()) {
                continue;
            }
            let f = match rt.call_method(root, "get", vec![Value::str(m)]) {
                Some(Ok(f)) => f,
                _ => Value::Undefined,
            };
            let kind = match crate::eval::callable(&f) {
                Some(crate::eval::Callable::Item(i, n)) => {
                    match WorldHost(rt.world.clone()).native(i, n, None) {
                        Some(Err(_)) => Kind::Unwritten,
                        _ => Kind::Written,
                    }
                }
                Some(crate::eval::Callable::Unwritten(..)) => Kind::Unwritten,
                Some(_) => Kind::Written,
                None => Kind::Absent,
            };
            out.push((c.cpp.as_str(), m.as_str(), kind));
        }
    }
    out
}

/// Every method Qt declares that the engine has not written: (QML type, declaring class,
/// method, how it fails), over every QML type name of the registry.
pub fn list() -> Vec<(&'static str, &'static str, &'static str, Kind)> {
    let r = crate::types::registry();
    let mut names: Vec<&String> = r.by_name.keys().collect();
    names.sort();
    let mut out = Vec::new();
    for n in names {
        let Some((cpp, ti)) = r.resolve(n) else {
            continue;
        };
        let module = r.by_name[n]
            .iter()
            .find(|(_, c)| c == cpp)
            .map_or("QtQuick", |(m, _)| m.as_str());
        for (c, m, kind) in probe(module, n, ti) {
            if kind != Kind::Written {
                out.push((n.as_str(), c, m, kind));
            }
        }
    }
    out
}

/// The calls of these methods in a tree of QML files. It reads the scripts through the
/// `interp` IR, which a build of the shell alone does not have.
#[cfg(feature = "interp")]
mod scan;
#[cfg(feature = "interp")]
pub use scan::*;
