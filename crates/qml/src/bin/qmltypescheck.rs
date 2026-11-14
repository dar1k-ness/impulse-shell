fn main() {
    let dir = std::env::args().nth(1).expect("qmltypes dir");
    let mut stack = vec![std::path::PathBuf::from(dir)];
    let (mut files, mut comps, mut enums, mut bad) = (0, 0, 0, 0);
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "qmltypes") {
                files += 1;
                match qml::parse(&std::fs::read_to_string(&p).unwrap()) {
                    Ok(doc) => {
                        for m in &doc.root.members {
                            if let qml::ast::Member::Child(c) = m {
                                comps += 1;
                                enums += c
                                    .members
                                    .iter()
                                    .filter(|m| matches!(m, qml::ast::Member::Child(o) if o.type_name == "Enum"))
                                    .count();
                            }
                        }
                    }
                    Err(e) => {
                        bad += 1;
                        println!("{}: {e:?}", p.display());
                    }
                }
            }
        }
    }
    println!("files {files} components {comps} enums {enums} parse_err {bad}");
}
