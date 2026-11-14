//! A shell document's `required` property must be set when it is created, with its compiled
//! bindings or the interpreter alike (Qt 6: "Required property … was not initialized").
use std::path::PathBuf;

/// (`bad`, `good`, `direct`) of a user of StyledDropShadow, whose `target` is required.
fn created(compiled: bool) -> [String; 3] {
    let root = PathBuf::from(ricompiled::ROOT);
    let mut rt = qml::runtime::Runtime::new(Some(root.clone()));
    if compiled {
        rt.eng()
            .set_compiled(Some(std::rc::Rc::new(ricompiled::Compiled::new(&root))));
    }
    let tmp = std::env::temp_dir().join(format!("ri-required-{compiled}-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let user = tmp.join("User.qml");
    std::fs::write(
        &user,
        concat!(
            "import QtQuick\nimport ri.modules.common.widgets\n",
            "Item { id: r\n",
            " Component { id: c; StyledDropShadow {} }\n",
            " StyledDropShadow { id: d; target: r }\n",
            " property bool bad: c.createObject(r) === null\n",
            " property bool good: c.createObject(r, { target: r }).source === r\n",
            " property bool direct: d.source === r\n}\n",
        ),
    )
    .unwrap();
    let r = rt.load_file(&user);
    let _ = std::fs::remove_dir_all(&tmp);
    let r = r.unwrap_or_else(|e| panic!("{e}"));
    rt.settle();
    ["bad", "good", "direct"].map(|p| rt.get(r, p).to_js_string())
}

#[test]
fn unset_required_property_fails_creation_compiled_or_not() {
    let want = ["true", "true", "true"].map(String::from);
    assert_eq!(created(true), want, "compiled");
    assert_eq!(created(false), want, "interpreted");
}
