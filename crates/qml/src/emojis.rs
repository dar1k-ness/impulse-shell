//! The `Emojis` singleton of module `Impulse` (`Impulse/emojis.qmltypes`): the emoji list is
//! the data section of end-4's `fuzzel-emoji.sh` (vendored verbatim in `data/`), so the shell
//! does not read that bash script from `~/.config/hypr` and split it in JS at startup.

use crate::eval::{Interp, R, new_obj, set_own};
use crate::value::Value;

const SCRIPT: &str = include_str!("../data/fuzzel-emoji.sh");
const LINE_BEFORE_DATA: &str = "### DATA ###";

/// The trimmed non-empty lines after `### DATA ###`.
fn entries() -> Vec<&'static str> {
    let lines: Vec<&str> = SCRIPT.split('\n').collect();
    let Some(i) = lines.iter().position(|l| *l == LINE_BEFORE_DATA) else {
        return vec![];
    };
    lines[i + 1..]
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect()
}

/// `fuzzyQuery(search)`: `FuzzySort.go(search, preparedEntries, {all: true, key: "name"})`
/// mapped to the entries.
fn fuzzy_query(it: &mut Interp, this: &Value, a: &[Value]) -> R<Value> {
    let search = Value::str(&a.first().cloned().unwrap_or_default().to_js_string());
    let _ = this;
    let prepared = PREPARED.with(|p| p.get_or_init(prepare_entries).clone());
    let opts = new_obj();
    set_own(&opts, "all", Value::Bool(true));
    set_own(&opts, "key", Value::str("name"));
    let res =
        crate::jslibs::fuzzysort::go(it, &Value::Undefined, &[search, prepared, Value::Obj(opts)])?;
    let Value::Obj(o) = &res else {
        return Ok(Value::array(vec![]));
    };
    let rs = o.borrow().array.clone().unwrap_or_default();
    let mut out = Vec::with_capacity(rs.len());
    for r in rs {
        let obj = it.get(&r, "obj")?;
        out.push(it.get(&obj, "entry")?);
    }
    Ok(Value::array(out))
}

thread_local! {
    /// `preparedEntries`, built on the first search: fuzzysort targets for the 1928 emojis that
    /// are not built unless the emoji search is used.
    static PREPARED: std::cell::OnceCell<Value> = const { std::cell::OnceCell::new() };
}

fn prepare_entries() -> Value {
    Value::array(
        entries()
            .iter()
            .map(|e| {
                let o = new_obj();
                set_own(
                    &o,
                    "name",
                    crate::jslibs::fuzzysort::prepare(&Value::str(e)),
                );
                set_own(&o, "entry", Value::str(e));
                Value::Obj(o)
            })
            .collect(),
    )
}

pub static TYPE: crate::impulse::Type = crate::impulse::Type {
    name: "Emojis",
    methods: &[("fuzzyQuery", fuzzy_query)],
    init: None,
};

#[cfg(all(test, feature = "interp"))]
mod tests {
    use crate::runtime::Runtime;
    use crate::value::Value;

    fn strs(v: &Value) -> Vec<String> {
        match v {
            Value::Obj(o) => o
                .borrow()
                .array
                .clone()
                .unwrap_or_default()
                .iter()
                .map(|v| v.to_js_string())
                .collect(),
            _ => vec![],
        }
    }

    /// QML names the singleton: `fuzzyQuery` answers with entries of the data of
    /// `fuzzel-emoji.sh`, the best match first, every entry in the data's order for an empty
    /// search.
    #[test]
    fn fuzzy_query_answers_with_entries() {
        let mut rt = Runtime::new(None);
        let root = rt
            .load_str(
                r#"import QtQuick
import Impulse
Item {
    function q(s) { return Emojis.fuzzyQuery(s) }
}
"#,
            )
            .unwrap();
        rt.settle();
        let list: Vec<String> = super::entries().into_iter().map(String::from).collect();
        assert_eq!(list.len(), 1928);
        assert!(list[0].starts_with("😀 grinning face"), "{}", list[0]);
        let mut q = |s: &str| {
            strs(
                &rt.call_method(root, "q", vec![Value::str(s)])
                    .unwrap()
                    .unwrap(),
            )
        };
        assert_eq!(q(""), list);
        assert_eq!(q("grinning face")[0], list[0]);
        let fire = q("fire");
        assert!(fire.iter().any(|e| e.starts_with("🔥")), "{fire:?}");
        assert!(fire.iter().all(|e| list.contains(e)));
        assert!(q("zzzzqqqq").is_empty());
        assert!(rt.errors().is_empty(), "{:?}", rt.errors());
    }
}
