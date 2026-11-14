//! Report object type names in a QML tree that do not resolve to a QML file. usage: resolvecheck ROOT
use std::collections::HashMap;

fn walk(d: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(d).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_some_and(|x| x == "qml") {
            out.push(p);
        }
    }
}

fn val(v: &qml::Value, out: &mut Vec<String>) {
    match v {
        qml::Value::Object(c) => types(c, out),
        qml::Value::List(l) => l.iter().for_each(|c| types(c, out)),
        _ => {}
    }
}

fn members(ms: &[qml::Member], out: &mut Vec<String>) {
    for m in ms {
        match m {
            qml::Member::Property { value: Some(v), .. }
            | qml::Member::Binding { value: v, .. } => val(v, out),
            qml::Member::Group { members: g, .. } => members(g, out),
            qml::Member::On { object, .. }
            | qml::Member::Component { object, .. }
            | qml::Member::Child(object) => types(object, out),
            _ => {}
        }
    }
}

fn types(o: &qml::Object, out: &mut Vec<String>) {
    out.push(o.type_name.clone());
    members(&o.members, out);
}

fn main() {
    let root = std::path::PathBuf::from(std::env::args().nth(1).expect("root"));
    let mut files = Vec::new();
    walk(&root, &mut files);
    let mut unresolved: HashMap<String, usize> = HashMap::new();
    let (mut total, mut local) = (0, 0);
    for f in &files {
        let doc = qml::parse(&std::fs::read_to_string(f).unwrap()).unwrap();
        let vis = qml::resolve::visible_types(&root, f, &doc);
        let mut ts = Vec::new();
        types(&doc.root, &mut ts);
        for t in ts {
            total += 1;
            if vis.contains_key(&t) {
                local += 1;
            } else {
                *unresolved.entry(t).or_default() += 1;
            }
        }
    }
    let mut u: Vec<_> = unresolved.into_iter().collect();
    u.sort_by_key(|x| std::cmp::Reverse(x.1));
    println!(
        "objects {total} ri-file types {local} builtin/other kinds {}",
        u.len()
    );
    println!(
        "{}",
        u.iter()
            .map(|(k, n)| format!("{k}:{n}"))
            .collect::<Vec<_>>()
            .join(" ")
    );
}
