//! `value::HEAP.objs`, the list of every JS object the cycle collector walks. A freed
//! object's entry stays in it, and pins the object's allocation, until `Heap::compact` drops
//! the freed entries: the list's length is the ceiling of that memory. Every test runs on
//! its own thread, so it sees its own JS heap.
use qml::eval::new_obj;
use qml::runtime::Runtime;
use qml::value::{HEAP, Value};

fn cells() -> usize {
    HEAP.with(|h| h.borrow().objs.len())
}

/// With nothing live, the list never reaches 4096 entries, however many objects come and go.
#[test]
fn temporaries_alone_stay_under_4096_cells() {
    let mut most = 0;
    for _ in 0..100_000 {
        drop(new_obj());
        most = most.max(cells());
    }
    assert_eq!(most, 4095);
}

/// The same through the engine: a script's loop of object literals.
#[test]
fn a_scripts_temporaries_stay_under_4096_cells() {
    let source = "import QtQml\nQtObject {\n function read(count) {\n  let sum = 0\n  \
                  for (let i = 0; i < count; i++) sum += ({a: i}).a\n  return sum\n }\n}\n";
    let mut rt = Runtime::new(None);
    let root = rt.load_str(source).unwrap();
    rt.settle();
    for _ in 0..100 {
        let sum = rt.call_method(root, "read", vec![Value::Num(1000.0)]);
        assert!(matches!(sum, Some(Ok(Value::Num(n))) if n == 499_500.0));
        assert!(cells() < 4096, "{} cells", cells());
    }
}

/// Over many live objects the list grows to twice their number before it is walked, so a
/// walk comes once per as many temporaries as there are live objects, not at every one.
#[test]
fn over_live_objects_the_list_is_walked_once_per_as_many_temporaries() {
    const LIVE: usize = 200_000;
    let live: Vec<_> = (0..LIVE).map(|_| new_obj()).collect();
    let (mut walks, mut most, mut before) = (0, 0, cells());
    for _ in 0..1_000_000 {
        drop(new_obj());
        let now = cells();
        walks += (now < before) as usize;
        most = most.max(now);
        before = now;
    }
    assert!(most < 2 * (LIVE + 1), "{most} cells");
    assert!((4..=6).contains(&walks), "{walks} walks");
    drop(live);
}
