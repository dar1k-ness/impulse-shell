//! Compile the QML tree at `RI_ROOT` (default: the canonical tree `qml` of this repo) with
//! rigen, once, and split the modules over the `ricompiled-pNN` crates (see src/lib.rs).
use std::fmt::Write;
use std::path::{Path, PathBuf};

/// Number of `crates/ricompiled/parts/pNN` crates (regenerate them with `parts/mkparts.sh`).
const PARTS: usize = 32;

fn main() {
    println!("cargo:rerun-if-env-changed=RI_ROOT");
    // Cargo finds a changed tree by file times alone (Cargo Book, build scripts,
    // `rerun-if-changed`): a tree put in place with older times than the last build would keep
    // that build, and new times or a new .git alone would compile it again. A caller that
    // passes a checksum of the tree's content (tools/build.sh) has the tree compiled again
    // when the content is another, and only then.
    println!("cargo:rerun-if-env-changed=RI_ROOT_SUM");
    let given = std::env::var("RI_ROOT_SUM").ok().filter(|s| !s.is_empty());
    let summed = given.is_some();
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    check_part_profiles(&manifest.join("../../Cargo.toml"));
    let root = std::env::var_os("RI_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest.join("../../qml"));
    // The shipped shell has no interpreter: a script without compiled code would not run, so
    // anything short of a complete generation of the tree fails the build.
    let root = root
        .canonicalize()
        .ok()
        .filter(|p| p.join("shell.qml").exists())
        .unwrap_or_else(|| fail(&format!("no QML tree at {} (set RI_ROOT)", root.display())));
    if !summed {
        println!("cargo:rerun-if-changed={}", root.display());
    }
    // The checksum the binary names (`impulse-shell --version`) is of the files this build
    // reads: one a caller passes for another content fails the build.
    let sum = tree_sum(&root);
    if let Some(given) = given.filter(|g| *g != sum) {
        fail(&format!(
            "RI_ROOT_SUM is {given}, and the QML tree at {} sums to {sum}: it changed after \
             it was summed, or the sum is of another tree",
            root.display()
        ));
    }
    // Qt refuses to load a file instantiating an unknown type; native would create a plain
    // object without the type's behaviour, so any such site fails the build.
    let errors = qml::typecheck::check(&root).errors();
    if !errors.is_empty() {
        fail(&format!("unsupported QML types:\n{}", errors.join("\n")));
    }
    // A call of a method Qt declares and the engine has not written throws when it runs
    // (docs/qt-differences.md), so it fails the build instead.
    let unwritten = qml::unwritten::errors(&root);
    if !unwritten.is_empty() {
        fail(&format!(
            "calls of Qt methods the engine has not written:\n{}",
            unwritten.join("\n")
        ));
    }
    // QML text built at run time has no compiled code: the call would fail in the shipped
    // shell, so it fails the build instead.
    let dynamic = qml::runtime::uncompiled_create_qml_object(&root);
    if !dynamic.is_empty() {
        fail(&format!(
            "Qt.createQmlObject (its QML text is built at run time and has no compiled code): {}",
            dynamic.join(", ")
        ));
    }
    // So is JavaScript text: the shipped shell has no evaluator for `eval`.
    let evals = qml::runtime::uncompiled_eval(&root);
    if !evals.is_empty() {
        fail(&format!(
            "eval or Function of JavaScript text (it has no compiled code): {}",
            evals.join(", ")
        ));
    }
    let qmltypes = manifest.join("../qml/qmltypes");
    println!("cargo:rerun-if-changed={}", qmltypes.display());
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let dir = out.join("gen");
    let (files, stats) = generate(&root, &dir).unwrap_or_else(|e| panic!("rigen: {e}"));
    if !stats.unsupported.is_empty() {
        let list: Vec<String> = stats
            .unsupported
            .iter()
            .take(20)
            .map(|(k, u)| format!("{k}: {u}"))
            .collect();
        fail(&format!(
            "{} unsupported constructs:\n{}",
            stats.unsupported.len(),
            list.join("\n")
        ));
    }
    // Whole-file hashes let the AOT-only shell skip parsing unchanged files at startup.
    let sources = tree_hashes(&root);
    let mut files = files;
    files.push(("sources.rs".into(), sources_table(&sources)));
    let g = rigen::Generated { files, stats };
    rigen::write(&g, &dir).unwrap();
    let root = root.display().to_string();
    let mut lib = String::new();
    writeln!(lib, "pub const ROOT: &str = {root:?};").unwrap();
    writeln!(lib, "pub const ROOT_SUM: &str = {sum:?};").unwrap();
    writeln!(lib, "pub const STATS: &str = {:?};", g.stats.to_string()).unwrap();
    writeln!(lib, "pub const PARTS: usize = {PARTS};").unwrap();
    let gen_dir = format!("{}/", dir.display());
    writeln!(
        lib,
        "/// Expands to the generated file `$f` of this build.\n#[macro_export]\nmacro_rules! include_gen {{ ($($f:tt)*) => {{ include!(concat!({gen_dir:?}, $($f)*)); }}; }}"
    )
    .unwrap();
    let p = out.join("gen.rs");
    if std::fs::read_to_string(&p).ok().as_deref() != Some(&lib) {
        std::fs::write(&p, lib).unwrap();
    }
}

/// `(key, src_hash)` of every `.qml`/`.js` file of the QML tree, sorted by key.
fn tree_hashes(root: &Path) -> Vec<(String, u64)> {
    let tree = rigen::walk::tree(root);
    let mut out: Vec<_> = (tree.qml.iter().chain(&tree.js))
        .map(|(key, p)| {
            let src = std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            (key.clone(), qml::walk::src_hash(&src))
        })
        .collect();
    out.sort();
    out
}

/// The checksum of the tree's content that `tools/build.sh --sum` prints: the SHA-256 of
/// `sha256sum`'s lines for every file, read through links, sorted by path (`./a/b`) as bytes;
/// nothing named `.git` is read.
fn tree_sum(root: &Path) -> String {
    use sha2::{Digest, Sha256};
    use std::os::unix::ffi::OsStrExt;
    fn walk(dir: &Path, rel: &[u8], above: &mut Vec<PathBuf>, out: &mut Vec<(Vec<u8>, PathBuf)>) {
        let entries =
            std::fs::read_dir(dir).unwrap_or_else(|e| fail(&format!("{}: {e}", dir.display())));
        for e in entries {
            let e = e.unwrap_or_else(|e| fail(&format!("{}: {e}", dir.display())));
            let name = e.file_name();
            if name == ".git" {
                continue;
            }
            let path = e.path();
            let mut rel = rel.to_vec();
            rel.push(b'/');
            rel.extend_from_slice(name.as_bytes());
            // A link that names nothing is no file, as for `find -L -type f`.
            let Ok(meta) = std::fs::metadata(&path) else {
                continue;
            };
            if meta.is_file() {
                out.push((rel, path));
            } else if meta.is_dir() {
                // A link to a directory above itself would never end.
                let Ok(real) = path.canonicalize() else {
                    continue;
                };
                if above.contains(&real) {
                    continue;
                }
                above.push(real);
                walk(&path, &rel, above, out);
                above.pop();
            }
        }
    }
    let mut files = Vec::new();
    walk(root, b".", &mut vec![root.to_path_buf()], &mut files);
    files.sort();
    let hex = |d: &[u8]| d.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let mut all = Sha256::new();
    for (rel, path) in files {
        let bytes =
            std::fs::read(&path).unwrap_or_else(|e| fail(&format!("{}: {e}", path.display())));
        // sha256sum's line: a name with a backslash, a newline or a carriage return is written
        // with escapes, after a backslash at the start of the line.
        let mut name = Vec::with_capacity(rel.len());
        let mut escaped = false;
        for &b in &rel {
            match b {
                b'\\' => name.extend_from_slice(b"\\\\"),
                b'\n' => name.extend_from_slice(b"\\n"),
                b'\r' => name.extend_from_slice(b"\\r"),
                _ => {
                    name.push(b);
                    continue;
                }
            }
            escaped = true;
        }
        if escaped {
            all.update(b"\\");
        }
        all.update(hex(&Sha256::digest(&bytes)).as_bytes());
        all.update(b"  ");
        all.update(&name);
        all.update(b"\n");
    }
    hex(&all.finalize())
}

fn fail(m: &str) -> ! {
    panic!("ricompiled: {m}")
}

fn sources_table(sources: &[(String, u64)]) -> String {
    let mut t = String::from(
        "// @generated by ricompiled-gen\n/// `(key file, src_hash of the whole file)` of the build tree sorted by key: every\n/// script of such a file is compiled.\npub static SOURCES: &[(&str, u64)] = &[\n",
    );
    for (key, h) in sources {
        writeln!(t, "    ({key:?}, {h:#x}),").unwrap();
    }
    t.push_str("];\n");
    t
}

/// Compiles the QML tree at `root` plus the qml crate's built-in QML modules: one module file
/// per source file (`q<N>.rs`, `j<N>.rs`, `s<N>.rs`, written to `dir` by the caller), each in
/// one of [`PARTS`] part files (`ricompiled-pNN.rs`, balanced by generated size), and
/// `registry.rs` referring to the modules through their part crates.
fn generate(root: &Path, dir: &Path) -> Result<(Vec<(String, String)>, rigen::Stats), String> {
    let mut stats = rigen::Stats::default();
    let tree = rigen::walk::tree(root);
    let mut qml: Vec<_> = tree.qml;
    qml.extend(rigen::walk::module_tree());
    qml.sort();
    let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()));
    // (module name, key, source)
    let mut mods = Vec::new();
    for (i, (key, p)) in qml.iter().enumerate() {
        let src = read(p)?;
        let m = rigen::qml_module(key, &src, p, root, &mut stats)?;
        mods.push((format!("q{i}"), key, m));
    }
    for (i, (key, p)) in tree.js.iter().enumerate() {
        mods.push((
            format!("j{i}"),
            key,
            rigen::js_module(key, &read(p)?, &mut stats)?,
        ));
    }
    for i in rigen::include_targets(&tree.js, &mut stats)? {
        let (key, p) = &tree.js[i];
        mods.push((
            format!("s{i}"),
            key,
            rigen::js_script(key, &read(p)?, &mut stats)?,
        ));
    }
    stats.qml_files = qml.len();
    stats.js_files = tree.js.len();

    let mut order: Vec<usize> = (0..mods.len()).collect();
    order.sort_by_key(|&i| (std::cmp::Reverse(mods[i].2.len()), i));
    let mut load = [0usize; PARTS];
    let mut part_of = vec![0; mods.len()];
    for i in order {
        let p = (0..PARTS).min_by_key(|&p| (load[p], p)).unwrap();
        load[p] += mods[i].2.len();
        part_of[i] = p;
    }

    let mut parts = vec![String::from("// @generated by rigen\n"); PARTS];
    let mut qtab = String::new();
    let mut ltab = String::new();
    let mut jtab = String::new();
    let mut stab = String::new();
    let mut files = Vec::new();
    for (i, (name, key, src)) in mods.into_iter().enumerate() {
        let p = part_of[i];
        let path = dir.join(format!("{name}.rs")).display().to_string();
        writeln!(parts[p], "#[path = {path:?}] pub mod {name};").unwrap();
        if name.starts_with('q') {
            writeln!(qtab, "    ({key:?}, ricompiled_p{p:02}::{name}::ENTRIES),").unwrap();
            writeln!(ltab, "    ({key:?}, ricompiled_p{p:02}::{name}::LAYOUTS),").unwrap();
        } else if name.starts_with('s') {
            writeln!(stab, "    &ricompiled_p{p:02}::{name}::SCRIPT,").unwrap();
        } else {
            writeln!(jtab, "    &ricompiled_p{p:02}::{name}::LIB,").unwrap();
        }
        files.push((format!("{name}.rs"), src));
    }
    for (p, s) in parts.into_iter().enumerate() {
        files.push((format!("ricompiled-p{p:02}.rs"), s));
    }
    files.push((
        "registry.rs".into(),
        format!(
            "// @generated by rigen\n/// `(key file, entries)` sorted by file.\npub static FILES: &[(&str, &[Entry])] = &[\n{qtab}];\n\n/// `.js` libraries sorted by file.\npub static LIBS: &[&Entry] = &[\n{jtab}];\n\n/// `Qt.include` targets sorted by file.\npub static SCRIPTS: &[&Entry] = &[\n{stab}];\n\n/// `(key file, layouts)` sorted by file: per declaration objpath, its property names in\n/// declaration order (slot `i` = `names[i]`).\npub static LAYOUT_FILES: &[(&str, &[Layout])] = &[\n{ltab}];\n"
        ),
    ));
    files.push(("rt.rs".into(), rigen::PRELUDE.to_string()));
    stats.lines = files.iter().map(|(_, s)| s.lines().count()).sum();
    Ok((files, stats))
}

/// Every part crate, and only those, must have its `opt-level = "s"` release override in the
/// workspace manifest, or a change of [`PARTS`] silently builds parts at opt-level 3.
fn check_part_profiles(workspace: &Path) {
    println!("cargo:rerun-if-changed={}", workspace.display());
    let toml = std::fs::read_to_string(workspace).expect("workspace Cargo.toml");
    let block = (0..PARTS)
        .map(|i| format!("[profile.release.package.ricompiled-p{i:02}]\nopt-level = \"s\"\n"))
        .collect::<Vec<_>>()
        .join("\n");
    let overrides = toml
        .matches("[profile.release.package.ricompiled-p")
        .count();
    if overrides != PARTS || !toml.contains(&block) {
        panic!(
            "ricompiled: the workspace Cargo.toml's ricompiled-pNN profile overrides do not match \
             PARTS = {PARTS}: run crates/ricompiled/parts/mkparts.sh {PARTS}"
        );
    }
}
