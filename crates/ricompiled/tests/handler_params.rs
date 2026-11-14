//! A compiled handler binds the parameters of the signal it handles: a signal the document
//! declares on one object says nothing about a same-named signal of another object.
use qml::value::Value;
use std::path::PathBuf;

const PAGES: &str = "modules/waffle/looks/VerticalPageIndicator.qml";

/// `got` of a user of the page indicator after a click on its second page: the page index
/// the indicator's `clicked(int index)` carries, with the compiled code or the interpreter.
fn clicked_page(compiled: bool) -> String {
    let root = PathBuf::from(ricompiled::ROOT);
    let file = root.join(PAGES);
    let mut rt = qml::runtime::Runtime::new(Some(root.clone()));
    if compiled {
        rt.eng()
            .set_compiled(Some(std::rc::Rc::new(ricompiled::Compiled::new(&root))));
    }
    // The document under test keeps its compiled code (it is keyed by its canonical path); the
    // user next to it is not part of the tree.
    let tmp = std::env::temp_dir().join(format!(
        "ri-handler-params-{compiled}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    std::os::unix::fs::symlink(&file, tmp.join("VerticalPageIndicator.qml")).unwrap();
    let user = tmp.join("User.qml");
    std::fs::write(
        &user,
        "import QtQuick\nVerticalPageIndicator {\n    property var got: \"none\"\n    count: 3\n    onClicked: i => got = i\n}\n",
    )
    .unwrap();
    let pages = rt.load_file(&user);
    let _ = std::fs::remove_dir_all(&tmp);
    let pages = pages.unwrap();
    rt.settle();
    let delegates: Vec<usize> = {
        let w = rt.world.borrow();
        (0..w.items.len())
            .filter(|&i| {
                !w.dead.contains(&i)
                    && w.items[i].types.iter().any(|t| t == "MouseArea")
                    && w.items[i].props.get("index").is_some()
            })
            .collect()
    };
    assert_eq!(delegates.len(), 3, "page delegates");
    let second = delegates[1];
    assert_eq!(rt.get(second, "index").to_js_string(), "1");
    // MouseArea.clicked(mouse): the handler's `index` is the delegate's required property.
    rt.emit(
        second,
        "clicked",
        &["mouse"],
        vec![Value::str("mouse event")],
    );
    rt.settle();
    rt.get(pages, "got").to_js_string()
}

#[test]
fn child_handler_does_not_take_the_root_signal_parameters() {
    assert_eq!(clicked_page(true), "1", "compiled");
    assert_eq!(clicked_page(false), "1", "interpreted");
}
