//! A binding that reads the property it is bound to and writes another value than it read.
//! The write notifies the binding while it is still running: Qt logs `Binding loop detected
//! for property`, skips that run (QQmlBinding::update, updatingFlag) and the value the first
//! run wrote stays. The cases where the write leaves the value as it was, which are no loop,
//! are the rigen case `self_read_binding.qml`.
//!
//! Qt 6.12 (PySide6 6.12.0, QQmlComponent, offscreen) for `scenes/self_read/Loops.qml`: the
//! `result` below, and in its log a loop for `height` of `b`, `width` of `c`, `q` of `e`, `x`
//! of `i` and `height` of the Flickable `g`, and for no other property.

#[test]
fn a_binding_that_changes_the_property_it_read_is_a_binding_loop() {
    let (mut rt, root) = testscenes::load("self_read/Loops.qml");
    rt.settle();
    let first = "b=50 ran 1 read 0 signals 1; c=10 ran 1 read 0 signals 1; \
                 e=1 ran 1 read undefined signals 1; i=10 ran 1 read - signals 1; \
                 g=10 ran 1 read - signals 1";
    let second = "b=50 ran 1 read 0 signals 1; c=20 ran 2 read 0,10 signals 2; \
                  e=1 ran 1 read undefined signals 1; i=30 ran 2 read -,- signals 2; \
                  g=10 ran 1 read - signals 1";
    assert_eq!(
        rt.get(root, "result").to_js_string(),
        format!("{first} | {second}")
    );
    // `binding loop detected on item <n> property <name> (<types> in <file>)`.
    let mut loops: Vec<String> = (rt.errors().iter())
        .map(|e| {
            let (_, at) = e.split_once(" property ").unwrap();
            at.split_once(" in ").unwrap().0.to_string()
        })
        .collect();
    loops.sort();
    assert_eq!(
        loops,
        [
            "height (Item",
            "height (Item/Flickable",
            "q (Item",
            "width (Item",
            "x (Item"
        ]
    );
}
