//! Property descriptors and integrity levels: the [[GetOwnProperty]], [[DefineOwnProperty]],
//! [[OwnPropertyKeys]], [[GetPrototypeOf]], [[SetPrototypeOf]], [[IsExtensible]],
//! [[PreventExtensions]], [[HasProperty]], [[Set]] and [[Delete]] internal methods of
//! ordinary objects and arrays (ECMA-262 §6.2.6, §10.1, §10.4.2), behind `Object.*` and
//! `Reflect.*`. Each dispatches a proxy to [`crate::proxy`] (§10.5).

use crate::eval::{
    Interp, PropKey, R, WriteTarget, array_index, define_accessor, new_obj, set_own, sym_own,
};
use crate::value::{Accessor, Internal, JsSymbol, Obj, PERMANENT, READONLY, Value};
use std::rc::Rc;

/// A Property Descriptor (ECMA-262 §6.2.6): each field `None` when absent.
#[derive(Debug, Clone, Default)]
pub struct Desc {
    pub value: Option<Value>,
    pub writable: Option<bool>,
    pub get: Option<Value>,
    pub set: Option<Value>,
    pub enumerable: Option<bool>,
    pub configurable: Option<bool>,
}

impl Desc {
    pub(crate) fn is_accessor(&self) -> bool {
        self.get.is_some() || self.set.is_some()
    }

    pub(crate) fn is_data(&self) -> bool {
        self.value.is_some() || self.writable.is_some()
    }

    pub(crate) fn data(value: Value, writable: bool, enumerable: bool, configurable: bool) -> Desc {
        Desc {
            value: Some(value),
            writable: Some(writable),
            enumerable: Some(enumerable),
            configurable: Some(configurable),
            ..Desc::default()
        }
    }
}

/// ToPropertyDescriptor (ECMA-262 §6.2.6.5).
pub fn to_desc(it: &mut Interp, v: &Value) -> R<Desc> {
    if !matches!(v, Value::Obj(_) | Value::Item(_)) {
        return it.throw("TypeError", "Type error");
    }
    let mut d = Desc::default();
    let field = |it: &mut Interp, k: &str| -> R<Option<Value>> {
        if has_property(it, v, k)? {
            Ok(Some(it.get(v, k)?))
        } else {
            Ok(None)
        }
    };
    d.enumerable = field(it, "enumerable")?.map(|x| x.truthy());
    d.configurable = field(it, "configurable")?.map(|x| x.truthy());
    d.value = field(it, "value")?;
    d.writable = field(it, "writable")?.map(|x| x.truthy());
    for (k, slot) in [("get", &mut d.get), ("set", &mut d.set)] {
        if let Some(f) = field(it, k)? {
            if !matches!(f, Value::Undefined) && crate::eval::callable(&f).is_none() {
                return it.throw("TypeError", "Type error");
            }
            *slot = Some(f);
        }
    }
    if d.is_accessor() && d.is_data() {
        return it.throw("TypeError", "Type error");
    }
    Ok(d)
}

/// FromPropertyDescriptor (ECMA-262 §6.2.6.4).
pub fn from_desc(d: &Desc) -> Value {
    let o = new_obj();
    if let Some(v) = &d.value {
        set_own(&o, "value", v.clone());
    }
    if let Some(w) = d.writable {
        set_own(&o, "writable", Value::Bool(w));
    }
    for (k, f) in [("get", &d.get), ("set", &d.set)] {
        if let Some(f) = f {
            set_own(&o, k, f.clone());
        }
    }
    for (k, b) in [
        ("enumerable", d.enumerable),
        ("configurable", d.configurable),
    ] {
        if let Some(b) = b {
            set_own(&o, k, Value::Bool(b));
        }
    }
    Value::Obj(o)
}

/// HasProperty (ECMA-262 §7.3.12) of an object `v`.
fn has_property(it: &mut Interp, v: &Value, k: &str) -> R<bool> {
    match v {
        // QV4::Sequence has no element and no `length` of its own to `in`: its prototype
        // answers.
        Value::Obj(o)
            if (array_index(k).is_some() || k == "length")
                && crate::valuetypes::is_readonly_sequence(&o.borrow()) =>
        {
            match it.proto_of(o) {
                Some(p) => it.has_property(&p, k),
                None => Ok(false),
            }
        }
        Value::Obj(o) => it.has_property(o, k),
        Value::Item(id) => it.item_has(*id, k),
        // QQmlValueTypeWrapper::virtualHasProperty: the type's properties, not its methods.
        Value::Color(_) => {
            let proto = it.vt_protos.types[0].clone();
            Ok(crate::valuetypes::is_color_property(k) || it.has_property(&proto, k)?)
        }
        _ => Ok(false),
    }
}

/// [[GetOwnProperty]] (ECMA-262 §10.1.5.1, with the exotic own properties of arrays,
/// strings and functions).
pub fn own_desc(it: &mut Interp, v: &Value, k: &str) -> R<Option<Desc>> {
    match v {
        Value::Obj(o) if o.borrow().is_proxy() => {
            crate::proxy::own_desc(it, o, &PropKey::Str(k.into()))
        }
        // Any name a QML type with no attached type has, its own or inherited, is reported as
        // its own data property (QQmlTypeWrapper::virtualGetOwnProperty).
        Value::Obj(o) if crate::types::qml_type(o).is_some_and(|t| t.attached.is_none()) => {
            if !it.has_property(o, k)? {
                return Ok(None);
            }
            let val = it.get(v, k)?;
            Ok(Some(Desc::data(val, true, true, true)))
        }
        Value::Obj(o) => {
            if k == "prototype" {
                crate::eval::fn_prototype(o);
            }
            // An index written to a value type is kept beside it.
            if array_index(k).is_some()
                && let Some(m) = crate::valuetypes::members(o)
            {
                return own_desc(it, &Value::Obj(m), k);
            }
            let b = o.borrow();
            if let Internal::TypedArray(t) = &b.internal
                && let Some(e) = crate::typedarray::own_index(t, k)
            {
                return Ok(e.map(|v| Desc::data(v, true, true, true)));
            }
            let attr = b.attr(k);
            let configurable = attr & PERMANENT == 0;
            let writable = attr & READONLY == 0;
            if let Some(a) = &b.array {
                if k == "length" {
                    return Ok(Some(Desc::data(
                        Value::Num(b.array_length() as f64),
                        writable,
                        false,
                        false,
                    )));
                }
                // An element held as an ordinary property is a hole here (see
                // `JsObject::make_slow_element`).
                if let Some(i) = array_index(k).filter(|&i| i < a.len() && !b.is_hole(i)) {
                    // QV4 reads the element's attribute byte: a zero one is no property,
                    // and once sealed it is neither data nor accessor.
                    if attr & crate::value::HIDDEN != 0 {
                        return Ok((!configurable).then(|| Desc {
                            enumerable: Some(false),
                            configurable: Some(false),
                            ..Desc::default()
                        }));
                    }
                    return Ok(Some(Desc::data(a[i].clone(), writable, true, configurable)));
                }
            }
            // V4 answers for a member of `Qt` and of a QML type (an enum value) without
            // looking: writable, enumerable, configurable (QObjectWrapper's and
            // QQmlTypeWrapper's virtualGetOwnProperty). Their own keys leave the enum
            // values out all the same.
            let wrapper = crate::qtobj::is_qt(&b) || crate::types::is_type_wrapper(&b);
            let enumerable = wrapper || b.keys.iter().any(|x| **x == *k);
            if let Some(val) = b.props.get(k) {
                return Ok(Some(Desc::data(
                    val.clone(),
                    writable,
                    enumerable,
                    configurable,
                )));
            }
            if wrapper && let Some(a) = crate::eval::own_accessor(&b, k) {
                let get = a.get.clone();
                drop(b);
                let val = match get {
                    Some(g) => it.call_getter(&g, v)?,
                    None => Value::Undefined,
                };
                return Ok(Some(Desc::data(val, true, true, true)));
            }
            if crate::valuetypes::is_own_method(&b, k) {
                drop(b);
                let method = it.get(v, k)?;
                return Ok(Some(Desc::data(method, true, true, true)));
            }
            if let Some(a) = crate::eval::own_accessor(&b, k) {
                return Ok(Some(Desc {
                    get: Some(a.get.clone().unwrap_or_default()),
                    set: Some(a.set.clone().unwrap_or_default()),
                    enumerable: Some(enumerable),
                    configurable: Some(configurable),
                    ..Desc::default()
                }));
            }
            if let Some(v) = crate::eval::intrinsic(&b, k) {
                return Ok(Some(Desc::data(v, false, false, configurable)));
            }
            Ok(None)
        }
        Value::Str(_) | Value::Str16(_) => {
            if k == "length" {
                let n = crate::utf16::len_of(v);
                return Ok(Some(Desc::data(Value::Num(n as f64), false, false, false)));
            }
            Ok(array_index(k)
                .and_then(|i| crate::utf16::unit_of(v, i))
                .map(|u| Desc::data(Value::from_utf16(&[u]), false, true, false)))
        }
        Value::Item(id) => {
            // What script gave the object keeps its attributes; a member comes first.
            let given = match it.host.write_target(*id, k) {
                WriteTarget::Missing | WriteTarget::Extra => it.host.extras(*id, false),
                _ => None,
            };
            if let Some(own) = given
                && let Some(d) = own_desc(it, &Value::Obj(own), k)?
            {
                return Ok(Some(d));
            }
            Ok((it.host.get(*id, k)).map(|val| Desc::data(val, true, true, true)))
        }
        Value::Color(c) => Ok(crate::valuetypes::is_color_own(k)
            .then(|| Desc::data(crate::builtins::color_prop(c, k), true, true, true))),
        _ => Ok(None),
    }
}

/// [[OwnPropertyKeys]] (ECMA-262 §10.1.11): array indices ascending, then the other string
/// keys, enumerable ones in insertion order.
pub fn own_keys(it: &mut Interp, v: &Value) -> R<Vec<String>> {
    Ok(match v {
        Value::Obj(o) if o.borrow().is_proxy() => {
            let ks = crate::proxy::own_keys(it, o)?;
            return Ok(ks
                .into_iter()
                .filter_map(|k| match k {
                    PropKey::Str(k) => Some(k),
                    PropKey::Sym(_) => None,
                })
                .collect());
        }
        // A QML type with no attached type has no own names (QQmlTypeWrapper's
        // virtualOwnPropertyKeys): its enums are found by reading only.
        Value::Obj(o) if crate::types::qml_type(o).is_some_and(|t| t.attached.is_none()) => {
            Vec::new()
        }
        // A value type lists its own properties, then what script defined on it.
        Value::Obj(o) if crate::valuetypes::members(o).is_some() => {
            let named = o.borrow().named_keys();
            let mut out: Vec<String> = named.iter().map(|k| k.to_string()).collect();
            if let Some(m) = crate::valuetypes::members(o) {
                out.extend(own_keys(it, &Value::Obj(m))?);
            }
            out
        }
        // QV4::SequencePrototype defines its own properties in this order.
        Value::Obj(o) if Rc::ptr_eq(o, &it.vt_protos.sequence) => {
            ["valueOf", "length", "shift", "unshift", "push", "pop"]
                .map(String::from)
                .to_vec()
        }
        Value::Obj(o) => {
            crate::eval::fn_prototype(o);
            let b = o.borrow();
            let mut out: Vec<String> = Vec::new();
            if let Internal::TypedArray(t) = &b.internal {
                out.extend((0..t.len).map(|i| i.to_string()));
            }
            if let Some(a) = &b.array {
                out.extend(
                    (0..a.len())
                        .filter(|&i| !b.is_hole(i))
                        .map(|i| i.to_string()),
                );
            }
            // OrdinaryOwnPropertyKeys (ECMA-262 §10.1.11.1): integer indices ascending, then
            // string keys in creation order (an array's `length` and a function's `prototype`,
            // `name` and `length` come first). The engine's own non-enumerable keys follow, sorted, so the
            // result is deterministic.
            let mut idx: Vec<&Rc<str>> = (b.keys.iter())
                .chain(b.props.keys())
                .chain(b.accessors.iter().flat_map(|m| m.keys()))
                .filter(|k| array_index(k).is_some())
                .collect();
            idx.sort_by_key(|k| array_index(k));
            idx.dedup();
            out.extend(idx.into_iter().map(|k| k.to_string()));
            out.sort_by_key(|k| array_index(k));
            // A function's own `length` and `name` keep their place when redefined (a static
            // `name()` method), until deleted.
            // V4's order for a function: `prototype`, `name`, `length` (a class: `length`
            // before `name`).
            let class = b.call.as_ref().and_then(crate::eval::Callable::kind)
                == Some(crate::jsrt::FnKind::ClassConstructor);
            let first: Vec<&str> = match (b.call.is_some(), class) {
                (true, true) => ["prototype", "length", "name"].as_slice(),
                (true, false) => ["prototype", "name", "length"].as_slice(),
                (false, _) => ["length", "name"].as_slice(),
            }
            .iter()
            .copied()
            .filter(|&k| {
                (k == "length" && b.array.is_some())
                    // QV4 makes an arguments object's `length` before its `callee`.
                    || (k == "length"
                        && matches!(b.internal, Internal::Arguments(_))
                        && b.props.contains_key(k))
                    || crate::eval::intrinsic(&b, k).is_some()
                    || (b.call.is_some()
                        && b.attr(k) & crate::value::ABSENT == 0
                        && (b.props.contains_key(k)
                            || b.accessors.as_ref().is_some_and(|m| m.contains_key(k))))
            })
            .collect();
            // A built-in's keys in the order QV4 creates them in, wherever that puts the
            // keys above.
            let leads = match &b.ext {
                Some(e) if e.order_leads => e.order.as_deref(),
                _ => None,
            };
            let placed = |k: &str| leads.is_some_and(|o| o.iter().any(|x| **x == *k));
            out.extend((first.iter()).filter(|k| !placed(k)).map(|k| k.to_string()));
            // QV4::Sequence lists its elements and `length`; the Easing type the members of
            // the QObject behind it, not its enum values.
            if crate::valuetypes::is_readonly_sequence(&b) {
                return Ok(out);
            }
            if crate::builtins::is_easing(&b) {
                out.extend(b.keys.iter().map(|k| k.to_string()));
                return Ok(out);
            }
            let named: Vec<Rc<str>> = match leads {
                Some(o) => {
                    let mut named: Vec<Rc<str>> = (o.iter())
                        .filter(|k| {
                            first.contains(&&***k)
                                || b.props.contains_key(*k)
                                || b.accessors.as_ref().is_some_and(|m| m.contains_key(*k))
                        })
                        .cloned()
                        .collect();
                    let known: rustc_hash::FxHashSet<Rc<str>> = named.iter().cloned().collect();
                    named.extend(b.named_keys().into_iter().filter(|k| !known.contains(k)));
                    named
                }
                None => b.named_keys(),
            };
            let first: Vec<&str> = match leads {
                Some(_) => first.into_iter().filter(|k| !placed(k)).collect(),
                None => first,
            };
            let listed: rustc_hash::FxHashSet<&Rc<str>> = named.iter().collect();
            let mut rest: Vec<&Rc<str>> = (b.props.keys())
                .chain(b.accessors.iter().flat_map(|m| m.keys()))
                .filter(|k| array_index(k).is_none() && !listed.contains(k))
                .filter(|k| !first.contains(&&***k))
                .collect();
            rest.sort();
            out.extend(
                (named.iter())
                    .filter(|k| !first.contains(&&***k))
                    .map(|k| k.to_string()),
            );
            out.extend(rest.into_iter().map(|k| k.to_string()));
            out
        }
        Value::Str(_) | Value::Str16(_) => {
            let mut out: Vec<String> = (0..v.js_str().map_or(0, |s| s.len()))
                .map(|i| i.to_string())
                .collect();
            out.push("length".into());
            out
        }
        Value::Item(_) => it.keys(v)?,
        Value::Color(_) => (crate::valuetypes::COLOR_PROPERTIES.iter())
            .map(|k| k.to_string())
            .collect(),
        _ => Vec::new(),
    })
}

/// EnumerableOwnProperties(`v`, key) (ECMA-262 §7.3.23) through the internal methods: the own
/// keys whose [[GetOwnProperty]] is enumerable.
pub fn enumerable_own_keys(it: &mut Interp, v: &Value) -> R<Vec<String>> {
    let mut out = Vec::new();
    for k in own_keys(it, v)? {
        if own_desc(it, v, &k)?.is_some_and(|d| d.enumerable == Some(true)) {
            out.push(k);
        }
    }
    Ok(out)
}

/// [[DefineOwnProperty]] of `o` (ValidateAndApplyPropertyDescriptor, ECMA-262 §10.1.6.3;
/// ArrayDefineOwnProperty, §10.4.2.1): whether it succeeded.
pub fn define(it: &mut Interp, o: &Obj, k: &str, d: &Desc) -> R<bool> {
    if o.borrow().is_proxy() {
        return crate::proxy::define(it, o, &PropKey::Str(k.into()), d);
    }
    // A value type keeps what is defined on it beside its value, a name of the type too.
    if matches!(o.borrow().internal, Internal::ValueType(_)) {
        // One read from a property is a wrapper the definition is lost with.
        if o.borrow().owner.is_some() {
            return Ok(true);
        }
        let m = crate::valuetypes::members_mut(o);
        return define(it, &m, k, d);
    }
    // TypedArray [[DefineOwnProperty]] (§10.4.5.3): an element is a writable, enumerable,
    // configurable data property, and only its value can be set.
    let element = crate::typedarray::typed(o).and_then(|t| crate::typedarray::has_index(&t, k));
    if let Some(valid) = element {
        if !valid
            || d.configurable == Some(false)
            || d.enumerable == Some(false)
            || d.is_accessor()
            || d.writable == Some(false)
        {
            return Ok(false);
        }
        if let Some(v) = &d.value {
            crate::typedarray::put_index(it, o, k, v)?;
        }
        return Ok(true);
    }
    let ov = Value::Obj(o.clone());
    if o.borrow().array.is_some() {
        if k == "length" {
            return define_array_length(it, o, d);
        }
        if let Some(i) = array_index(k) {
            let b = o.borrow();
            if i >= b.array_length() && b.attr("length") & READONLY != 0 {
                return Ok(false);
            }
            // QV4 finds no property at an element with a zero attribute byte and no place
            // for a new one either.
            let stored = b.array.as_ref().map_or(0, Vec::len);
            if i < stored && !b.is_hole(i) && b.attr(k) & crate::value::HIDDEN != 0 {
                return Ok(false);
            }
        }
    }
    let current = own_desc(it, &ov, k)?;
    let extensible = !o.borrow().non_extensible;
    let Some(m) = apply(d, current.as_ref(), extensible) else {
        return Ok(false);
    };
    let enumerable = m.enumerable == Some(true);
    let mut bits = if m.configurable == Some(true) {
        0
    } else {
        PERMANENT
    };
    let element = array_index(k).filter(|_| o.borrow().array.is_some());
    if m.is_accessor() {
        write_accessor(o, k, &m, enumerable);
        if let Some(i) = element {
            let mut b = o.borrow_mut();
            // QV4 moves the elements to its sparse storage (`ArrayData::insert`).
            b.set_sparse();
            b.make_slow_element(i);
        }
    } else {
        if m.writable != Some(true) {
            bits |= READONLY;
        }
        // QV4 gives the array attribute bytes once an element's attributes are not the
        // default ones (`Object::setArrayAttributes`), for the cells it has by then.
        if element.is_some() && (bits != 0 || !enumerable) {
            o.borrow_mut().ensure_bytes();
        }
        write_value(o, k, m.value.unwrap_or_default(), Some(enumerable));
        if let Some(i) = element
            && bits == 0
        {
            bits = o.borrow().cleared_attr(i);
        }
    }
    o.borrow_mut().set_attr(k, bits);
    Ok(true)
}

/// ValidateAndApplyPropertyDescriptor (ECMA-262 §10.1.6.3): the complete descriptor own
/// property `current` (`None`: absent) of an object that is `extensible` becomes under `d`,
/// `None` where `d` is refused.
fn apply(d: &Desc, current: Option<&Desc>, extensible: bool) -> Option<Desc> {
    if !compatible(extensible, d, current) {
        return None;
    }
    let accessor = |get: Option<Value>, set: Option<Value>, enumerable, configurable| Desc {
        get: Some(get.unwrap_or_default()),
        set: Some(set.unwrap_or_default()),
        enumerable: Some(enumerable),
        configurable: Some(configurable),
        ..Desc::default()
    };
    let Some(cur) = current else {
        let enumerable = d.enumerable.unwrap_or(false);
        let configurable = d.configurable.unwrap_or(false);
        return Some(if d.is_accessor() {
            accessor(d.get.clone(), d.set.clone(), enumerable, configurable)
        } else {
            Desc::data(
                d.value.clone().unwrap_or_default(),
                d.writable.unwrap_or(false),
                enumerable,
                configurable,
            )
        });
    };
    let enumerable = d.enumerable.unwrap_or(cur.enumerable == Some(true));
    let configurable = d.configurable.unwrap_or(cur.configurable == Some(true));
    Some(if d.is_accessor() {
        let (get, set) = if cur.is_accessor() {
            (
                d.get.clone().or(cur.get.clone()),
                d.set.clone().or(cur.set.clone()),
            )
        } else {
            (d.get.clone(), d.set.clone())
        };
        accessor(get, set, enumerable, configurable)
    } else if d.is_data() || !cur.is_accessor() {
        Desc::data(
            d.value.clone().or(cur.value.clone()).unwrap_or_default(),
            d.writable
                .unwrap_or(!cur.is_accessor() && cur.writable == Some(true)),
            enumerable,
            configurable,
        )
    } else {
        accessor(cur.get.clone(), cur.set.clone(), enumerable, configurable)
    })
}

/// [[GetOwnProperty]] for a property key (see [`own_desc`]); only objects have symbol-keyed
/// own properties.
pub fn own_desc_pk(it: &mut Interp, v: &Value, k: &PropKey) -> R<Option<Desc>> {
    let s = match k {
        PropKey::Str(k) => return own_desc(it, v, k),
        PropKey::Sym(s) => s,
    };
    match v {
        Value::Obj(o) if o.borrow().is_proxy() => crate::proxy::own_desc(it, o, k),
        Value::Obj(o) => Ok(sym_desc(o, s)),
        Value::Item(i) => Ok((it.host.extras(*i, false)).and_then(|own| sym_desc(&own, s))),
        _ => Ok(None),
    }
}

/// The descriptor of own symbol-keyed property `s` of ordinary object `o`.
fn sym_desc(o: &Obj, s: &Rc<JsSymbol>) -> Option<Desc> {
    let b = o.borrow();
    let p = sym_own(&b, s)?;
    let configurable = p.attrs & PERMANENT == 0;
    Some(match &p.accessor {
        Some(a) => Desc {
            get: Some(a.get.clone().unwrap_or_default()),
            set: Some(a.set.clone().unwrap_or_default()),
            enumerable: Some(p.enumerable),
            configurable: Some(configurable),
            ..Desc::default()
        },
        None => Desc::data(
            p.value.clone(),
            p.attrs & READONLY == 0,
            p.enumerable,
            configurable,
        ),
    })
}

/// [[DefineOwnProperty]] for a property key (see [`define`]).
pub fn define_pk(it: &mut Interp, o: &Obj, k: &PropKey, d: &Desc) -> R<bool> {
    let s = match k {
        PropKey::Str(k) => return define(it, o, k, d),
        PropKey::Sym(s) => s,
    };
    if o.borrow().is_proxy() {
        return crate::proxy::define(it, o, k, d);
    }
    let extensible = !o.borrow().non_extensible;
    let Some(m) = apply(d, sym_desc(o, s).as_ref(), extensible) else {
        return Ok(false);
    };
    let mut attrs = if m.configurable == Some(true) {
        0
    } else {
        PERMANENT
    };
    let enumerable = m.enumerable == Some(true);
    if m.is_accessor() {
        let defined = |f: Option<Value>| f.filter(|f| !matches!(f, Value::Undefined));
        let a = Accessor {
            get: defined(m.get),
            set: defined(m.set),
        };
        crate::eval::write_sym(o, s, Value::Undefined, Some(Box::new(a)), enumerable, attrs);
    } else {
        if m.writable != Some(true) {
            attrs |= READONLY;
        }
        let v = m.value.unwrap_or_default();
        crate::eval::write_sym(o, s, v, None, enumerable, attrs);
    }
    Ok(true)
}

/// [[Delete]] for a property key (see [`delete`]).
pub fn delete_pk(it: &mut Interp, o: &Obj, k: &PropKey) -> R<bool> {
    let s = match k {
        PropKey::Str(k) => return delete(it, o, k),
        PropKey::Sym(s) => s,
    };
    if o.borrow().is_proxy() {
        return crate::proxy::delete(it, o, k);
    }
    if sym_own(&o.borrow(), s).is_some_and(|p| p.attrs & PERMANENT != 0) {
        return Ok(false);
    }
    crate::eval::delete_sym(o, s);
    Ok(true)
}

/// HasProperty (ECMA-262 §7.3.12) of an object `v` for a property key.
pub fn has_pk(it: &mut Interp, v: &Value, k: &PropKey) -> R<bool> {
    match (k, v) {
        (PropKey::Str(k), _) => has_property(it, v, k),
        (PropKey::Sym(s), Value::Obj(o)) => it.has_sym(o, s),
        (PropKey::Sym(_), _) => Ok(false),
    }
}

/// [[OwnPropertyKeys]] (ECMA-262 §10.1.11) with the symbol keys: the string keys of
/// [`own_keys`], then the symbols in creation order.
pub fn own_property_keys(it: &mut Interp, v: &Value) -> R<Vec<PropKey>> {
    if let Value::Obj(o) = v
        && o.borrow().is_proxy()
    {
        return crate::proxy::own_keys(it, o);
    }
    let mut ks: Vec<PropKey> = own_keys(it, v)?.into_iter().map(PropKey::Str).collect();
    if let Value::Obj(o) = v {
        let b = o.borrow();
        ks.extend(
            b.ext
                .iter()
                .flat_map(|e| &e.syms)
                .map(|p| PropKey::Sym(p.key.clone())),
        );
    }
    Ok(ks)
}

/// IsCompatiblePropertyDescriptor (ECMA-262 §10.1.6.2): whether ValidateAndApplyPropertyDescriptor
/// accepts `d` over own property `current` (`None`: absent) of an object that is `extensible`.
pub fn compatible(extensible: bool, d: &Desc, current: Option<&Desc>) -> bool {
    let Some(cur) = current else {
        return extensible;
    };
    if cur.configurable == Some(true) {
        return true;
    }
    let same = |a: &Option<Value>, b: &Option<Value>| {
        crate::builtins::same_value(
            a.as_ref().unwrap_or(&Value::Undefined),
            b.as_ref().unwrap_or(&Value::Undefined),
        )
    };
    if d.configurable == Some(true) || d.enumerable.is_some_and(|e| Some(e) != cur.enumerable) {
        return false;
    }
    if d.is_accessor() != cur.is_accessor() && (d.is_accessor() || d.is_data()) {
        return false;
    }
    if cur.is_accessor() {
        !((d.get.is_some() && !same(&d.get, &cur.get))
            || (d.set.is_some() && !same(&d.set, &cur.set)))
    } else {
        cur.writable != Some(false)
            || !(d.writable == Some(true) || (d.value.is_some() && !same(&d.value, &cur.value)))
    }
}

/// Stores data property `k` (an array element, or a named property, listed in `keys` when
/// `enumerable`), replacing an accessor `k`. A non-enumerable array element is held as an
/// ordinary property (see `JsObject::make_slow_element`).
fn write_value(o: &Obj, k: &str, v: Value, enumerable: Option<bool>) {
    let mut b = o.borrow_mut();
    if b.array.is_some()
        && let Some(i) = array_index(k)
    {
        if enumerable == Some(false) {
            b.make_slow_element(i);
        } else {
            drop_slow_element(&mut b, k);
            b.store_element(i, v);
            return;
        }
    }
    let b = &mut *b;
    crate::eval::touch(b, k);
    if let Some(m) = &mut b.accessors {
        m.remove(k);
    }
    let key: Rc<str> = match b.props.get_key_value(k) {
        Some((key, _)) => key.clone(),
        None => k.into(),
    };
    b.props.insert(key.clone(), v);
    set_listed(b, key, enumerable.unwrap_or(false));
}

/// Removes the ordinary property `k` an array element was held as, if any.
fn drop_slow_element(b: &mut crate::value::JsObject, k: &str) {
    let data = b.props.remove(k).is_some();
    let acc = b.accessors.as_mut().is_some_and(|m| m.remove(k).is_some());
    if data || acc {
        b.keys.retain(|x| **x != *k);
    }
}

/// Stores accessor property `k` from the `get`/`set` of `d` (absent or `undefined` ones
/// left without a function).
fn write_accessor(o: &Obj, k: &str, d: &Desc, enumerable: bool) {
    let defined = |f: &Option<Value>| f.clone().filter(|f| !matches!(f, Value::Undefined));
    match (defined(&d.get), defined(&d.set)) {
        (None, None) => {
            define_accessor(o, k, true, Value::Undefined, enumerable);
            let mut b = o.borrow_mut();
            if let Some(a) = b.accessors.as_mut().and_then(|m| m.get_mut(k)) {
                a.get = None;
                a.set = None;
            }
        }
        (g, s) => {
            {
                let mut b = o.borrow_mut();
                if let Some(a) = b.accessors.as_mut().and_then(|m| m.get_mut(k)) {
                    a.get = None;
                    a.set = None;
                }
            }
            if let Some(g) = g {
                define_accessor(o, k, true, g, enumerable);
            }
            if let Some(s) = s {
                define_accessor(o, k, false, s, enumerable);
            }
        }
    }
}

fn set_listed(b: &mut crate::value::JsObject, key: Rc<str>, enumerable: bool) {
    b.set_enumerable(&key, enumerable);
}

/// ArraySetLength through [[DefineOwnProperty]] (ECMA-262 §10.4.2.4).
fn define_array_length(it: &mut Interp, o: &Obj, d: &Desc) -> R<bool> {
    if d.is_accessor() || d.configurable == Some(true) || d.enumerable == Some(true) {
        return Ok(false);
    }
    let ro = o.borrow().attr("length") & READONLY != 0;
    if let Some(v) = &d.value {
        let n = it.to_numeric(v)?;
        let n = crate::eval::array_length(it, n)?;
        let len = o.borrow().array_length();
        if n != len {
            if ro {
                return Ok(false);
            }
            let ov = Value::Obj(o.clone());
            let before = o.borrow().array_length();
            it.put(&ov, "length", Value::Num(n as f64))?;
            let after = o.borrow().array_length();
            if after != n {
                debug_assert!(after <= before);
                if d.writable == Some(false) {
                    o.borrow_mut().set_attr("length", READONLY);
                }
                return Ok(false);
            }
        }
    }
    if ro && d.writable == Some(true) {
        return Ok(false);
    }
    if d.writable == Some(false) {
        o.borrow_mut().set_attr("length", READONLY);
    }
    Ok(true)
}

/// [[PreventExtensions]] (ECMA-262 §10.1.4): whether it succeeded.
pub fn prevent_extensions(it: &mut Interp, o: &Obj) -> R<bool> {
    if o.borrow().is_proxy() {
        return crate::proxy::prevent_extensions(it, o);
    }
    // Qt makes a QML type's object anew on each read: what is done to one does not stay.
    if crate::types::qml_type(o).is_some() {
        return Ok(true);
    }
    // The Easing type stays extensible and does not say so; so does a value type read from
    // a property, a wrapper the change is lost with.
    if crate::builtins::is_easing(&o.borrow()) || crate::valuetypes::is_property_copy(o) {
        return Ok(true);
    }
    o.borrow_mut().non_extensible = true;
    if let Some(m) = crate::valuetypes::members(o) {
        m.borrow_mut().non_extensible = true;
    }
    Ok(true)
}

/// SetIntegrityLevel(frozen) of an ordinary array that holds only data properties, e.g.
/// the template objects GetTemplateObject (ECMA-262 §13.2.8.4) builds.
pub fn freeze_array(o: &Obj) {
    let mut b = o.borrow_mut();
    let n = b.array.as_ref().map_or(0, Vec::len);
    let named: Vec<Rc<str>> = b.props.keys().cloned().collect();
    b.ensure_bytes();
    for k in (0..n).map(|i| Rc::<str>::from(i.to_string())).chain(named) {
        b.set_attr(&k, READONLY | PERMANENT);
    }
    b.set_attr("length", READONLY);
    b.non_extensible = true;
}

/// [[IsExtensible]] (ECMA-262 §10.1.3).
pub fn is_extensible(it: &mut Interp, o: &Obj) -> R<bool> {
    if o.borrow().is_proxy() {
        return crate::proxy::is_extensible(it, o);
    }
    Ok(!o.borrow().non_extensible || crate::builtins::is_easing(&o.borrow()))
}

/// SetIntegrityLevel (ECMA-262 §7.3.15): `sealed`, or frozen; whether it succeeded.
pub fn set_integrity(it: &mut Interp, o: &Obj, frozen: bool) -> R<bool> {
    if !prevent_extensions(it, o)? {
        return Ok(false);
    }
    if crate::builtins::is_easing(&o.borrow()) {
        return Ok(true);
    }
    // A value type's properties stay writable: what script defined on it is sealed.
    if matches!(o.borrow().internal, Internal::ValueType(_)) {
        if crate::valuetypes::is_property_copy(o) {
            return Ok(true);
        }
        o.borrow_mut().ext.get_or_insert_default().sealed = true;
        return match crate::valuetypes::members(o) {
            Some(m) => set_integrity(it, &m, frozen),
            None => Ok(true),
        };
    }
    // A typed array's elements stay configurable (§10.4.5.3): DefinePropertyOrThrow fails.
    if crate::typedarray::typed(o).is_some_and(|t| t.len > 0) {
        let what = if frozen { "freeze" } else { "seal" };
        return it.throw(
            "TypeError",
            format!("Cannot {what} a typed array with elements"),
        );
    }
    let ov = Value::Obj(o.clone());
    if o.borrow().is_proxy() {
        for k in own_property_keys(it, &ov)? {
            let mut d = Desc {
                configurable: Some(false),
                ..Desc::default()
            };
            if frozen {
                match own_desc_pk(it, &ov, &k)? {
                    None => continue,
                    Some(c) if !c.is_accessor() => d.writable = Some(false),
                    Some(_) => {}
                }
            }
            // V4 goes on whatever the proxy's `defineProperty` trap answers.
            define_pk(it, o, &k, &d)?;
        }
        return Ok(true);
    }
    if let Some(e) = o.borrow_mut().ext.as_deref_mut() {
        for p in &mut e.syms {
            p.attrs |= PERMANENT;
            if frozen && p.accessor.is_none() {
                p.attrs |= READONLY;
            }
        }
    }
    // QV4 (`ObjectPrototype::method_seal`, `method_freeze`, qv4objectproto.cpp) gives an
    // array attribute bytes for the cells it has and goes over those: a stored element
    // becomes non-configurable, also one that had a zero byte, and `freeze` makes an empty
    // cell read-only, which the value `a[i] = v` stores there later inherits.
    if o.borrow().array.is_some() {
        let b = &mut *o.borrow_mut();
        b.ensure_bytes();
        if !b.sparse() {
            let len = b.array.as_ref().map_or(0, Vec::len);
            let slots = b.slots();
            for i in 0..len {
                let k = i.to_string();
                if b.is_hole(i) {
                    let slow = b.props.contains_key(&*k);
                    if frozen && !slow && i < slots {
                        let keep = b.attr(&k) & crate::value::BYTE;
                        b.set_attr(&k, keep | READONLY);
                    }
                } else if b.attr(&k) & crate::value::HIDDEN != 0 {
                    b.set_attr(&k, crate::value::HIDDEN | PERMANENT);
                }
            }
        }
    }
    for k in own_keys(it, &ov)? {
        let Some(cur) = own_desc(it, &ov, &k)? else {
            continue;
        };
        if cur.value.is_none() && !cur.is_accessor() {
            continue;
        }
        let mut bits = PERMANENT;
        if frozen && !cur.is_accessor() {
            bits |= READONLY;
        }
        if o.borrow().array.is_some() && k == "length" {
            if frozen {
                o.borrow_mut().set_attr("length", READONLY);
            }
            continue;
        }
        let mut b = o.borrow_mut();
        let keep = b.attr(&k) & (READONLY | crate::value::BYTE);
        b.set_attr(&k, bits | keep);
    }
    Ok(true)
}

/// TestIntegrityLevel (ECMA-262 §7.3.16): `sealed`, or frozen.
pub fn test_integrity(it: &mut Interp, o: &Obj, frozen: bool) -> R<bool> {
    if is_extensible(it, o)? {
        return Ok(false);
    }
    // A value type's properties are not own properties V4 counts: what script defined on
    // it are.
    if matches!(o.borrow().internal, Internal::ValueType(_)) {
        // V4 calls sealed only what `Object.seal` or `Object.freeze` was called on.
        if !frozen && !o.borrow().ext.as_ref().is_some_and(|e| e.sealed) {
            return Ok(false);
        }
        return match crate::valuetypes::members(o) {
            Some(m) => test_integrity(it, &m, frozen),
            None => Ok(true),
        };
    }
    let ov = Value::Obj(o.clone());
    for k in own_property_keys(it, &ov)? {
        let Some(d) = own_desc_pk(it, &ov, &k)? else {
            continue;
        };
        if d.configurable == Some(true) || (frozen && d.writable == Some(true)) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// [[GetPrototypeOf]] of ToObject(`v`): a primitive's is its wrapper's prototype.
pub fn get_prototype(it: &mut Interp, v: &Value) -> R<Option<Obj>> {
    Ok(match v {
        Value::Obj(o) if o.borrow().is_proxy() => return crate::proxy::get_prototype(it, o),
        Value::Obj(o) => it.proto_of(o),
        Value::Str(_) | Value::Str16(_) => Some(it.string_proto.clone()),
        Value::Num(_) => Some(it.number_proto.clone()),
        Value::Bool(_) => Some(it.boolean_proto.clone()),
        Value::Symbol(_) => Some(it.sym.symbol_proto.clone()),
        Value::Native(_) => Some(it.function_proto.clone()),
        Value::Color(_) => Some(it.vt_protos.types[0].clone()),
        _ => Some(it.object_proto.clone()),
    })
}

/// [[SetPrototypeOf]] (OrdinarySetPrototypeOf, ECMA-262 §10.1.2.1): whether it succeeded.
/// The cycle check stops at a proxy, whose [[GetPrototypeOf]] is not the ordinary one.
pub fn set_prototype(it: &mut Interp, o: &Obj, p: Option<&Obj>) -> R<bool> {
    if o.borrow().is_proxy() {
        return crate::proxy::set_prototype(it, o, p);
    }
    // As `prevent_extensions`: a QML type's object keeps its prototype.
    if crate::types::qml_type(o).is_some() {
        return Ok(true);
    }
    let cur = it.proto_of(o);
    if cur.as_ref().map(Rc::as_ptr) == p.map(Rc::as_ptr) {
        return Ok(true);
    }
    // A value type read from a property is a wrapper the prototype is lost with; the
    // Easing type keeps its own without a word.
    if crate::valuetypes::is_property_copy(o) || crate::builtins::is_easing(&o.borrow()) {
        return Ok(true);
    }
    if o.borrow().non_extensible || o.borrow().locked {
        return Ok(false);
    }
    let mut walk = p.cloned();
    while let Some(q) = walk {
        if Rc::ptr_eq(&q, o) {
            return Ok(false);
        }
        walk = it.proto_of(&q);
    }
    let mut b = o.borrow_mut();
    b.null_proto = p.is_none();
    b.proto = p.cloned();
    Ok(true)
}

/// [[Delete]] (ECMA-262 §10.1.10): whether it succeeded.
pub fn delete(it: &mut Interp, o: &Obj, k: &str) -> R<bool> {
    if o.borrow().is_proxy() {
        return crate::proxy::delete(it, o, &PropKey::Str(k.into()));
    }
    if crate::types::host_keeps(&o.borrow()) {
        return Ok(true);
    }
    Ok(crate::eval::delete_own(o, k))
}

/// IsArray (ECMA-262 §7.2.2): an array, or a proxy for one (a TypeError when revoked).
pub fn is_array(it: &mut Interp, v: &Value) -> R<bool> {
    match v {
        Value::Obj(o) if o.borrow().is_proxy() => crate::proxy::is_array(it, o),
        Value::Obj(o) => {
            let o = o.borrow();
            Ok(o.array.is_some() && !matches!(o.internal, crate::value::Internal::QmlList))
        }
        _ => Ok(false),
    }
}

/// OrdinarySet (ECMA-262 §10.1.9.2) of `k` found from `o` with `receiver` (the object the
/// write lands on, which a setter gets as `this`).
pub fn set_with_receiver(
    it: &mut Interp,
    o: &Value,
    k: &str,
    v: Value,
    receiver: &Value,
) -> R<bool> {
    if receiver.strict_eq(o) {
        return it.try_put(o, k, v);
    }
    set_with_receiver_pk(it, o, &PropKey::Str(k.into()), v, receiver)
}

/// [`set_with_receiver`] for a property key.
pub fn set_with_receiver_pk(
    it: &mut Interp,
    o: &Value,
    k: &PropKey,
    v: Value,
    receiver: &Value,
) -> R<bool> {
    if let PropKey::Str(s) = k
        && receiver.strict_eq(o)
    {
        return it.try_put(o, s, v);
    }
    let mut cur = o.clone();
    loop {
        if let Value::Obj(ob) = &cur
            && ob.borrow().is_proxy()
        {
            return crate::proxy::set(it, ob, k, v, receiver);
        }
        match own_desc_pk(it, &cur, k)? {
            Some(d) if d.is_accessor() => {
                return match d.set {
                    Some(s) if crate::eval::callable(&s).is_some() => {
                        it.call(&s, receiver.clone(), &[v])?;
                        Ok(true)
                    }
                    _ => Ok(false),
                };
            }
            Some(d) if d.writable == Some(false) => return Ok(false),
            Some(_) => break,
            None => match get_prototype(it, &cur)? {
                Some(p) => cur = Value::Obj(p),
                None => break,
            },
        }
    }
    let Value::Obj(r) = receiver else {
        return Ok(false);
    };
    match own_desc_pk(it, receiver, k)? {
        Some(d) if d.is_accessor() || d.writable == Some(false) => Ok(false),
        Some(_) => define_pk(
            it,
            r,
            k,
            &Desc {
                value: Some(v),
                ..Desc::default()
            },
        ),
        None => define_pk(it, r, k, &Desc::data(v, true, true, true)),
    }
}
