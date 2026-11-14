//! Proxy exotic objects (ECMA-262 §10.5): each internal method calls the handler's trap, or
//! forwards to the target without one, and checks the trap's result against the invariants of
//! the target. The %Proxy% constructor and `Proxy.revocable` (§28.2) are in `builtins`.

use crate::eval::{Callable, Interp, PropKey, ProxyFn, R, callable, new_obj};
use crate::props::{self, Desc};
use crate::value::{Internal, Obj, Value};
use std::rc::Rc;

/// The TypeError of a refused proxy operation: QV4 throws its plain `Type error` for each.
pub fn type_error<T>(it: &Interp) -> R<T> {
    it.throw("TypeError", "Type error")
}

fn val(o: &Obj) -> Value {
    Value::Obj(o.clone())
}

/// [[ProxyTarget]] and [[ProxyHandler]] of proxy `p`, a TypeError once it is revoked
/// (ValidateNonRevokedProxy, §10.5.14).
fn parts(it: &Interp, p: &Obj) -> R<(Obj, Obj)> {
    let th = match &p.borrow().internal {
        Internal::Proxy(Some(th)) => Some((th.0.clone(), th.1.clone())),
        _ => None,
    };
    th.map_or_else(|| type_error(it), Ok)
}

/// GetMethod(handler, `name`) (§7.3.11): the trap, `None` when it is undefined or null.
fn trap(it: &mut Interp, h: &Obj, name: &str) -> R<Option<Value>> {
    match it.get(&val(h), name)? {
        Value::Undefined | Value::Null => Ok(None),
        f if callable(&f).is_some() => Ok(Some(f)),
        _ => type_error(it),
    }
}

fn same(a: Option<&Obj>, b: Option<&Obj>) -> bool {
    a.map(Rc::as_ptr) == b.map(Rc::as_ptr)
}

/// IsConstructor (§7.2.4) of a value: what `new` accepts.
pub fn is_constructor(v: &Value) -> bool {
    let Value::Obj(o) = v else { return false };
    let b = o.borrow();
    match &b.call {
        None | Some(Callable::Item(..) | Callable::Unwritten(..)) => false,
        Some(Callable::Native(..)) => b.builtin_ctor,
        Some(Callable::Bound(f)) => {
            let t = f.target.clone();
            drop(b);
            is_constructor(&t)
        }
        Some(Callable::Proxy(ProxyFn::Object { construct })) => *construct,
        Some(Callable::Proxy(ProxyFn::Constructor)) => true,
        Some(Callable::Proxy(ProxyFn::Revoke)) => false,
        Some(c) => c.is_constructor(),
    }
}

/// ProxyCreate (§10.5.15): a TypeError unless `target` and `handler` are objects.
pub fn create(it: &Interp, target: &Value, handler: &Value) -> R<Obj> {
    let (Value::Obj(t), Value::Obj(h)) = (target, handler) else {
        return type_error(it);
    };
    Ok(new(t, h))
}

/// A proxy of `target` with the traps of `handler`.
pub fn new(target: &Obj, handler: &Obj) -> Obj {
    let o = new_obj();
    {
        let mut b = o.borrow_mut();
        // The proxy's own storage stays empty: every internal method goes through `internal`.
        b.null_proto = true;
        let target_value = val(target);
        b.call = callable(&target_value).map(|_| {
            Callable::Proxy(ProxyFn::Object {
                construct: is_constructor(&target_value),
            })
        });
        b.internal = Internal::Proxy(Some(Box::new((target.clone(), handler.clone()))));
    }
    o
}

/// The revocation function `f` of `Proxy.revocable` (§28.2.2.1.1): revokes its proxy once.
pub fn revoke(f: &Value) {
    let Value::Obj(fo) = f else { return };
    let p = match &mut fo.borrow_mut().internal {
        Internal::RevocableProxy(p) => p.take(),
        _ => None,
    };
    if let Some(p) = p {
        p.borrow_mut().internal = Internal::Proxy(None);
    }
}

/// [[GetPrototypeOf]] (§10.5.1).
pub fn get_prototype(it: &mut Interp, p: &Obj) -> R<Option<Obj>> {
    let (t, h) = parts(it, p)?;
    let Some(f) = trap(it, &h, "getPrototypeOf")? else {
        return props::get_prototype(it, &val(&t));
    };
    let proto = match it.call(&f, val(&h), &[val(&t)])? {
        Value::Obj(o) => Some(o),
        Value::Null => None,
        _ => return type_error(it),
    };
    if props::is_extensible(it, &t)? {
        return Ok(proto);
    }
    let tp = props::get_prototype(it, &val(&t))?;
    if !same(proto.as_ref(), tp.as_ref()) {
        return type_error(it);
    }
    Ok(proto)
}

/// [[SetPrototypeOf]] (§10.5.2).
pub fn set_prototype(it: &mut Interp, p: &Obj, v: Option<&Obj>) -> R<bool> {
    let (t, h) = parts(it, p)?;
    let Some(f) = trap(it, &h, "setPrototypeOf")? else {
        return props::set_prototype(it, &t, v);
    };
    let pv = v.map_or(Value::Null, val);
    if !it.call(&f, val(&h), &[val(&t), pv])?.truthy() {
        return Ok(false);
    }
    if props::is_extensible(it, &t)? {
        return Ok(true);
    }
    let tp = props::get_prototype(it, &val(&t))?;
    if !same(v, tp.as_ref()) {
        return type_error(it);
    }
    Ok(true)
}

/// [[IsExtensible]] (§10.5.3).
pub fn is_extensible(it: &mut Interp, p: &Obj) -> R<bool> {
    let (t, h) = parts(it, p)?;
    let Some(f) = trap(it, &h, "isExtensible")? else {
        return props::is_extensible(it, &t);
    };
    let r = it.call(&f, val(&h), &[val(&t)])?.truthy();
    if r != props::is_extensible(it, &t)? {
        return type_error(it);
    }
    Ok(r)
}

/// [[PreventExtensions]] (§10.5.4).
pub fn prevent_extensions(it: &mut Interp, p: &Obj) -> R<bool> {
    let (t, h) = parts(it, p)?;
    let Some(f) = trap(it, &h, "preventExtensions")? else {
        return props::prevent_extensions(it, &t);
    };
    let r = it.call(&f, val(&h), &[val(&t)])?.truthy();
    if r && props::is_extensible(it, &t)? {
        return type_error(it);
    }
    Ok(r)
}

/// CompletePropertyDescriptor (§6.2.6.6).
fn complete(mut d: Desc) -> Desc {
    if d.is_accessor() {
        d.get.get_or_insert(Value::Undefined);
        d.set.get_or_insert(Value::Undefined);
    } else {
        d.value.get_or_insert(Value::Undefined);
        d.writable.get_or_insert(false);
    }
    d.enumerable.get_or_insert(false);
    d.configurable.get_or_insert(false);
    d
}

/// [[GetOwnProperty]] (§10.5.5).
pub fn own_desc(it: &mut Interp, p: &Obj, k: &PropKey) -> R<Option<Desc>> {
    let (t, h) = parts(it, p)?;
    let Some(f) = trap(it, &h, "getOwnPropertyDescriptor")? else {
        return props::own_desc_pk(it, &val(&t), k);
    };
    let r = it.call(&f, val(&h), &[val(&t), k.to_value()])?;
    if !matches!(r, Value::Obj(_) | Value::Undefined) {
        return type_error(it);
    }
    let target_desc = props::own_desc_pk(it, &val(&t), k)?;
    if matches!(r, Value::Undefined) {
        let Some(td) = target_desc else {
            return Ok(None);
        };
        if td.configurable == Some(false) || !props::is_extensible(it, &t)? {
            return type_error(it);
        }
        return Ok(None);
    }
    let extensible = props::is_extensible(it, &t)?;
    let d = complete(props::to_desc(it, &r)?);
    if !props::compatible(extensible, &d, target_desc.as_ref()) {
        return type_error(it);
    }
    if d.configurable == Some(false) {
        match &target_desc {
            Some(td) if td.configurable == Some(false) => {
                if d.writable == Some(false) && td.writable == Some(true) {
                    return type_error(it);
                }
            }
            _ => return type_error(it),
        }
    }
    Ok(Some(d))
}

/// [[DefineOwnProperty]] (§10.5.6).
pub fn define(it: &mut Interp, p: &Obj, k: &PropKey, d: &Desc) -> R<bool> {
    let (t, h) = parts(it, p)?;
    let Some(f) = trap(it, &h, "defineProperty")? else {
        return props::define_pk(it, &t, k, d);
    };
    let dv = props::from_desc(d);
    if !it.call(&f, val(&h), &[val(&t), k.to_value(), dv])?.truthy() {
        return Ok(false);
    }
    let target_desc = props::own_desc_pk(it, &val(&t), k)?;
    let extensible = props::is_extensible(it, &t)?;
    let non_configurable = d.configurable == Some(false);
    let ok = match &target_desc {
        None => extensible && !non_configurable,
        Some(td) => {
            props::compatible(extensible, d, Some(td))
                && !(non_configurable && td.configurable == Some(true))
                && !(td.is_data()
                    && td.configurable == Some(false)
                    && td.writable == Some(true)
                    && d.writable == Some(false))
        }
    };
    if !ok {
        return type_error(it);
    }
    Ok(true)
}

/// [[HasProperty]] (§10.5.7).
pub fn has(it: &mut Interp, p: &Obj, k: &PropKey) -> R<bool> {
    let (t, h) = parts(it, p)?;
    let Some(f) = trap(it, &h, "has")? else {
        return props::has_pk(it, &val(&t), k);
    };
    let r = it.call(&f, val(&h), &[val(&t), k.to_value()])?.truthy();
    if !r
        && let Some(td) = props::own_desc_pk(it, &val(&t), k)?
        && (td.configurable == Some(false) || !props::is_extensible(it, &t)?)
    {
        return type_error(it);
    }
    Ok(r)
}

/// [[Get]] (§10.5.8) of `k` for `receiver`.
pub fn get(it: &mut Interp, p: &Obj, k: &PropKey, receiver: &Value) -> R<Value> {
    let (t, h) = parts(it, p)?;
    let Some(f) = trap(it, &h, "get")? else {
        return match k {
            PropKey::Str(k) => it.get_from(&t, k, receiver),
            PropKey::Sym(s) => it.get_sym_from(Some(t), s, receiver),
        };
    };
    let r = it.call(&f, val(&h), &[val(&t), k.to_value(), receiver.clone()])?;
    if let Some(td) = props::own_desc_pk(it, &val(&t), k)?
        && td.configurable == Some(false)
    {
        let refused = if td.is_accessor() {
            matches!(td.get, Some(Value::Undefined) | None) && !matches!(r, Value::Undefined)
        } else {
            td.writable == Some(false)
                && !crate::builtins::same_value(&r, td.value.as_ref().unwrap_or(&Value::Undefined))
        };
        if refused {
            return type_error(it);
        }
    }
    Ok(r)
}

/// [[Set]] (§10.5.9) of `k` to `v` for `receiver`.
pub fn set(it: &mut Interp, p: &Obj, k: &PropKey, v: Value, receiver: &Value) -> R<bool> {
    let (t, h) = parts(it, p)?;
    let Some(f) = trap(it, &h, "set")? else {
        return props::set_with_receiver_pk(it, &val(&t), k, v, receiver);
    };
    let args = [val(&t), k.to_value(), v.clone(), receiver.clone()];
    if !it.call(&f, val(&h), &args)?.truthy() {
        return Ok(false);
    }
    if let Some(td) = props::own_desc_pk(it, &val(&t), k)?
        && td.configurable == Some(false)
    {
        let refused = if td.is_accessor() {
            matches!(td.set, Some(Value::Undefined) | None)
        } else {
            td.writable == Some(false)
                && !crate::builtins::same_value(&v, td.value.as_ref().unwrap_or(&Value::Undefined))
        };
        if refused {
            return type_error(it);
        }
    }
    Ok(true)
}

/// [[Delete]] (§10.5.10).
pub fn delete(it: &mut Interp, p: &Obj, k: &PropKey) -> R<bool> {
    let (t, h) = parts(it, p)?;
    let Some(f) = trap(it, &h, "deleteProperty")? else {
        return props::delete_pk(it, &t, k);
    };
    if !it.call(&f, val(&h), &[val(&t), k.to_value()])?.truthy() {
        return Ok(false);
    }
    if let Some(td) = props::own_desc_pk(it, &val(&t), k)?
        && (td.configurable == Some(false) || !props::is_extensible(it, &t)?)
    {
        return type_error(it);
    }
    Ok(true)
}

/// [[OwnPropertyKeys]] (§10.5.11).
pub fn own_keys(it: &mut Interp, p: &Obj) -> R<Vec<PropKey>> {
    let (t, h) = parts(it, p)?;
    let Some(f) = trap(it, &h, "ownKeys")? else {
        return props::own_property_keys(it, &val(&t));
    };
    let ra = it.call(&f, val(&h), &[val(&t)])?;
    // CreateListFromArrayLike(trapResultArray, « String, Symbol »), without duplicates.
    if !matches!(ra, Value::Obj(_)) {
        return type_error(it);
    }
    let n = it.get(&ra, "length")?;
    let n = it.to_numeric(&n)?;
    let len = if n > 0.0 {
        crate::eval::array_len(it, n.trunc().min(4294967295.0))?
    } else {
        0
    };
    let mut keys: Vec<PropKey> = Vec::with_capacity(len);
    let mut unchecked: std::collections::HashSet<PropKey> = Default::default();
    for i in 0..len {
        let k = match it.get(&ra, &i.to_string())? {
            Value::Str(k) => PropKey::Str(k.to_string()),
            Value::Symbol(s) => PropKey::Sym(s),
            _ => return type_error(it),
        };
        if !unchecked.insert(k.clone()) {
            return type_error(it);
        }
        keys.push(k);
    }
    let extensible = props::is_extensible(it, &t)?;
    let tv = val(&t);
    let (mut configurable, mut fixed) = (Vec::new(), Vec::new());
    for k in props::own_property_keys(it, &tv)? {
        match props::own_desc_pk(it, &tv, &k)? {
            Some(d) if d.configurable == Some(false) => fixed.push(k),
            _ => configurable.push(k),
        }
    }
    if extensible && fixed.is_empty() {
        return Ok(keys);
    }
    for k in &fixed {
        if !unchecked.remove(k) {
            return type_error(it);
        }
    }
    if extensible {
        return Ok(keys);
    }
    for k in &configurable {
        if !unchecked.remove(k) {
            return type_error(it);
        }
    }
    if !unchecked.is_empty() {
        return type_error(it);
    }
    Ok(keys)
}

/// [[Call]] (§10.5.12) of callable proxy `f`.
pub fn call(it: &mut Interp, f: &Value, this: Value, args: &[Value]) -> R<Value> {
    let Value::Obj(p) = f else {
        return type_error(it);
    };
    let (t, h) = parts(it, p)?;
    let Some(tr) = trap(it, &h, "apply")? else {
        return it.call(&val(&t), this, args);
    };
    it.call(&tr, val(&h), &[val(&t), this, Value::array(args.to_vec())])
}

/// [[Construct]] (§10.5.13) of constructor proxy `f` for NewTarget `nt`.
pub fn construct(it: &mut Interp, f: &Value, args: &[Value], nt: &Value) -> R<Value> {
    let Value::Obj(p) = f else {
        return type_error(it);
    };
    let (t, h) = parts(it, p)?;
    let Some(tr) = trap(it, &h, "construct")? else {
        return it.construct_new(&val(&t), args, nt);
    };
    let r = it.call(
        &tr,
        val(&h),
        &[val(&t), Value::array(args.to_vec()), nt.clone()],
    )?;
    if !matches!(r, Value::Obj(_)) {
        return type_error(it);
    }
    Ok(r)
}

/// IsArray (§7.2.2) of proxy `p`: its target's.
pub fn is_array(it: &mut Interp, p: &Obj) -> R<bool> {
    let (t, _) = parts(it, p)?;
    props::is_array(it, &val(&t))
}
