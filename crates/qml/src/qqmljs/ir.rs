//! What Qt's `QQmlJS::IRBuilder` (qqmlirbuilder.cpp) has against a document its parser took:
//! the checks it makes while it turns the tree into QmlIR, in its order. One error does not
//! end them, but after one Qt defines no further object, and what hangs on a defined
//! object is not checked any more.

use super::build::{GLOBAL_NAMES, NameAt, RawKind, RawMember, RawObject, RawProperty, RawValue};
use super::build::{letter, lower, upper};
use super::lexer::Loc;
use super::parser::Diagnostic;

/// The types `Parameter::stringToBuiltinType` knows.
const COMMON_TYPES: &[&str] = &[
    "void", "int", "bool", "double", "real", "string", "url", "date", "regexp", "rect", "point",
    "size", "variant", "var",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Bound {
    Value,
    /// A grouped or an attached property, with what is bound in it.
    Group(usize),
    Attached(usize),
}

struct Binding {
    name: String,
    bound: Bound,
    /// `Type on property`.
    on: bool,
}

/// What `QmlIR::Object` keeps of an object or a group.
#[derive(Default)]
struct Scope {
    /// The object a group declares into (`declarationsOverride`).
    target: Option<usize>,
    bindings: Vec<Binding>,
    properties: Vec<String>,
    aliases: Vec<String>,
    signals: Vec<String>,
    enums: Vec<String>,
    functions: Vec<(String, Loc)>,
    default: bool,
    id: bool,
}

struct Ir<'a> {
    errors: Vec<Diagnostic>,
    scopes: Vec<Scope>,
    qualifiers: &'a [String],
    code: &'a [u16],
    /// The inline components of the document, and whether one is being defined.
    components: Vec<&'a str>,
    in_component: bool,
}

/// The errors of the document with the root `root`, after those of its pragmas and
/// imports; `qualifiers` are the names its imports give modules and `code` its text.
pub(super) fn check(
    root: &RawObject,
    header: Vec<Diagnostic>,
    qualifiers: &[String],
    code: &[u16],
) -> Vec<Diagnostic> {
    let mut ir = Ir {
        errors: header,
        scopes: Vec::new(),
        qualifiers,
        code,
        components: Vec::new(),
        in_component: false,
    };
    ir.define(root, true, None);
    ir.errors
}

impl<'a> Ir<'a> {
    fn error(&mut self, at: Loc, message: &str) {
        self.errors.push(Diagnostic {
            line: at.line,
            column: at.column,
            message: message.to_string(),
        });
    }

    fn scope(&mut self, target: Option<usize>) -> usize {
        self.scopes.push(Scope {
            target,
            ..Scope::default()
        });
        self.scopes.len() - 1
    }

    /// `Object::findBinding`: the latest binding of that name.
    fn find(&self, scope: usize, name: &str) -> Option<&Binding> {
        self.scopes[scope]
            .bindings
            .iter()
            .rev()
            .find(|b| b.name == name)
    }

    /// `IRBuilder::defineQMLObject`: the scope of object `o`, of a type unless it is a group
    /// of the object `target`; none when it stays undefined.
    fn define(&mut self, o: &'a RawObject, typed: bool, target: Option<usize>) -> Option<usize> {
        if typed && !upper(o.name.rsplit('.').next().unwrap_or_default()) {
            self.error(o.last, "Expected type name");
            return None;
        }
        let scope = self.scope(target);
        stacker::maybe_grow(64 << 10, 1 << 20, || {
            for m in &o.members {
                self.member(m, scope);
            }
        });
        if !self.errors.is_empty() {
            return None;
        }
        // `IRBuilder::sanityCheckFunctionNames`.
        let s = &self.scopes[scope];
        for (i, (name, at)) in s.functions.iter().enumerate() {
            let error = if s.functions[..i].iter().any(|f| f.0 == *name) || s.signals.contains(name)
            {
                "Duplicate method name"
            } else if upper(name) {
                "Method names cannot begin with an upper case letter"
            } else if GLOBAL_NAMES.contains(&name.as_str()) {
                "Illegal method name"
            } else {
                continue;
            };
            let at = *at;
            self.error(at, error);
            return None;
        }
        Some(scope)
    }

    /// Where the name after the first one and its `.` starts, for names that start at `first`.
    fn second_name(&self, first: Loc) -> Loc {
        let space = |c: u16| char::from_u32(c as u32).is_some_and(char::is_whitespace);
        let mut at = first;
        let mut i = first.offset as usize;
        let mut dot = false;
        while let Some(&c) = self.code.get(i) {
            if c == b'.' as u16 && !dot {
                dot = true;
            } else if !space(c) && dot {
                break;
            }
            i += 1;
            at.column += 1;
            if c == b'\n' as u16 {
                at.line += 1;
                at.column = 1;
            }
        }
        at.offset = i as u32;
        at
    }

    /// `IRBuilder::resolveQualifiedId`: the scope that the binding `name`, written in
    /// `scope`, binds in (that of its group or attached object) and its last name.
    fn resolve(
        &mut self,
        name: &'a str,
        at: NameAt,
        scope: usize,
        on: bool,
    ) -> Option<(usize, &'a str)> {
        let names: Vec<&str> = name.split('.').collect();
        if names[0] == "id" && names.len() > 1 {
            self.error(at.first, "Invalid use of id property");
            return None;
        }
        let mut i = 0;
        let mut current = names[0].to_string();
        // After the name of a module comes a type.
        if names.len() > 1 && self.qualifiers.iter().any(|q| q == names[0]) {
            i = 1;
            current = format!("{}.{}", names[0], names[1]);
            if !upper(names[1]) {
                self.error(self.second_name(at.first), "Expected type name");
                return None;
            }
        }
        let mut object = scope;
        while i + 1 < names.len() {
            let attached = upper(names[i]);
            let inner = match self.find(object, &current).map(|b| b.bound) {
                Some(Bound::Attached(s)) if attached => Some(s),
                Some(Bound::Group(s)) if !attached => Some(s),
                _ => None,
            };
            object = match inner {
                Some(s) => s,
                // Qt defines an object for the group, which it does not after an error.
                None if !self.errors.is_empty() => return None,
                None => {
                    let s = self.scope(None);
                    self.scopes[object].bindings.push(Binding {
                        name: current,
                        bound: if attached {
                            Bound::Attached(s)
                        } else {
                            Bound::Group(s)
                        },
                        on,
                    });
                    s
                }
            };
            i += 1;
            current = names[i].to_string();
        }
        Some((object, names[names.len() - 1]))
    }

    /// `Object::appendBinding` of a value: an error at `at` when the property has one.
    fn bind(&mut self, scope: usize, name: &str, at: Loc, on: bool) {
        let set = self
            .find(scope, name)
            .is_some_and(|b| b.bound == Bound::Value && !b.on);
        if set && !on {
            self.error(at, "Property value set multiple times");
        } else {
            self.scopes[scope].bindings.push(Binding {
                name: name.to_string(),
                bound: Bound::Value,
                on,
            });
        }
    }

    /// `IRBuilder::appendBinding` of the defined object or list item.
    fn bind_object(&mut self, scope: usize, name: &str, at: NameAt, on: bool, item: bool) {
        if name == "id" {
            self.error(at.last, "Invalid component id specification");
        } else if item {
            self.scopes[scope].bindings.push(Binding {
                name: name.to_string(),
                bound: Bound::Value,
                on,
            });
        } else {
            self.bind(scope, name, at.first, on);
        }
    }

    /// The value `value` of the binding or the property `name`, written in `scope`.
    fn value(&mut self, name: &'a str, at: NameAt, value: &'a RawValue, null: bool, scope: usize) {
        match value {
            RawValue::Script(..) => {
                if let Some((target, name)) = self.resolve(name, at, scope, false)
                    && !null
                {
                    self.bind(target, name, at.first, false);
                }
            }
            RawValue::Object(o) => {
                if self.define(o, true, None).is_some()
                    && let Some((target, name)) = self.resolve(name, at, scope, false)
                {
                    self.bind_object(target, name, at, false, false);
                }
            }
            RawValue::List(objects) => {
                let Some((target, name)) = self.resolve(name, at, scope, false) else {
                    return;
                };
                if self.find(target, name).is_some() {
                    self.error(at.last, "Property value set multiple times");
                    return;
                }
                for o in objects.iter().rev() {
                    if self.define(o, true, None).is_none() {
                        return;
                    }
                    self.bind_object(target, name, at, false, true);
                }
            }
        }
    }

    /// `IRBuilder::setId`.
    fn id(&mut self, name: &str, value_at: Loc, at: Loc, scope: usize) {
        let mut rest = name.chars();
        rest.next();
        // Qt asks each UTF-16 unit: a surrogate is no letter.
        let part = |c: char| (c.is_alphanumeric() && c.len_utf16() == 1) || c == '_';
        let error = match name {
            "" => "Invalid empty ID",
            _ if letter(name) && !lower(name) => "IDs cannot start with an uppercase letter",
            _ if !letter(name) && !name.starts_with('_') => {
                "IDs must start with a letter or underscore"
            }
            _ if !rest.all(part) => "IDs must contain only letters, numbers, and underscores",
            _ if GLOBAL_NAMES.contains(&name) => "ID illegally masks global JavaScript property",
            _ if self.scopes[scope].id => {
                self.error(at, "Property value set multiple times");
                return;
            }
            _ => {
                self.scopes[scope].id = true;
                return;
            }
        };
        self.error(value_at, error);
    }

    /// `IRBuilder::visit(UiPublicMember)` of a property.
    fn property(&mut self, p: &'a RawProperty, first: Loc, scope: usize) {
        let alias = p.ty == "alias";
        if alias && p.value.is_none() {
            return self.error(first, "No property alias location");
        }
        if let Some(at) = p.modifier_at
            && !alias
        {
            return self.error(at, "Invalid property type modifier");
        }
        if GLOBAL_NAMES.contains(&p.name.as_str()) {
            return self.error(p.name_at, "Illegal property name");
        }
        // `Object::appendProperty`, `Object::appendAlias`.
        let target = self.scopes[scope].target.unwrap_or(scope);
        let t = &mut self.scopes[target];
        let error = if alias && t.aliases.contains(&p.name) {
            Some("Duplicate alias name")
        } else if alias && t.properties.contains(&p.name) {
            Some("Alias has same name as existing property")
        } else if alias && upper(&p.name) {
            Some("Alias names cannot begin with an upper case letter")
        } else if alias {
            None
        } else if t.properties.contains(&p.name) {
            Some("Duplicate property name")
        } else if t.aliases.contains(&p.name) {
            Some("Property duplicates alias name")
        } else if upper(&p.name) {
            Some("Property names cannot begin with an upper case letter")
        } else {
            None
        };
        if let Some(e) = error {
            return self.error(p.name_at, e);
        }
        match alias {
            true => t.aliases.push(p.name.clone()),
            false => t.properties.push(p.name.clone()),
        }
        if p.modifiers_default() {
            if t.default {
                return self.error(p.default_at, "Duplicate default property");
            }
            t.default = true;
        }
        if alias {
            return;
        }
        if let Some(value) = &p.value {
            // `isRedundantNullInitializerForPropertyDeclaration`.
            let object_type = !COMMON_TYPES.contains(&p.ty.as_str()) && !p.ty.contains('<');
            let null = matches!(value, RawValue::Script(_, true)) && object_type;
            let at = NameAt {
                first: p.name_at,
                last: p.name_at,
            };
            // The value of a property declared in a group is the object's.
            self.value(&p.name, at, value, null, target);
        }
    }

    fn member(&mut self, m: &'a RawMember, scope: usize) {
        match &m.kind {
            RawKind::Id(name, value_at) => self.id(name, *value_at, m.first, scope),
            RawKind::Property(p) => self.property(p, m.first, scope),
            RawKind::Required(_) => {}
            RawKind::Signal(name, at, _) => {
                let cased = name.char_indices().find_map(|(i, _)| {
                    let rest = &name[i..];
                    (lower(rest) || upper(rest)).then(|| upper(rest))
                });
                if cased == Some(true) {
                    self.error(*at, "Signal names cannot begin with an upper case letter");
                } else if GLOBAL_NAMES.contains(&name.as_str()) {
                    self.error(*at, "Illegal signal name");
                } else if self.scopes[scope].signals.contains(name) {
                    self.error(*at, "Duplicate signal name");
                } else {
                    let target = self.scopes[scope].target.unwrap_or(scope);
                    self.scopes[target].signals.push(name.clone());
                }
            }
            RawKind::Function(name, at, _) => {
                if self.scopes[scope].target.is_some() {
                    self.error(m.first, "Function declaration inside grouped property");
                } else {
                    self.scopes[scope].functions.push((name.clone(), *at));
                }
            }
            RawKind::Declaration => {
                self.error(m.first, "JavaScript declaration outside Script element");
            }
            RawKind::Binding(name, at, value) => self.value(name, *at, value, false, scope),
            RawKind::Group(g) => {
                let at = NameAt {
                    first: g.at,
                    last: g.last,
                };
                if let Some(inner) = self.define(g, false, Some(scope))
                    && let Some((target, name)) = self.resolve(&g.name, at, scope, false)
                {
                    if name == "id" {
                        self.error(at.last, "Invalid component id specification");
                    } else {
                        self.scopes[target].bindings.push(Binding {
                            name: name.to_string(),
                            bound: Bound::Group(inner),
                            on: false,
                        });
                    }
                }
            }
            RawKind::On(prop, at, o) => {
                if self.define(o, true, None).is_some()
                    && let Some((target, name)) = self.resolve(prop, *at, scope, true)
                {
                    self.bind_object(target, name, *at, true, false);
                }
            }
            RawKind::Child(o) => {
                self.define(o, true, None);
            }
            RawKind::Component(name, o) => {
                if self.in_component {
                    self.error(m.first, "Nested inline components are not supported");
                } else if self.components.contains(&name.as_str()) {
                    self.error(m.first, "Inline component names must be unique per file");
                } else {
                    self.components.push(name);
                    self.in_component = true;
                    self.define(o, true, None);
                    self.in_component = false;
                }
            }
            RawKind::Enum(name, values) => {
                if lower(name) {
                    return self.error(
                        m.first,
                        "Scoped enum names must begin with an upper case letter",
                    );
                }
                for v in values {
                    let error = match v.value {
                        _ if lower(&v.name) => {
                            Some((v.at, "Enum names must begin with an upper case letter"))
                        }
                        Some((_, n, at)) if n.fract() != 0.0 => {
                            Some((at, "Enum value must be an integer"))
                        }
                        Some((_, n, at)) if n > i32::MAX as f64 || n < i32::MIN as f64 => {
                            Some((at, "Enum value out of range"))
                        }
                        _ => None,
                    };
                    if let Some((at, e)) = error {
                        return self.error(at, e);
                    }
                }
                if self.scopes[scope].enums.contains(name) {
                    self.error(m.first, "Duplicate scoped enum name");
                } else {
                    let target = self.scopes[scope].target.unwrap_or(scope);
                    self.scopes[target].enums.push(name.clone());
                }
            }
        }
    }
}
