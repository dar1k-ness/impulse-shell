//! `eval::collect_step`: the generational, incremental cycle collector. Each
//! increment examines the young generation plus a bounded slice of an old-generation sweep,
//! so the host can run it every loop turn, animating or not, and still free every garbage
//! cycle. Every test runs on its own thread, so it sees its own JS heap.
use qml::eval::{
    GcStep, collect_cycles, collect_step, new_obj, request_sweep, set_own, sweep_running,
};
use qml::value::{Obj, Value};
use std::rc::{Rc, Weak};

/// `a.next = b; b.next = a`: a two-object cycle, returned weakly once nothing else holds it.
fn garbage_cycle() -> (
    Weak<std::cell::RefCell<qml::value::JsObject>>,
    Weak<std::cell::RefCell<qml::value::JsObject>>,
) {
    let (a, b) = (new_obj(), new_obj());
    link(&a, &b);
    link(&b, &a);
    (Rc::downgrade(&a), Rc::downgrade(&b))
}

fn link(from: &Obj, to: &Obj) {
    set_own(from, "next", Value::Obj(to.clone()));
}

/// A live acyclic heap of `n` objects (an array of `n - 1` empty objects).
fn live_heap(n: usize) -> Value {
    Value::array((1..n).map(|_| Value::Obj(new_obj())).collect())
}

/// Run increments until the running sweep ends; returns them.
fn finish_sweep(budget: usize) -> Vec<GcStep> {
    let mut steps = Vec::new();
    while sweep_running() {
        steps.push(collect_step(budget));
        assert!(steps.len() < 1_000_000, "sweep never ends");
    }
    steps
}

/// Garbage that dies young is freed by the next increment, without a sweep.
#[test]
fn young_cycle_is_freed_by_the_next_step() {
    let _heap = live_heap(1000);
    collect_cycles();
    let (a, b) = garbage_cycle();
    let s = collect_step(0);
    assert!(!sweep_running());
    assert_eq!(s.freed, 2);
    assert!(a.upgrade().is_none() && b.upgrade().is_none());
}

/// An increment costs what was allocated since the last one, not the heap size: the
/// examined count for one young cycle is the same over a 1k-object and a 100k-object heap.
#[test]
fn young_step_work_is_independent_of_heap_size() {
    let mut examined = Vec::new();
    for n in [1_000, 100_000] {
        let heap = live_heap(n);
        collect_cycles();
        let _cycle = garbage_cycle();
        let s = collect_step(0);
        assert_eq!(s.freed, 2);
        examined.push(s.examined);
        drop(heap);
        collect_cycles();
    }
    assert_eq!(examined, [2, 2]);
}

/// A cycle that becomes garbage after it was promoted is not found by young increments; a
/// sweep frees it in slices of about `budget` nodes, each far smaller than the heap.
#[test]
fn old_cycle_is_freed_by_a_sweep_in_bounded_slices() {
    let heap = live_heap(50_000);
    let a = new_obj();
    let b = new_obj();
    link(&a, &b);
    link(&b, &a);
    collect_cycles();
    let (wa, wb) = (Rc::downgrade(&a), Rc::downgrade(&b));
    drop((a, b));
    assert_eq!(
        collect_step(0).freed,
        0,
        "young increments do not walk the old generation"
    );
    assert!(wa.upgrade().is_some());
    request_sweep();
    let steps = finish_sweep(1000);
    assert!(wa.upgrade().is_none() && wb.upgrade().is_none());
    assert_eq!(steps.iter().map(|s| s.freed).sum::<usize>(), 2);
    assert!(steps.last().unwrap().swept);
    assert!(steps.len() >= 50, "{} slices", steps.len());
    // A slice stops taking seeds at the budget; the last seed's closure may add to it (here
    // `a` brings `b`).
    let max = steps.iter().map(|s| s.examined).max().unwrap();
    assert!(max <= 1001, "slice of {max} nodes");
    drop(heap);
}

/// A garbage cycle whose members were allocated far apart (first and last of the heap) lies
/// in one increment whole: the increment takes everything reachable from its seeds.
#[test]
fn cycle_spanning_the_heap_is_collected_by_a_sliced_sweep() {
    let first = new_obj();
    let heap: Vec<Obj> = (0..20_000).map(|_| new_obj()).collect();
    let last = new_obj();
    link(&first, &last);
    link(&last, &first);
    collect_cycles();
    let w = Rc::downgrade(&first);
    drop((first, last));
    request_sweep();
    let steps = finish_sweep(100);
    assert!(w.upgrade().is_none());
    assert_eq!(steps.iter().map(|s| s.freed).sum::<usize>(), 2);
    assert!(
        steps.iter().all(|s| s.examined <= 101),
        "{:?}",
        steps.iter().map(|s| s.examined).max()
    );
    drop(heap);
}

/// The mutator runs between increments: an object only reachable through one examined
/// earlier in the sweep (and so outside later increments) must survive.
#[test]
fn reference_moved_into_an_examined_object_keeps_its_target() {
    let holder = new_obj();
    let filler: Vec<Obj> = (0..100).map(|_| new_obj()).collect();
    let (x, y) = (new_obj(), new_obj());
    link(&x, &y);
    link(&y, &x);
    collect_cycles();
    request_sweep();
    collect_step(10);
    assert!(sweep_running());
    // `holder` was examined by the first slice; x/y are still pending.
    link(&holder, &x);
    let wx = Rc::downgrade(&x);
    drop((x, y));
    finish_sweep(10);
    let x = wx.upgrade().expect("live object freed");
    assert!(x.borrow().props.contains_key("next"), "live object cleared");
    drop(filler);
}

/// The young generation keeps being collected while a sweep runs, and the sweep still ends.
#[test]
fn young_garbage_during_a_sweep() {
    let _heap = live_heap(10_000);
    collect_cycles();
    request_sweep();
    let mut freed = 0;
    let mut turns = 0;
    while sweep_running() {
        let _ = garbage_cycle();
        freed += collect_step(500).freed;
        turns += 1;
    }
    assert_eq!(freed, 2 * turns);
}

/// A sweep starts on its own once the old generation grew by a quarter since the last one
/// (and young increments alone never walk it before that).
#[test]
fn sweep_starts_when_the_old_generation_grows_by_a_quarter() {
    let _heap = live_heap(4000);
    collect_cycles();
    let _more = live_heap(900);
    assert_eq!(collect_step(0).examined, 900);
    assert_eq!(collect_step(0).examined, 0);
    assert!(
        !sweep_running(),
        "900 promoted over 4000 old: under a quarter"
    );
    let _more2 = live_heap(200);
    collect_step(0);
    assert!(!sweep_running());
    let s = collect_step(0);
    assert!(sweep_running(), "1100 promoted over 4000 old");
    assert!(s.examined > 0 && s.examined < 10, "{s:?}");
    let steps = finish_sweep(1000);
    assert!(steps.len() >= 5);
    // The finished sweep found 5100 live old objects: the next one needs over 1275 more.
    let _more3 = live_heap(1200);
    collect_step(0);
    collect_step(0);
    assert!(!sweep_running());
}

fn heap_entries() -> usize {
    qml::value::HEAP.with(|h| h.borrow().objs.len())
}

/// A freed object's `Weak` entry pins its allocation: a sweep drops the entries it walks
/// over, so the heap's table shrinks back to the live objects.
#[test]
fn sweep_drops_the_entries_of_freed_objects() {
    let live: Vec<Obj> = (0..10_000).map(|_| new_obj()).collect();
    let dead: Vec<Obj> = (0..10_000).map(|_| new_obj()).collect();
    collect_step(0);
    drop(dead);
    assert!(heap_entries() >= 20_000);
    request_sweep();
    finish_sweep(1000);
    assert_eq!(heap_entries(), live.len());
}

/// A table compaction triggered by an allocation in the middle of a sweep keeps the sweep
/// on the same objects: it still frees the old cycle and every live object survives.
#[test]
fn compaction_during_a_sweep() {
    let live: Vec<Obj> = (0..10_000).map(|_| new_obj()).collect();
    let dead: Vec<Obj> = (0..10_000).map(|_| new_obj()).collect();
    let held: Vec<Obj> = (0..100)
        .map(|_| {
            let (a, b) = (new_obj(), new_obj());
            link(&a, &b);
            link(&b, &a);
            a
        })
        .collect();
    let probe = Rc::downgrade(&held[99]);
    collect_step(0);
    drop((dead, held));
    request_sweep();
    collect_step(1000);
    // Enough allocations to reach the compaction threshold.
    let more: Vec<Obj> = (0..70_000).map(|_| new_obj()).collect();
    let freed: usize = finish_sweep(1000).iter().map(|s| s.freed).sum();
    assert_eq!(freed + collect_step(0).freed, 200);
    assert!(probe.upgrade().is_none());
    assert!(live.iter().chain(&more).all(|o| Rc::strong_count(o) == 1));
    request_sweep();
    finish_sweep(usize::MAX);
    assert_eq!(heap_entries(), live.len() + more.len());
}

/// Pause of one increment against a full collection over a 200k-object heap (each object
/// with a property and a two-element array, plus a garbage cycle every 100 objects): the
/// increment is bounded by `GC_SLICE`, the full collection by the heap.
#[test]
fn increment_pause_against_full_collection() {
    use std::time::Instant;
    let heap: Vec<Obj> = (0..200_000)
        .map(|i| {
            let o = new_obj();
            set_own(&o, "i", Value::Num(i as f64));
            set_own(
                &o,
                "a",
                Value::array(vec![Value::Num(1.0), Value::Obj(new_obj())]),
            );
            if i % 100 == 0 {
                let _ = garbage_cycle();
            }
            o
        })
        .collect();
    collect_step(0);
    for _ in 0..2000 {
        let _ = garbage_cycle();
    }
    let t = Instant::now();
    let full = collect_cycles();
    let full_t = t.elapsed();
    assert_eq!(full, 2 * 2000);
    // Old garbage: cycles that survived as young (held from Rust), then were dropped.
    let held: Vec<Obj> = (0..2000)
        .map(|_| {
            let (a, b) = (new_obj(), new_obj());
            link(&a, &b);
            link(&b, &a);
            a
        })
        .collect();
    assert_eq!(collect_step(0).freed, 0);
    drop(held);
    request_sweep();
    let (mut worst, mut freed, mut slices, mut max_n) = (Default::default(), 0, 0, 0);
    while sweep_running() {
        let t = Instant::now();
        let s = collect_step(qml::eval::GC_SLICE);
        worst = t.elapsed().max(worst);
        freed += s.freed;
        slices += 1;
        max_n = max_n.max(s.examined);
    }
    assert_eq!(freed, 2 * 2000);
    assert!(max_n <= qml::eval::GC_SLICE + 3, "{max_n}");
    eprintln!(
        "full collection {full_t:?} over {} objects; sweep in {slices} slices, worst {worst:?} ({max_n} nodes)",
        heap.len() * 3
    );
}
