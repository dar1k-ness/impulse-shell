//! What a property of a fresh instance of a Qt type reads in Qt 6.12 where that is not the
//! zero of its C++ type: `qt_defaults.tsv`, written by `tests/unset_defaults/table.py` from the
//! values Qt gives (`tests/unset_defaults/*.txt`).
//!
//! A line is `<key>.<property>`, a tab and the value: a number, boolean, string or colour,
//! `null`, `value:` and a value type as String() gives it, or `group` for an object the type
//! always has (`drag`, `visibleArea`; `group:` and its text for a gadget), whose fields are
//! lines too. The key is the C++ class declaring the
//! property when every type of that class reads the same, else the QML type (a constructor
//! or the Basic style sets it: `RadioButton.checkable`, `ToolTip.margins`), or the C++ class
//! of an attached object (`QQuickLayoutAttached`, from `tests/unset_defaults/attached.txt`).

use crate::runtime::Sym;
use crate::value::Value;
use std::collections::HashMap;

/// What a line of the table gives.
#[derive(Clone)]
enum Spec {
    Value(Value),
    /// A value type, made anew for each read: its type and numbers.
    Made(crate::valuetypes::Vt, Vec<f64>),
    /// An object or gadget the type always has, which a script is given as a group of its
    /// owner; the text String() of a gadget gives.
    Group(Option<&'static str>),
}

type Table = HashMap<&'static str, Vec<(&'static str, Spec)>>;

fn spec(s: &'static str) -> Spec {
    if s == "group" {
        return Spec::Group(None);
    }
    if let Some(text) = s.strip_prefix("group:") {
        return Spec::Group(Some(text));
    }
    if let Some(v) = s.strip_prefix("value:") {
        let (ty, nums) = v.split_once('(').expect(s);
        let nums = nums.trim_end_matches(')').split(", ");
        let vt = crate::valuetypes::Vt::of_type(ty).expect(s);
        return Spec::Made(vt, nums.map(|n| n.parse().expect(s)).collect());
    }
    Spec::Value(if s == "null" {
        Value::Null
    } else if let Some(n) = s.strip_prefix("number:") {
        Value::Num(n.parse().expect(s))
    } else if let Some(b) = s.strip_prefix("boolean:") {
        Value::Bool(b == "true")
    } else if let Some(c) = s.strip_prefix("color:#") {
        let v = u32::from_str_radix(c, 16).expect(s);
        Value::Color(crate::color::Color::from_argb32(if c.len() == 6 {
            0xff00_0000 | v
        } else {
            v
        }))
    } else {
        Value::str(
            s.strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .expect(s),
        )
    })
}

thread_local! {
    /// Property name → (class or QML type, what the line gives).
    static TABLE: Table = {
        let mut t = Table::new();
        for l in include_str!("qt_defaults.tsv").lines() {
            let (path, v) = l.split_once('\t').expect(l);
            let (key, prop) = path.split_once('.').expect(l);
            t.entry(prop).or_default().push((key, spec(v)));
        }
        t
    };
}

fn find(types: &[Sym], name: &str) -> Option<Spec> {
    find_in(types, name, true)
}

/// The line of `name` for `types`; `typed`: also one a QML type has for itself.
fn find_in(types: &[Sym], name: &str, typed: bool) -> Option<Spec> {
    TABLE.with(|t| {
        let rows = t.get(name)?;
        let find = |key: &str| rows.iter().find(|r| r.0 == key).map(|r| r.1.clone());
        if typed && let Some(v) = types.iter().find_map(|t| find(t)) {
            return Some(v);
        }
        let r = crate::types::registry();
        let (_, ti) = types.iter().find_map(|t| r.resolve(t))?;
        r.chain(&ti.cpp).into_iter().find_map(|c| find(&c.cpp))
    })
}

/// The default of property `name` (`border.width` for one of a group) of an object of the
/// QML types `types`, most derived first.
pub fn get(types: &[Sym], name: &str) -> Option<Value> {
    match find(types, name)? {
        Spec::Value(v) => Some(v),
        Spec::Made(vt, nums) => Some(crate::valuetypes::make_num(vt, &nums)),
        Spec::Group(_) => None,
    }
}

/// The default the C++ class of such an object gives `name`, whatever a QML type has for
/// itself.
pub fn of_class(types: &[Sym], name: &str) -> Option<Value> {
    match find_in(types, name, false)? {
        Spec::Value(v) => Some(v),
        Spec::Made(vt, nums) => Some(crate::valuetypes::make_num(vt, &nums)),
        Spec::Group(_) => None,
    }
}

/// Whether `name` (`drag`, `point.device`) of such an object is an object or gadget a script
/// reads as a group of its owner, and the text String() of the gadget gives.
pub fn group(types: &[Sym], name: &str) -> Option<Option<&'static str>> {
    match find(types, name)? {
        Spec::Group(text) => Some(text),
        _ => None,
    }
}

/// The default of property `name` of an attached object of C++ class `class`
/// (`QQuickLayoutAttached`), where it is not the zero of its type.
pub fn attached(class: &str, name: &str) -> Option<Value> {
    TABLE.with(|t| match &t.get(name)?.iter().find(|r| r.0 == class)?.1 {
        Spec::Value(v) => Some(v.clone()),
        _ => None,
    })
}
