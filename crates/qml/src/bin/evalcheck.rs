//! Execute every `.js` resource of a QML tree with the native evaluator. usage: evalcheck DIR

fn walk(d: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(d).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_some_and(|x| x == "js") {
            out.push(p);
        }
    }
}

fn main() {
    qml::debug::init_log();
    let dir = std::env::args().nth(1).expect("dir");
    let mut files = Vec::new();
    walk(std::path::Path::new(&dir), &mut files);
    let (mut ok, mut bad) = (0, 0);
    for f in &files {
        let realm = qml::eval::Interp::default().global;
        let (mut shared, mut errors) = Default::default();
        let mut libs = qml::eval::Libraries {
            shared: &mut shared,
            files: &mut Default::default(),
            modules: &|_| None,
            errors: &mut errors,
            scope: &mut || None,
        };
        match qml::eval::load_library(f, &realm, &mut libs) {
            Ok(_) if errors.is_empty() => ok += 1,
            Ok(_) => {
                bad += 1;
                println!("FAIL {}", errors.join("; "));
            }
            Err(m) => {
                bad += 1;
                println!("FAIL {m}");
            }
        }
    }
    println!("js files {} ok {ok} failed {bad}", files.len());
}
