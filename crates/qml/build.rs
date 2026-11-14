use std::fmt::Write;

fn walk(d: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(d).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn table(name: &str, dir: &std::path::Path, files: &[std::path::PathBuf], s: &mut String) {
    writeln!(s, "pub static {name}: &[(&str, &str)] = &[").unwrap();
    for f in files {
        let rel = f.strip_prefix(dir).unwrap().display().to_string();
        writeln!(
            s,
            "    ({rel:?}, include_str!({:?})),",
            f.display().to_string()
        )
        .unwrap();
    }
    s.push_str("];\n");
}

/// The syntax definitions and themes, deflated: `src/syntax.rs` unpacks one when it is asked for.
fn pack_syntax_data() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/syntax/data");
    println!("cargo:rerun-if-changed={}", dir.display());
    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("syntax");
    for part in ["syntax", "themes"] {
        std::fs::create_dir_all(out.join(part)).unwrap();
        let mut files = Vec::new();
        walk(&dir.join(part), &mut files);
        for f in files {
            let level = flate2::Compression::best();
            let mut packed = flate2::write::DeflateEncoder::new(Vec::new(), level);
            std::io::Write::write_all(&mut packed, &std::fs::read(&f).unwrap()).unwrap();
            let to = out.join(part).join(f.file_name().unwrap());
            std::fs::write(to, packed.finish().unwrap()).unwrap();
        }
    }
}

fn main() {
    pack_syntax_data();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qmltypes");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut files = Vec::new();
    walk(&dir, &mut files);
    files.sort();
    let (types, module): (Vec<_>, Vec<_>) = files
        .into_iter()
        .partition(|p| p.extension().is_some_and(|x| x == "qmltypes"));
    let module: Vec<_> = module
        .into_iter()
        .filter(|p| p.extension().is_some_and(|x| x == "qml") || p.ends_with("qmldir"))
        .collect();
    let mut s = String::new();
    table("QMLTYPES", &dir, &types, &mut s);
    table("MODULE_FILES", &dir, &module, &mut s);
    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("qmltypes.rs");
    std::fs::write(out, s).unwrap();
}
