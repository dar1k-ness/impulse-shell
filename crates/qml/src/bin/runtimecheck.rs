//! Instantiate the original singletons + given QML files in the native runtime and summarize binding errors.
//! usage: runtimecheck ROOT [FILE...]
use std::collections::HashMap;

fn main() {
    qml::debug::init_log();
    let args: Vec<String> = std::env::args().collect();
    let root = std::path::PathBuf::from(&args[1]);
    let mut rt = qml::runtime::Runtime::new(Some(root.clone()));
    let t = std::time::Instant::now();
    rt.load_singletons(&[
        root.join("modules/common"),
        root.join("modules/common/functions"),
        root.join("modules/common/utils"),
        root.join("services"),
        root.clone(),
    ]);
    if std::env::var("SCREENS").is_ok() {
        rt.set_screens(&[("Virtual-1".into(), 0, 0, 1920, 1080)]);
    }
    let singletons = rt.world.borrow().singletons.len();
    let first_new = rt.world.borrow().items.len();
    for f in &args[2..] {
        if let Err(e) = rt.load_file(std::path::Path::new(f)) {
            rt.world.borrow_mut().errors.push(e);
        }
    }
    if std::env::var("NATIVE").is_ok() {
        for _ in 0..5 {
            if !rt.poll_native() {
                break;
            }
        }
    }
    if let Ok(p) = std::env::var("PROBE") {
        for path in p.split(',') {
            let mut parts = path.split('.');
            let first = parts.next().unwrap_or_default();
            let sg = rt.world.borrow().singletons.get(first).copied();
            let mut v = sg.or_else(|| rt.id(first)).map(qml::value::Value::Item);
            for part in parts {
                v = match v {
                    Some(qml::value::Value::Item(i)) => rt.world.borrow_mut().read(i, part),
                    _ => None,
                };
            }
            println!("PROBE {path} = {v:?}");
        }
    }
    let errors = rt.errors();
    let w = rt.world.borrow();
    let bindings: usize = w
        .items
        .iter()
        .map(|i| i.props.values().filter(|p| p.binding.is_some()).count())
        .sum();
    println!(
        "singletons {singletons} items {} bindings {bindings} errors {} time {:?}",
        w.items.len(),
        errors.len(),
        t.elapsed()
    );
    if let Ok(f) = std::env::var("SHOW") {
        errors
            .iter()
            .filter(|e| e.contains(&f))
            .take(15)
            .for_each(|e| println!("  {e}"));
    }
    if std::env::var("DUMP").is_ok() {
        for (i, it) in w.items.iter().enumerate().skip(first_new) {
            println!("#{i} {:?} parent={:?}", it.types, it.parent);
            let mut ks: Vec<_> = it.props.iter().collect();
            ks.sort_by_key(|x| *x.0);
            for (k, p) in ks {
                println!("    {k}: {:?} b={}", p.value, p.binding.is_some());
            }
        }
    }
    let mut kinds: HashMap<String, usize> = HashMap::new();
    for e in &errors {
        let k = e
            .split_once(": ")
            .map(|x| x.1)
            .unwrap_or(e)
            .chars()
            .take(70)
            .collect::<String>();
        *kinds.entry(k).or_default() += 1;
    }
    let mut k: Vec<_> = kinds.into_iter().collect();
    k.sort_by_key(|x| std::cmp::Reverse(x.1));
    for (m, n) in k.iter().take(25) {
        println!("{n:5} {m}");
    }
}
