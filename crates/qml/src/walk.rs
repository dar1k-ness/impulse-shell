//! Every compiled unit of a QML tree and its registry key.
//!
//! Key of a QML script: `(file, object path, member name)`.
//! - file: path relative to the original root (`modules/bar/Bar.qml`), or `qrc:/<rel>` for the QML
//!   module files built into the qml crate.
//! - object path: the root object is `0`; the objects directly owned by object `P` (children,
//!   `Behavior on x { }` objects, inline `component X: ...`, object/list values of properties and
//!   bindings, also inside groups) are `P.0`, `P.1`, ... in [`crate::Object::walk`] order.
//! - member name: binding/property name with its group prefix (`anchors.fill`, `font.pixelSize`),
//!   handler name (`onClicked`, `Component.onCompleted`, `Keys.onPressed`) or function name.
//!
//! Key of a `.js` library: its file path relative to the original root.
use crate::{Document, Member, Object, Script, Value};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BodyKind {
    /// Property initializer or binding (`x: a + b`, `property int n: 3`). Lowered as a function
    /// body whose completion value is returned.
    Binding,
    /// Signal handler (`onClicked: ...`, `ScriptAction { script: ... }`). The body may evaluate
    /// to a function (`onX: (a) => ...`), which the runtime then calls with the signal args.
    Handler,
    /// `function name(...) { }` member. Body is `[Stmt::Func(f)]`; running it defines `name`.
    Function,
    /// `.js` library program (`.pragma library`); its top-level names are the exports.
    Library,
}

impl BodyKind {
    pub fn as_str(self) -> &'static str {
        match self {
            BodyKind::Binding => "Binding",
            BodyKind::Handler => "Handler",
            BodyKind::Function => "Function",
            BodyKind::Library => "Library",
        }
    }
}

/// One script member of a QML document.
#[derive(Debug)]
pub struct Site<'a> {
    pub path: String,
    pub name: String,
    pub kind: BodyKind,
    pub script: &'a Script,
    /// Type name of the object the member is declared on.
    pub type_name: &'a str,
    /// Parameter names of the signal this handler handles, when the object it is on declares
    /// that signal itself (see [`own_signal_params`]).
    pub params: Vec<String>,
}

impl Site<'_> {
    /// What compiled code of this site depends on: its source and the parameter names of its
    /// own signal, which a compiled handler binds by position (see [`site_hash`]).
    pub fn hash(&self) -> u64 {
        site_hash(&self.script.src, &self.params)
    }
}

/// [`src_hash`] of a script, with the signal `params` its handler binds mixed in: a changed
/// signal declaration (renamed or reordered parameters) is a changed site.
pub fn site_hash(src: &str, params: &[String]) -> u64 {
    if params.is_empty() {
        return src_hash(src);
    }
    src_hash(&format!("{src}\0{}", params.join(",")))
}

/// QQmlSignalNames::isHandlerName: `on`, then a name whose first letter is in upper case;
/// what stands before that letter (`on_ZChanged`, `on$S`, `on_1Q`) is no letter.
pub fn is_handler_name(name: &str) -> bool {
    let last = name.rsplit('.').next().unwrap_or(name);
    let Some(signal) = last.strip_prefix("on").filter(|s| !s.is_empty()) else {
        return false;
    };
    (signal.chars().find(|c| c.is_alphabetic())).is_none_or(char::is_uppercase)
}

/// The handler name of `signal` (QQmlSignalNames::signalNameToHandlerName): `on` and the
/// name with its first letter in upper case (`triggered` -> `onTriggered`, `_sig` -> `on_Sig`).
pub fn handler_of(signal: &str) -> String {
    match signal.is_empty() {
        true => String::new(),
        false => format!("on{}", upper_first(signal)),
    }
}

/// How the script value of binding `name` (with its group prefix) runs: a handler for
/// `onX` names and for `script` of a `ScriptAction` or `StateChangeScript` (a
/// `QQmlScriptString` in Qt, also on types deriving from them), else a binding.
/// `script_action` tells whether the object the binding is on is one; it is only asked for
/// `script`. The code generator and
/// the runtime both classify sites with this.
pub fn binding_kind(name: &str, script_action: impl FnOnce() -> bool) -> BodyKind {
    if is_handler_name(name) || (name == "script" && script_action()) {
        BodyKind::Handler
    } else {
        BodyKind::Binding
    }
}

/// Whether type `name` as written in `doc` (at `file`, in the QML tree at `root`) is a
/// `ScriptAction`: the C++ type itself, or an inline or file component deriving from it,
/// resolved the way the runtime instantiates it (inline component first, then the visible
/// QML file types, then the C++ type).
pub fn is_script_action(root: &Path, file: &Path, doc: &Document, name: &str) -> bool {
    fn find_inline<'a>(o: &'a Object, name: &str) -> Option<&'a Object> {
        o.members.iter().find_map(|m| match m {
            Member::Component { name: n, object } if n == name => Some(object),
            Member::Child(c) | Member::Component { object: c, .. } => find_inline(c, name),
            _ => None,
        })
    }
    fn derives(root: &Path, file: &Path, doc: &Document, name: &str, depth: usize) -> bool {
        if depth > 64 {
            return false;
        }
        if let Some(o) = find_inline(&doc.root, name).filter(|o| o.type_name != name) {
            return derives(root, file, doc, &o.type_name, depth + 1);
        }
        let vis = crate::resolve::visible_types(root, file, doc);
        if let Some(t) = vis.get(name).filter(|t| *t.file != *file) {
            let base = std::fs::read_to_string(&t.file)
                .ok()
                .and_then(|s| crate::parse(&s).ok());
            return base.is_some_and(|d| derives(root, &t.file, &d, &d.root.type_name, depth + 1));
        }
        matches!(
            name.strip_prefix("QQuick").unwrap_or(name),
            "ScriptAction" | "StateChangeScript"
        )
    }
    derives(root, file, doc, name, 0)
}

/// `signal foo(int a, string b)` / `signal foo(a: int)` parameter names, as the runtime reads them.
pub fn signal_params(params: &str) -> Vec<String> {
    params
        .split(',')
        .filter_map(|p| {
            let p = p.trim();
            let n = p
                .split_once(':')
                .map(|x| x.0)
                .unwrap_or_else(|| p.rsplit(' ').next().unwrap_or(p));
            (!n.trim().is_empty()).then(|| n.trim().to_string())
        })
        .collect()
}

/// Calls `f(site)` for every script member of `doc`, in source order. Only an object written
/// as `ScriptAction` counts as one (see [`sites_typed`]).
pub fn sites<'a>(doc: &'a Document, f: &mut dyn FnMut(Site<'a>)) {
    sites_typed(doc, &|t| t == "ScriptAction" || t == "StateChangeScript", f)
}

/// [`sites`], with `script_action(type_name)` telling which object types are `ScriptAction`s
/// (see [`is_script_action`]): their `script` is a handler.
pub fn sites_typed<'a>(
    doc: &'a Document,
    script_action: &dyn Fn(&str) -> bool,
    f: &mut dyn FnMut(Site<'a>),
) {
    object(&doc.root, "0".into(), script_action, f);
}

/// Parameter names of the signal `onX` handler `handler` handles when `o` declares that
/// signal itself. A same-named signal declared on another object of the document is another
/// signal (`MouseArea { onClicked: ... }` inside an item with `signal clicked(int index)`
/// handles `MouseArea.clicked(mouse)`): those parameters are left to the runtime, which binds
/// the emitted signal's own.
fn own_signal_params(o: &Object, handler: &str) -> Vec<String> {
    o.members
        .iter()
        .find_map(|m| match m {
            Member::Signal { name, params } if handler_of(name) == handler => {
                Some(signal_params(params))
            }
            _ => None,
        })
        .unwrap_or_default()
}

/// `name` with its first letter in upper case, as `QChar::toUpper` maps it: one character
/// to one, so a letter with no such form (`ß`, `名`) stays as it is. What stands before the
/// letter stays too (`_z` gives `_Z`: QQmlSignalNames::signalNameToHandlerName).
pub(crate) fn upper_first(name: &str) -> String {
    let Some(at) = name.find(char::is_alphabetic) else {
        return name.to_owned();
    };
    let mut rest = name[at..].chars();
    let first = rest.next().map(|c| {
        let mut upper = c.to_uppercase();
        match (upper.next(), upper.next()) {
            (Some(u), None) => u,
            _ => c,
        }
    });
    (name[..at].chars()).chain(first).chain(rest).collect()
}

fn object<'a>(
    o: &'a Object,
    path: String,
    script_action: &dyn Fn(&str) -> bool,
    f: &mut dyn FnMut(Site<'a>),
) {
    let mut next = 0usize;
    members(o, &o.members, &path, "", &mut next, script_action, f);
}

fn members<'a>(
    o: &'a Object,
    ms: &'a [Member],
    path: &str,
    prefix: &str,
    next: &mut usize,
    script_action: &dyn Fn(&str) -> bool,
    f: &mut dyn FnMut(Site<'a>),
) {
    let child = |c: &'a Object, next: &mut usize, f: &mut dyn FnMut(Site<'a>)| {
        let p = format!("{path}.{next}");
        *next += 1;
        object(c, p, script_action, f);
    };
    for m in ms {
        let site = |name: String, kind, script| Site {
            params: match kind {
                BodyKind::Handler => own_signal_params(o, &name),
                _ => Vec::new(),
            },
            path: path.to_string(),
            name,
            kind,
            script,
            type_name: &o.type_name,
        };
        match m {
            Member::Child(c)
            | Member::On { object: c, .. }
            | Member::Component { object: c, .. } => child(c, next, f),
            Member::Group { name, members: gm } => members(
                o,
                gm,
                path,
                &format!("{prefix}{name}."),
                next,
                script_action,
                f,
            ),
            Member::Function { name, script } => {
                f(site(format!("{prefix}{name}"), BodyKind::Function, script))
            }
            Member::Property {
                ty,
                value: Some(v),
                name,
                ..
            } => match v {
                // An alias target is a property path, not JS.
                Value::Script(_) if ty == "alias" => {}
                Value::Script(s) => f(site(format!("{prefix}{name}"), BodyKind::Binding, s)),
                Value::Object(c) => child(c, next, f),
                Value::List(cs) => cs.iter().for_each(|c| child(c, next, f)),
            },
            Member::Binding { name, value } => match value {
                Value::Script(s) => {
                    let full = format!("{prefix}{name}");
                    let kind = binding_kind(&full, || script_action(&o.type_name));
                    f(site(full, kind, s))
                }
                Value::Object(c) => child(c, next, f),
                Value::List(cs) => cs.iter().for_each(|c| child(c, next, f)),
            },
            _ => {}
        }
    }
}

/// Calls `f(objpath, object, names)` for every object declaration of `doc` (objpaths as in
/// [`sites`]), with the property names it declares or binds in declaration order: its layout.
pub fn decls<'a>(doc: &'a Document, f: &mut dyn FnMut(&str, &'a Object, Vec<String>)) {
    decl(&doc.root, "0".into(), f);
}

fn decl<'a>(o: &'a Object, path: String, f: &mut dyn FnMut(&str, &'a Object, Vec<String>)) {
    let mut names = Vec::new();
    let mut kids = Vec::new();
    decl_members(&o.members, "", &mut names, &mut kids);
    f(&path, o, names);
    for (n, c) in kids.into_iter().enumerate() {
        decl(c, format!("{path}.{n}"), f);
    }
}

fn decl_members<'a>(
    ms: &'a [Member],
    prefix: &str,
    names: &mut Vec<String>,
    kids: &mut Vec<&'a Object>,
) {
    fn add(names: &mut Vec<String>, n: String) {
        if !names.contains(&n) {
            names.push(n);
        }
    }
    for m in ms {
        match m {
            Member::Child(c)
            | Member::On { object: c, .. }
            | Member::Component { object: c, .. } => kids.push(c),
            Member::Group {
                name: g,
                members: gm,
            } => decl_members(gm, &format!("{prefix}{g}."), names, kids),
            Member::Property { name: n, value, .. } => {
                add(names, format!("{prefix}{n}"));
                match value {
                    Some(Value::Object(c)) => kids.push(c),
                    Some(Value::List(cs)) => kids.extend(cs.iter()),
                    _ => {}
                }
            }
            Member::Binding { name: n, value } => {
                if !is_handler_name(n) {
                    add(names, format!("{prefix}{n}"));
                }
                match value {
                    Value::Object(c) => kids.push(c),
                    Value::List(cs) => kids.extend(cs.iter()),
                    _ => {}
                }
            }
            Member::Signal { name: n, .. } | Member::Enum { name: n, .. } => add(names, n.clone()),
            _ => {}
        }
    }
}

/// Source files of a QML tree: `(key file, absolute path)`, sorted by key.
#[derive(Debug, Default)]
pub struct Tree {
    pub qml: Vec<(String, PathBuf)>,
    pub js: Vec<(String, PathBuf)>,
}

pub fn tree(root: &Path) -> Tree {
    fn walk(d: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n == ".git") {
                    continue;
                }
                walk(&p, out);
            } else {
                out.push(p);
            }
        }
    }
    let mut files = Vec::new();
    walk(root, &mut files);
    let mut t = Tree::default();
    for p in files {
        let rel = p
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        match p.extension().and_then(|x| x.to_str()) {
            Some("qml") => t.qml.push((rel, p)),
            Some("js") => t.js.push((rel, p)),
            _ => {}
        }
    }
    t.qml.sort();
    t.js.sort();
    t
}

/// The QML module files the qml crate embeds (`qrc:/Quickshell/Io/FileView.qml`).
pub fn module_tree() -> Vec<(String, PathBuf)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../qml/qmltypes");
    let mut t = tree(&dir).qml;
    for (k, _) in &mut t {
        *k = format!("{}{k}", crate::types::MODULE_PREFIX);
    }
    t
}

/// FNV-1a 64 of a script source: a compiled entry only applies to the exact source it was
/// generated from. It guards against accidental drift between the tree and the build (an
/// edited file), matched together with the entry's `(file, path, member)`, not against
/// deliberate collisions: the user owns both the tree and the binary, so there is no adversary
/// a cryptographic hash would stop, and a `const fn` hash keeps the tables static.
pub const fn src_hash(s: &str) -> u64 {
    let b = s.as_bytes();
    let mut h = 0xcbf29ce484222325u64;
    let mut i = 0;
    while i < b.len() {
        h ^= b[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Qt 6.12 answers `onСигнал`, `onǄaChanged` and `on𝒳Changed`, and has no handler for
    /// a property named `ßa` or `名`.
    #[test]
    fn a_handler_name_upper_cases_one_character() {
        let cases = [
            ("clicked", "onClicked"),
            ("сигнал", "onСигнал"),
            ("ǆa", "onǄa"),
            ("𝒳", "on𝒳"),
            ("ßa", "onßa"),
            ("名", "on名"),
            ("_z", "on_Z"),
            ("__y", "on__Y"),
            ("", ""),
            ("_zChanged", "on_ZChanged"),
            ("__s", "on__S"),
            ("$s", "on$S"),
            ("_1qChanged", "on_1QChanged"),
            ("__", "on__"),
        ];
        for (signal, handler) in cases {
            assert_eq!(handler_of(signal), handler);
        }
        for (name, is) in [
            ("onСигнал", true),
            ("onßa", false),
            ("on名", false),
            ("onǅa", false),
            ("on_ZChanged", true),
            ("on$S", true),
            ("on_1QChanged", true),
            ("on__", true),
            ("on_zChanged", false),
            ("on", false),
        ] {
            assert_eq!(is_handler_name(name), is, "{name}");
        }
    }

    fn kinds(doc: &Document, script_action: &dyn Fn(&str) -> bool) -> Vec<(String, BodyKind)> {
        let mut v = Vec::new();
        sites_typed(doc, script_action, &mut |s| {
            v.push((format!("{}|{}", s.path, s.name), s.kind))
        });
        v
    }

    #[test]
    fn handlers_take_only_their_own_object_signal_parameters() {
        let src = "import QtQuick\nItem {\n    signal clicked(int index)\n    signal picked(string name, var data)\n    onClicked: index\n    onPicked: name\n    MouseArea { onClicked: index }\n    Item { signal picked(int other)\n        onPicked: other }\n}\n";
        let doc = crate::parse(src).unwrap();
        let mut got = Vec::new();
        sites(&doc, &mut |s| {
            got.push((format!("{}|{}", s.path, s.name), s.params))
        });
        let params = |k: &str| &got.iter().find(|(n, _)| n == k).unwrap().1;
        assert_eq!(params("0|onClicked"), &["index"], "{got:?}");
        assert_eq!(params("0|onPicked"), &["name", "data"], "{got:?}");
        assert!(params("0.0|onClicked").is_empty(), "{got:?}");
        assert_eq!(params("0.1|onPicked"), &["other"], "{got:?}");
    }

    #[test]
    fn a_changed_signal_declaration_changes_its_handler_site() {
        let hashes = |sig: &str| {
            let src = format!(
                "import QtQuick\nItem {{\n    signal picked({sig})\n    onPicked: name + data\n    width: 1\n}}\n"
            );
            let doc = crate::parse(&src).unwrap();
            let mut v = Vec::new();
            sites(&doc, &mut |s| v.push((s.name.clone(), s.hash())));
            v
        };
        let (a, b) = (
            hashes("string name, var data"),
            hashes("var data, string name"),
        );
        assert_eq!(a[0].0, "onPicked");
        assert_ne!(a[0].1, b[0].1);
        assert_ne!(a[0].1, hashes("string name, var other")[0].1);
        assert_eq!(a[0].1, hashes("string name, var data")[0].1);
        // A site without signal parameters keeps the hash of its source.
        assert_eq!(a[1], ("width".to_string(), src_hash("1")));
    }

    #[test]
    fn binding_kind_asks_the_type_only_for_script() {
        assert_eq!(
            binding_kind("onClicked", || unreachable!()),
            BodyKind::Handler
        );
        assert_eq!(
            binding_kind("Keys.onPressed", || unreachable!()),
            BodyKind::Handler
        );
        assert_eq!(binding_kind("width", || unreachable!()), BodyKind::Binding);
        assert_eq!(binding_kind("script", || true), BodyKind::Handler);
        assert_eq!(binding_kind("script", || false), BodyKind::Binding);
    }

    #[test]
    fn script_of_a_derived_script_action_is_a_handler() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("MyAction.qml"),
            "import QtQuick\nScriptAction {}\n",
        )
        .unwrap();
        std::fs::write(root.join("Deeper.qml"), "import QtQuick\nMyAction {}\n").unwrap();
        std::fs::write(
            root.join("Other.qml"),
            "import QtQuick\nItem { property var script }\n",
        )
        .unwrap();
        let file = root.join("Main.qml");
        let src = "import QtQuick\nItem {\n    component Inline: ScriptAction {}\n    MyAction { script: a() }\n    Deeper { script: b() }\n    Inline { script: c() }\n    Other { script: d() }\n    ScriptAction { script: e() }\n}\n";
        std::fs::write(&file, src).unwrap();
        let doc = crate::parse(src).unwrap();
        let sa = |t: &str| is_script_action(root, &file, &doc, t);
        let got = kinds(&doc, &sa);
        let script = |p: &str| {
            got.iter()
                .find(|(k, _)| k == &format!("{p}|script"))
                .unwrap()
                .1
        };
        assert_eq!(script("0.1"), BodyKind::Handler, "{got:?}");
        assert_eq!(script("0.2"), BodyKind::Handler, "{got:?}");
        assert_eq!(script("0.3"), BodyKind::Handler, "{got:?}");
        assert_eq!(script("0.4"), BodyKind::Binding, "{got:?}");
        assert_eq!(script("0.5"), BodyKind::Handler, "{got:?}");
        // Without type information only the C++ name is known.
        let untyped = kinds(&doc, &|t| t == "ScriptAction");
        assert_eq!(
            untyped
                .iter()
                .filter(|(_, k)| *k == BodyKind::Handler)
                .count(),
            1
        );
    }
}
