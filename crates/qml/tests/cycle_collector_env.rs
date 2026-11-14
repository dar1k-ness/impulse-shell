//! `eval::collect_cycles` with compiled closures (`Callable::Compiled`): an `Env` frame is
//! shared by every closure created in it and by its child frames, and the shell or a
//! compiled module may hold one from Rust. Each such reference must count once, else a
//! value the collector cannot see the holder of looks fully explained by internal edges and
//! gets wiped while still in use.
use qml::eval::{Callable, Frame, R, Scope, collect_cycles, new_obj, set_own};
use qml::jsrt::{Cx, Env, FnKind, FnMeta};
use qml::value::{Obj, Value};
use std::cell::RefCell;
use std::rc::{Rc, Weak};

static META: FnMeta = FnMeta {
    name: "f",
    kind: FnKind::Normal,
    nparams: 0,
    nslots: 0,
    strict: false,
};

fn body(_: &mut Cx, _: &Env, _: &Value, _: &[Value]) -> R<Value> {
    Ok(Value::Undefined)
}

fn closure(env: &Env) -> Value {
    qml::jsrt::closure(body, env, &META)
}

fn scope() -> Scope {
    Rc::new(RefCell::new(Frame::default()))
}

fn marked() -> Obj {
    let o = new_obj();
    set_own(&o, "alive", Value::Bool(true));
    o
}

fn alive(o: &Obj) -> bool {
    o.borrow().props.contains_key("alive")
}

fn obj(v: &Value) -> Obj {
    match v {
        Value::Obj(o) => o.clone(),
        _ => panic!("not an object"),
    }
}

fn callable(v: &Value) -> bool {
    obj(v).borrow().call.is_some()
}

fn set(env: &Env, slot: u32, v: Value) {
    env.set(0, slot, v);
}

/// `function f(){} function g(){} var x = held` in one frame: f and g close over the same
/// Env (a garbage cycle with it), x is also held from outside. x and the QML scope frame
/// must survive; f and g are garbage.
#[test]
fn two_closures_sharing_an_env_count_its_slots_once() {
    let sc = scope();
    let qobj = marked();
    sc.borrow_mut()
        .vars
        .insert("ctx".into(), Value::Obj(qobj.clone()));
    let x = marked();
    let (wf, wg);
    {
        let e = Env::from_scope(sc.clone()).child(3);
        let f = closure(&e);
        let g = closure(&e);
        wf = Rc::downgrade(&obj(&f));
        wg = Rc::downgrade(&obj(&g));
        set(&e, 0, f);
        set(&e, 1, g);
        set(&e, 2, Value::Obj(x.clone()));
    }
    collect_cycles();
    assert!(alive(&x), "externally held slot value was wiped");
    assert!(
        sc.borrow().vars.contains_key("ctx"),
        "externally held QML scope frame was wiped"
    );
    assert!(alive(&qobj));
    assert!(
        wf.upgrade().is_none() && wg.upgrade().is_none(),
        "the f/g/Env cycle itself is garbage and must be freed"
    );
}

/// Three closures on one frame and the frame's object held outside: more closures must not
/// make more internal references.
#[test]
fn many_closures_on_one_env() {
    let x = marked();
    {
        let e = Env::from_scope(scope()).child(9);
        for i in 0..8 {
            let f = closure(&e);
            set(&e, i, f);
        }
        set(&e, 8, Value::Obj(x.clone()));
    }
    collect_cycles();
    assert!(alive(&x));
}

/// `function outer(){ var x; function g(){}; { let y; function f(){} } }`: f closes over a
/// child frame whose parent is g's frame. A parent's slots are reached through the parent
/// frame once, not again through every child frame.
#[test]
fn parent_frame_slots_count_once_through_child_frames() {
    let x = marked();
    let (wf, wg);
    {
        let e0 = Env::from_scope(scope()).child(2);
        let e1 = Env::child(&e0, 1);
        let e2 = Env::child(&e1, 1);
        let g = closure(&e0);
        let f = closure(&e1);
        let h = closure(&e2);
        wf = Rc::downgrade(&obj(&f));
        wg = Rc::downgrade(&obj(&g));
        set(&e0, 0, g);
        set(&e0, 1, Value::Obj(x.clone()));
        set(&e1, 0, f);
        set(&e2, 0, h);
    }
    collect_cycles();
    assert!(alive(&x), "parent frame slot counted once per child frame");
    assert!(wf.upgrade().is_none() && wg.upgrade().is_none());
}

/// A frame held from Rust only (a compiled module's top-level Env, a timer's frame): the
/// function declared in it and the values in its slots stay usable.
#[test]
fn env_held_from_rust_keeps_its_slots() {
    let e = Env::from_scope(scope()).child(2);
    let y = marked();
    let wy: Weak<_> = Rc::downgrade(&y);
    set(&e, 0, closure(&e));
    set(&e, 1, Value::Obj(y));
    collect_cycles();
    assert!(
        callable(&e.get(0, 0)),
        "function in a live frame lost its body"
    );
    assert!(
        alive(&wy.upgrade().unwrap()),
        "value in a live frame was wiped"
    );
    let c = Env::child(&e, 1);
    set(&c, 0, closure(&c));
    drop(c);
    collect_cycles();
    assert!(callable(&e.get(0, 0)));
    assert!(alive(&wy.upgrade().unwrap()));
}

/// One closure on a frame held from outside, one only from the frame: whichever the
/// collector meets first, the live closure keeps the frame (and everything in it) alive.
#[test]
fn live_closure_keeps_shared_env_in_either_order() {
    for live_first in [true, false] {
        let e = Env::from_scope(scope()).child(2);
        let (live, dead) = if live_first {
            let l = closure(&e);
            (l, closure(&e))
        } else {
            let d = closure(&e);
            (closure(&e), d)
        };
        let y = marked();
        let wy = Rc::downgrade(&y);
        set(&e, 0, dead);
        set(&e, 1, Value::Obj(y));
        drop(e);
        collect_cycles();
        let Value::Obj(l) = &live else { unreachable!() };
        let env = match &l.borrow().call {
            Some(Callable::Compiled(_, env, _)) => env.clone(),
            _ => panic!("live closure lost its body (live_first={live_first})"),
        };
        assert!(callable(&env.slots.borrow()[0]), "live_first={live_first}");
        assert!(alive(&wy.upgrade().unwrap()), "live_first={live_first}");
    }
}

/// The fix must not stop collection: a closure/frame cycle nobody holds is freed, with
/// what only it references.
#[test]
fn unreferenced_shared_env_cycle_is_freed() {
    let (wf, wg, wx);
    {
        let e0 = Env::from_scope(scope()).child(3);
        let e1 = Env::child(&e0, 1);
        let f = closure(&e0);
        let g = closure(&e1);
        let x = marked();
        set_own(&x, "back", g.clone());
        wf = Rc::downgrade(&obj(&f));
        wg = Rc::downgrade(&obj(&g));
        wx = Rc::downgrade(&x);
        set(&e0, 0, f);
        set(&e0, 1, Value::Obj(x));
        set(&e1, 0, g);
    }
    let freed = collect_cycles();
    assert!(freed >= 3, "freed {freed}");
    assert!(wf.upgrade().is_none() && wg.upgrade().is_none() && wx.upgrade().is_none());
}
