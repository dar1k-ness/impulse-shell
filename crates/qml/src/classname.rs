//! The class name of a QML object, as `QObject::toString` and V4's error texts show it:
//! `QQuickRectangle_QML_4`, `Foo_QMLTYPE_0`, `IC`.
//!
//! Qt names a class when it compiles a document (QQmlPropertyCacheCreator, qtdeclarative
//! `qqmlpropertycachecreator_p.h`). An object gets a class of its own when it declares a
//! property, alias, signal, function, enum or inline component, when an object is assigned
//! `on` one of its properties, or when it is the root of a file that names a type
//! (`buildMetaObjectRecursively`, `:633`); any other object keeps the class of its type. The
//! new class is `<base class>_QML_<n>` (`createMetaObject`, `:790`), the root of an inline
//! component is named as the component, and the root of a file `Foo.qml` is
//! `Foo_QMLTYPE_<n>` (`createClassNameTypeByUrl`, qqmlpropertycachecreator.cpp:45).
//!
//! `<n>` comes from one counter of the process (`classIndexCounter`, `:14`), taken in the
//! order the compiler visits the objects of a document: every inline component takes one
//! for its type, then the inline components' objects, the last declared first, then the root.
//! An object is visited before the objects of its bindings; those go in the order of QmlIR's
//! binding list, where a binding to a named property (`states: [..]`, `property Item p:
//! Item {}`, a group, `X on p {}`) is put in front of the earlier ones and the children
//! assigned to the default property follow in source order.
//!
//! The numbers are those of a document compiled from source. Qt's numbers of a document read
//! from its disk cache are higher after the first `Component`, and the number of a file type
//! depends on the order the type loader finishes the files, so only the numbers of one
//! document's objects are meant to equal Qt's.

use crate::ast::{Member, NodeId, Object, Value};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

#[derive(Clone, Copy)]
enum Class {
    /// `<base class>_QML_<n>`.
    Number(u32),
    /// The root of an inline component or of a file type.
    Named(&'static str),
}

/// How the root of a document is named.
pub enum Root<'a> {
    /// The document itself is loaded: its root is an object like any other.
    Plain,
    /// The file name of a type (`Foo` of `Foo.qml`).
    Type(&'a str),
    /// A document without a file name of a type (`Qt.createQmlObject`): Qt takes a number for
    /// `ANON_QML_TYPE_<n>` and then names the root like any other object.
    Anonymous,
}

#[derive(Default)]
pub struct Classes {
    counter: u32,
    docs: HashSet<u64>,
    of: HashMap<NodeId, Class>,
    /// The class name of the objects made from a declaration with a class of its own.
    names: HashMap<NodeId, &'static str>,
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// Whether the grouped property `name` is a value (`font`): an object assigned `on` a property
/// inside it intercepts through the object that owns the group, which then needs the class.
/// For any other group (`anchors`, `border`) the group's own object gets it.
fn value_group(name: &str) -> bool {
    name == "font"
}

impl Classes {
    fn take(&mut self) -> u32 {
        self.counter += 1;
        self.counter - 1
    }

    /// Names the classes of the document `root` is the root of, once per document.
    pub fn document(&mut self, root: &Object, how: Root) {
        if root.decl.is_none() || !self.docs.insert(root.decl.doc) {
            return;
        }
        let mut inline = Vec::new();
        fn collect<'a>(ms: &'a [Member], out: &mut Vec<(&'a str, &'a Object)>) {
            for m in ms {
                match m {
                    Member::Component { name, object } => {
                        out.push((name, object));
                        collect(&object.members, out);
                    }
                    Member::Child(o) | Member::On { object: o, .. } => collect(&o.members, out),
                    Member::Group { members, .. } => collect(members, out),
                    Member::Property { value: Some(v), .. } | Member::Binding { value: v, .. } => {
                        match v {
                            Value::Object(o) => collect(&o.members, out),
                            Value::List(os) => os.iter().for_each(|o| collect(&o.members, out)),
                            Value::Script(_) => {}
                        }
                    }
                    _ => {}
                }
            }
        }
        collect(&root.members, &mut inline);
        let type_name = match how {
            Root::Plain => None,
            Root::Type(name) => Some(leak(format!("{name}_QMLTYPE_{}", self.take()))),
            Root::Anonymous => {
                self.take();
                None
            }
        };
        self.counter += inline.len() as u32;
        for (name, object) in inline.iter().rev() {
            self.object(object, Some(leak(name.to_string())));
        }
        self.object(root, type_name);
    }

    fn object(&mut self, o: &Object, name: Option<&'static str>) {
        if o.type_name == "Component" {
            // An explicit Component has QQmlComponent's class; its object is compiled in place.
            return self.bindings(o, &o.members);
        }
        let own = name.is_some()
            || o.members.iter().any(|m| {
                matches!(
                    m,
                    Member::Property { .. }
                        | Member::Signal { .. }
                        | Member::Function { .. }
                        | Member::Enum { .. }
                        | Member::Component { .. }
                        | Member::On { .. }
                )
            });
        if own {
            self.own(o, name);
        }
        self.bindings(o, &o.members);
    }

    fn own(&mut self, o: &Object, name: Option<&'static str>) {
        if !self.of.contains_key(&o.decl) {
            let class = match name {
                Some(n) => Class::Named(n),
                None => Class::Number(self.take()),
            };
            self.of.insert(o.decl, class);
        }
    }

    /// The objects of `ms`, the members of `owner` or of one of its groups, in QmlIR's order.
    fn bindings(&mut self, owner: &Object, ms: &[Member]) {
        for m in ms.iter().rev() {
            match m {
                Member::Property { value: Some(v), .. } | Member::Binding { value: v, .. } => {
                    match v {
                        Value::Object(o) => self.object(o, None),
                        Value::List(os) => os.iter().for_each(|o| self.object(o, None)),
                        Value::Script(_) => {}
                    }
                }
                Member::On { object, .. } => self.object(object, None),
                Member::Group { name, members } => {
                    if members.iter().any(|m| matches!(m, Member::On { .. })) {
                        if value_group(name) {
                            self.own(owner, None);
                        } else {
                            // The class of the group's object (`QQuickAnchors_QML_<n>`).
                            self.take();
                        }
                    }
                    self.bindings(owner, members);
                }
                _ => {}
            }
        }
        for m in ms {
            if let Member::Child(o) = m {
                self.object(o, None);
            }
        }
    }

    /// The class of an object made from `decl`, whose class is `base` without the
    /// declaration: `None` when the declaration gives it no class of its own.
    pub fn name(&mut self, decl: NodeId, base: impl FnOnce() -> String) -> Option<&'static str> {
        match *self.of.get(&decl)? {
            Class::Named(n) => Some(n),
            Class::Number(n) => Some(
                self.names
                    .entry(decl)
                    .or_insert_with(|| leak(format!("{}_QML_{n}", base()))),
            ),
        }
    }
}
