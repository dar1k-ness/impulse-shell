//! Symbol (ECMA-262 §20.4), the built-in iterators of the iteration protocol (§27.1, §23.1.5,
//! §22.1.5) and WeakMap/WeakSet (§24.3, §24.4), as Qt 6.12's V4 has them (no WeakRef or
//! FinalizationRegistry).

use std::rc::Rc;

use crate::eval::{
    Interp, NativeFn, R, callable, define_accessor, define_sym, define_sym_accessor,
    link_builtin_prototype, new_obj, set_hidden,
};
use crate::value::{
    Internal, IterKind, JsSymbol, ListIter, Obj, Value, WeakColl, WellKnown, symbol_for,
};

/// The realm's intrinsics this module adds, and the built-ins it installs on the shared
/// prototypes (while a value still has those, iteration and `instanceof` take native paths).
/// One per realm: installed with the builtins and shared through the global frame by every
/// interpreter over it ([`crate::eval::Frame::symbol_realm`]).
#[derive(Debug, Clone)]
pub struct Realm {
    /// %RegExp.prototype.exec%, which `RegExp.prototype[@@split]` runs natively.
    pub regexp_exec: Value,
    /// The built-in methods the dispatch fast path ([`crate::builtins::method`]) stands for,
    /// by (prototype address, name), as installed: a call runs the fast path only while
    /// `this.name` still resolves to one of these.
    pub builtin_methods: Rc<std::collections::HashMap<(usize, &'static str), Value>>,
    /// %RegExpStringIteratorPrototype% (ECMA-262 §22.2.9.2).
    pub regexp_string_iter_proto: Obj,
    /// %Symbol.prototype%.
    pub symbol_proto: Obj,
    /// %IteratorPrototype% (ECMA-262 §27.1.2), %ArrayIteratorPrototype% (§23.1.5.2) and
    /// %StringIteratorPrototype% (§22.1.5.1).
    pub iterator_proto: Obj,
    pub array_iter_proto: Obj,
    pub string_iter_proto: Obj,
    /// %MapIteratorPrototype% and %SetIteratorPrototype% (ECMA-262 §24.1.5.2, §24.2.5.2).
    pub map_iter_proto: Obj,
    pub set_iter_proto: Obj,
    pub weak_map_proto: Obj,
    pub weak_set_proto: Obj,
    /// `Array.prototype[@@iterator]`, `String.prototype[@@iterator]`,
    /// `Function.prototype[@@hasInstance]`.
    pub array_values: Value,
    /// `%TypedArray%.prototype.values`, its initial `@@iterator`.
    pub typed_values: Value,
    /// `URLSearchParams.prototype.entries`, its initial `@@iterator`, and
    /// %URLSearchParamsIteratorPrototype% (WebIDL pair iterator).
    pub params_iterator: Value,
    pub params_iter_proto: Obj,
    /// `Map.prototype.entries` and `Set.prototype.values`, their initial `@@iterator`.
    pub map_entries: Value,
    pub set_values: Value,
    pub string_iterator: Value,
    pub has_instance: Value,
}

impl Default for Realm {
    fn default() -> Self {
        Realm {
            regexp_exec: Value::Undefined,
            builtin_methods: Rc::default(),
            regexp_string_iter_proto: new_obj(),
            symbol_proto: new_obj(),
            iterator_proto: new_obj(),
            array_iter_proto: new_obj(),
            string_iter_proto: new_obj(),
            map_iter_proto: new_obj(),
            set_iter_proto: new_obj(),
            weak_map_proto: new_obj(),
            weak_set_proto: new_obj(),
            array_values: Value::Undefined,
            typed_values: Value::Undefined,
            params_iterator: Value::Undefined,
            params_iter_proto: new_obj(),
            map_entries: Value::Undefined,
            set_values: Value::Undefined,
            string_iterator: Value::Undefined,
            has_instance: Value::Undefined,
        }
    }
}

impl Realm {
    /// Every object the realm holds.
    pub fn objs(&self) -> impl Iterator<Item = Obj> + '_ {
        let protos = [
            &self.symbol_proto,
            &self.iterator_proto,
            &self.array_iter_proto,
            &self.string_iter_proto,
            &self.map_iter_proto,
            &self.set_iter_proto,
            &self.weak_map_proto,
            &self.weak_set_proto,
            &self.params_iter_proto,
        ];
        let fns = [
            &self.array_values,
            &self.typed_values,
            &self.params_iterator,
            &self.map_entries,
            &self.set_values,
            &self.string_iterator,
            &self.has_instance,
            &self.regexp_exec,
        ];
        protos
            .into_iter()
            .cloned()
            .chain(fns.into_iter().filter_map(|v| match v {
                Value::Obj(o) => Some(o.clone()),
                _ => None,
            }))
    }
}

/// %TypedArray%.prototype: the prototype of `Int8Array.prototype`.
fn typed_array_proto(it: &Interp) -> Option<Obj> {
    let Some(Value::Obj(c)) = it.global.borrow().vars.get("Int8Array").cloned() else {
        return None;
    };
    let Some(Value::Obj(p)) = c.borrow().props.get("prototype").cloned() else {
        return None;
    };
    p.borrow().proto.clone()
}

fn arg(a: &[Value], i: usize) -> Value {
    a.get(i).cloned().unwrap_or_default()
}

use crate::builtins::native;

fn nat(o: &Obj, name: &'static str, length: u32, f: NativeFn) {
    set_hidden(o, name, native(name, length, f));
}

fn nat_sym(o: &Obj, w: WellKnown, name: &'static str, length: u32, f: NativeFn) -> Value {
    let f = native(name, length, f);
    define_sym(o, &w.sym(), f.clone(), false);
    f
}

/// SpeciesConstructor (ECMA-262 §7.3.22) of `o`: `None` for the caller's default
/// constructor (no `constructor`, or one without a `@@species`).
pub fn species_constructor(it: &mut Interp, o: &Value) -> R<Option<Value>> {
    let c = it.get(o, "constructor")?;
    match c {
        Value::Undefined => return Ok(None),
        Value::Obj(_) => {}
        _ => return it.throw("TypeError", "Type error"),
    }
    match it.get_sym(&c, &WellKnown::Species.sym())? {
        Value::Undefined | Value::Null => Ok(None),
        s if crate::proxy::is_constructor(&s) => Ok(Some(s)),
        _ => it.throw("TypeError", "Type error"),
    }
}

/// CreateIterResultObject (ECMA-262 §7.4.14).
pub fn iter_result(value: Value, done: bool) -> Value {
    let o = new_obj();
    crate::eval::set_own(&o, "value", value);
    crate::eval::set_own(&o, "done", Value::Bool(done));
    Value::Obj(o)
}

/// A built-in iterator over `src` (live array-like) or `items` (a snapshot).
pub fn list_iter(proto: &Obj, src: Value, items: Vec<Value>, kind: IterKind) -> Value {
    let o = new_obj();
    {
        let mut b = o.borrow_mut();
        b.proto = Some(proto.clone());
        b.internal = Internal::Iter(Box::new(ListIter {
            src,
            items,
            kind,
            pos: 0,
            done: false,
        }));
    }
    Value::Obj(o)
}

/// %ArrayIteratorPrototype%.next and kin (ECMA-262 §23.1.5.2.1).
fn iter_next(it: &mut Interp, this: &Value, _: &[Value]) -> R<Value> {
    let Value::Obj(o) = this else {
        return it.throw("TypeError", "Not an Array Iterator instance");
    };
    let (src, kind, pos) = match &mut o.borrow_mut().internal {
        Internal::Iter(l) => {
            if l.done {
                return Ok(iter_result(Value::Undefined, true));
            }
            if matches!(l.src, Value::Undefined) {
                let v = l.items.get(l.pos).cloned();
                l.pos += 1;
                if v.is_none() {
                    l.done = true;
                    l.items = Vec::new();
                }
                return Ok(match v {
                    Some(v) => iter_result(v, false),
                    None => iter_result(Value::Undefined, true),
                });
            }
            (l.src.clone(), l.kind, l.pos)
        }
        _ => return it.throw("TypeError", "Not an Array Iterator instance"),
    };
    let len = it.get(&src, "length")?;
    let len = it.to_numeric(&len)?;
    if !(pos as f64).lt(&len) {
        if let Internal::Iter(l) = &mut o.borrow_mut().internal {
            l.done = true;
            l.src = Value::Undefined;
        }
        return Ok(iter_result(Value::Undefined, true));
    }
    if let Internal::Iter(l) = &mut o.borrow_mut().internal {
        l.pos += 1;
    }
    let index = Value::Num(pos as f64);
    let v = match kind {
        IterKind::Keys => index,
        IterKind::Values => it.get(&src, &pos.to_string())?,
        IterKind::Entries => {
            let v = it.get(&src, &pos.to_string())?;
            Value::array(vec![index, v])
        }
    };
    Ok(iter_result(v, false))
}

fn this_symbol(it: &Interp, this: &Value) -> R<Rc<JsSymbol>> {
    // thisSymbolValue (ECMA-262 §20.4.3): a symbol or a Symbol object's [[SymbolData]].
    match this {
        Value::Symbol(s) => Ok(s.clone()),
        Value::Obj(o) if let Internal::Symbol(s) = &o.borrow().internal => Ok(s.clone()),
        _ => it.throw("TypeError", "Type error"),
    }
}

/// CreateArrayIterator (ECMA-262 §23.1.5.1) of `this` for `Array.prototype[name]`.
fn array_iter(it: &mut Interp, this: &Value, kind: IterKind) -> R<Value> {
    if matches!(this, Value::Undefined | Value::Null) {
        return it.no_object(this);
    }
    Ok(list_iter(
        &it.sym.array_iter_proto,
        this.clone(),
        Vec::new(),
        kind,
    ))
}

/// CreateArrayIterator (ECMA-262 §23.1.5.1) of typed array `this` for
/// `%TypedArray%.prototype.{keys, values, entries}` (§23.2.3.19, §23.2.3.37, §23.2.3.7).
fn typed_iter(it: &mut Interp, this: &Value, kind: IterKind) -> R<Value> {
    match this {
        Value::Obj(o) if crate::typedarray::typed(o).is_some() => Ok(list_iter(
            &it.sym.array_iter_proto,
            this.clone(),
            Vec::new(),
            kind,
        )),
        _ => it.throw("TypeError", "Type error"),
    }
}

/// Installs `Symbol`, the symbol-keyed built-ins of the other prototypes, `WeakMap` and
/// `WeakSet`, and `Object.getOwnPropertySymbols`; returns the globals to define.
pub fn install(it: &mut Interp) -> [(&'static str, Value); 3] {
    // Symbol (§20.4.1).
    let ctor = native("Symbol", 0, |it, _, a| {
        if std::mem::take(&mut it.constructing) {
            return it.throw("TypeError", "Symbol can't be used together with |new|.");
        }
        let d = match arg(a, 0) {
            Value::Undefined => None,
            v => Some(it.to_str_named(&v)?.to_js_string().into()),
        };
        Ok(Value::Symbol(JsSymbol::new(d)))
    });
    let Value::Obj(c) = &ctor else { unreachable!() };
    nat(c, "for", 1, |it, _, a| {
        let k = it.to_str_named(&arg(a, 0))?.to_js_string();
        Ok(Value::Symbol(symbol_for(&k)))
    });
    nat(c, "keyFor", 1, |it, _, a| match arg(a, 0) {
        Value::Symbol(s) if s.registered => Ok(Value::Str(s.description.clone().unwrap())),
        Value::Symbol(_) => Ok(Value::Undefined),
        _ => it.throw("TypeError", "Symbol.keyFor: Argument is not a symbol."),
    });
    for w in WellKnown::ALL {
        set_hidden(c, w.name(), Value::Symbol(w.sym()));
        // Non-writable and non-configurable (ECMA-262 §20.4.2).
        c.borrow_mut()
            .set_attr(w.name(), crate::value::READONLY | crate::value::PERMANENT);
    }
    let sp = it.sym.symbol_proto.clone();
    link_builtin_prototype(c, &sp);
    nat(&sp, "toString", 0, |it, this, _| {
        Ok(Value::str(&this_symbol(it, this)?.descriptive()))
    });
    nat(&sp, "valueOf", 0, |it, this, _| {
        Ok(Value::Symbol(this_symbol(it, this)?))
    });
    define_accessor(
        &sp,
        "description",
        true,
        native("get description", 0, |it, this, _| {
            Ok(match &this_symbol(it, this)?.description {
                Some(d) => Value::Str(d.clone()),
                None => Value::Undefined,
            })
        }),
        false,
    );
    nat_sym(
        &sp,
        WellKnown::ToPrimitive,
        "[Symbol.toPrimitive]",
        1,
        |it, this, _| Ok(Value::Symbol(this_symbol(it, this)?)),
    );
    crate::eval::define_tag(&sp, "Symbol");
    // Math[@@toStringTag] and JSON[@@toStringTag]: read-only, configurable (§21.3.1.9,
    // §25.5.3).
    for name in ["Math", "JSON", "Atomics"] {
        if let Some(Value::Obj(o)) = it.global.borrow().vars.get(name).cloned() {
            let s = WellKnown::ToStringTag.sym();
            crate::eval::write_sym(
                &o,
                &s,
                Value::str(name),
                None,
                false,
                crate::value::READONLY,
            );
        }
    }
    // The @@toStringTag of the keyed collections and the binary data objects (§24.1.3.13,
    // §24.2.3.12, §25.1.6.7, §25.2.5.7, §25.3.4.25).
    for name in ["ArrayBuffer", "SharedArrayBuffer", "DataView"] {
        let c = it.global.borrow().vars.get(name).cloned();
        if let Some(Value::Obj(c)) = c
            && let Some(Value::Obj(p)) = c.borrow().props.get("prototype").cloned()
        {
            crate::eval::define_tag(&p, name);
        }
    }
    // Promise.prototype[@@toStringTag] (§27.2.5.5).
    let promise = it.global.borrow().vars.get("Promise").cloned();
    if let Some(Value::Obj(c)) = promise
        && let Some(Value::Obj(pp)) = c.borrow().props.get("prototype").cloned()
    {
        crate::eval::define_tag(&pp, "Promise");
    }

    // `get [Symbol.species]` of the species-aware constructors returns its receiver (§23.1.2.5,
    // §25.1.5.3, §25.2.4.2, §24.1.2.2, §24.2.2.2, §27.2.4.8, §22.2.5.2, §23.2.2.4).
    let global = |k: &str| match it.global.borrow().vars.get(k) {
        Some(Value::Obj(c)) => Some(c.clone()),
        _ => None,
    };
    let typed_array = global("Int8Array").and_then(|c| c.borrow().proto.clone());
    let ctors = [
        "Array",
        "ArrayBuffer",
        "SharedArrayBuffer",
        "Map",
        "Set",
        "Promise",
        "RegExp",
    ]
    .map(global);
    let rp = it.regexp_proto.clone();
    it.sym.regexp_exec = rp.borrow().props.get("exec").cloned().unwrap_or_default();
    it.sym.builtin_methods = Rc::new(crate::builtins::builtin_methods(it));
    // The RegExp symbol methods String.prototype's match, matchAll, replace, search and split
    // dispatch to (§22.2.6.8-9, 11-12, 14).
    use crate::builtins as b;
    for (w, name, length, f) in [
        (
            WellKnown::Match,
            "[Symbol.match]",
            1,
            b::regexp_match as NativeFn,
        ),
        (
            WellKnown::MatchAll,
            "[Symbol.matchAll]",
            1,
            b::regexp_match_all,
        ),
        (WellKnown::Replace, "[Symbol.replace]", 2, b::regexp_replace),
        (WellKnown::Search, "[Symbol.search]", 1, b::regexp_search),
        (WellKnown::Split, "[Symbol.split]", 2, b::regexp_split),
    ] {
        nat_sym(&rp, w, name, length, f);
    }
    for c in ctors.into_iter().chain([typed_array]).flatten() {
        let get = native("get [Symbol.species]", 0, |_, this, _| Ok(this.clone()));
        define_sym_accessor(&c, &WellKnown::Species.sym(), true, get, false);
    }

    // The built-in iterators: %IteratorPrototype%[@@iterator] returns the iterator (§27.1.2.1);
    // each kind's prototype has its `next` and `@@toStringTag` (§23.1.5.2, §22.1.5.1).
    let ip = it.sym.iterator_proto.clone();
    nat_sym(
        &ip,
        WellKnown::Iterator,
        "[Symbol.iterator]",
        0,
        |_, this, _| Ok(this.clone()),
    );
    let rsp = it.sym.regexp_string_iter_proto.clone();
    rsp.borrow_mut().proto = Some(ip.clone());
    nat(&rsp, "next", 0, crate::builtins::regexp_string_iter_next);
    crate::eval::define_tag(&rsp, "RegExp String Iterator");
    for (p, tag) in [
        (it.sym.array_iter_proto.clone(), "Array Iterator"),
        (it.sym.string_iter_proto.clone(), "String Iterator"),
    ] {
        p.borrow_mut().proto = Some(ip.clone());
        nat(&p, "next", 0, iter_next);
        p.borrow_mut().builtin_next = true;
        crate::eval::define_tag(&p, tag);
    }
    // %GeneratorPrototype% inherits %IteratorPrototype% (§27.5.1).
    let gp = it.generator_protos.generator.clone();
    gp.borrow_mut().proto = Some(ip.clone());
    // Their @@toStringTag is read-only (§27.3.3.3, §27.5.1.5).
    let tag = |o: &Obj, t: &str| {
        let s = WellKnown::ToStringTag.sym();
        crate::eval::write_sym(o, &s, Value::str(t), None, false, crate::value::READONLY);
    };
    tag(&gp, "Generator");
    tag(&it.generator_protos.function, "GeneratorFunction");
    // Array.prototype.{keys, values, entries} (§23.1.3.18, §23.1.3.40, §23.1.3.7).
    let ap = it.array_proto.clone();
    it.sym.array_values = nat_sym(&ap, WellKnown::Iterator, "values", 0, |it, this, _| {
        array_iter(it, this, IterKind::Values)
    });
    // %Array.prototype.values% is the initial `@@iterator` (§23.1.3.40).
    crate::eval::set_hidden(&ap, "values", it.sym.array_values.clone());
    // Array.prototype[@@unscopables] (§23.1.3.38) with the names V4 lists.
    let blocked = crate::eval::new_obj();
    blocked.borrow_mut().null_proto = true;
    for k in [
        "copyWithin",
        "entries",
        "fill",
        "find",
        "findIndex",
        "includes",
        "keys",
        "values",
    ] {
        crate::eval::set_own(&blocked, k, Value::Bool(true));
    }
    crate::eval::write_sym(
        &ap,
        &WellKnown::Unscopables.sym(),
        Value::Obj(blocked),
        None,
        false,
        crate::value::READONLY | crate::value::PERMANENT,
    );
    // %URLSearchParamsIteratorPrototype% and URLSearchParams.prototype[@@iterator], which is
    // its `entries` (WebIDL §3.7.9, §3.7.10.2).
    if let Some(Value::Obj(c)) = it.global.borrow().vars.get("URLSearchParams").cloned()
        && let Some(Value::Obj(pp)) = c.borrow().props.get("prototype").cloned()
    {
        let ipp = it.sym.params_iter_proto.clone();
        ipp.borrow_mut().proto = Some(ip.clone());
        nat(&ipp, "next", 0, crate::url::params_iter_next);
        ipp.borrow_mut().builtin_next = true;
        crate::eval::define_tag(&ipp, "URLSearchParams Iterator");
        let entries = pp
            .borrow()
            .props
            .get("entries")
            .cloned()
            .unwrap_or_default();
        define_sym(&pp, &WellKnown::Iterator.sym(), entries.clone(), false);
        it.sym.params_iterator = entries;
    }
    nat(&ap, "keys", 0, |it, this, _| {
        array_iter(it, this, IterKind::Keys)
    });
    nat(&ap, "entries", 0, |it, this, _| {
        array_iter(it, this, IterKind::Entries)
    });

    // %TypedArray%.prototype.values, its initial `@@iterator` (§23.2.3.37, §23.2.3.38).
    if let Some(tp) = typed_array_proto(it) {
        it.sym.typed_values = nat_sym(&tp, WellKnown::Iterator, "values", 0, |it, this, _| {
            typed_iter(it, this, IterKind::Values)
        });
        crate::eval::set_hidden(&tp, "values", it.sym.typed_values.clone());
        nat(&tp, "keys", 0, |it, this, _| {
            typed_iter(it, this, IterKind::Keys)
        });
        nat(&tp, "entries", 0, |it, this, _| {
            typed_iter(it, this, IterKind::Entries)
        });
    }

    // Map and Set iterators (§24.1.5, §24.2.5): `Map.prototype[@@iterator]` is `entries`,
    // `Set.prototype[@@iterator]` and `keys` are `values` (§24.1.3.12, §24.2.3.11, §24.2.3.8).
    use crate::builtins::{coll_iter, coll_iter_next};
    let mp = it.map_proto.clone();
    it.sym.map_entries = nat_sym(&mp, WellKnown::Iterator, "entries", 0, |it, this, _| {
        coll_iter(it, this, true, IterKind::Entries)
    });
    crate::eval::set_hidden(&mp, "entries", it.sym.map_entries.clone());
    nat(&mp, "keys", 0, |it, this, _| {
        coll_iter(it, this, true, IterKind::Keys)
    });
    nat(&mp, "values", 0, |it, this, _| {
        coll_iter(it, this, true, IterKind::Values)
    });
    let sp = it.set_proto.clone();
    it.sym.set_values = nat_sym(&sp, WellKnown::Iterator, "values", 0, |it, this, _| {
        coll_iter(it, this, false, IterKind::Values)
    });
    crate::eval::set_hidden(&sp, "values", it.sym.set_values.clone());
    crate::eval::set_hidden(&sp, "keys", it.sym.set_values.clone());
    nat(&sp, "entries", 0, |it, this, _| {
        coll_iter(it, this, false, IterKind::Entries)
    });
    // The @@toStringTag of the keyed collections (§24.1.3.13, §24.2.3.12), after
    // `@@iterator` as V4 lists them.
    crate::eval::define_tag(&mp, "Map");
    crate::eval::define_tag(&sp, "Set");
    for (p, tag, next) in [
        (
            it.sym.map_iter_proto.clone(),
            "Map Iterator",
            coll_iter_next::<true> as NativeFn,
        ),
        (
            it.sym.set_iter_proto.clone(),
            "Set Iterator",
            coll_iter_next::<false>,
        ),
    ] {
        p.borrow_mut().proto = Some(ip.clone());
        nat(&p, "next", 0, next);
        p.borrow_mut().builtin_next = true;
        crate::eval::define_tag(&p, tag);
    }
    let strp = it.string_proto.clone();
    it.sym.string_iterator = nat_sym(
        &strp,
        WellKnown::Iterator,
        "[Symbol.iterator]",
        0,
        |it, this, _| {
            if matches!(this, Value::Undefined | Value::Null) {
                return it.throw("TypeError", "Type error");
            }
            let s = it.to_string(this)?;
            let items = s
                .chars()
                .map(|c| Value::str(c.encode_utf8(&mut [0; 4])))
                .collect();
            Ok(list_iter(
                &it.sym.string_iter_proto,
                Value::Undefined,
                items,
                IterKind::Values,
            ))
        },
    );

    // Function.prototype[@@hasInstance] (§20.2.3.6).
    let fp = it.function_proto.clone();
    it.sym.has_instance = nat_sym(
        &fp,
        WellKnown::HasInstance,
        "[Symbol.hasInstance]",
        1,
        |it, this, a| Ok(Value::Bool(it.ordinary_has_instance(this, &arg(a, 0))?)),
    );

    // Object.getOwnPropertySymbols (§20.1.2.11).
    if let Some(Value::Obj(object)) = it.global.borrow().vars.get("Object").cloned() {
        nat(&object, "getOwnPropertySymbols", 1, |it, _, a| {
            match arg(a, 0) {
                v @ (Value::Undefined | Value::Null) => it.no_object(&v),
                v @ Value::Obj(_) => Ok(Value::array(
                    crate::props::own_property_keys(it, &v)?
                        .into_iter()
                        .filter_map(|k| match k {
                            crate::eval::PropKey::Sym(s) => Some(Value::Symbol(s)),
                            crate::eval::PropKey::Str(_) => None,
                        })
                        .collect(),
                )),
                _ => Ok(Value::array(Vec::new())),
            }
        });
    }

    let weak_map = weak_ctor(it, true);
    let weak_set = weak_ctor(it, false);
    [
        ("Symbol", ctor),
        ("WeakMap", weak_map),
        ("WeakSet", weak_set),
    ]
}

fn weak_ctor(it: &mut Interp, is_map: bool) -> Value {
    let (ctor, proto) = if is_map {
        (
            native("WeakMap", 0, |it, _, a| new_weak(it, true, &arg(a, 0))),
            it.sym.weak_map_proto.clone(),
        )
    } else {
        (
            native("WeakSet", 0, |it, _, a| new_weak(it, false, &arg(a, 0))),
            it.sym.weak_set_proto.clone(),
        )
    };
    let Value::Obj(c) = &ctor else { unreachable!() };
    link_builtin_prototype(c, &proto);
    if is_map {
        nat(&proto, "set", 2, |it, this, a| {
            let o = this_weak(it, this, true)?;
            weak_set(it, &o, &arg(a, 0), arg(a, 1))?;
            Ok(this.clone())
        });
        nat(&proto, "get", 1, |it, this, a| {
            let o = this_weak(it, this, true)?;
            Ok(weak_get(&o, &arg(a, 0)).unwrap_or_default())
        });
    } else {
        nat(&proto, "add", 1, |it, this, a| {
            let o = this_weak(it, this, false)?;
            weak_set(it, &o, &arg(a, 0), Value::Undefined)?;
            Ok(this.clone())
        });
    }
    nat(&proto, "has", 1, |it, this, a| {
        let o = this_weak_any(it, this)?;
        Ok(Value::Bool(weak_get(&o, &arg(a, 0)).is_some()))
    });
    nat(&proto, "delete", 1, |it, this, a| {
        let o = this_weak_any(it, this)?;
        Ok(Value::Bool(weak_delete(&o, &arg(a, 0))))
    });
    let tag = if is_map { "WeakMap" } else { "WeakSet" };
    crate::eval::define_tag(&proto, tag);
    ctor
}

/// The WeakMap/WeakSet constructor (§24.3.1.1, §24.4.1.1).
fn new_weak(it: &mut Interp, is_map: bool, init: &Value) -> R<Value> {
    if !std::mem::take(&mut it.constructing) {
        let what = if is_map { "(Weak)Map" } else { "Set" };
        return it.throw("TypeError", format!("{what} requires new"));
    }
    let o = new_obj();
    {
        let mut b = o.borrow_mut();
        let c = Box::new(WeakColl {
            id: Rc::as_ptr(&o) as usize,
            keys: Vec::new(),
            others: Vec::new(),
        });
        b.internal = if is_map {
            b.proto = Some(it.sym.weak_map_proto.clone());
            Internal::WeakMap(c)
        } else {
            b.proto = Some(it.sym.weak_set_proto.clone());
            Internal::WeakSet(c)
        };
    }
    let v = Value::Obj(o.clone());
    if matches!(init, Value::Undefined | Value::Null) {
        return Ok(v);
    }
    let adder = it.get(&v, if is_map { "set" } else { "add" })?;
    if callable(&adder).is_none() {
        return it.throw("TypeError", "Type error");
    }
    let mut src = it.get_iterator(init)?;
    while let Some(x) = it.iter_step(&mut src)? {
        let r = if is_map {
            if !matches!(x, Value::Obj(_)) {
                let Err(e) = it.throw::<()>("TypeError", "Type error") else {
                    unreachable!()
                };
                return Err(it.iter_close_throw(&mut src, e));
            }
            match (it.get(&x, "0"), it.get(&x, "1")) {
                (Ok(k), Ok(val)) => it.call(&adder, v.clone(), &[k, val]),
                (Err(e), _) | (_, Err(e)) => Err(e),
            }
        } else {
            it.call(&adder, v.clone(), &[x])
        };
        if let Err(e) = r {
            return Err(it.iter_close_throw(&mut src, e));
        }
    }
    Ok(v)
}

fn this_weak(it: &Interp, this: &Value, is_map: bool) -> R<Obj> {
    if let Value::Obj(o) = this {
        let ok = match &o.borrow().internal {
            Internal::WeakMap(_) => is_map,
            Internal::WeakSet(_) => !is_map,
            _ => false,
        };
        if ok {
            return Ok(o.clone());
        }
    }
    it.type_error()
}

fn this_weak_any(it: &Interp, this: &Value) -> R<Obj> {
    if let Value::Obj(o) = this
        && let Internal::WeakMap(_) | Internal::WeakSet(_) = o.borrow().internal
    {
        return Ok(o.clone());
    }
    it.throw("TypeError", "Type error")
}

fn coll<R>(o: &Obj, f: impl FnOnce(&mut WeakColl) -> R) -> R {
    match &mut o.borrow_mut().internal {
        Internal::WeakMap(c) | Internal::WeakSet(c) => f(c),
        _ => unreachable!("checked by this_weak"),
    }
}

/// Whether `k` can be held weakly (CanBeHeldWeakly, §9.13 as of ES2023 minus symbols,
/// which V4 rejects): objects, and QML objects, which the JS heap does not own.
fn weak_key(k: &Value) -> bool {
    matches!(k, Value::Obj(_) | Value::Item(_) | Value::Color(_))
}

/// The object a weak collection finds by identity; a QML object and a value of a value type
/// (equal when its fields are) are found by comparing.
fn weak_obj(k: &Value) -> Option<&Obj> {
    match k {
        Value::Obj(o) if !crate::valuetypes::is_value(k) => Some(o),
        _ => None,
    }
}

fn weak_set(it: &Interp, o: &Obj, k: &Value, v: Value) -> R<()> {
    if !weak_key(k) {
        return it.throw("TypeError", "Type error");
    }
    let Some(key) = weak_obj(k) else {
        coll(o, |c| {
            match c.others.iter_mut().find(|(x, _)| x.strict_eq(k)) {
                Some(e) => e.1 = v,
                None => c.others.push((k.clone(), v)),
            }
        });
        return Ok(());
    };
    let id = Rc::as_ptr(o) as usize;
    let fresh = {
        let mut kb = key.borrow_mut();
        let e = kb.ext.get_or_insert_default();
        match e.weak.iter_mut().find(|(m, _)| m.as_ptr() as usize == id) {
            Some(entry) => {
                entry.1 = v;
                false
            }
            None => {
                e.weak.push((Rc::downgrade(o), v));
                true
            }
        }
    };
    if fresh {
        coll(o, |c| {
            if c.keys.len().is_power_of_two() {
                c.keys.retain(|k| k.strong_count() > 0);
            }
            c.keys.push(Rc::downgrade(key));
        });
    }
    Ok(())
}

fn weak_get(o: &Obj, k: &Value) -> Option<Value> {
    match k {
        Value::Obj(key) if weak_obj(k).is_some() => {
            let id = Rc::as_ptr(o) as usize;
            let kb = key.borrow();
            kb.ext
                .as_deref()?
                .weak
                .iter()
                .find(|(m, _)| m.as_ptr() as usize == id)
                .map(|(_, v)| v.clone())
        }
        Value::Item(_) | Value::Obj(_) | Value::Color(_) => coll(o, |c| {
            c.others
                .iter()
                .find(|(x, _)| x.strict_eq(k))
                .map(|(_, v)| v.clone())
        }),
        _ => None,
    }
}

fn weak_delete(o: &Obj, k: &Value) -> bool {
    match k {
        Value::Obj(key) if weak_obj(k).is_some() => {
            let id = Rc::as_ptr(o) as usize;
            let removed = {
                let mut kb = key.borrow_mut();
                kb.ext.as_deref_mut().and_then(|e| {
                    let i = e.weak.iter().position(|(m, _)| m.as_ptr() as usize == id)?;
                    Some(e.weak.remove(i))
                })
            };
            let found = removed.is_some();
            drop(removed);
            if found {
                let kp = Rc::as_ptr(key);
                coll(o, |c| c.keys.retain(|w| w.as_ptr() != kp));
            }
            found
        }
        Value::Item(_) | Value::Obj(_) | Value::Color(_) => {
            let removed = coll(o, |c| {
                let i = c.others.iter().position(|(x, _)| x.strict_eq(k))?;
                Some(c.others.remove(i))
            });
            removed.is_some()
        }
        _ => false,
    }
}

#[cfg(all(test, feature = "interp"))]
mod tests {
    use std::rc::Rc;

    use crate::eval::{Ex, Interp};
    use crate::value::Value;

    /// `(name, function body, String(result) or "THROWS <error name>")`, pinned from Qt 6.12
    /// (PySide6 6.12.0) unless noted.
    const CASES: &[(&str, &str, &str)] = &[
        (r#"typeof_ctor"#, r#"return typeof Symbol"#, r#"function"#),
        (r#"typeof_value"#, r#"return typeof Symbol()"#, r#"symbol"#),
        (
            r#"string_explicit"#,
            r#"return String(Symbol('a')) + String(Symbol())"#,
            r#"Symbol(a)Symbol()"#,
        ),
        (
            r#"to_string_method"#,
            r#"return Symbol('x').toString()"#,
            r#"Symbol(x)"#,
        ),
        (
            r#"concat_throws"#,
            r#"return '' + Symbol('a')"#,
            r#"THROWS TypeError"#,
        ),
        (
            r#"template_throws"#,
            r#"return `${Symbol('a')}`"#,
            r#"THROWS TypeError"#,
        ),
        (
            r#"number_throws"#,
            r#"return +Symbol('a')"#,
            r#"THROWS TypeError"#,
        ),
        (
            r#"new_throws"#,
            r#"return new Symbol()"#,
            r#"THROWS TypeError"#,
        ),
        (
            r#"identity"#,
            r#"var s = Symbol('a'); return [s === s, Symbol('a') === Symbol('a'), s == s].join()"#,
            r#"true,false,true"#,
        ),
        (
            r#"registry"#,
            r#"return [Symbol.for('x') === Symbol.for('x'), Symbol.keyFor(Symbol.for('x')), Symbol.keyFor(Symbol('x'))].join()"#,
            r#"true,x,"#,
        ),
        (
            r#"key_for_throws"#,
            r#"return Symbol.keyFor('x')"#,
            r#"THROWS TypeError"#,
        ),
        (
            r#"well_known"#,
            r#"return [typeof Symbol.iterator, typeof Symbol.toPrimitive, typeof Symbol.toStringTag, typeof Symbol.hasInstance, Symbol.iterator.toString()].join()"#,
            r#"symbol,symbol,symbol,symbol,Symbol(Symbol.iterator)"#,
        ),
        (
            r#"truthy"#,
            r#"return !!Symbol() && Boolean(Symbol())"#,
            r#"true"#,
        ),
        (
            r#"tags"#,
            r#"return [Object.prototype.toString.call(Symbol()), Object.prototype.toString.call(new WeakMap()), Object.prototype.toString.call(new WeakSet()), Object.prototype.toString.call([][Symbol.iterator]()), Object.prototype.toString.call(''[Symbol.iterator]())].join()"#,
            r#"[object Symbol],[object WeakMap],[object WeakSet],[object Array Iterator],[object String Iterator]"#,
        ),
        (
            r#"sym_props"#,
            r#"var s = Symbol('k'); var o = {a: 1}; o[s] = 2; return [o[s], Object.keys(o).join(), JSON.stringify(o), s in o, Object.getOwnPropertySymbols(o).length, Object.getOwnPropertySymbols(o)[0] === s].join()"#,
            r#"2,a,{"a":1},true,1,true"#,
        ),
        (
            r#"sym_for_in"#,
            r#"var s = Symbol(); var o = {a: 1, [s]: 2}; var ks = []; for (var k in o) ks.push(k); return ks.join() + o[s]"#,
            r#"a2"#,
        ),
        (
            r#"sym_order"#,
            r#"var a = Symbol('a'), b = Symbol('b'); var o = {}; o[b] = 1; o[a] = 2; return Object.getOwnPropertySymbols(o).map(String).join()"#,
            r#"Symbol(b),Symbol(a)"#,
        ),
        (
            r#"sym_delete"#,
            r#"var s = Symbol(); var o = {}; o[s] = 1; delete o[s]; return [s in o, o[s], Object.getOwnPropertySymbols(o).length].join()"#,
            r#"false,,0"#,
        ),
        (
            r#"sym_inherit"#,
            r#"var s = Symbol(); var p = {}; p[s] = 7; var o = Object.create(p); return [o[s], s in o, o.hasOwnProperty(s), p.hasOwnProperty(s)].join()"#,
            r#"7,true,false,true"#,
        ),
        (
            r#"sym_getter"#,
            r#"var s = Symbol(); var o = { get [s]() { return 5 } }; return o[s]"#,
            r#"5"#,
        ),
        (
            r#"sym_class"#,
            r#"var s = Symbol(); class A { [s]() { return 'm' } static [s]() { return 'st' } } return new A()[s]() + A[s]()"#,
            r#"mst"#,
        ),
        (
            r#"sym_compound"#,
            r#"var s = Symbol(); var o = {}; o[s] = 1; o[s] += 2; o[s]++; return o[s]"#,
            r#"4"#,
        ),
        (
            r#"sym_method_call"#,
            r#"var s = Symbol(); var o = {v: 3}; o[s] = function () { return this.v }; return o[s]()"#,
            r#"3"#,
        ),
        (
            r#"sym_destructure"#,
            r#"var s = Symbol(); var o = {[s]: 9}; var {[s]: x} = o; return x"#,
            r#"9"#,
        ),
        (
            r#"iter_custom"#,
            r#"var it = {[Symbol.iterator]() { var i = 0; return {next() { return i < 3 ? {value: i++, done: false} : {done: true} }} }}; var a = []; for (var x of it) a.push(x); return a.join() + '|' + [...it].join() + '|' + Array.from(it).join()"#,
            r#"0,1,2|0,1,2|0,1,2"#,
        ),
        (
            r#"iter_destructure"#,
            r#"var it = {[Symbol.iterator]() { var i = 0; return {next() { return {value: i++, done: false} }} }}; var [a, b] = it; return a + ',' + b"#,
            r#"0,1"#,
        ),
        (
            r#"iter_close_break"#,
            r#"var log = []; var it = {[Symbol.iterator]() { return {next() { log.push('n'); return {value: 1, done: false} }, return() { log.push('r'); return {} }} }}; for (var x of it) { if (log.length > 2) break } return log.join()"#,
            r#"n,n,n,r"#,
        ),
        (
            r#"iter_close_throw"#,
            r#"var log = []; var it = {[Symbol.iterator]() { return {next() { return {value: 1, done: false} }, return() { log.push('r'); throw new Error('ignored') }} }}; try { for (var x of it) throw new TypeError('t') } catch (e) { log.push(e.name) } return log.join()"#,
            r#"r,TypeError"#,
        ),
        (
            r#"iter_close_return"#,
            r#"var log = []; var it = {[Symbol.iterator]() { return {next() { return {value: 1, done: false} }, return() { log.push('r'); return {} }} }}; function f() { for (var x of it) return 'x' } return f() + log.join()"#,
            r#"xr"#,
        ),
        (
            r#"iter_close_destructure"#,
            r#"var log = []; var it = {[Symbol.iterator]() { return {next() { return {value: 1, done: false} }, return() { log.push('r'); return {} }} }}; var [a] = it; return log.join()"#,
            r#"r"#,
        ),
        (
            r#"iter_no_close_when_done"#,
            r#"var log = []; var it = {[Symbol.iterator]() { var d = false; return {next() { var r = {value: 1, done: d}; d = true; return r }, return() { log.push('r'); return {} }} }}; var [a, b] = it; return log.join() + a + b"#,
            r#"1undefined"#,
        ),
        (
            r#"iter_return_nonobject"#,
            r#"var it = {[Symbol.iterator]() { return {next() { return {value: 1, done: false} }, return() { return 1 }} }}; for (var x of it) break; return 'ok'"#,
            r#"THROWS TypeError"#,
        ),
        (
            r#"not_iterable"#,
            r#"for (var x of {}) {} return 1"#,
            r#"THROWS TypeError"#,
        ),
        (
            r#"iter_result_nonobject"#,
            r#"var it = {[Symbol.iterator]() { return {next() { return 1 }} }}; for (var x of it) {} return 1"#,
            r#"THROWS TypeError"#,
        ),
        (
            r#"array_iter"#,
            r#"var i = [1, 2][Symbol.iterator](); var a = i.next(), b = i.next(), c = i.next(); return [a.value, a.done, b.value, c.value, c.done, i[Symbol.iterator]() === i].join()"#,
            r#"1,false,2,,true,true"#,
        ),
        (
            r#"string_iter"#,
            r#"return [...'ab'].join('-') + [...''[Symbol.iterator]()].length"#,
            r#"a-b0"#,
        ),
        (
            r#"array_iter_identity"#,
            r#"return [][Symbol.iterator] === [].values"#,
            r#"true"#,
        ),
        (
            r#"map_from_iter"#,
            r#"var it = {[Symbol.iterator]() { var i = 0; return {next() { return i < 2 ? {value: [i, i * 10], done: false} : {done: true}; i++ }} }}; var m = new Map([[1, 'a'], [2, 'b']]); return m.size + m.get(2)"#,
            r#"2b"#,
        ),
        // As V4: a template literal converts with hint "default"; ECMA-262 §13.15.5.1 says "string".
        (
            r#"to_primitive"#,
            r#"var o = {[Symbol.toPrimitive](h) { return h === 'number' ? 42 : 's:' + h }}; return [+o, '' + o, `${o}`, o * 2, String(o)].join()"#,
            r#"42,s:default,s:default,84,s:string"#,
        ),
        (
            r#"to_primitive_object_throws"#,
            r#"var o = {[Symbol.toPrimitive]() { return {} }}; return +o"#,
            r#"THROWS TypeError"#,
        ),
        (
            r#"to_string_tag"#,
            r#"var o = {[Symbol.toStringTag]: 'Foo'}; var p = {[Symbol.toStringTag]: 1}; return Object.prototype.toString.call(o) + String(o) + Object.prototype.toString.call(p)"#,
            r#"[object Foo][object Foo][object Object]"#,
        ),
        (
            r#"has_instance"#,
            r#"var Even = {[Symbol.hasInstance](n) { return n % 2 === 0 }}; return [2 instanceof Even, 3 instanceof Even].join()"#,
            r#"true,false"#,
        ),
        // Spec over Qt: V4 has no Function.prototype[@@hasInstance] (ECMA-262 §20.2.3.6).
        (
            r#"has_instance_fn"#,
            r#"function F() {} return [new F() instanceof F, Function.prototype[Symbol.hasInstance].call(F, new F()), Function.prototype[Symbol.hasInstance].call({}, {})].join()"#,
            r#"true,true,false"#,
        ),
        (
            r#"has_instance_noncallable"#,
            r#"var o = {[Symbol.hasInstance]: 1}; return 1 instanceof o"#,
            r#"THROWS TypeError"#,
        ),
        // Spec over Qt: V4 has no Symbol.prototype.description (ECMA-262 §20.4.3.2).
        (
            r#"description"#,
            r#"return [Symbol('d').description, Symbol().description, Symbol('').description].join('|')"#,
            r#"d||"#,
        ),
        (
            r#"sym_json"#,
            r#"return JSON.stringify({a: Symbol(), b: 1}) + JSON.stringify([Symbol()]) + JSON.stringify(Symbol())"#,
            r#"{"b":1}[null]undefined"#,
        ),
        (
            r#"sym_value_of"#,
            r#"var s = Symbol(); return s.valueOf() === s"#,
            r#"true"#,
        ),
        (
            r#"symbol_with_symbol_desc"#,
            r#"return Symbol(Symbol())"#,
            r#"THROWS TypeError"#,
        ),
        (
            r#"wm_basic"#,
            r#"var wm = new WeakMap(); var k = {}, f = function () {}; return [wm.set(k, 1) === wm, wm.get(k), wm.has(k), wm.has({}), wm.delete(k), wm.has(k), wm.delete(k), wm.set(f, 2).get(f), wm.get(1)].join()"#,
            r#"true,1,true,false,true,false,false,2,"#,
        ),
        (
            r#"wm_bad_key"#,
            r#"return new WeakMap().set(1, 1)"#,
            r#"THROWS TypeError"#,
        ),
        (
            r#"wm_sym_key"#,
            r#"return new WeakMap().set(Symbol(), 1)"#,
            r#"THROWS TypeError"#,
        ),
        (r#"wm_no_new"#, r#"return WeakMap()"#, r#"THROWS TypeError"#),
        (
            r#"wm_init"#,
            r#"var a = {}, b = {}; var wm = new WeakMap([[a, 1], [b, 2]]); return wm.get(a) + wm.get(b)"#,
            r#"3"#,
        ),
        (
            r#"wm_two_maps"#,
            r#"var k = {}; var m1 = new WeakMap(), m2 = new WeakMap(); m1.set(k, 1); m2.set(k, 2); m1.delete(k); return [m1.has(k), m2.get(k)].join()"#,
            r#"false,2"#,
        ),
        (
            r#"ws_basic"#,
            r#"var ws = new WeakSet(); var k = {}; return [ws.add(k) === ws, ws.has(k), ws.has({}), ws.delete(k), ws.has(k)].join()"#,
            r#"true,true,false,true,false"#,
        ),
        (
            r#"ws_bad"#,
            r#"return new WeakSet().add('x')"#,
            r#"THROWS TypeError"#,
        ),
        (
            r#"ws_init"#,
            r#"var a = {}; return new WeakSet([a]).has(a)"#,
            r#"true"#,
        ),
        (
            r#"weak_absent"#,
            r#"return [typeof WeakRef, typeof FinalizationRegistry].join()"#,
            r#"undefined,undefined"#,
        ),
        (
            r#"weak_proto"#,
            r#"return [typeof WeakMap.prototype.get, WeakMap.prototype.constructor === WeakMap, new WeakMap() instanceof WeakMap, 'size' in new WeakMap()].join()"#,
            r#"function,true,true,false"#,
        ),
        (
            r#"coll_sym_key"#,
            r#"var s = Symbol(); var m = new Map([[s, 1]]); var st = new Set([s, s]); return m.get(s) + ',' + st.size"#,
            r#"1,1"#,
        ),
        (
            r#"symbol_object"#,
            r#"var s = Symbol("a"), w = Object(s); return [typeof w, w == s, w === s, w.description, String(w.valueOf() === s), Object.prototype.toString.call(w)].join()"#,
            r#"object,true,false,a,true,[object Symbol]"#,
        ),
        (
            r#"fn_length_name"#,
            r#"var r = []; function L(f) { r.push(f.length + ":" + f.name) } L(Symbol); L(Symbol.for); L(Symbol.keyFor); L(Symbol.prototype[Symbol.toPrimitive]); L(WeakMap); L(WeakMap.prototype.set); L(WeakSet.prototype.add); L(Object.getOwnPropertySymbols); L(Function.prototype[Symbol.hasInstance]); L(Array.prototype[Symbol.iterator]); L(String.prototype[Symbol.iterator]); L([][Symbol.iterator]().next); return r.join(" ")"#,
            r#"0:Symbol 1:for 1:keyFor 1:[Symbol.toPrimitive] 0:WeakMap 2:set 1:add 1:getOwnPropertySymbols 1:[Symbol.hasInstance] 0:values 0:[Symbol.iterator] 0:next"#,
        ),
        // Spec over Qt: V4 cannot parse object spread; CopyDataProperties copies symbol keys (ECMA-262 §7.3.25).
        (
            r#"sym_spread_assign"#,
            r#"var s = Symbol(); var o = {[s]: 1}; var c = {...o}; return c[s]"#,
            r#"1"#,
        ),
    ];

    #[test]
    fn symbol_and_weak_collection_cases_match_qt() {
        let mut bad = Vec::new();
        for (name, body, want) in CASES {
            let src = format!(
                "try {{ return String((function () {{ {body} }})()) }} catch (e) {{ return 'THROWS ' + e.name }}"
            );
            let mut it = Interp::default();
            let g = it.global.clone();
            let got = match it.run(&crate::ir::lower_program(&src).unwrap(), &g) {
                Ok(v) => v.to_js_string(),
                Err(Ex::Throw(e)) => format!("UNCAUGHT {}", e.to_js_string()),
                Err(Ex::Short) => "SHORT".into(),
            };
            if got != *want {
                bad.push(format!("{name}: got {got:?}, want {want:?}"));
            }
        }
        assert!(bad.is_empty(), "{}", bad.join("\n"));
    }

    /// A weak collection must not keep its keys alive: a key in a cycle with its own entry's
    /// value is garbage once nothing else holds it (ECMA-262 §24.3, WeakMap objects).
    #[test]
    fn weak_collection_keys_are_collectable() {
        let mut it = Interp::default();
        let g = it.global.clone();
        let mut run = |src: &str| it.run(&crate::ir::lower_program(src).unwrap(), &g).unwrap();
        let Value::Obj(k) = run("var m = new WeakMap(), s = new WeakSet(); \
            function make() { var k = {}; k.self = k; m.set(k, {back: k}); s.add(k); return k } \
            var held = make(); return held")
        else {
            panic!("key is not an object")
        };
        let weak = Rc::downgrade(&k);
        drop(k);
        let probe = run("return m.has(held) && s.has(held) && m.get(held).back === held");
        assert_eq!(probe.to_js_string(), "true");
        run("held = null");
        crate::eval::collect_cycles();
        assert!(
            weak.upgrade().is_none(),
            "the weak collections kept their key alive"
        );
        let v = run("var k = {}; m.set(k, 1); s.add(k); return m.get(k) + ',' + s.has(k)");
        assert_eq!(v.to_js_string(), "1,true");
    }

    /// An interpreter over shared globals (a `.pragma library`) has the realm's intrinsics: its
    /// built-in iteration is native, and a patched `next` is called through the protocol.
    #[test]
    fn shared_realm_iterates_through_the_realms_iterator_prototypes() {
        let it = Interp::default();
        let mut lib = Interp::for_library(&it.realm());
        assert!(Rc::ptr_eq(
            &lib.sym.array_iter_proto,
            &it.sym.array_iter_proto
        ));
        let g = lib.global.clone();
        let mut run = |src: &str| {
            let body = crate::ir::lower_program(src).unwrap();
            match lib.run(&body, &g) {
                Ok(v) => v.to_js_string(),
                Err(Ex::Throw(e)) => panic!("threw {}", e.to_js_string()),
                Err(Ex::Short) => panic!("short"),
            }
        };
        assert_eq!(
            run("return [...[1, 2], ...'ab', ...new Set([3])].join()"),
            "1,2,a,b,3"
        );
        assert_eq!(
            run("var P = Object.getPrototypeOf([].values()), n = P.next; \
                 P.next = function () { var r = n.call(this); if (!r.done) r.value *= 10; return r }; \
                 return [...[1, 2]].join()"),
            "10,20"
        );
    }

    /// Built-in iterables are iterated natively until their own protocol is patched (a
    /// `next` of their iterator prototype, or their `@@iterator`); the others stay native.
    #[test]
    fn builtin_iteration_is_native_until_its_protocol_is_patched() {
        const KINDS: &[(&str, &str)] = &[
            ("[1]", "Object.getPrototypeOf([][Symbol.iterator]())"),
            (
                "new Map([[1, 2]])",
                "Object.getPrototypeOf(new Map()[Symbol.iterator]())",
            ),
            (
                "new Set([1])",
                "Object.getPrototypeOf(new Set()[Symbol.iterator]())",
            ),
            (
                "new URLSearchParams('a=1')",
                "Object.getPrototypeOf(new URLSearchParams()[Symbol.iterator]())",
            ),
            ("'ab'", "Object.getPrototypeOf(''[Symbol.iterator]())"),
        ];
        let patches = [
            "P.next = P.next",
            "Object.defineProperty(P, 'next', { get: function () { return function () { return { done: true } } } })",
            "delete P.next",
        ];
        fn ok<T>(r: Result<T, Ex>) -> T {
            match r {
                Ok(v) => v,
                Err(Ex::Throw(e)) => panic!("threw {}", e.to_js_string()),
                Err(Ex::Short) => panic!("short"),
            }
        }
        let native = |it: &mut Interp, v: &Value| {
            !matches!(ok(it.get_iterator(v)), crate::eval::IterSrc::Js { .. })
        };
        for (i, (_, proto)) in KINDS.iter().enumerate() {
            for patch in patches {
                let mut it = Interp::default();
                let g = it.global.clone();
                let run = |it: &mut Interp, src: &str| {
                    ok(it.run(&crate::ir::lower_program(src).unwrap(), &g))
                };
                let vals: Vec<Value> = KINDS
                    .iter()
                    .map(|(v, _)| run(&mut it, &format!("return {v}")))
                    .collect();
                assert!(vals.iter().all(|v| native(&mut it, v)), "{proto}: pristine");
                run(&mut it, &format!("var P = {proto}; {patch}"));
                for (j, v) in vals.iter().enumerate() {
                    assert_eq!(
                        native(&mut it, v),
                        i != j,
                        "{proto}: {patch}: {}",
                        KINDS[j].0
                    );
                }
            }
        }
        for (v, own) in [
            ("[1]", "Array.prototype"),
            ("new Map([[1, 2]])", "Map.prototype"),
            ("new Set([1])", "Set.prototype"),
            ("new URLSearchParams('a=1')", "URLSearchParams.prototype"),
            ("'ab'", "String.prototype"),
        ] {
            // The prototype's `@@iterator` is locked in a QML engine (Qt 6.12 ignores the
            // assignment too), so it is replaced by redefining it on the instance.
            let mut it = Interp::default();
            let g = it.global.clone();
            let src = format!(
                "var v = {v}, values = [].values; \
                 {own}[Symbol.iterator] = function () {{ return values.call([]) }}; return v"
            );
            let made = ok(it.run(&crate::ir::lower_program(&src).unwrap(), &g));
            assert!(
                native(&mut it, &made),
                "{own}[Symbol.iterator] is read-only"
            );
            if matches!(made, Value::Obj(_)) {
                let src = format!(
                    "var v = {v}, values = [].values; Object.defineProperty(v, Symbol.iterator, \
                     {{ value: function () {{ return values.call([]) }} }}); return v"
                );
                let v = ok(it.run(&crate::ir::lower_program(&src).unwrap(), &g));
                assert!(!native(&mut it, &v), "{own}: own @@iterator");
            }
        }
    }
}
