//! `Component.incubateObject(parent, properties, mode)`: the object is made by the engine's
//! incubation controller, and the call returns an incubator to watch it with.
//!
//! The incubator is a JavaScript object whose prototype has `status` and `object` (read
//! only), `onStatusChanged` (a function to call when the status changes) and
//! `forceCompletion()`. With an incubation controller (a window sets one, Quickshell has its
//! own) the object of an asynchronous incubator is made on a later turn of the event loop;
//! `Qt.Synchronous` makes it at once.

use crate::eval::{Callable, Interp, R, define_accessor, fn_value, new_obj, set_hidden};
use crate::runtime::{
    NOT_READY, World, new_object, not_an_object, object_not_array, ready_template, world_of,
};
use crate::value::{Obj, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// QQmlIncubator::Status.
const READY: f64 = 1.0;
const LOADING: f64 = 2.0;
const ERROR: f64 = 3.0;

/// What an incubator holds (QV4::Heap::QmlIncubatorObject).
struct Incubation {
    template: usize,
    parent: Option<usize>,
    properties: Value,
    status: f64,
    object: Option<usize>,
    on_status_changed: Value,
}

fn incubation(v: &Value) -> Option<Rc<RefCell<Incubation>>> {
    match v {
        Value::Obj(o) => o
            .borrow()
            .native
            .clone()?
            .downcast::<RefCell<Incubation>>()
            .ok(),
        _ => None,
    }
}

/// The incubator of `this`, or the TypeError its properties throw on another object.
fn incubation_of(it: &mut Interp, this: &Value) -> R<Rc<RefCell<Incubation>>> {
    match incubation(this) {
        Some(i) => Ok(i),
        None => it.throw("TypeError", "Type error"),
    }
}

fn proto(w: &mut World) -> Obj {
    if let Some(p) = &w.incubator_proto {
        return p.clone();
    }
    let native = |name, f| fn_value(Callable::Native(name, f));
    let p = new_obj();
    define_accessor(
        &p,
        "onStatusChanged",
        true,
        native("onStatusChanged", |it, this, _| {
            Ok(incubation_of(it, this)?.borrow().on_status_changed.clone())
        }),
        false,
    );
    define_accessor(
        &p,
        "onStatusChanged",
        false,
        native("onStatusChanged", |it, this, a| {
            let inc = incubation_of(it, this)?;
            inc.borrow_mut().on_status_changed = a.first().cloned().unwrap_or_default();
            Ok(Value::Undefined)
        }),
        false,
    );
    define_accessor(
        &p,
        "status",
        true,
        native("status", |it, this, _| {
            Ok(Value::Num(incubation_of(it, this)?.borrow().status))
        }),
        false,
    );
    define_accessor(
        &p,
        "object",
        true,
        native("object", |it, this, _| {
            let object = incubation_of(it, this)?.borrow().object;
            let world = world_of(it);
            let gone = |o: &usize| world.as_ref().is_none_or(|w| w.borrow().dead.contains(o));
            Ok(object.filter(|o| !gone(o)).map_or(Value::Null, Value::Item))
        }),
        false,
    );
    set_hidden(
        &p,
        "forceCompletion",
        native("forceCompletion", |it, this, _| {
            incubation_of(it, this)?;
            if let (Value::Obj(inc), Some(world)) = (this, world_of(it)) {
                incubation_complete(it, &world, inc);
            }
            Ok(Value::Undefined)
        }),
    );
    w.incubator_proto = Some(p.clone());
    p
}

/// `Component.incubateObject`: an incubator, or null when the properties are no object or the
/// component is not ready (QQmlComponent::incubateObject).
pub(crate) fn incubate_object(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    let (Value::Item(c), Some(world)) = (this, world_of(it)) else {
        return Ok(Value::Null);
    };
    let parent = match a.first() {
        Some(Value::Item(p)) => Some(*p),
        _ => None,
    };
    let properties = match a.get(1) {
        None | Some(Value::Null) => Value::Undefined,
        Some(v) if !object_not_array(v) => {
            not_an_object(&mut world.borrow_mut(), *c);
            return Ok(Value::Null);
        }
        Some(v @ Value::Obj(_)) => v.clone(),
        Some(_) => Value::Undefined,
    };
    // QQmlIncubator::AsynchronousIfNested, what `Qt.Synchronous` (1) stands for, is
    // synchronous unless the call is made while another incubator makes its object.
    let synchronous = a.get(2).is_some_and(|m| m.to_number() as u32 == 1);
    let template = ready_template(&world.borrow(), *c);
    let Some(template) = template else {
        world.borrow_mut().errors.push(NOT_READY.into());
        return Ok(Value::Null);
    };
    let inc = new_obj();
    {
        let mut o = inc.borrow_mut();
        o.proto = Some(proto(&mut world.borrow_mut()));
        o.native = Some(Rc::new(RefCell::new(Incubation {
            template,
            parent,
            properties,
            status: LOADING,
            object: None,
            on_status_changed: Value::Undefined,
        })));
    }
    world.borrow_mut().incubating.push(inc.clone());
    if synchronous {
        incubation_complete(it, &world, &inc);
    }
    Ok(Value::Obj(inc))
}

/// Incubator `inc` makes its object if it has not yet, and tells `onStatusChanged`.
pub(crate) fn incubation_complete(it: &mut Interp, world: &Rc<RefCell<World>>, inc: &Obj) {
    let waiting = {
        let mut w = world.borrow_mut();
        let n = w.incubating.len();
        w.incubating.retain(|o| !Rc::ptr_eq(o, inc));
        w.incubating.len() < n
    };
    let this = Value::Obj(inc.clone());
    let Some(state) = incubation(&this).filter(|_| waiting) else {
        return;
    };
    let (template, parent, properties) = {
        let s = state.borrow();
        (s.template, s.parent, s.properties.clone())
    };
    // The parent is held by a guarded pointer: one deleted meanwhile is no parent.
    let parent = parent.filter(|p| !world.borrow().dead.contains(p));
    let object = new_object(it, world, template, parent, &properties, None);
    let status = if object.is_some() { READY } else { ERROR };
    let callback = {
        let mut s = state.borrow_mut();
        s.object = object;
        s.status = status;
        s.properties = Value::Undefined;
        s.on_status_changed.clone()
    };
    if crate::eval::callable(&callback).is_some()
        && let Err(crate::eval::Ex::Throw(e)) = it.call(&callback, this, &[Value::Num(status)])
    {
        let message = (it.get(&e, "message")).map_or_else(|_| String::new(), |m| m.to_js_string());
        world.borrow_mut().errors.push(format!("Error: {message}"));
    }
}
