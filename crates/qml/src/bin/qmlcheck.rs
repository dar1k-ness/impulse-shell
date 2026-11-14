//! Parse every .qml under a directory, validate all JS spans with oxc, print type usage stats
//! and the object types the native runtime cannot resolve or does not implement.
use std::collections::HashMap;

fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().is_some_and(|x| x == "qml") {
            out.push(p);
        }
    }
}

fn main() {
    let root = std::env::args().nth(1).expect("dir");
    let mut files = Vec::new();
    walk(std::path::Path::new(&root), &mut files);
    files.sort();
    let (mut perr, mut jerr, mut nobj, mut nscript) = (0, 0, 0, 0);
    let mut types: HashMap<String, usize> = HashMap::new();
    let mut unsup: HashMap<String, usize> = HashMap::new();
    let mut jsfiles = Vec::new();
    fn walk_js(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk_js(&p, out);
            } else if p.extension().is_some_and(|x| x == "js") {
                out.push(p);
            }
        }
    }
    walk_js(std::path::Path::new(&root), &mut jsfiles);
    for f in &jsfiles {
        match qml::ir::lower_program(&std::fs::read_to_string(f).unwrap()) {
            Ok(b) => {
                let mut u = Vec::new();
                qml::ir::unsupported(&b, &mut u);
                for k in u {
                    *unsup.entry(format!("js:{k}")).or_default() += 1;
                }
            }
            Err(e) => println!("JSFILE {}: {e}", f.display()),
        }
    }
    for f in &files {
        let src = std::fs::read_to_string(f).unwrap();
        match qml::parse(&src) {
            Err(e) => {
                perr += 1;
                println!("PARSE {}:{e}", f.display());
            }
            Ok(doc) => {
                doc.root.walk(&mut |o| {
                    nobj += 1;
                    *types.entry(o.type_name.clone()).or_default() += 1;
                    let mut ss = Vec::new();
                    o.scripts(&mut ss);
                    for s in ss {
                        nscript += 1;
                        match qml::ir::lower(s) {
                            Ok(b) => {
                                let mut u = Vec::new();
                                qml::ir::unsupported(&b, &mut u);
                                for k in u {
                                    if k == "Label" {
                                        println!("LABEL {}:{}", f.display(), s.line);
                                    }
                                    *unsup.entry(k).or_default() += 1;
                                }
                            }
                            Err(_) => {
                                *unsup.entry("lower-failed".into()).or_default() += 1;
                                let head: String = s.src.chars().take(160).collect();
                                println!("LOWER {}:{}: {head}", f.display(), s.line);
                            }
                        }
                        if let Err(e) = qml::js::check(s) {
                            jerr += 1;
                            if jerr <= 40 {
                                let head: String = s.src.chars().take(120).collect();
                                println!("JS {}:{}: {e}\n    {head}", f.display(), s.line);
                            }
                        }
                    }
                });
            }
        }
    }
    let mut t: Vec<_> = types.into_iter().collect();
    t.sort_by_key(|x| std::cmp::Reverse(x.1));
    let top: Vec<String> = t.iter().take(70).map(|(k, v)| format!("{k}:{v}")).collect();
    println!("types {}: {}", t.len(), top.join(" "));
    let tc = qml::typecheck::check(std::path::Path::new(&root));
    for (t, sites) in &tc.unknown {
        println!("UNKNOWN {t}: {}", sites.join(" "));
    }
    for (t, sites) in &tc.unimplemented {
        println!("UNIMPLEMENTED {t}: {}", sites.join(" "));
    }
    println!("unknown_types {:?}", tc.unknown.keys().collect::<Vec<_>>());
    println!(
        "unimplemented_types {:?}",
        tc.unimplemented.keys().collect::<Vec<_>>()
    );
    let mut u: Vec<_> = unsup.into_iter().collect();
    u.sort_by_key(|x| std::cmp::Reverse(x.1));
    println!("unsupported {u:?}");
    println!(
        "files {} parse_err {perr} objects {nobj} scripts {nscript} js_err {jerr}",
        files.len()
    );
    if !tc.errors().is_empty() {
        std::process::exit(1);
    }
}
