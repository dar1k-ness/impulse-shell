//! Names written behind an import qualifier (`import QtQuick as QQ`, `QQ.Timer {}`,
//! `QQ.Component.onCompleted: …`). QQmlImports resolves `QQ.Timer` in the imports that have
//! the qualifier `QQ` and nowhere else, and what it finds is the type itself: nothing later
//! tells an object made as `QQ.Timer` from one made as `Timer`. So a parsed document names a
//! C++ type by its bare name, with [`Object::scope`] saying where it is from: the name is not
//! looked up among the document's QML files again.
use crate::ast::{Document, Import, Member, Object, Scope, Value};

/// Whether some C++ type is registered in the module `uri`.
fn registered_module(uri: &str) -> bool {
    use std::{collections::HashSet, sync::OnceLock};
    static MODULES: OnceLock<HashSet<&'static str>> = OnceLock::new();
    let all = MODULES.get_or_init(|| {
        let names = crate::types::registry().by_name.values();
        names.flatten().map(|(m, _)| m.as_str()).collect()
    });
    // The controls of QtQuick.Controls are registered as its templates (`Registry::exports`).
    all.contains(uri) || uri == "QtQuick.Controls"
}

/// What `import uri as Q` makes of `Q.name`: a C++ type (`true`), a QML file of the import
/// (`false`), or nothing.
fn exports(uri: &str, name: &str) -> Option<bool> {
    // A directory (`"parts"`, `ri.modules.bar`) has QML files only (`resolve::visible_types`),
    // looked up when the object is made.
    if uri.starts_with('"') || uri == "ri" || uri.starts_with("ri.") {
        return None;
    }
    if crate::types::module_types(uri).contains_key(name) {
        return Some(false);
    }
    if registered_module(uri) {
        return crate::types::registry().exports(uri, name).then_some(true);
    }
    // A module nothing is registered for: its types are the ones the engine implements by
    // name.
    let known = crate::typecheck::NATIVE.contains(&name) || crate::impulse::element(name).is_some();
    known.then_some(true)
}

/// The C++ type `written` names in a document with these imports and its module, when its
/// first component is an import qualifier: the last import with that qualifier that has the
/// name decides (QQmlImports: a later import shadows an earlier one).
pub fn native<'a, 'i>(imports: &'i [Import], written: &'a str) -> Option<(&'a str, &'i str)> {
    let (alias, name) = written.split_once('.')?;
    let with_alias = imports
        .iter()
        .rev()
        .filter(|i| i.alias.as_deref() == Some(alias));
    let (uri, cpp) = with_alias
        .filter_map(|i| Some((i.uri.as_str(), exports(&i.uri, name)?)))
        .next()?;
    cpp.then_some((name, uri))
}

/// Whether the first component of `written` is an import qualifier.
fn qualifier(imports: &[Import], written: &str) -> bool {
    let alias = written.split_once('.').map(|(a, _)| a);
    alias.is_some() && imports.iter().any(|i| i.alias.as_deref() == alias)
}

/// `Q.Type.rest` (an attached property or handler, `QQ.Component.onCompleted`) without the
/// qualifier, when `Q.Type` is a C++ type.
fn attached(imports: &[Import], written: &str) -> Option<String> {
    let (alias, rest) = written.split_once('.')?;
    let (ty, _) = rest.split_once('.')?;
    let qualified = &written[..alias.len() + 1 + ty.len()];
    native(imports, qualified).map(|_| rest.to_string())
}

/// The type `property QQ.Item p` / `property list<QQ.Item> p` / `property QQ.color p` declares,
/// without the qualifier of a C++ type or of a value type (lower case, which only modules
/// have).
fn property_type(imports: &[Import], ty: &str) -> Option<String> {
    if let Some(inner) = ty.strip_prefix("list<").and_then(|t| t.strip_suffix('>')) {
        return property_type(imports, inner).map(|t| format!("list<{t}>"));
    }
    if let Some((name, _)) = native(imports, ty) {
        return Some(name.to_string());
    }
    let (_, name) = ty.split_once('.').filter(|_| qualifier(imports, ty))?;
    let value_type = name.starts_with(char::is_lowercase) && !name.contains('.');
    value_type.then(|| name.to_string())
}

fn object(imports: &[Import], o: &mut Object) {
    if let Some((name, module)) = native(imports, &o.type_name) {
        o.scope = Scope::Module(module.into());
        o.type_name = match (module, name) {
            // Kirigami is not shipped: its Icon is the engine's under this name (`types.rs`).
            ("org.kde.kirigami", "Icon") => "Kirigami.Icon".into(),
            _ => name.to_string(),
        };
    } else if qualifier(imports, &o.type_name) {
        o.scope = Scope::Qualified;
    }
    members(imports, &mut o.members);
}

fn members(imports: &[Import], ms: &mut [Member]) {
    for m in ms {
        match m {
            Member::Child(o)
            | Member::On { object: o, .. }
            | Member::Component { object: o, .. } => object(imports, o),
            Member::Group { name, members: ms } => {
                if let Some(n) = attached(imports, &format!("{name}.")) {
                    *name = n.trim_end_matches('.').to_string();
                }
                members(imports, ms);
            }
            Member::Binding { name, value } => {
                if let Some(n) = attached(imports, name) {
                    *name = n;
                }
                self::value(imports, value);
            }
            Member::Property { ty, value, .. } => {
                if let Some(t) = property_type(imports, ty) {
                    *ty = t;
                }
                if let Some(v) = value {
                    self::value(imports, v);
                }
            }
            _ => {}
        }
    }
}

fn value(imports: &[Import], v: &mut Value) {
    match v {
        Value::Object(o) => object(imports, o),
        Value::List(os) => os.iter_mut().for_each(|o| object(imports, o)),
        Value::Script(_) => {}
    }
}

/// Resolves the qualified names of a parsed document.
pub fn resolve(doc: &mut Document) {
    if doc.imports.iter().all(|i| i.alias.is_none()) {
        return;
    }
    let imports = std::mem::take(&mut doc.imports);
    object(&imports, &mut doc.root);
    doc.imports = imports;
}
