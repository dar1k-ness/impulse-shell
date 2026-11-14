//! Where a tree of QML files calls a method Qt declares and the engine has not written.
use super::*;

/// A call in a document of a method Qt declares and the engine has not written.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Site {
    /// `file:line`, the file relative to the tree's root.
    pub at: String,
    /// The class that declares the method, and the method.
    pub class: &'static str,
    pub method: &'static str,
}

/// What [`calls`] found in a tree.
#[derive(Debug, Default)]
pub struct Scan {
    /// Calls of unwritten methods.
    pub sites: Vec<Site>,
    /// The `.qml` and `.js` files read.
    pub files: usize,
    /// Calls of methods the engine has written, of members QML declares, of what is neither
    /// (JavaScript functions and globals), and calls on a value only a run knows.
    pub written: usize,
    pub qml: usize,
    pub other: usize,
    pub unknown: usize,
    /// Of the last, those whose name some type's unwritten method has, by name.
    pub by_name: std::collections::BTreeMap<&'static str, usize>,
    /// Those calls: `file:line` and the name.
    pub unknown_sites: Vec<(String, &'static str)>,
    /// The QML types of Qt (`Rectangle`) the tree's documents make objects of.
    pub made: std::collections::BTreeSet<String>,
    /// The modules the tree's documents import.
    pub imports: std::collections::BTreeSet<String>,
    /// Every word of the tree's files: no file reads a property whose name is not one.
    pub words: std::collections::BTreeSet<String>,
    /// The words some file has with no `.` after them: a singleton whose name is not one is
    /// only ever the receiver of a call that names it.
    pub bare: std::collections::BTreeSet<String>,
}

/// The methods of a QML type: the declaring class, the name and how a call answers.
type Kinds = std::rc::Rc<Vec<(&'static str, &'static str, Kind)>>;

/// The kinds of the methods of the QML type `name`, probed once per type.
fn kinds(name: &str) -> Kinds {
    thread_local! {
        static SEEN: std::cell::RefCell<std::collections::HashMap<String, Kinds>> =
            Default::default();
    }
    if let Some(k) = SEEN.with(|s| s.borrow().get(name).cloned()) {
        return k;
    }
    let r = crate::types::registry();
    let k = std::rc::Rc::new(match r.resolve(name) {
        Some((cpp, ti)) => {
            let module = r
                .by_name
                .get(name)
                .and_then(|v| v.iter().find(|(_, c)| c == cpp));
            probe(module.map_or("QtQuick", |(m, _)| m.as_str()), name, ti)
        }
        None => Vec::new(),
    });
    SEEN.with(|s| s.borrow_mut().insert(name.to_string(), k.clone()));
    k
}

/// What a document knows of an object's type: the members QML files and inline components
/// declare on the way down to the C++ type, and that type's QML name.
#[derive(Default, Clone)]
struct Shape {
    qml: std::collections::HashSet<String>,
    cpp: Option<String>,
}

struct Tree<'a> {
    root: &'a std::path::Path,
    docs: std::collections::HashMap<std::path::PathBuf, Option<std::rc::Rc<crate::Document>>>,
}

impl Tree<'_> {
    fn doc(&mut self, f: &std::path::Path) -> Option<std::rc::Rc<crate::Document>> {
        if let Some(d) = self.docs.get(f) {
            return d.clone();
        }
        let d = std::fs::read_to_string(f)
            .ok()
            .and_then(|s| crate::parse(&s).ok())
            .map(std::rc::Rc::new);
        self.docs.insert(f.to_path_buf(), d.clone());
        d
    }

    fn own(o: &crate::ast::Object, sh: &mut Shape) {
        use crate::ast::Member;
        for m in &o.members {
            if let Member::Property { name, .. }
            | Member::Function { name, .. }
            | Member::Signal { name, .. } = m
            {
                sh.qml.insert(name.clone());
            }
        }
    }

    /// The shape of an object written as `ty` in the document `doc` at `file`.
    fn shape(
        &mut self,
        file: &std::path::Path,
        doc: &crate::Document,
        ty: &str,
        depth: usize,
    ) -> Shape {
        let mut sh = Shape::default();
        if depth > 32 {
            return sh;
        }
        fn inline<'d>(ms: &'d [crate::ast::Member], ty: &str) -> Option<&'d crate::ast::Object> {
            use crate::ast::Member;
            ms.iter().find_map(|m| match m {
                Member::Component { name, object } if name == ty => Some(object),
                Member::Component { object: o, .. }
                | Member::Child(o)
                | Member::On { object: o, .. } => inline(&o.members, ty),
                Member::Group { members, .. } => inline(members, ty),
                _ => None,
            })
        }
        if let Some(o) = inline(&doc.root.members, ty) {
            Self::own(o, &mut sh);
            let base = self.shape(file, doc, &o.type_name, depth + 1);
            sh.qml.extend(base.qml);
            sh.cpp = base.cpp;
            return sh;
        }
        let visible = crate::resolve::visible_types(self.root, file, doc);
        if let Some(t) = visible.get(ty) {
            let f = t.file.to_path_buf();
            if let Some(d) = self.doc(&f) {
                Self::own(&d.root, &mut sh);
                let base = self.shape(&f, &d, &d.root.type_name, depth + 1);
                sh.qml.extend(base.qml);
                sh.cpp = base.cpp;
            }
            return sh;
        }
        let reg = crate::types::registry();
        let bare = match ty.split_once('.') {
            Some((a, b)) if doc.imports.iter().any(|i| i.alias.as_deref() == Some(a)) => b,
            _ => ty,
        };
        sh.cpp = reg.resolve(bare).map(|_| bare.to_string());
        sh
    }
}

/// The calls, in the `.qml` files under `root`, of methods Qt declares and the engine has not
/// written, where the document says what the method is called on: a bare `m()` (the object
/// the script belongs to, then the document's root), `this.m()` and `<id>.m()`. A call on
/// anything else is known only when it runs, where it throws [`error`].
pub fn calls(root: &std::path::Path) -> Scan {
    enum Recv<'d> {
        Scope,
        Object(&'d crate::ast::Object),
        Name(String),
        Unknown,
    }
    let unwritten: std::collections::HashSet<&'static str> =
        list().into_iter().map(|(_, _, m, _)| m).collect();
    let mut scan = Scan::default();
    use crate::ast::{Member, Object, Value};
    use crate::ir::Expr;
    fn files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                files(&p, out);
            } else if p.extension().is_some_and(|x| x == "qml") {
                out.push(p);
            }
        }
    }
    fn objects<'d>(o: &'d Object, out: &mut Vec<&'d Object>) {
        fn members<'d>(ms: &'d [Member], out: &mut Vec<&'d Object>) {
            for m in ms {
                match m {
                    Member::Property { value: Some(v), .. } | Member::Binding { value: v, .. } => {
                        match v {
                            Value::Script(_) => {}
                            Value::Object(c) => objects(c, out),
                            Value::List(l) => l.iter().for_each(|c| objects(c, out)),
                        }
                    }
                    Member::Group { members: g, .. } => members(g, out),
                    Member::Child(c)
                    | Member::On { object: c, .. }
                    | Member::Component { object: c, .. } => objects(c, out),
                    _ => {}
                }
            }
        }
        out.push(o);
        members(&o.members, out);
    }
    fn scripts<'d>(
        o: &'d Object,
        out: &mut Vec<(&'d Object, &'d crate::Script)>,
        ids: &mut Vec<(&'d str, &'d Object)>,
    ) {
        fn members<'d>(
            o: &'d Object,
            ms: &'d [Member],
            out: &mut Vec<(&'d Object, &'d crate::Script)>,
            ids: &mut Vec<(&'d str, &'d Object)>,
        ) {
            for m in ms {
                match m {
                    Member::Id(id) => ids.push((id, o)),
                    Member::Function { script, .. } => out.push((o, script)),
                    Member::Property { value: Some(v), .. } | Member::Binding { value: v, .. } => {
                        match v {
                            Value::Script(s) => out.push((o, s)),
                            Value::Object(c) => scripts(c, out, ids),
                            Value::List(l) => l.iter().for_each(|c| scripts(c, out, ids)),
                        }
                    }
                    Member::Group { members: g, .. } => members(o, g, out, ids),
                    Member::Child(c)
                    | Member::On { object: c, .. }
                    | Member::Component { object: c, .. } => scripts(c, out, ids),
                    _ => {}
                }
            }
        }
        members(o, &o.members, out, ids);
    }
    let mut fs = Vec::new();
    files(root, &mut fs);
    fs.sort();
    let mut tree = Tree {
        root,
        docs: Default::default(),
    };
    for f in &fs {
        let Some(doc) = tree.doc(f) else { continue };
        let text = std::fs::read_to_string(f).unwrap_or_default();
        let word = |c: char| c.is_alphanumeric() || c == '_';
        let mut rest = text.as_str();
        while let Some(at) = rest.find(word) {
            let end = rest[at..].find(|c| !word(c)).map_or(rest.len(), |n| at + n);
            let w = &rest[at..end];
            rest = &rest[end..];
            scan.words.insert(w.to_owned());
            if !rest.trim_start().starts_with('.') {
                scan.bare.insert(w.to_owned());
            }
        }
        let rel = f.strip_prefix(root).unwrap_or(f).display().to_string();
        let (mut ss, mut ids) = (Vec::new(), Vec::new());
        scripts(&doc.root, &mut ss, &mut ids);
        scan.imports
            .extend(doc.imports.iter().map(|i| i.uri.clone()));
        let mut made = Vec::new();
        objects(&doc.root, &mut made);
        for o in made {
            scan.made.extend(tree.shape(f, &doc, &o.type_name, 0).cpp);
        }
        let mut shapes: std::collections::HashMap<*const Object, Shape> = Default::default();
        let mut shape_of = |tree: &mut Tree, o: &Object| -> Shape {
            shapes
                .entry(o as *const Object)
                .or_insert_with(|| {
                    let mut sh = tree.shape(f, &doc, &o.type_name, 0);
                    Tree::own(o, &mut sh);
                    sh
                })
                .clone()
        };
        for (o, script) in ss {
            let Ok(body) = crate::ir::lower(script) else {
                continue;
            };
            let mut found: Vec<(Recv, String)> = Vec::new();
            crate::ir::visit(
                &body,
                &mut |e| {
                    let Expr::Call(callee, ..) = e else { return };
                    match &**callee {
                        Expr::Ident(m) => found.push((Recv::Scope, m.clone())),
                        Expr::Member(on, m, _) => match &**on {
                            Expr::This => found.push((Recv::Object(o), m.to_string())),
                            Expr::Ident(id) => match ids.iter().find(|(i, _)| i == id) {
                                Some((_, t)) => found.push((Recv::Object(t), m.to_string())),
                                None => found.push((Recv::Name(id.clone()), m.to_string())),
                            },
                            _ => found.push((Recv::Unknown, m.to_string())),
                        },
                        _ => {}
                    }
                },
                &mut |_| {},
            );
            let mut nth: std::collections::HashMap<String, usize> = Default::default();
            for (on, m) in found {
                // The line of this call: the n-th `m(` of the script.
                let n = nth.entry(m.clone()).or_default();
                let at = script
                    .src
                    .match_indices(&format!("{m}("))
                    .nth(*n)
                    .map_or(0, |(i, _)| script.src[..i].matches('\n').count());
                *n += 1;
                // A bare name is the script's object's, then the document root's.
                let shapes: Vec<Shape> = match on {
                    Recv::Object(t) => vec![shape_of(&mut tree, t)],
                    Recv::Scope if script.src.contains(&format!("function {m}")) => continue,
                    Recv::Scope => vec![shape_of(&mut tree, o), shape_of(&mut tree, &doc.root)],
                    // A singleton by its type name; a local or a property is not known.
                    Recv::Name(n) if n.starts_with(char::is_uppercase) => {
                        vec![tree.shape(f, &doc, &n, 0)]
                    }
                    Recv::Name(_) | Recv::Unknown => {
                        scan.unknown += 1;
                        if let Some(k) = unwritten.get(m.as_str()) {
                            *scan.by_name.entry(k).or_default() += 1;
                            scan.unknown_sites
                                .push((format!("{rel}:{}", script.line + at), k));
                        }
                        continue;
                    }
                };
                let mut hit = false;
                for sh in shapes {
                    if sh.qml.contains(&m) {
                        hit = true;
                        scan.qml += 1;
                        break;
                    }
                    let Some(cpp) = &sh.cpp else { continue };
                    let Some(&(class, method, kind)) = kinds(cpp).iter().find(|(_, n, _)| *n == m)
                    else {
                        continue;
                    };
                    hit = true;
                    if kind == Kind::Written {
                        scan.written += 1;
                    } else {
                        scan.sites.push(Site {
                            at: format!("{rel}:{}", script.line + at),
                            class,
                            method,
                        });
                    }
                    break;
                }
                if !hit {
                    scan.other += 1;
                }
            }
        }
    }
    // A `.js` file has no object of its own: a call on a value there is known only by name.
    let mut js = Vec::new();
    fn scripts_of(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                scripts_of(&p, out);
            } else if p.extension().is_some_and(|x| x == "js") {
                out.push(p);
            }
        }
    }
    scripts_of(root, &mut js);
    scan.files = js.len();
    for f in js {
        let Some((src, body)) = std::fs::read_to_string(&f)
            .ok()
            .and_then(|s| Some((crate::ir::lower_program(&s).ok()?, s)))
            .map(|(b, s)| (s, b))
        else {
            continue;
        };
        let rel = f.strip_prefix(root).unwrap_or(&f).display().to_string();
        let mut nth: std::collections::HashMap<&'static str, usize> = Default::default();
        crate::ir::visit(
            &body,
            &mut |e| {
                let Expr::Call(callee, ..) = e else { return };
                let Expr::Member(_, m, _) = &**callee else {
                    scan.other += 1;
                    return;
                };
                scan.unknown += 1;
                if let Some(k) = unwritten.get(&**m) {
                    *scan.by_name.entry(k).or_default() += 1;
                    let n = nth.entry(k).or_default();
                    let line = src
                        .match_indices(&format!("{k}("))
                        .nth(*n)
                        .map_or(0, |(i, _)| 1 + src[..i].matches('\n').count());
                    *n += 1;
                    scan.unknown_sites.push((format!("{rel}:{line}"), k));
                }
            },
            &mut |_| {},
        );
    }
    scan.files += fs.len();
    scan.sites.sort();
    scan
}

/// The QML types an object a script of the tree holds may be of: those its documents make
/// (`made`); the singletons of the modules it imports that a file names with no `.` after
/// the name (the others are the receiver only of a call that names them, and give their
/// properties); those no document can make, nothing
/// derives from and no property is of, of a module the tree imports (the engine hands them
/// out: a model's rows, a signal's argument); and the types of the properties of all of
/// these that a file of the tree names.
pub fn reach(scan: &Scan) -> std::collections::BTreeSet<&'static str> {
    let r = crate::types::registry();
    let bases: std::collections::HashSet<&str> = r
        .by_cpp
        .values()
        .filter_map(|t| t.prototype.as_deref())
        .collect();
    // The classes some property is of: an object of one is read from that property.
    let held: std::collections::HashSet<&str> = r
        .by_cpp
        .values()
        .flat_map(|t| t.props.iter().map(|p| p.ty.trim_end_matches('*')))
        .collect();
    let mut out = std::collections::BTreeSet::new();
    let mut named_only = std::collections::HashSet::new();
    let mut todo: Vec<&'static str> = Vec::new();
    for (n, cands) in &r.by_name {
        let Some((_, ti)) = r.resolve(n) else {
            continue;
        };
        // `Rustshell.Hyprland._Ipc` is what `import Rustshell.Hyprland` brings.
        let imported = |m: &str| scan.imports.contains(m.split("._").next().unwrap_or(m));
        let here = n.starts_with(char::is_uppercase)
            && cands.iter().any(|(m, c)| *c == ti.cpp && imported(m));
        let handed =
            !ti.creatable && !bases.contains(ti.cpp.as_str()) && !held.contains(ti.cpp.as_str());
        if scan.made.contains(n) || here && (ti.singleton || handed) {
            todo.push(n);
        }
        // A singleton a script only names before a `.` is no value of a script.
        if here && ti.singleton && !scan.bare.contains(n.as_str()) {
            named_only.insert(n.as_str());
        }
    }
    // The QML name of a C++ class.
    let named: std::collections::HashMap<&str, &'static str> = r
        .by_name
        .keys()
        .filter_map(|n| Some((r.resolve(n)?.1.cpp.as_str(), n.as_str())))
        .collect();
    while let Some(n) = todo.pop() {
        if !out.insert(n) {
            continue;
        }
        let Some((_, ti)) = r.resolve(n) else {
            continue;
        };
        // A property of a type nothing can make holds an object of a type derived from it.
        for c in r.chain(&ti.cpp) {
            for p in &c.props {
                // The object of a property no file names stays where it is.
                if !scan.words.contains(&p.name) {
                    continue;
                }
                let Some(&of) = named.get(p.ty.trim_end_matches('*')) else {
                    continue;
                };
                if r.resolve(of)
                    .is_some_and(|(_, t)| t.creatable || !bases.contains(t.cpp.as_str()))
                {
                    named_only.remove(of);
                    todo.push(of);
                }
            }
        }
    }
    out.retain(|n| !named_only.contains(n));
    out
}

/// The list `docs/unwritten-methods.tsv` holds: every (declaring class, method) the engine
/// has not written, the number of QML types that have it, how a call of it ends (`throws`:
/// [`error`]; `no member`: the engine's object has no such member, Qt's "is not a
/// function"), and its calls in the tree at `root`: those [`calls`] resolves, and the calls
/// of the same name on a value only a run knows (the most there can be).
pub fn table(root: &std::path::Path) -> String {
    let scan = calls(root);
    let reach = reach(&scan);
    let mut by: std::collections::BTreeMap<(&str, &str), (usize, Kind)> = Default::default();
    let mut held: std::collections::BTreeMap<(&str, &str), Vec<&str>> = Default::default();
    for (t, c, m, kind) in list() {
        let e = by.entry((c, m)).or_insert((0, kind));
        e.0 += 1;
        e.1 = e.1.min(kind);
        if reach.contains(t) {
            held.entry((c, m)).or_default().push(t);
        }
    }
    let mut s = String::from(
        "class\tmethod\ttypes\tcall\tcalls in qml/\tsame name, receiver unknown\tmay be this method\ton a type qml/ may hold\n",
    );
    for ((c, m), (types, kind)) in by {
        let n = scan
            .sites
            .iter()
            .filter(|s| s.class == c && s.method == m)
            .count();
        let how = if kind == Kind::Unwritten {
            "throws"
        } else {
            "no member"
        };
        let same = scan.by_name.get(m).copied().unwrap_or(0);
        let held: Vec<&str> = held.get(&(c, m)).cloned().unwrap_or_default();
        let may = if held.is_empty() { 0 } else { same };
        s.push_str(&format!(
            "{c}\t{m}\t{types}\t{how}\t{n}\t{same}\t{may}\t{}\n",
            held.join(" ")
        ));
    }
    s
}

/// One `file:line: message` line for every call in the tree at `root` of a method Qt declares
/// and the engine has not written, on a receiver whose type the file shows; empty when the
/// tree calls none. Such a call throws at run time ([`error`]), so a build of the tree fails
/// on it instead (`ricompiled-gen`, `riaotcheck`).
pub fn errors(root: &std::path::Path) -> Vec<String> {
    let mut out: Vec<String> = (calls(root).sites.iter())
        .map(|s| format!("{}: {}", s.at, message(s.class, s.method)))
        .collect();
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    /// `docs/unwritten-methods.tsv` is what the code finds: a method that stops being
    /// written, a new one Qt declares or a new call in `qml/` changes the file, in a diff a
    /// reviewer reads. `UPDATE_UNWRITTEN_METHODS=1 cargo test -p qml --lib unwritten` rewrites
    /// it.
    #[test]
    fn the_list_in_docs_is_the_one_the_code_finds() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let file = dir.join("docs/unwritten-methods.tsv");
        let table = super::table(&dir.join("qml"));
        // No call in `qml/` on a value whose type only a run shows can be one of these.
        let may: Vec<&str> = (table.lines().skip(1))
            .filter(|l| l.split('\t').nth(6) != Some("0"))
            .collect();
        assert!(
            may.is_empty(),
            "a call in qml/ may reach a method that is not written (write it, with a Qt \
             fixture in tests/unwritten):\n{}",
            may.join("\n")
        );
        if std::env::var_os("UPDATE_UNWRITTEN_METHODS").is_some() {
            std::fs::write(&file, &table).unwrap();
        }
        let listed = std::fs::read_to_string(&file).unwrap_or_default();
        let (new, old): (std::collections::BTreeSet<_>, std::collections::BTreeSet<_>) =
            (table.lines().collect(), listed.lines().collect());
        assert!(
            table == listed,
            "{} is behind the code (UPDATE_UNWRITTEN_METHODS=1 rewrites it)\nnot listed: {:#?}\n\
             listed, not found: {:#?}",
            file.display(),
            new.difference(&old).collect::<Vec<_>>(),
            old.difference(&new).collect::<Vec<_>>()
        );
    }

    /// Every row of the list is a method whose call says it is not written.
    #[test]
    fn every_listed_method_throws() {
        let l = super::list();
        assert!(!l.is_empty());
        let silent: Vec<_> = (l.iter())
            .filter(|(.., kind)| *kind != super::Kind::Unwritten)
            .collect();
        assert!(silent.is_empty(), "{silent:?}");
    }
}
